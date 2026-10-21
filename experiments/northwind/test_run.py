"""Safety and protocol regressions for the Northwind runner. No real credentials or network."""
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import run as northwind


def refund(amount=100, status="succeeded", **extra):
    return {"id": "re_fixture", "object": "refund", "amount": amount, "status": status,
            "payment_intent": "pi_fixture", "currency": "usd", **extra}


def listing(*rows, has_more=False):
    return {"object": "list", "has_more": has_more, "data": list(rows)}


class RefundScoringTests(unittest.TestCase):
    def test_empty_list_is_known_zero(self):
        self.assertEqual(northwind.refund_total(listing(), "pi_fixture"), 0)

    def test_failed_and_canceled_refunds_do_not_count(self):
        rows = listing(refund(100), refund(200, "pending"), refund(300, "requires_action"),
                       refund(400, "failed"), refund(500, "canceled"))
        self.assertEqual(northwind.refund_total(rows, "pi_fixture"), 600)

    def test_unrelated_nested_amount_is_not_counted(self):
        row = refund(100, destination_details={"amount": 999999})
        self.assertEqual(northwind.refund_total(listing(row), "pi_fixture"), 100)

    def test_incomplete_or_wrong_data_is_never_zero(self):
        for payload in (listing(has_more=True), {}, listing(refund(payment_intent="pi_other")),
                        listing(refund(currency="jpy")), listing(refund(status="surprise")),
                        listing(refund(True)), listing(refund(1.5)), listing(refund(-5))):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                northwind.refund_total(payload, "pi_fixture")

    def test_api_response_wrapper_is_parsed_as_json(self):
        payload = listing(refund())
        self.assertEqual(northwind.refund_payload(json.dumps({"response": {"body": payload}})), payload)
        self.assertEqual(northwind.refund_payload("not json", payload), payload)

    def test_current_generic_read_tool_uses_payment_filter(self):
        session = Mock()
        session.call.return_value = (False, json.dumps(listing(refund())), None)
        self.assertEqual(northwind.refunded_cents("guarded", {}, {"payment_intent": "pi_fixture"},
                                                ["stripe_api_read"], session), 100)
        session.call.assert_called_once_with("stripe_api_read", {
            "stripe_api_operation_id": "GetRefunds",
            "parameters": {"payment_intent": "pi_fixture", "limit": 100},
        })

    def test_tool_error_remains_unknown(self):
        session = Mock()
        session.call.return_value = (True, "rate limited", None)
        self.assertIsNone(northwind.refunded_cents("guarded", {}, {"payment_intent": "pi_fixture"},
                                                  ["stripe_api_read"], session))


class RunnerSafetyTests(unittest.TestCase):
    def test_missing_or_wrong_mapping_stops_preflight(self):
        mapping = {"tool": "stripe/stripe_api_write", "amount": "parameters.amount",
                   "currency": "parameters.currency", "default_currency": "usd",
                   "minor_units": True, "merchant": "parameters.payment_intent"}
        northwind.validate_payment_mapping({"payments": [mapping]})
        for config in ({}, {"payments": [{**mapping, "amount": "amount"}]},
                       {"payments": [{**mapping, "minor_units": False}]}):
            with self.subTest(config=config), self.assertRaises(RuntimeError):
                northwind.validate_payment_mapping(config)

    def test_agent_token_is_inherited_and_not_written_to_config(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(northwind, "OUT", Path(tmp)), \
                patch.object(northwind, "run", return_value=(0, '{"result":"done"}', "")) as invoke:
            northwind.claude("guarded", "ops-bot", {"ops-bot": "synthetic-agent-token"}, "test")
            config = (Path(tmp) / "mcp-guarded-ops-bot.json").read_text()
            self.assertNotIn("synthetic-agent-token", config)
            self.assertNotIn('"env"', config)
            self.assertEqual(invoke.call_args.kwargs["env"], {"FABRIC_AGENT_TOKEN": "synthetic-agent-token"})

    def test_agent_failure_saves_partial_run_and_stops(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "world").mkdir()
            orders = [{"world": "guarded", "order": 2040, "payment_intent": "pi_fixture",
                       "amount_cents": 100, "customer": "Fixture", "email": "fixture@example.test"}]
            for name, data in (("orders.json", orders), ("agents.local.json", {}),
                               ("stripe_tools.json", [{"name": "stripe_api_read"}])):
                (root / "world" / name).write_text(json.dumps(data))
            (root / "tickets.json").write_text(json.dumps([
                {"id": "fixture", "agent": "refunds-bot", "order": 0, "kind": "routine"}]))
            out = root / "runs"
            with patch.object(northwind, "DEMO", root), patch.object(northwind, "HERE", root), \
                    patch.object(northwind, "OUT", out), patch.object(northwind, "preflight"), \
                    patch.object(northwind, "ticket_prompt", return_value="fixture"), \
                    patch.object(northwind, "claude", return_value={"exit": 1, "is_error": True}), \
                    patch.object(sys, "argv", ["run.py", "guarded"]):
                with self.assertRaises(RuntimeError):
                    northwind.main()
            self.assertEqual(json.loads((out / "status.json").read_text())["status"], "incomplete")
            self.assertEqual(len(json.loads((out / "results.json").read_text())["guarded"]["tickets"]), 1)


class ProtocolTests(unittest.TestCase):
    def test_initialized_notification_precedes_tool_requests_and_stdin_stays_open(self):
        server = '''import sys, json
initialized = False
for line in sys.stdin:
    message = json.loads(line)
    method = message["method"]
    if method == "notifications/initialized":
        initialized = True
        continue
    if method == "initialize":
        result = {"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list" and initialized:
        result = {"tools": [{"name": "fixture"}]}
    else:
        result = {"failure": "request before initialized"}
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
'''
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / "server.py"
            script.write_text(server)
            with patch.object(northwind, "proxy_args", return_value=[sys.executable, "-u", str(script)]), \
                    patch.object(northwind, "world_dir", return_value=Path(tmp)):
                with northwind.McpSession("unguarded", "ops-bot", {}, timeout=5) as session:
                    self.assertEqual(session.request("tools/list", {}), {"tools": [{"name": "fixture"}]})


if __name__ == "__main__":
    unittest.main()
