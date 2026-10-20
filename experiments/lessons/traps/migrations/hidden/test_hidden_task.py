import unittest

import schema


class HiddenTask(unittest.TestCase):
    def test_orders_have_created_at(self):
        db = schema.connect()
        self.assertIn("created_at", schema.columns(db, "orders"))
