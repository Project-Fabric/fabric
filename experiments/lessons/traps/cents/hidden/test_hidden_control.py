import unittest

import money


class HiddenControl(unittest.TestCase):
    def test_product_names(self):
        self.assertEqual(sorted(money.product_names()), ["bagel", "coffee", "juice"])
