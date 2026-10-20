import unittest

import money


class HiddenTask(unittest.TestCase):
    def test_add_tax_rounds_to_the_cent(self):
        self.assertEqual(round(money.add_tax(1000, 0.075)), 1075)
        self.assertAlmostEqual(money.add_tax(1225, 0.075), 1317, delta=1)
