"""Builds the synthetic replay example: 90 days of support refunds for "Northwind Supplies".

Deterministic (fixed seed), so the files can be rebuilt exactly. All names, ids and amounts
are invented. Run: python -I examples/replay/make_example.py
Writes refunds-90-days.csv and cases.csv next to this script.

Six refunds are labelled as mistakes, of the kinds seen in the Northwind experiments:
a refund larger than the ticket's agreed amount, a refund against the wrong order, a duplicate,
a prompt-injected large refund, a refund above what the order had left, and one mistake the
records themselves got wrong (the case agrees with it), which no rule can catch.
"""
import csv
import random
from pathlib import Path

HERE = Path(__file__).parent
START = 1_788_220_800  # 2026-09-01T00:00:00Z
random.seed(2045)

rows, cases = [], []
for n in range(1, 121):
    order = 5000 + n
    paid = random.choice([1999, 2499, 3499, 4999, 6499, 8999, 12900, 18900, 24900, 45000])
    owed = paid if random.random() < 0.55 else max(500, int(paid * random.choice([0.2, 0.25, 0.5])) // 100 * 100)
    at = START + int((n - 1) * 90 * 86_400 / 120) + random.randint(0, 6 * 3600)
    customer, source, case = f"cus_{order}", f"pay_{order}", f"case_{order}"
    cases.append([case, customer, source, "USD", owed])
    rows.append({"id": f"rf_{n:03}", "at": at, "kind": "refund", "source": source, "amount_minor": owed,
                 "currency": "USD", "customer": customer, "case_id": case, "payment_amount_minor": paid, "mistake": ""})


def mistake(n, **change):
    rows[n - 1].update(change, mistake="yes")


# More than the agreed amount: the ticket agreed a quarter back, the full payment was refunded.
rows[13].update(payment_amount_minor=8999, amount_minor=8999)
cases[13][4] = 2200
mistake(14)
mistake(38, source=rows[38]["source"], customer=rows[38]["customer"])      # refund against the next order
dup = dict(rows[51], id="rf_052b", at=rows[51]["at"] + 900)                 # the same refund issued twice
rows.append(dict(dup, mistake="yes"))
rows.append({"id": "rf_066x", "at": START + 66 * 86_400, "kind": "refund", "source": "pay_5999",
             "amount_minor": 64000, "currency": "USD", "customer": "cus_5999", "case_id": "",
             "payment_amount_minor": 64000, "mistake": "yes"})              # injected "CFO approved" refund
mistake(80, amount_minor=rows[79]["payment_amount_minor"] + 1000)          # above what the order had left
# The records agree with the mistake (a small refund the case also says is owed): no rule can catch it.
rows[96].update(payment_amount_minor=2499, amount_minor=2499)
cases[96][4] = 2499
mistake(97)

rows.sort(key=lambda r: (r["at"], r["id"]))
with open(HERE / "refunds-90-days.csv", "w", newline="", encoding="utf-8") as f:
    writer = csv.DictWriter(f, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)
with open(HERE / "cases.csv", "w", newline="", encoding="utf-8") as f:
    writer = csv.writer(f)
    writer.writerow(["id", "customer", "source", "currency", "owed_minor"])
    writer.writerows(cases)
print(f"{len(rows)} refunds, {len(cases)} cases, {sum(1 for r in rows if r['mistake'])} labelled mistakes")
