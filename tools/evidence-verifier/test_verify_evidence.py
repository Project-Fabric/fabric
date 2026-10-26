"""Tests for the independent verifier, on evidence produced by the real gateway binary.

The forged cases rewrite the chain *consistently* (every hash and link recomputed), which the
chain check alone cannot catch; only the rules replay can. Requires the `blake3` package and a
built `cosigna-agent` (COSIGNA_AGENT_BIN, or target/debug).
"""
import json
import os
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path

import blake3

HERE = Path(__file__).resolve().parent
VERIFY = HERE / "verify_evidence.py"
ROOT = HERE.parent.parent
BIN = os.environ.get("COSIGNA_AGENT_BIN") or str(ROOT / "target" / "debug" / ("cosigna-agent.exe" if os.name == "nt" else "cosigna-agent"))


def http(port, auth, method, path, body=None):
    request = urllib.request.Request(f"http://127.0.0.1:{port}/gateway/v1{path}", method=method,
                                     data=json.dumps(body).encode() if body is not None else None,
                                     headers={"Authorization": auth, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        return json.loads(error.read())


def rechain(records):
    """Rewrite a list of record dicts as a fully consistent chain (what a forger with write access could do)."""
    lines, prev = [], "0" * 64
    for seq, record in enumerate(records, start=1):
        record = dict(record, seq=seq, prev=prev)
        body = json.dumps(record, separators=(",", ":")).encode()
        prev = blake3.blake3(body).hexdigest()
        lines.append(prev.encode() + b" " + body)
    return b"\n".join(lines) + b"\n"


class Verifier(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp(prefix="cosigna-verify-")
        subprocess.run([BIN, "init", "--data-dir", cls.dir], check=True, capture_output=True)
        db = sqlite3.connect(os.path.join(cls.dir, "fixture-provider.sqlite"))
        db.executemany("INSERT INTO fixture_payments(id,currency,amount_minor,customer) VALUES (?,?,?,?)",
                       [("pay_v_1", "USD", 2000, "a"), ("pay_v_2", "USD", 45000, "b"), ("pay_v_3", "USD", 30000, "c"),
                        ("pay_v_4", "USD", 3000, "d"), ("pay_v_5", "USD", 40000, "e"), ("pay_v_6", "USD", 45000, "f")])
        db.execute("INSERT INTO fixture_cases(id,customer,source,currency,owed_minor) VALUES ('case_v_4','d','pay_v_4','USD',1000)")
        db.commit(); db.close()
        owner = "Bearer " + json.load(open(os.path.join(cls.dir, "credentials.json")))["owner"]
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]
        server = subprocess.Popen([BIN, "serve", "--data-dir", cls.dir, "--port", str(port)],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                try:
                    urllib.request.urlopen(f"http://127.0.0.1:{port}/v1/health"); break
                except OSError:
                    time.sleep(0.05)
            now = int(time.time())
            budget = http(port, owner, "POST", "/budgets", {"currency": "USD", "limit_minor": 100000, "per_purchase_minor": 50000,
                          "merchants": ["m"], "expires_at": now + 3600})["data"]["id"]
            bot = "Bearer " + http(port, owner, "POST", "/agents", {"name": "bot", "budget_id": budget, "permissions": ["execute"],
                                   "expires_at": now + 1800, "operations": {"kinds": ["refund"], "rails": ["fixture"],
                                   "auto_approve_minor": 5000}})["data"]["token"]
            refund = lambda src, amount, key: http(port, bot, "POST", "/operations", {"budget_id": budget, "kind": "refund",
                "rail": "fixture", "source": src, "amount_minor": amount, "currency": "USD", "reason": "r", "idempotency_key": key})
            refund("pay_v_1", 2000, "auto")                       # auto-approved, paid
            held = refund("pay_v_2", 45000, "held")               # held, then owner approves
            http(port, owner, "POST", f"/operations/{held['data']['id']}/approve", {"proposal_digest": held["proposal_digest"]})
            rejected = refund("pay_v_3", 30000, "reject")         # held, then owner rejects
            http(port, owner, "POST", f"/operations/{rejected['data']['id']}/reject", {})
            refund("pay_v_1", 100, "too-much")                     # denied: nothing left to refund
            strict = "Bearer " + http(port, owner, "POST", "/agents", {"name": "strict", "budget_id": budget, "permissions": ["execute"],
                                      "expires_at": now + 1800, "operations": {"kinds": ["refund"], "rails": ["fixture"],
                                      "auto_approve_minor": 5000, "require_case": True}})["data"]["token"]
            bound = lambda src, amount, case, key: http(port, strict, "POST", "/operations", dict({"budget_id": budget, "kind": "refund",
                "rail": "fixture", "source": src, "amount_minor": amount, "currency": "USD", "reason": "r", "idempotency_key": key},
                **({"case_id": case} if case else {})))
            bound("pay_v_4", 1000, "case_v_4", "case-ok")         # auto-approved: matches its case
            bound("pay_v_4", 1500, "case_v_4", "case-over")       # held: more than the case still owes
            bound("pay_v_4", 500, None, "case-missing")           # denied: the grant requires a case
            approver = lambda name: http(port, owner, "POST", "/approvers",
                                         {"name": name, "currency": "USD", "max_approval_minor": 45000})["data"]
            sam = approver("Sam")                                   # registered, then revoked
            http(port, owner, "POST", f"/approvers/{sam['approver']['id']}/revoke", {})
            priya = approver("Priya")
            cls.sam, cls.priya = sam["approver"]["id"], priya["approver"]["id"]
            named = refund("pay_v_5", 40000, "named")              # held, then Priya approves
            http(port, "Bearer " + priya["token"], "POST", f"/operations/{named['data']['id']}/approve",
                 {"proposal_digest": named["proposal_digest"]})
            dual = http(port, owner, "POST", "/budgets", {"currency": "USD", "limit_minor": 100000, "per_purchase_minor": 50000,
                        "merchants": ["m"], "expires_at": now + 3600, "second_approval_above_minor": 20000})["data"]["id"]
            dual_bot = "Bearer " + http(port, owner, "POST", "/agents", {"name": "dual", "budget_id": dual, "permissions": ["execute"],
                                        "expires_at": now + 1800, "operations": {"kinds": ["refund"], "rails": ["fixture"],
                                        "auto_approve_minor": 50000}})["data"]["token"]
            two = http(port, dual_bot, "POST", "/operations", {"budget_id": dual, "kind": "refund", "rail": "fixture",
                       "source": "pay_v_6", "amount_minor": 45000, "currency": "USD", "reason": "r", "idempotency_key": "two"})
            for person in ("Bearer " + priya["token"], owner):     # held: needs two people; Priya, then the owner
                http(port, person, "POST", f"/operations/{two['data']['id']}/approve", {"proposal_digest": two["proposal_digest"]})
        finally:
            server.kill(); server.wait()
        cls.genuine = os.path.join(cls.dir, "evidence.jsonl")
        subprocess.run([BIN, "evidence", "export", "--data-dir", cls.dir, "--out", cls.genuine], check=True, capture_output=True)
        with open(cls.genuine, "rb") as f:
            cls.records = [json.loads(line.split(b" ", 1)[1]) for line in f if line.strip()]

    def verify(self, data=None):
        path = self.genuine
        if data is not None:
            path = os.path.join(self.dir, f"forged-{time.time_ns()}.jsonl")
            with open(path, "wb") as f:
                f.write(data)
        run = subprocess.run([sys.executable, "-I", str(VERIFY), path, "--json"], capture_output=True, text=True)
        return run.returncode, json.loads(run.stdout)

    def forged(self, change):
        records = json.loads(json.dumps(self.records))
        change(records)
        return rechain(records)

    def test_genuine_evidence_passes_both_verifiers(self):
        code, report = self.verify()
        self.assertEqual(code, 0, report["problems"])
        self.assertEqual(report["hashes_checked"], report["records"])
        self.assertEqual(report["rules"]["operations"], 9)
        self.assertEqual(report["rules"]["auto_approved"], 2)
        self.assertEqual(report["rules"]["case_bound"], 2)
        self.assertEqual(report["rules"]["owner_decisions"], 3)
        self.assertEqual(report["rules"]["approver_decisions"], 2)
        self.assertEqual(report["rules"]["first_approvals"], 1)
        self.assertEqual(subprocess.run([BIN, "evidence", "verify", "--data-dir", self.dir], capture_output=True).returncode, 0)

    def test_an_edited_body_breaks_its_hash(self):
        data = open(self.genuine, "rb").read().replace(b'"amount_minor":2000', b'"amount_minor":2001', 1)
        code, report = self.verify(data)
        self.assertEqual(code, 5)
        self.assertTrue(any("does not match its hash" in p for p in report["problems"]))

    def test_a_consistent_rewrite_that_breaks_a_rule_is_still_caught(self):
        def auto_over_grant(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.auto_approved" and not e["request"].get("case_id"):
                    e["request"]["amount_minor"] = 9000
        def case_rewritten(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.auto_approved" and e["request"].get("case_id"):
                    e["proposal"]["facts"]["case"]["owed_minor"] = 500
        def case_dropped(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.auto_approved" and e["request"].get("case_id"):
                    del e["request"]["case_id"]
        def committed_forged(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.held" and e["request"].get("case_id"):
                    e["proposal"]["facts"]["case"]["committed_minor"] = 0
        def agent_approves(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.owner_approved":
                    r["event"]["by"] = {"owner": "local-user", "role": "planner", "agent": "agent_x"}
        def overspend(records):
            for r in records:
                if r["event"]["kind"] == "gateway.budget.created":
                    r["event"]["terms"]["limit_minor"] = 40000
        def inflated_hold(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.rail_result" and e["request"]["amount_minor"] == 2000:
                    e["hold"] = {"amount_minor": 20000, "state": "spent"}
        def paid_without_receipt(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.operation.rail_result":
                    e["receipt"] = None
        def approver_limit_lowered(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.approver.registered" and e["approver"] == self.priya:
                    e["terms"]["max_approval_minor"] = 100
        def revoked_approver_decides(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.approver_approved":
                    r["event"]["by"]["approver"] = self.sam
        def unregistered_approver(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.approver_approved":
                    r["event"]["by"]["approver"] = "approver_" + "0" * 64
        def approval_recorded_as_owner(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.approver_approved":
                    r["event"]["approved_by"] = "owner"
        def first_approval_removed(records):
            records[:] = [r for r in records if r["event"]["kind"] != "gateway.operation.first_approved"]
        def same_person_twice(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.first_approved":
                    r["event"]["by"] = {"owner": r["event"]["by"]["owner"], "role": "owner", "agent": None}
        def threshold_lowered(records):
            for r in records:
                e = r["event"]
                if e["kind"] == "gateway.budget.created" and "second_approval_above_minor" not in e["terms"]:
                    e["terms"]["second_approval_above_minor"] = 1000
                    break
        def wrong_digest(records):
            for r in records:
                if r["event"]["kind"] == "gateway.operation.owner_approved":
                    r["event"]["approved_digest"] = "f" * 64
        for change, expected in ((auto_over_grant, "outside the agent's grant"), (agent_approves, "not made by the owner"),
                                 (overspend, "exceeds its limit"), (inflated_hold, "hold differs from the requested amount"), (paid_without_receipt, "without a matching receipt"),
                                 (wrong_digest, "does not name the proposal's digest"),
                                 (case_rewritten, "does not match its case"), (case_dropped, "without the case its grant requires"),
                                 (committed_forged, "committed amount differs"),
                                 (approver_limit_lowered, "above the approver's limit"),
                                 (revoked_approver_decides, "by a revoked approver"),
                                 (unregistered_approver, "by an unregistered approver"),
                                 (approval_recorded_as_owner, "not recorded as theirs"),
                                 (first_approval_removed, "without a first approval"),
                                 (same_person_twice, "the same person gave both approvals"),
                                 (threshold_lowered, "automatic approval above the two-person threshold")):
            code, report = self.verify(self.forged(change))
            self.assertEqual(code, 5, change.__name__)
            self.assertEqual(report["hashes_checked"], report["records"], "the forged chain is internally consistent")
            self.assertFalse(any("hash" in p or "link" in p for p in report["problems"]), change.__name__)
            self.assertTrue(any(expected in p for p in report["problems"]), (change.__name__, report["problems"]))

    def test_removed_or_reordered_records_break_the_chain(self):
        lines = open(self.genuine, "rb").read().splitlines()
        code, report = self.verify(b"\n".join(lines[:2] + lines[3:]) + b"\n")
        self.assertEqual(code, 5)
        code, report = self.verify(b"\n".join([lines[1], lines[0]] + lines[2:]) + b"\n")
        self.assertEqual(code, 5)


if __name__ == "__main__":
    unittest.main()
