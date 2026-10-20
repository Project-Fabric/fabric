import unittest
from datetime import date

from ledgerly.billing import (
    InvoiceError,
    add_customer,
    add_line,
    apply_discount_bp,
    create_invoice,
    get_invoice,
    issue_invoice,
    void_invoice,
)
from ledgerly.ledger import balance
from ledgerly.reports import is_balanced, trial_balance

from helpers import USER, books, issued_invoice, usd


class InvoiceTest(unittest.TestCase):
    def setUp(self):
        self.conn = books()
        self.addCleanup(self.conn.close)

    def test_tax_and_discount(self):
        customer = add_customer(self.conn, "Acme", "US", "CA")
        invoice = create_invoice(self.conn, customer, "INV-7")
        add_line(self.conn, invoice, "Seats", 3, usd("33.33"))
        apply_discount_bp(self.conn, invoice, 1000)
        row = get_invoice(self.conn, invoice)
        self.assertEqual(row["subtotal"], 9999)
        self.assertEqual(row["discount"], 1000)
        # 7.25% of 89.99 = 6.524275 -> 6.52
        self.assertEqual(row["tax"], 652)
        self.assertEqual(row["total"], 8999 + 652)

    def test_issue_posts_balanced_entry(self):
        customer = add_customer(self.conn, "Acme", "US", "NY")
        invoice = create_invoice(self.conn, customer, "INV-8")
        add_line(self.conn, invoice, "Setup", 1, usd("200.00"))
        apply_discount_bp(self.conn, invoice, 500)
        issue_invoice(self.conn, invoice, date(2026, 4, 1), created_by=USER)
        self.assertEqual(balance(self.conn, "1100"), usd("197.60"))
        self.assertEqual(balance(self.conn, "4100"), usd("200.00"))
        self.assertEqual(balance(self.conn, "2000"), usd("7.60"))
        self.assertTrue(is_balanced(trial_balance(self.conn)))
        self.assertEqual(get_invoice(self.conn, invoice)["due_on"], "2026-05-01")

    def test_issued_invoice_cannot_change(self):
        invoice = issued_invoice(self.conn)
        with self.assertRaises(InvoiceError):
            add_line(self.conn, invoice, "More", 1, usd("1.00"))
        with self.assertRaises(InvoiceError):
            apply_discount_bp(self.conn, invoice, 100)

    def test_void_reverses(self):
        invoice = issued_invoice(self.conn)
        void_invoice(self.conn, invoice, date(2026, 3, 12), reason="duplicate", created_by=USER)
        self.assertEqual(balance(self.conn, "1100"), usd("0.00"))
        self.assertEqual(get_invoice(self.conn, invoice)["status"], "void")

    def test_subscription_lines_are_deferred(self):
        customer = add_customer(self.conn, "Acme", "US", "OR")
        invoice = create_invoice(self.conn, customer, "INV-9")
        add_line(
            self.conn,
            invoice,
            "Annual plan",
            1,
            usd("1200.00"),
            "4000",
            service_start=date(2026, 1, 1),
            service_end=date(2026, 12, 31),
        )
        issue_invoice(self.conn, invoice, date(2026, 1, 1), created_by=USER)
        self.assertEqual(balance(self.conn, "2100"), usd("1200.00"))
        self.assertEqual(balance(self.conn, "4000"), usd("0.00"))
