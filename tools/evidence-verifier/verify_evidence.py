"""Independent verifier for Cosigna evidence (`evidence.jsonl`, record version 1).

Written separately from the Rust code, with Python's standard library only, so that one bug cannot
both produce and accept a bad record. Short enough for an auditor to read.

1. Chain: every line is `<hash> <body>`, where hash is the BLAKE3 of the body's exact bytes. BLAKE3
   is not in Python's standard library, so hashes are checked with the `blake3` package when it is
   installed and otherwise reported as "not checked", never assumed. seq starts at 1 and increases
   by one; prev links each record to the previous hash (64 zeros for the first).
2. Rules (gateway records, `gateway.*` kinds), replayed from the records alone:
   - a budget's spent holds never exceed its limit; no operation exceeds the per-operation limit;
   - an automatic approval is within the requesting agent's grant (kind, rail, auto limit) at
     that moment, and the agent was not revoked;
   - only the owner role approves, rejects or resolves; agents without a grant never get money;
   - a succeeded operation has a receipt matching its proposal amount and currency, and its hold
     is spent; failed, denied and rejected operations never end with spent money;
   - an approval names the digest of the proposal it approved;
   - case binding (ledger version 6): a refund that names a case records that case in its proposal;
     an automatic approval matches the case's payment, currency and customer and stays within
     what the case still owes, where "already committed" is recomputed here from earlier
     operations' holds rather than trusted; a grant that requires a case never gets money
     without one.

Usage: python -I verify_evidence.py evidence.jsonl [--json]
Exit code 0: all checks passed. 5: a problem was found (same code as the Rust verifiers).
"""
import argparse
import json
import sys

GENESIS = "0" * 64


def blake3_hex(data):
    try:
        import blake3  # optional third-party package
    except ImportError:
        return None
    return blake3.blake3(data).hexdigest()


def read_chain(path, problems):
    records, prev, hashes_checked = [], GENESIS, 0
    with open(path, "rb") as f:
        for number, raw in enumerate(f, start=1):
            line = raw.rstrip(b"\r\n")
            if not line:
                continue
            hash_hex, _, body = line.partition(b" ")
            try:
                record = json.loads(body)
            except ValueError:
                problems.append(f"record {number}: body is not JSON")
                continue
            computed = blake3_hex(body)
            if computed is not None:
                hashes_checked += 1
                if computed != hash_hex.decode():
                    problems.append(f"record {number}: body does not match its hash")
            if record.get("v") != 1 or record.get("seq") != number:
                problems.append(f"record {number}: wrong version or sequence")
            if record.get("prev") != prev:
                problems.append(f"record {number}: does not link to the previous record")
            prev = hash_hex.decode()
            records.append(record)
    return records, hashes_checked


def check_case(seq, step, event, request, ops, grant, problems, stats):
    """Case binding, from the first record of an operation. `ops` holds the latest record of every
    earlier operation, so the committed total is recomputed independently."""
    case_id = request.get("case_id")
    if step != "denied" and request.get("kind") == "refund" and not case_id and (grant or {}).get("require_case"):
        problems.append(f"record {seq}: refund without the case its grant requires")
    if not case_id or step == "denied":
        return
    stats["case_bound"] = stats.get("case_bound", 0) + 1
    facts = (event.get("proposal") or {}).get("facts") or {}
    case = facts.get("case")
    if not case or case.get("id") != case_id:
        problems.append(f"record {seq}: the proposal does not record the case the refund names")
        return
    committed = sum((e.get("hold") or {}).get("amount_minor", 0) for op_id, e in ops.items()
                    if op_id != event["operation"] and e["request"].get("case_id") == case_id
                    and e.get("budget") is not None and (e.get("hold") or {}).get("state") in ("held", "spent"))
    if case.get("committed_minor") != committed:
        problems.append(f"record {seq}: the case's committed amount differs from earlier operations")
    if step == "auto_approved":
        customer = facts.get("customer")
        if (case.get("source") != request.get("source") or case.get("currency") != request.get("currency")
                or (customer is not None and customer != case.get("customer"))
                or request["amount_minor"] > case.get("owed_minor", 0) - committed):
            problems.append(f"record {seq}: automatic approval that does not match its case")


