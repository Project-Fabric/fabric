"""The chart of accounts."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass

TYPES = ("asset", "liability", "equity", "revenue", "expense")
# Accounts whose balance normally sits on the debit side.
DEBIT_NORMAL = ("asset", "expense")


@dataclass(frozen=True)
class Account:
    code: str
    name: str
    type: str
    currency: str

    @property
    def debit_normal(self) -> bool:
        return self.type in DEBIT_NORMAL


STANDARD_ACCOUNTS = [
    Account("1000", "Cash", "asset", "USD"),
    Account("1100", "Accounts receivable", "asset", "USD"),
    Account("1200", "Payment processor clearing", "asset", "USD"),
    Account("2000", "Sales tax payable", "liability", "USD"),
    Account("2100", "Deferred revenue", "liability", "USD"),
    Account("2200", "Customer refunds payable", "liability", "USD"),
    Account("3000", "Retained earnings", "equity", "USD"),
    Account("4000", "Subscription revenue", "revenue", "USD"),
    Account("4100", "Services revenue", "revenue", "USD"),
    Account("4900", "Discounts and allowances", "revenue", "USD"),
    Account("6000", "Payment processing fees", "expense", "USD"),
    Account("6100", "Bad debt expense", "expense", "USD"),
    Account("7000", "FX gains and losses", "expense", "USD"),
]


def add_account(conn: sqlite3.Connection, account: Account) -> None:
    if account.type not in TYPES:
        raise ValueError(f"unknown account type {account.type!r}")
    conn.execute(
        "INSERT INTO accounts (code, name, type, currency) VALUES (?, ?, ?, ?)",
        (account.code, account.name, account.type, account.currency),
    )


def seed_accounts(conn: sqlite3.Connection) -> None:
    for account in STANDARD_ACCOUNTS:
        add_account(conn, account)
    conn.commit()


def get_account(conn: sqlite3.Connection, code: str) -> Account:
    row = conn.execute(
        "SELECT code, name, type, currency FROM accounts WHERE code = ?", (code,)
    ).fetchone()
    if row is None:
        raise KeyError(f"no account {code}")
    return Account(row["code"], row["name"], row["type"], row["currency"])
