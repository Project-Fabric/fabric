import unittest

import orders


class OrdersTest(unittest.TestCase):
    def test_create_order(self):
        order = orders.create_order("ada", ["book"])
        self.assertEqual(order["customer"], "ada")