def replay(records, problems):
    budgets, agents, ops, tasks = {}, {}, {}, {}
    stats = {"operations": 0, "auto_approved": 0, "owner_decisions": 0, "denied": 0}
    for record in records:
        event, seq = record.get("event", {}), record.get("seq")
        kind = event.get("kind", "")
        by = event.get("by") or {}
        if kind.startswith("gateway.budget."):
            budgets[event["budget"]] = {"terms": event["terms"], "revoked": event["revoked"]}
        elif kind.startswith("gateway.agent."):
            agents[event["agent"]] = event["grant"]
        elif kind.startswith("gateway.task."):
            tasks[event["task"]] = event
        elif kind.startswith("gateway.operation."):
            step = kind.rsplit(".", 1)[1]
            op_id, request = event["operation"], event["request"]
            first = op_id not in ops
            ops[op_id] = event
            budget = budgets.get(event["budget"])
            if budget is None:
                problems.append(f"record {seq}: operation {op_id[:12]} on an unrecorded budget")
                continue
            terms = budget["terms"]
            if first:
                check_case(seq, step, event, request, ops, (agents.get(event.get("agent")) or {}).get("operations"), problems, stats)
                stats["operations"] += 1
                agent_id = event.get("agent")
                grant = (agents.get(agent_id) or {}).get("operations") if agent_id else None
                if step == "auto_approved":
                    stats["auto_approved"] += 1
                    if not agent_id or not grant:
                        problems.append(f"record {seq}: automatic approval without an agent grant")
                    elif (request["kind"] not in grant["kinds"] or request["rail"] not in grant["rails"]
                          or request["amount_minor"] > grant["auto_approve_minor"]):
                        problems.append(f"record {seq}: automatic approval outside the agent's grant")
                    if (agents.get(agent_id) or {}).get("revoked"):
                        problems.append(f"record {seq}: automatic approval for a revoked agent")
                if step == "denied":
                    stats["denied"] += 1
                if step != "denied" and agent_id and not grant:
                    problems.append(f"record {seq}: an agent without a grant got an operation past denial")
                if step != "denied" and request["amount_minor"] > terms["per_purchase_minor"]:
                    problems.append(f"record {seq}: operation over the per-operation limit")
                if step != "denied" and request["currency"] != terms["currency"]:
                    problems.append(f"record {seq}: operation currency differs from the budget")
            if step in ("owner_approved", "owner_rejected"):
                stats["owner_decisions"] += 1
                if by.get("role") != "owner" or by.get("agent"):
                    problems.append(f"record {seq}: {step} not made by the owner")
            if step in ("owner_approved", "auto_approved"):
                proposal = event.get("proposal") or {}
                if event.get("approved_digest") != proposal.get("digest"):
                    problems.append(f"record {seq}: approval does not name the proposal's digest")
            hold = event.get("hold") or {}
            stage = event.get("stage")
            if hold and hold.get("amount_minor") != request["amount_minor"]:
                problems.append(f"record {seq}: hold differs from the requested amount")
            if stage == "succeeded":
                receipt = event.get("receipt") or {}
                if (not receipt.get("succeeded") or receipt.get("amount_minor") != request["amount_minor"]
                        or receipt.get("currency") != request["currency"] or hold.get("state") != "spent"):
                    problems.append(f"record {seq}: succeeded without a matching receipt and spent hold")
            if stage in ("failed", "denied", "rejected") and hold.get("state") == "spent":
                problems.append(f"record {seq}: {stage} operation ended with spent money")
    # Final money per budget, from the latest record of every operation and task.
    for budget_id, budget in budgets.items():
        spent = sum((e.get("hold") or {}).get("amount_minor", 0) for e in ops.values()
                    if e["budget"] == budget_id and (e.get("hold") or {}).get("state") == "spent")
        spent += sum((e.get("reservation") or {}).get("amount_minor", 0) for e in tasks.values()
                     if e["budget"] == budget_id and (e.get("reservation") or {}).get("state") == "spent")
        if spent > budget["terms"]["limit_minor"]:
            problems.append(f"budget {budget_id[:16]}: spent {spent} exceeds its limit {budget['terms']['limit_minor']}")
    stats["budgets"] = len(budgets)
    return stats


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("evidence")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--require-hashes", action="store_true", help="fail if hashes cannot be checked (blake3 missing)")
    args = parser.parse_args()
    problems = []
    records, hashes_checked = read_chain(args.evidence, problems)
    if not records:
        problems.append("no records")
    elif records[0].get("event", {}).get("kind") not in ("gateway.ledger.created", "gateway.ledger.upgraded", "genesis"):
        problems.append("record 1: the chain does not start with a ledger or genesis record")
    stats = replay(records, problems)
    if args.require_hashes and records and hashes_checked != len(records):
        problems.append("hashes could not be checked: install the blake3 package")
    report = {"schema": "cosigna.independent-verify/v1", "records": len(records),
              "hashes_checked": hashes_checked if hashes_checked else "not checked (pip install blake3)",
              "rules": stats, "ok": not problems, "problems": problems[:100]}
    print(json.dumps(report, indent=2) if args.json else
          ("OK" if not problems else "PROBLEMS") + f": {len(records)} records, hashes checked: {report['hashes_checked']}, "
          f"{stats['operations']} operations replayed" + "".join(f"\n  - {p}" for p in problems[:100]))
    return 0 if not problems else 5


if __name__ == "__main__":
    sys.exit(main())
