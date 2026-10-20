import unittest
from datetime import date

from ledgerly.billing import add_customer, add_line, create_invoice, issue_invoice
from ledgerly.db import MigrationError, connect, migrate
from ledgerly.ledger import balance
from ledgerly.payments import record_payment, settle
from ledgerly.reconciliation import StatementLine, reconcile
from ledgerly.reports import ar_aging, income_statement
from ledgerly.revenue import build_schedule, recognise_period

from helpers import USER, books, issued_invoice, usd


class RecognitionTest(unittest.TestCase):
    def setUp(self):
        self.conn = books()
        self.addCleanup(self.conn.close)
        customer = add_customer(self.conn, "Acme", "US", "OR")
        self.invoice = create_invoice(self.conn, customer, "INV-Y")
        add_line(
            self.conn,
            self.invoice,
            "Q1 plan",
            1,
            usd("900.00"),
            "4000",
            service_start=date(2026, 1, 1),
            service_end=date(2026, 3, 31),
        )
        issue_invoice(self.conn, self.invoice, date(2026, 1, 1), created_by=USER)

    def test_schedule_sums_to_line(self):
        build_schedule(self.conn, self.invoice)
        amounts = [r["amount"] for r in self.conn.execute("SELECT amount FROM revenue_schedule ORDER BY id")]
        self.assertEqual(sum(amounts), 90000)
        self.assertEqual(len(amounts), 3)
        # 31, 28 and 31 days of 90.
        self.assertEqual(amounts, [31000, 28000, 31000])

    def test_recognise_once(self):
        build_schedule(self.conn, self.invoice)
        recognise_period(self.conn, "2026-01", date(2026, 1, 31), created_by=USER)
        recognise_period(self.conn, "2026-01", date(2026, 1, 31), created_by=USER)
        self.assertEqual(balance(self.conn, "4000"), usd("310.00"))
        self.assertEqual(balance(self.conn, "2100"), usd("590.00"))
        self.assertEqual(income_statement(self.conn, "2026-01")["revenue"], 31000)


class ReportsTest(unittest.TestCase):
    def test_aging(self):
        conn = books()
        self.addCleanup(conn.close)
        issued_invoice(conn, "50.00", number="A", on=date(2026, 1, 1))
        issued_invoice(conn, "70.00", number="B", on=date(2026, 3, 1))
        aging = ar_aging(conn, date(2026, 3, 15))
        # A was due 31 January: 43 days late.
        self.assertEqual(aging["31-60"], 5000)
        self.assertEqual(aging["current"], 7000)

    def test_reconciliation(self):
        conn = books()
        self.addCleanup(conn.close)
        invoice = issued_invoice(conn, "100.00")
        record_payment(
            conn, invoice, usd("100.00"), idempotency_key="k", received_on=date(2026, 3, 15),
            created_by=USER,
        )
        settle(conn, usd("100.00"), date(2026, 3, 17), created_by=USER)
        result = reconcile(
            conn,
            [StatementLine(date(2026, 3, 18), 10000, "PAYOUT 1"), StatementLine(date(2026, 3, 19), 500, "?")],
            date(2026, 3, 1),
            date(2026, 3, 31),
        )
        self.assertEqual(len(result.matched), 1)
        self.assertEqual(len(result.unmatched_statement), 1)
        self.assertFalse(result.complete)


class MigrationTest(unittest.TestCase):
    def test_changed_migration_is_refused(self):
        import shutil
        import tempfile
        from pathlib import Path

        from ledgerly.db import MIGRATIONS

        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "migrations"
            shutil.copytree(MIGRATIONS, copy)
            conn = connect()
            self.addCleanup(conn.close)
            migrate(conn, copy)
            first = sorted(copy.glob("*.sql"))[0]
            first.write_text(first.read_text() + "\n-- edited\n")
            with self.assertRaises(MigrationError):
                migrate(conn, copy)
