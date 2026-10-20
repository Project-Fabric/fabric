"""Shared fixtures: a fresh in-memory books with the standard chart and open periods."""

from __future__ import annotations

import sqlite3
from datetime import date

from ledgerly.billing import add_customer, add_line, create_invoice, issue_invoice
from ledgerly.db import connect
from ledgerly.ledger import open_period, seed_accounts
from ledgerly.money import Money

USER = "test@ledgerly.example"


def books() -> sqlite3.Connection:
    conn = connect()
    seed_accounts(conn)
    for month in range(1, 13):
        open_period(conn, 2026, month)
    return conn


def usd(text: str) -> Money:
    return Money.parse(text, "USD")


def issued_invoice(
    conn: sqlite3.Connection,
    amount: str = "100.00",
    *,
    region: str | None = "OR",
    number: str = "INV-1",
    on: date = date(2026, 3, 10),
) -> int:
    customer = add_customer(conn, "Acme", "US", region)
    invoice = create_invoice(conn, customer, number)
    add_line(conn, invoice, "Consulting", 1, usd(amount))
    issue_invoice(conn, invoice, on, created_by=USER)
    return invoice
