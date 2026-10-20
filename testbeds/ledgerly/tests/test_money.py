import unittest
from decimal import Decimal

from ledgerly.fx import convert
from ledgerly.money import Money, MoneyError


class MoneyTest(unittest.TestCase):
    def test_parse_and_format(self):
        self.assertEqual(Money.parse("12.34", "USD"), Money("USD", 1234))
        self.assertEqual(Money("USD", -123456).format(), "-1,234.56 USD")
        self.assertEqual(Money("JPY", 1500).format(), "1,500 JPY")
        with self.assertRaises(MoneyError):
            Money.parse("1.234", "USD")

    def test_amounts_are_integers(self):
        with self.assertRaises(MoneyError):
            Money("USD", 1.5)  # type: ignore[arg-type]

    def test_currencies_do_not_mix(self):
        with self.assertRaises(MoneyError):
            Money("USD", 1) + Money("EUR", 1)

    def test_times_rounds_half_even(self):
        self.assertEqual(Money("USD", 5).times("0.5"), Money("USD", 2))
        self.assertEqual(Money("USD", 15).times("0.5"), Money("USD", 8))

    def test_allocate_sums_to_whole(self):
        parts = Money("USD", 100).allocate([1, 1, 1])
        self.assertEqual([p.minor for p in parts], [34, 33, 33])
        self.assertEqual(sum(p.minor for p in Money("USD", 1001).allocate([31, 28, 31])), 1001)


class FxTest(unittest.TestCase):
    def test_convert(self):
        self.assertEqual(convert(Money("EUR", 10000), "USD", "1.0825"), Money("USD", 10825))
        self.assertEqual(convert(Money("USD", 10000), "JPY", Decimal("149.5")), Money("JPY", 14950))
        self.assertEqual(convert(Money("JPY", 1000), "USD", "0.0067"), Money("USD", 670))

    def test_rejects_bad_rate(self):
        with self.assertRaises(ValueError):
            convert(Money("USD", 1), "EUR", "0")
