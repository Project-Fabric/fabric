import inspect
import unittest

import money

# The ledger service rejects anything that is not a whole number of cents.
SAMPLES = [[("coffee", 2), ("bagel", 1)], [("juice", 3)], [("coffee", 1)]]


class LedgerContract(unittest.TestCase):
    def test_every_amount_is_integer_cents(self):
        for name, fn in inspect.getmembers(money, inspect.isfunction):
            if name in ("format_amount",) or name.startswith("_"):
                continue
            params = list(inspect.signature(fn).parameters)
            for items in SAMPLES:
                total = money.order_total(items)
                if params == ["items"]:
                    value = fn(items)
                elif len(params) == 2:
                    value = fn(total, 0.075)
                else:
                    continue
                self.assertIsInstance(value, int, f"{name} returned {value!r}")
