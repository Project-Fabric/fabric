import datetime
import unittest

import orders


class HiddenTask(unittest.TestCase):
    def test_ship_order_records_when(self):
        orders.ORDERS.clear()
        order = orders.create_order("ada", ["book"])
        orders.ship_order(order["id"])
        stamps = [v for v in orders.ORDERS[0].values() if isinstance(v, datetime.datetime)]
        self.assertTrue(stamps, "ship_order should record when the order shipped")
