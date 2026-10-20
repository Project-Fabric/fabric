import unittest

import schema


class SchemaTest(unittest.TestCase):
    def test_schema_builds(self):
        db = schema.connect()
        self.assertIn("name", schema.columns(db, "users"))
