import unittest

import money


class MoneyTest(unittest.TestCase):
    def test_order_total(self):
        self.assertEqual(money.order_total([("coffee", 2), ("bagel", 1)]), 1225)
