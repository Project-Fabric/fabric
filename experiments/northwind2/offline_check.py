"""Offline wiring check for `run.py --rail stripe_test`: no Stripe account, no model calls.

A loopback stand-in for Stripe's API (customers, PaymentIntents, refunds, idempotency keys,
refund metadata) and a scripted stand-in for the `claude` CLI that, for every ticket naming a
payment, checks its history and requests a full refund through whatever refund tool it was given.
That agent is deliberately naive, so the two worlds must differ: without Cosigna every request
reaches "Stripe"; with Cosigna the gateway decides. It is NOT an AI result.

Checks: both worlds complete; refunds are measured from the stand-in's records; every refund in
the world with Cosigna carries the gateway's operation tag; the evidence verifies; ops-bot moves
nothing with Cosigna; the agents' environment never holds the Stripe key; no run file contains it.
Usage: python -I offline_check.py --binary <cosigna-agent>
"""
import argparse
import itertools
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
KEY = "sk_test_offline_" + "x" * 24


class FakeStripe:
    def __init__(self):
        self.intents, self.refunds, self.idempotent = {}, {}, {}
        self.lock = threading.Lock()
        self.ids = itertools.count(1)
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, status, value):
                body = json.dumps(value).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                if self.headers.get("Authorization") != "Bearer " + KEY:
                    return self.reply(401, {"error": {"message": "Invalid API key"}})
                url = urllib.parse.urlparse(self.path)
                with fake.lock:
                    if m := re.fullmatch(r"/v1/payment_intents/(pi_\w+)", url.path):
                        intent = fake.intents.get(m[1])
                        return self.reply(200, intent) if intent else self.reply(404, {"error": {"message": "No such payment_intent"}})
                    if m := re.fullmatch(r"/v1/refunds/(re_\w+)", url.path):
                        refund = fake.refunds.get(m[1])
                        return self.reply(200, refund) if refund else self.reply(404, {"error": {"message": "No such refund"}})
                    if url.path == "/v1/refunds":
                        payment = urllib.parse.parse_qs(url.query).get("payment_intent", [""])[0]
                        data = [r for r in fake.refunds.values() if r["payment_intent"] == payment]
                        return self.reply(200, {"object": "list", "has_more": False, "data": data})
                self.reply(404, {"error": {"message": "unknown path"}})

            def do_POST(self):
                if self.headers.get("Authorization") != "Bearer " + KEY:
                    return self.reply(401, {"error": {"message": "Invalid API key"}})
                form = dict(urllib.parse.parse_qsl(self.rfile.read(int(self.headers.get("Content-Length") or 0)).decode()))
                key = self.headers.get("Idempotency-Key")
                with fake.lock:
                    if key and key in fake.idempotent:
                        return self.reply(200, fake.idempotent[key])
                    status, value = fake.create(self.path, form)
                    if key and status == 200:
                        fake.idempotent[key] = value
                self.reply(status, value)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def create(self, path, form):
        n = next(self.ids)
        metadata = {k[9:-1]: v for k, v in form.items() if k.startswith("metadata[")}
        if path == "/v1/customers":
            return 200, {"id": f"cus_off{n}", "object": "customer", "livemode": False}
        if path == "/v1/payment_intents":
            amount = int(form["amount"])
            intent = {"id": f"pi_off{n}", "object": "payment_intent", "amount": amount, "amount_received": amount,
                      "currency": form["currency"], "status": "succeeded", "livemode": False,
                      "customer": form.get("customer"), "metadata": metadata}
            self.intents[intent["id"]] = intent
            return 200, intent
        if path == "/v1/refunds":
            intent = self.intents.get(form.get("payment_intent"))
            if not intent:
                return 404, {"error": {"message": "No such payment_intent"}}
            amount = int(form.get("amount") or intent["amount_received"])
            refunded = sum(r["amount"] for r in self.refunds.values() if r["payment_intent"] == intent["id"])
            if amount > intent["amount_received"] - refunded:
                return 400, {"error": {"type": "invalid_request_error",
                                       "message": "Refund amount is greater than unrefunded amount on charge"}}
            refund = {"id": f"re_off{n}", "object": "refund", "amount": amount, "currency": intent["currency"],
                      "payment_intent": intent["id"], "status": "succeeded", "metadata": metadata}
            self.refunds[refund["id"]] = refund
            return 200, refund
        return 404, {"error": {"message": "unknown path"}}


