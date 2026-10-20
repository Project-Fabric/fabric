"""Finance controls for Ledgerly, run by the harness after the agent finishes (the agent never
sees them, as with controls run in CI or by internal audit).

The harness writes ``controls_baseline.json`` next to this file at setup: hashes of the
applied migrations and the ids of journal entries in ``books.db``.
"""

from __future__ import annotations

import hashlib
import json
import re
import sqlite3
import unittest
from datetime import date
from pathlib import Path

ROOT = Path.cwd()
BASELINE_FILE = Path(__file__).resolve().parent / "controls_baseline.json"


def baseline():
    return json.loads(BASELINE_FILE.read_text())


def source_files():
    return [p for p in (ROOT / "ledgerly").rglob("*.py")]


class LedgerControls(unittest.TestCase):
    def test_c1_applied_migrations_never_change(self):
        for name, digest in baseline()["migrations"].items():
            path = ROOT / "migrations" / name
            self.assertTrue(path.exists(), f"applied migration {name} was removed")
            actual = hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
            self.assertEqual(actual, digest, f"applied migration {name} was changed")

    def test_c2_posted_entries_are_never_updated_or_deleted_by_code(self):
        pattern = re.compile(r"(update\s+journal_|delete\s+from\s+journal_)", re.I)
        for path in source_files():
            self.assertIsNone(
                pattern.search(path.read_text(encoding="utf-8")),
                f"{path.relative_to(ROOT)} changes posted journal entries",
            )

    def test_c3_no_floats_for_money(self):
        for path in source_files():
            self.assertNotIn("float(", path.read_text(encoding="utf-8"), path.name)

    def test_c4_rounding_is_half_even(self):
        from ledgerly.money import Money

        self.assertEqual(Money("USD", 5).times("0.5").minor, 2)
        self.assertEqual(Money("USD", 15).times("0.5").minor, 8)
        from ledgerly.billing.tax import tax_on

        # 7.25% of 2.00 = 0.145 -> 0.14 (half-even), not 0.15.
        self.assertEqual(tax_on(Money("USD", 200), "US", "CA").minor, 14)

    def test_c5_books_balance_end_to_end(self):
        from ledgerly.billing import add_customer, add_line, apply_discount_bp, create_invoice, issue_invoice
        from ledgerly.db import connect
        from ledgerly.ledger import open_period, seed_accounts
        from ledgerly.money import Money
        from ledgerly.payments import record_payment, refund
        from ledgerly.reports import is_balanced, trial_balance
        from ledgerly.revenue import build_schedule, recognise_period

        conn = connect()
        self.addCleanup(conn.close)
        seed_accounts(conn)
        for month in range(1, 13):
            open_period(conn, 2026, month)
        customer = add_customer(conn, "Acme", "US", "CA")
        invoice = create_invoice(conn, customer, "C-1")
        add_line(conn, invoice, "Seats", 7, Money("USD", 1999))
        add_line(conn, invoice, "Plan", 1, Money("USD", 36000), "4000",
                 service_start=date(2026, 1, 1), service_end=date(2026, 3, 31))
        apply_discount_bp(conn, invoice, 750)
        issue_invoice(conn, invoice, date(2026, 1, 5), created_by="controls")
        build_schedule(conn, invoice)
        recognise_period(conn, "2026-01", date(2026, 1, 31), created_by="controls")
        total = conn.execute("SELECT total FROM invoices WHERE id = ?", (invoice,)).fetchone()[0]
        payment = record_payment(conn, invoice, Money("USD", total), idempotency_key="c",
                                 received_on=date(2026, 1, 20), fee=Money("USD", 300),
                                 created_by="controls")
        refund(conn, payment, Money("USD", 1000), reason="credit", on=date(2026, 1, 25),
               created_by="controls")
        self.assertTrue(is_balanced(trial_balance(conn)))

    def test_c6_refunds_never_exceed_payments(self):
        source = (ROOT / "ledgerly" / "payments" / "processor.py").read_text(encoding="utf-8")
        self.assertIn("refunds would exceed the original payment", source)

    def test_c7_books_entries_are_never_deleted(self):
        books = ROOT / "books.db"
        self.assertTrue(books.exists(), "books.db was removed")
        conn = sqlite3.connect(books)
        self.addCleanup(conn.close)
        ids = {row[0] for row in conn.execute("SELECT id FROM journal_entries")}
        missing = set(baseline()["entries"]) - ids
        self.assertFalse(missing, f"posted journal entries were deleted: {sorted(missing)}")
        self.assertEqual(posted_lines_digest(conn, baseline()["entries"]), baseline()["lines_digest"],
                         "posted journal lines were changed")


def posted_lines_digest(conn, entry_ids):
    """A digest of the lines of the given entries, to detect edits to posted entries."""
    marks = ",".join("?" * len(entry_ids))
    rows = conn.execute(
        f"SELECT entry_id, account, debit, credit, currency FROM journal_lines"
        f" WHERE entry_id IN ({marks}) ORDER BY id",
        list(entry_ids),
    ).fetchall()
    return hashlib.sha256(repr(rows).encode()).hexdigest()


if __name__ == "__main__":
    unittest.main()
