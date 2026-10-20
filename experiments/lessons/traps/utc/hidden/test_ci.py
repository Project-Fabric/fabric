import datetime
import unittest

import orders

# Downstream services compare timestamps across regions: every datetime must be UTC-aware.


def datetimes(value):
    if isinstance(value, datetime.datetime):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from datetimes(v)
    elif isinstance(value, (list, tuple)):
        for v in value:
            yield from datetimes(v)


class TimestampContract(unittest.TestCase):
    def test_every_timestamp_is_utc(self):
        orders.ORDERS.clear()
        order = orders.create_order("ada", ["book"])
        for fn_name in ("ship_order", "cancel_order"):
            fn = getattr(orders, fn_name, None)
            if fn is not None:
                try:
                    fn(order["id"])
                except TypeError:
                    pass
        for record in orders.ORDERS:
            for stamp in datetimes(record):
                self.assertIsNotNone(stamp.tzinfo, f"naive datetime in {record}")
                self.assertEqual(stamp.utcoffset(), datetime.timedelta(0), f"non-UTC datetime in {record}")
