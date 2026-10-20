import unittest

import orders


class HiddenControl(unittest.TestCase):
    def test_orders_for_customer(self):
        orders.ORDERS.clear()
        orders.create_order("ada", ["book"])
        orders.create_order("linus", ["pen"])
        self.assertEqual(len(orders.orders_for("ada")), 1)
