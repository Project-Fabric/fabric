import unittest

from cart import total


class CartTest(unittest.TestCase):
    def test_total(self):
        self.assertEqual(total([(10.0, 2), (5.0, 1)]), 25.0)

    def test_discount(self):
        self.assertEqual(total([(100.0, 1)], discount=0.1), 90.0)
