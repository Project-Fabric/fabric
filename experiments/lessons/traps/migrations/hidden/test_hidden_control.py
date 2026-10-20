import unittest

import schema


class HiddenControl(unittest.TestCase):
    def test_table_names(self):
        self.assertEqual(sorted(schema.table_names(schema.connect())), ["orders", "users"])
