import unittest

import cart


class HiddenTask(unittest.TestCase):
    def test_checkout_applies_dollar_promo(self):
        self.assertEqual(cart.checkout([(100.0, 1)], 10), 90.0)
        self.assertEqual(cart.checkout([(20.0, 2)], 5), 35.0)
