"""Ledgerly's operations API as an MCP server: the tools a finance-operations agent uses to
read the books and act on them (post entries, record payments, refund, void, reverse).

Standard library only; MCP over stdio (newline-delimited JSON-RPC 2.0).

    python -m ledgerly.mcp_server --db books.db

Amounts are decimal strings in major units (``"125.00"``), never floats. Every write is
attributed to the calling agent (``agent:<client name>``) in ``created_by``.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from datetime import date
from typing import Any, Callable

from .billing import InvoiceError, void_invoice
from .db import connect
from .ledger import JournalError, Line, PeriodError, post, reverse
from .money import Money, MoneyError
from .payments import PaymentError, record_payment, refund
from .reports import is_balanced, trial_balance

PROTOCOL_VERSION = "2025-06-18"


class ToolError(Exception):
    """A problem the agent should see and can act on."""


def _date(text: Any) -> date:
    try:
        return date.fromisoformat(str(text))
    except ValueError as e:
        raise ToolError(f"dates are YYYY-MM-DD: {e}") from e


def _money(text: Any, currency: str) -> Money:
    if not isinstance(text, str):
        raise ToolError("amounts are decimal strings such as \"125.00\", never numbers")
    try:
        return Money.parse(text, currency)
    except (MoneyError, ArithmeticError) as e:
        raise ToolError(str(e)) from e


def _invoice(conn: sqlite3.Connection, number: Any) -> sqlite3.Row:
    row = conn.execute("SELECT * FROM invoices WHERE number = ?", (str(number),)).fetchone()
    if row is None:
        raise ToolError(f"no invoice {number}")
    return row


def _fmt(minor: int, currency: str = "USD") -> str:
    sign = "-" if minor < 0 else ""
    minor = abs(minor)
    return f"{sign}{minor // 100}.{minor % 100:02d} {currency}"


class Books:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self.conn = conn
        self.agent = "agent:unknown"

    # ---- reads ---------------------------------------------------------------------

    def trial_balance(self, args: dict) -> str:
        through = _date(args["through"]) if args.get("through") else None
        rows = trial_balance(self.conn, through=through)
        lines = [f"{r.account} {r.name}: debit {_fmt(r.debit)}, credit {_fmt(r.credit)}" for r in rows]
        lines.append("Balanced." if is_balanced(rows) else "NOT BALANCED.")
        return "\n".join(lines)

    def list_invoices(self, args: dict) -> str:
        sql = "SELECT i.*, c.name AS customer FROM invoices i JOIN customers c ON c.id = i.customer_id"
        params: list[object] = []
        if args.get("status"):
            sql += " WHERE i.status = ?"
            params.append(args["status"])
        rows = self.conn.execute(sql + " ORDER BY i.id", params).fetchall()
        out = []
        for r in rows:
            paid = self.conn.execute(
                "SELECT COALESCE(SUM(amount), 0) AS p FROM payments WHERE invoice_id = ?", (r["id"],)
            ).fetchone()["p"]
            out.append(
                f"{r['number']} {r['customer']} {r['status']} total {_fmt(r['total'], r['currency'])}"
                f" paid {_fmt(paid, r['currency'])} due {r['due_on'] or '-'}"
            )
        return "\n".join(out) or "No invoices."

    def list_payments(self, args: dict) -> str:
        rows = self.conn.execute(
            "SELECT p.*, i.number FROM payments p JOIN invoices i ON i.id = p.invoice_id ORDER BY p.id"
        ).fetchall()
        out = []
        for r in rows:
            refunded = self.conn.execute(
                "SELECT COALESCE(SUM(amount), 0) AS t FROM refunds WHERE payment_id = ?", (r["id"],)
            ).fetchone()["t"]
            out.append(
                f"payment {r['id']} for {r['number']}: {_fmt(r['amount'], r['currency'])}"
                f" on {r['received_on']}, refunded {_fmt(refunded, r['currency'])}"
            )
        return "\n".join(out) or "No payments."

    def list_entries(self, args: dict) -> str:
        limit = int(args.get("limit") or 20)
        rows = self.conn.execute(
            "SELECT * FROM journal_entries ORDER BY id DESC LIMIT ?", (limit,)
        ).fetchall()
        out = []
        for e in rows:
            lines = self.conn.execute(
                "SELECT * FROM journal_lines WHERE entry_id = ? ORDER BY id", (e["id"],)
            ).fetchall()
            detail = ", ".join(
                f"{l['account']} {'Dr' if l['debit'] else 'Cr'} {_fmt(l['debit'] or l['credit'], l['currency'])}"
                for l in lines
            )
            out.append(
                f"entry {e['id']} {e['posted_on']} {e['memo']!r} by {e['created_by']}"
                f"{' (reverses ' + str(e['reverses']) + ')' if e['reverses'] else ''}: {detail}"
            )
        return "\n".join(out) or "No entries."

    # ---- writes --------------------------------------------------------------------

    def post_journal_entry(self, args: dict) -> str:
        currency = args.get("currency") or "USD"
        lines = []
        for item in args.get("lines") or []:
            account = str(item.get("account", ""))
            if item.get("debit") is not None:
                lines.append(Line.dr(account, _money(item["debit"], currency)))
            elif item.get("credit") is not None:
                lines.append(Line.cr(account, _money(item["credit"], currency)))
            else:
                raise ToolError(f"line for {account} needs a debit or a credit")
        entry = post(
            self.conn,
            _date(args.get("date")),
            str(args.get("memo") or ""),
            lines,
            source="manual",
            created_by=self.agent,
        )
        return f"Posted entry {entry}."

    def reverse_entry(self, args: dict) -> str:
        entry = reverse(
            self.conn,
            int(args["entry_id"]),
            _date(args.get("date")),
            reason=str(args.get("reason") or ""),
            created_by=self.agent,
        )
        return f"Posted reversing entry {entry} for entry {args['entry_id']}."

    def record_payment(self, args: dict) -> str:
        invoice = _invoice(self.conn, args.get("invoice"))
        payment = record_payment(
            self.conn,
            invoice["id"],
            _money(args.get("amount"), invoice["currency"]),
            idempotency_key=str(args.get("reference") or ""),
            received_on=_date(args.get("date")),
            fee=_money(args["fee"], invoice["currency"]) if args.get("fee") else None,
            created_by=self.agent,
        )
        return f"Recorded payment {payment} against {invoice['number']}."

    def issue_refund(self, args: dict) -> str:
        payment = self.conn.execute(
            "SELECT * FROM payments WHERE id = ?", (int(args["payment_id"]),)
        ).fetchone()
        if payment is None:
            raise ToolError(f"no payment {args['payment_id']}")
        refund_id = refund(
            self.conn,
            payment["id"],
            _money(args.get("amount"), payment["currency"]),
            reason=str(args.get("reason") or ""),
            on=_date(args.get("date")),
            created_by=self.agent,
        )
        return f"Recorded refund {refund_id} against payment {payment['id']}."

    def void_invoice(self, args: dict) -> str:
        invoice = _invoice(self.conn, args.get("invoice"))
        reversal = void_invoice(
            self.conn,
            invoice["id"],
            _date(args.get("date")),
            reason=str(args.get("reason") or ""),
            created_by=self.agent,
        )
        return f"Voided {invoice['number']}" + (f"; reversing entry {reversal}." if reversal else ".")


DATE = {"type": "string", "description": "YYYY-MM-DD"}
AMOUNT = {"type": "string", "description": "Decimal amount in major units, e.g. \"125.00\""}

TOOLS: list[tuple[str, str, dict, list[str]]] = [
    ("trial_balance", "Trial balance of the books, optionally through a date.",
     {"through": DATE}, []),
    ("list_invoices", "Invoices with customer, status, total, amount paid and due date.",
     {"status": {"type": "string", "enum": ["draft", "issued", "paid", "void"]}}, []),
    ("list_payments", "Payments with their invoice and amounts refunded.", {}, []),
    ("list_entries", "The most recent journal entries with their lines.",
     {"limit": {"type": "integer"}}, []),
    ("post_journal_entry", "Post a balanced manual journal entry into an open period.",
     {"date": DATE, "memo": {"type": "string"}, "currency": {"type": "string"},
      "lines": {"type": "array", "items": {"type": "object", "properties": {
          "account": {"type": "string"}, "debit": AMOUNT, "credit": AMOUNT}}}},
     ["date", "memo", "lines"]),
    ("reverse_entry", "Post the mirror image of an entry; the original stays unchanged.",
     {"entry_id": {"type": "integer"}, "date": DATE, "reason": {"type": "string"}},
     ["entry_id", "date", "reason"]),
    ("record_payment", "Record a customer payment against an invoice.",
     {"invoice": {"type": "string"}, "amount": AMOUNT, "date": DATE,
      "reference": {"type": "string", "description": "Bank reference; makes the call idempotent"},
      "fee": AMOUNT},
     ["invoice", "amount", "date", "reference"]),
    ("issue_refund", "Refund part or all of a payment.",
     {"payment_id": {"type": "integer"}, "amount": AMOUNT, "date": DATE, "reason": {"type": "string"}},
     ["payment_id", "amount", "date", "reason"]),
    ("void_invoice", "Void an unpaid invoice; an issued one is reversed in the ledger.",
     {"invoice": {"type": "string"}, "date": DATE, "reason": {"type": "string"}},
     ["invoice", "date", "reason"]),
]

BOOK_ERRORS = (JournalError, PeriodError, PaymentError, InvoiceError, MoneyError, ToolError,
               KeyError, ValueError, sqlite3.Error)


def tool_list() -> list[dict]:
    return [
        {"name": name, "description": description,
         "inputSchema": {"type": "object", "properties": props, "required": required}}
        for name, description, props, required in TOOLS
    ]


def handle(books: Books, message: dict) -> dict | None:
    method = message.get("method")
    if "id" not in message:
        return None  # a notification
    params = message.get("params") or {}
    if method == "initialize":
        client = (params.get("clientInfo") or {}).get("name") or "unknown"
        books.agent = f"agent:{client}"
        result: dict = {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "ledgerly", "version": "1.0"},
        }
    elif method == "tools/list":
        result = {"tools": tool_list()}
    elif method == "tools/call":
        name = params.get("name")
        fn: Callable[[dict], str] | None = getattr(books, name, None) if name in {t[0] for t in TOOLS} else None
        if fn is None:
            result = {"content": [{"type": "text", "text": f"unknown tool {name!r}"}], "isError": True}
        else:
            try:
                text = fn(params.get("arguments") or {})
                result = {"content": [{"type": "text", "text": text}]}
            except BOOK_ERRORS as e:
                books.conn.rollback()
                result = {"content": [{"type": "text", "text": f"{type(e).__name__}: {e}"}], "isError": True}
    elif method == "ping":
        result = {}
    else:
        return {"jsonrpc": "2.0", "id": message["id"],
                "error": {"code": -32601, "message": f"unknown method {method}"}}
    return {"jsonrpc": "2.0", "id": message["id"], "result": result}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--db", default="books.db")
    books = Books(connect(parser.parse_args().db))
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        response = handle(books, message)
        if response is not None:
            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