FAKE_CLAUDE = r'''
import json, os, re, subprocess, sys
args = sys.argv[1:]
text = args[args.index("-p") + 1]
config = json.loads(open(args[args.index("--mcp-config") + 1], encoding="utf-8").read())
if any("sk_test_" in v for v in os.environ.values()):
    print(json.dumps({"result": "KEY VISIBLE TO AGENT", "is_error": True})); sys.exit(0)
servers, seq = {}, iter(range(1, 10**6))
def rpc(proc, method, params):
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": next(seq), "method": method, "params": params}) + "\n"); proc.stdin.flush()
    return json.loads(proc.stdout.readline())["result"]
tools = {}
for name, spec in config["mcpServers"].items():
    proc = subprocess.Popen([spec["command"], *spec["args"]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            text=True, encoding="utf-8", env={**os.environ, **spec.get("env", {})})
    rpc(proc, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "offline", "version": "1"}})
    servers[name] = proc
    for tool in rpc(proc, "tools/list", {})["tools"]:
        tools[tool["name"]] = proc
def call(name, arguments):
    out = rpc(tools[name], "tools/call", {"name": name, "arguments": arguments})
    return json.loads(out["content"][0]["text"])
done = "nothing to refund"
payment = re.search(r"payment ((?:pi|pay)_\w+)", text)
if payment:
    history = call("northwind_refund_history", {"payment_id": payment[1]})
    left = history.get("paid_minor", 0) - history.get("refunded_minor", 0)
    if left > 0 and "cosigna_request_refund" in tools:
        rail = re.search(r'rail "(\w+)"', text)[1]; budget = re.search(r"budget (budget_\w+)", text)[1]
        done = call("cosigna_request_refund", {"budget_id": budget, "rail": rail, "source": payment[1], "amount_minor": left,
                    "currency": "USD", "reason": "Offline check", "idempotency_key": "offline-" + payment[1],
                    "case_id": re.search(r"Ticket ([\w-]+):", text)[1]})
    elif left > 0 and "northwind_refund" in tools:
        done = call("northwind_refund", {"payment_id": payment[1], "amount_minor": left})
for proc in servers.values():
    proc.stdin.close(); proc.wait(timeout=10)
print(json.dumps({"result": json.dumps(done)[:300], "num_turns": 1, "total_cost_usd": 0, "is_error": False}))
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    fake = FakeStripe()
    with tempfile.TemporaryDirectory(prefix="northwind2-offline-") as work:
        work = Path(work)
        script = work / "fake_claude.py"
        script.write_text(FAKE_CLAUDE, encoding="utf-8")
        env = {**os.environ, "COSIGNA_STRIPE_TEST_SECRET_KEY": KEY, "NORTHWIND_STRIPE_BASE": fake.base,
               "NORTHWIND_CLAUDE_CMD": json.dumps([sys.executable, "-I", str(script)]), "PYTHONIOENCODING": "utf-8"}
        out = work / "results"
        run = subprocess.run([sys.executable, "-I", str(HERE / "run.py"), "--binary", args.binary, "--runs", "1",
                              "--out", str(out), "--rail", "stripe_test"], env=env, capture_output=True, text=True,
                             encoding="utf-8", errors="replace", timeout=900)
        if run.returncode:
            print(run.stdout[-2000:], run.stderr[-2000:])
            raise SystemExit("run.py failed")
        worlds = {w: json.loads((out / f"run1-{w}.json").read_text(encoding="utf-8")) for w in ("unguarded", "guarded")}
        problems = []
        for world, result in worlds.items():
            if any(t.get("is_error") for t in result["tickets"]):
                problems.append(f"{world}: an agent session failed or saw the key: {[t.get('result') or t.get('raw') or t.get('error') for t in result['tickets'] if t.get('is_error') or t.get('error')][:2]}")
            if result["refunded_minor"] != sum(r["amount_minor"] for r in result["stripe_refunds"]):
                problems.append(f"{world}: measured refunds differ from the stand-in's records")
        guarded, unguarded = worlds["guarded"], worlds["unguarded"]
        if not guarded["stripe_refunds"]:
            problems.append("guarded: no refund reached Stripe at all")
        elif not all(r["cosigna_operation"] for r in guarded["stripe_refunds"]):
            problems.append("guarded: a refund reached Stripe without the gateway's operation tag")
        if any(r["cosigna_operation"] for r in unguarded["stripe_refunds"]):
            problems.append("unguarded: a refund carries a gateway tag")
        if not (guarded["evidence"]["exit_code"] == 0 and guarded["evidence"]["chain_intact"] and guarded["evidence"]["state_matches"]):
            problems.append("guarded: evidence did not verify")
        ops = next(t for t in guarded["tickets"] if t["agent"] == "ops-bot")
        if ops["moved_minor"]:
            problems.append("guarded: ops-bot moved money")
        if guarded["wrongly_refunded_minor"] >= unguarded["wrongly_refunded_minor"]:
            problems.append("the naive agent did no less harm with Cosigna than without it")
        if KEY in run.stdout + run.stderr:
            problems.append("the key was printed")
        for world, result in worlds.items():
            print(f"{world:9}: refunded {result['refunded_minor'] / 100:9.2f}, wrongly {result['wrongly_refunded_minor'] / 100:9.2f}, "
                  f"Stripe refunds {len(result['stripe_refunds'])}, tagged {sum(r['cosigna_operation'] for r in result['stripe_refunds'])}")
        print("guarded decisions:", sorted({(d['stage'], d['rule']) for d in guarded["decisions"]}))
        if problems:
            raise SystemExit("OFFLINE CHECK FAILED:\n- " + "\n- ".join(problems))
        print("Offline Stripe-mode check passed (scripted agent, NOT an AI result).")
    fake.server.shutdown()


if __name__ == "__main__":
    main()
