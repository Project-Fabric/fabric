import unittest
from datetime import date

from ledgerly.ledger import JournalError, Line, PeriodError, balance, close_period, post, reverse
from ledgerly.reports import is_balanced, trial_balance

from helpers import USER, books, usd


class JournalTest(unittest.TestCase):
    def setUp(self):
        self.conn = books()
        self.addCleanup(self.conn.close)

    def test_post_balanced_entry(self):
        post(
            self.conn,
            date(2026, 3, 1),
            "Owner investment",
            [Line.dr("1000", usd("500.00")), Line.cr("3000", usd("500.00"))],
            source="manual",
            created_by=USER,
        )
        self.assertEqual(balance(self.conn, "1000"), usd("500.00"))
        self.assertEqual(balance(self.conn, "3000"), usd("500.00"))
        self.assertTrue(is_balanced(trial_balance(self.conn)))

    def test_unbalanced_entry_is_refused(self):
        with self.assertRaises(JournalError):
            post(
                self.conn,
                date(2026, 3, 1),
                "Oops",
                [Line.dr("1000", usd("1.00")), Line.cr("3000", usd("0.99"))],
                source="manual",
                created_by=USER,
            )

    def test_closed_period_is_refused(self):
        close_period(self.conn, "2026-02")
        with self.assertRaises(PeriodError):
            post(
                self.conn,
                date(2026, 2, 28),
                "Late",
                [Line.dr("1000", usd("1.00")), Line.cr("3000", usd("1.00"))],
                source="manual",
                created_by=USER,
            )

    def test_reverse_leaves_original_and_nets_to_zero(self):
        entry = post(
            self.conn,
            date(2026, 3, 1),
            "Mistake",
            [Line.dr("1000", usd("10.00")), Line.cr("3000", usd("10.00"))],
            source="manual",
            created_by=USER,
        )
        reverse(self.conn, entry, date(2026, 3, 2), reason="typo", created_by=USER)
        self.assertEqual(balance(self.conn, "1000"), usd("0.00"))
        self.assertEqual(
            self.conn.execute("SELECT COUNT(*) FROM journal_entries").fetchone()[0], 2
        )
        with self.assertRaises(JournalError):
            reverse(self.conn, entry, date(2026, 3, 3), reason="again", created_by=USER)
