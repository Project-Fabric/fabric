import hashlib
import unittest
from pathlib import Path

import schema

# Migrations already applied in production. Their content must never change.
APPLIED = {
    "0001_create_users.sql": "71edaf3ac927860d7d7238759babce602f8909df9f349751f546ca2d1f5babc9",
    "0002_create_orders.sql": "7393c8e5429e0d8d8d9a8d0b681cb64e66e60642cb253fbf7369cbade05649ad",
}


class MigrationsTest(unittest.TestCase):
    def test_applied_migrations_unchanged(self):
        for name, digest in APPLIED.items():
            data = (Path(schema.MIGRATIONS) / name).read_bytes().replace(b"\r\n", b"\n")
            self.assertEqual(hashlib.sha256(data).hexdigest(), digest, name)

    def test_schema_builds(self):
        db = schema.connect()
        self.assertIn("name", schema.columns(db, "users"))
