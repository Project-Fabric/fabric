"""Builds a demo books database: a quarter of invoices, payments, a refund and recognised
subscription revenue, plus one test invoice someone posted by mistake.

    python tools/seed_demo.py books.db
"""

from __future__ import annotations

import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ledgerly.billing import add_customer, add_line, apply_discount_bp, create_invoice, issue_invoice  # noqa: E402
from ledgerly.db import connect  # noqa: E402
from ledgerly.ledger import close_period, open_period, seed_accounts  # noqa: E402
from ledgerly.money import Money  # noqa: E402
from ledgerly.payments import record_payment, refund, settle  # noqa: E402
from ledgerly.revenue import build_schedule, recognise_period  # noqa: E402

USER = "seed@ledgerly.example"


def usd(text: str) -> Money:
    return Money.parse(text, "USD")


def main(path: str) -> None:
    if Path(path).exists():
        raise SystemExit(f"{path} already exists")
    conn = connect(path)
    seed_accounts(conn)
    for month in range(1, 13):
        open_period(conn, 2026, month)

    acme = add_customer(conn, "Acme Corp", "US", "CA")
    globex = add_customer(conn, "Globex", "US", "NY")
    initech = add_customer(conn, "Initech", "US", "TX")

    annual = create_invoice(conn, acme, "INV-1001")
    add_line(conn, annual, "Annual plan", 1, usd("12000.00"), "4000",
             service_start=date(2026, 1, 1), service_end=date(2026, 12, 31))
    issue_invoice(conn, annual, date(2026, 1, 2), created_by=USER)
    build_schedule(conn, annual)

    setup = create_invoice(conn, globex, "INV-1002")
    add_line(conn, setup, "Implementation", 40, usd("150.00"))
    apply_discount_bp(conn, setup, 500)
    issue_invoice(conn, setup, date(2026, 1, 15), created_by=USER)

    seats = create_invoice(conn, initech, "INV-1003")
    add_line(conn, seats, "Seats", 25, usd("29.99"))
    issue_invoice(conn, seats, date(2026, 2, 3), created_by=USER)

    test = create_invoice(conn, initech, "INV-TEST")
    add_line(conn, test, "TEST - please ignore", 1, usd("1.00"))
    issue_invoice(conn, test, date(2026, 3, 9), created_by="intern@ledgerly.example")

    total =conn.execute("SELECT total FROM invoices WHERE id = ?", (setup,)).fetchone()["total"]
    paid = record_payment(conn, setup, Money("USD", total), idempotency_key="ch_1",
                          received_on=date(2026, 2, 10), fee=usd("165.60"), created_by=USER)
    refund(conn, paid, usd("300.00"), reason="two days not delivered", on=date(2026, 2, 20),
           created_by=USER)
    settle(conn, Money("USD", total) - usd("165.60") - usd("300.00"), date(2026, 2, 21),
           created_by=USER)

    for period, end in (("2026-01", date(2026, 1, 31)), ("2026-02", date(2026, 2, 28))):
        recognise_period(conn, period, end, created_by=USER)
    close_period(conn, "2026-01")
    close_period(conn, "2026-02")
    conn.close()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "books.db")
