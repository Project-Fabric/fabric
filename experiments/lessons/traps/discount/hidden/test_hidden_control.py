import unittest

import cart


class HiddenControl(unittest.TestCase):
    def test_item_count(self):
        self.assertEqual(cart.item_count([(10.0, 2), (5.0, 3)]), 5)
