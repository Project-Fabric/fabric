"""Customers."""

from __future__ import annotations

import sqlite3

from ..money import CURRENCIES


def add_customer(
    conn: sqlite3.Connection, name: str, country: str, region: str | None, currency: str = "USD"
) -> int:
    if currency not in CURRENCIES:
        raise ValueError(f"unsupported currency {currency!r}")
    cur = conn.execute(
        "INSERT INTO customers (name, country, region, currency) VALUES (?, ?, ?, ?)",
        (name, country, region, currency),
    )
    conn.commit()
    assert cur.lastrowid is not None
    return cur.lastrowid


def get_customer(conn: sqlite3.Connection, customer_id: int) -> sqlite3.Row:
    row = conn.execute("SELECT * FROM customers WHERE id = ?", (customer_id,)).fetchone()
    if row is None:
        raise KeyError(f"no customer {customer_id}")
    return row
