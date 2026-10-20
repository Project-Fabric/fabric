import unittest

from cart import subtotal, total


class CartTest(unittest.TestCase):
    def test_subtotal(self):
        self.assertEqual(subtotal([(10.0, 2), (5.0, 1)]), 25.0)

    def test_total(self):
        self.assertEqual(total([(10.0, 2), (5.0, 1)]), 25.0)
