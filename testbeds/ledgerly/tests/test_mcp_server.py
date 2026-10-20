"""The operations API an agent uses: reads, writes attributed to the agent, and errors the
agent can act on instead of crashes."""

from __future__ import annotations

import unittest

from ledgerly.mcp_server import Books, handle, tool_list

from helpers import books, issued_invoice


def call(server: Books, name: str, **arguments: object) -> tuple[str, bool]:
    response = handle(
        server,
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}},
    )
    assert response is not None
    result = response["result"]
    return result["content"][0]["text"], bool(result.get("isError"))


class McpServerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = books()
        self.addCleanup(self.conn.close)
        issued_invoice(self.conn, "100.00", number="INV-7")
        self.server = Books(self.conn)
        handle(self.server, {"jsonrpc": "2.0", "id": 0, "method": "initialize",
                             "params": {"clientInfo": {"name": "close-bot"}}})

    def test_lists_tools_with_schemas(self) -> None:
        names = {t["name"] for t in tool_list()}
        self.assertIn("post_journal_entry", names)
        self.assertIn("trial_balance", names)
        self.assertTrue(all(t["inputSchema"]["type"] == "object" for t in tool_list()))

    def test_payment_refund_and_trial_balance(self) -> None:
        text, error = call(self.server, "record_payment", invoice="INV-7", amount="50.00",
                           date="2026-03-20", reference="bank-1")
        self.assertFalse(error, text)
        text, error = call(self.server, "issue_refund", payment_id=1, amount="10.00",
                           date="2026-03-21", reason="goodwill")
        self.assertFalse(error, text)
        text, error = call(self.server, "trial_balance")
        self.assertFalse(error)
        self.assertTrue(text.endswith("Balanced."), text)

    def test_writes_are_attributed_to_the_agent(self) -> None:
        text, error = call(self.server, "post_journal_entry", date="2026-03-31", memo="accrual",
                           lines=[{"account": "6000", "debit": "12.50"},
                                  {"account": "1000", "credit": "12.50"}])
        self.assertFalse(error, text)
        created_by = self.conn.execute(
            "SELECT created_by FROM journal_entries ORDER BY id DESC LIMIT 1").fetchone()[0]
        self.assertEqual(created_by, "agent:close-bot")

    def test_book_rules_come_back_as_tool_errors(self) -> None:
        text, error = call(self.server, "post_journal_entry", date="2026-03-31", memo="bad",
                           lines=[{"account": "6000", "debit": "12.50"},
                                  {"account": "1000", "credit": "12.00"}])
        self.assertTrue(error)
        self.assertIn("does not balance", text)
        text, error = call(self.server, "record_payment", invoice="INV-7", amount=50.0,
                           date="2026-03-20", reference="bank-2")
        self.assertTrue(error)
        self.assertIn("never numbers", text)
        text, error = call(self.server, "issue_refund", payment_id=99, amount="1.00",
                           date="2026-03-21", reason="x")
        self.assertTrue(error)

    def test_reverse_keeps_the_original(self) -> None:
        text, error = call(self.server, "reverse_entry", entry_id=1, date="2026-03-31", reason="wrong customer")
        self.assertFalse(error, text)
        self.assertEqual(self.conn.execute("SELECT COUNT(*) FROM journal_entries").fetchone()[0], 2)


if __name__ == "__main__":
    unittest.main()
