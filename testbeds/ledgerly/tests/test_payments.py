import unittest
from datetime import date

from ledgerly.billing import get_invoice
from ledgerly.ledger import balance
from ledgerly.payments import PaymentError, record_payment, refund, settle
from ledgerly.reports import is_balanced, trial_balance

from helpers import USER, books, issued_invoice, usd


class PaymentTest(unittest.TestCase):
    def setUp(self):
        self.conn = books()
        self.addCleanup(self.conn.close)
        self.invoice = issued_invoice(self.conn, "100.00")

    def pay(self, amount="100.00", key="pay-1", fee="3.20"):
        return record_payment(
            self.conn,
            self.invoice,
            usd(amount),
            idempotency_key=key,
            received_on=date(2026, 3, 15),
            fee=usd(fee),
            created_by=USER,
        )

    def test_full_payment(self):
        self.pay()
        self.assertEqual(get_invoice(self.conn, self.invoice)["status"], "paid")
        self.assertEqual(balance(self.conn, "1100"), usd("0.00"))
        self.assertEqual(balance(self.conn, "1200"), usd("96.80"))
        self.assertEqual(balance(self.conn, "6000"), usd("3.20"))
        self.assertTrue(is_balanced(trial_balance(self.conn)))

    def test_idempotent_retry(self):
        first = self.pay("40.00")
        self.assertEqual(self.pay("40.00"), first)
        with self.assertRaises(PaymentError):
            self.pay("41.00")
        self.assertEqual(balance(self.conn, "1100"), usd("60.00"))

    def test_overpayment_is_refused(self):
        with self.assertRaises(PaymentError):
            self.pay("100.01")

    def test_refunds_cannot_exceed_payment(self):
        payment = self.pay()
        refund(self.conn, payment, usd("60.00"), reason="partial", on=date(2026, 3, 20), created_by=USER)
        with self.assertRaises(PaymentError):
            refund(self.conn, payment, usd("40.01"), reason="more", on=date(2026, 3, 21), created_by=USER)
        with self.assertRaises(PaymentError):
            refund(self.conn, payment, usd("1.00"), reason=" ", on=date(2026, 3, 21), created_by=USER)

    def test_settlement(self):
        self.pay()
        settle(self.conn, usd("96.80"), date(2026, 3, 17), created_by=USER)
        self.assertEqual(balance(self.conn, "1000"), usd("96.80"))
        self.assertEqual(balance(self.conn, "1200"), usd("0.00"))
