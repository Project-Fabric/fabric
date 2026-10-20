"""Financial reports derived from the journal: trial balance, income statement, aging."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import date

from .ledger.accounts import DEBIT_NORMAL


@dataclass(frozen=True)
class TrialBalanceRow:
    account: str
    name: str
    debit: int
    credit: int


def trial_balance(conn: sqlite3.Connection, *, through: date | None = None) -> list[TrialBalanceRow]:
    sql = (
        "SELECT a.code, a.name, a.type, COALESCE(SUM(l.debit), 0) AS dr, COALESCE(SUM(l.credit), 0) AS cr"
        " FROM accounts a"
        " LEFT JOIN journal_lines l ON l.account = a.code"
        " LEFT JOIN journal_entries e ON e.id = l.entry_id"
    )
    args: list[object] = []
    if through is not None:
        sql += " WHERE e.posted_on IS NULL OR e.posted_on <= ?"
        args.append(through.isoformat())
    sql += " GROUP BY a.code ORDER BY a.code"
    rows = []
    for r in conn.execute(sql, args):
        net = r["dr"] - r["cr"]
        if net == 0:
            continue
        rows.append(
            TrialBalanceRow(r["code"], r["name"], net if net > 0 else 0, -net if net < 0 else 0)
        )
    return rows


def is_balanced(rows: list[TrialBalanceRow]) -> bool:
    return sum(r.debit for r in rows) == sum(r.credit for r in rows)


def income_statement(conn: sqlite3.Connection, period: str) -> dict[str, int]:
    """Revenue, expenses and net income for one period, in minor units."""
    totals = {"revenue": 0, "expense": 0}
    for r in conn.execute(
        "SELECT a.type, SUM(l.debit) AS dr, SUM(l.credit) AS cr"
        " FROM journal_lines l"
        " JOIN journal_entries e ON e.id = l.entry_id"
        " JOIN accounts a ON a.code = l.account"
        " WHERE e.period = ? AND a.type IN ('revenue', 'expense')"
        " GROUP BY a.type",
        (period,),
    ):
        net = r["dr"] - r["cr"]
        totals[r["type"]] = net if r["type"] in DEBIT_NORMAL else -net
    totals["net_income"] = totals["revenue"] - totals["expense"]
    return totals


def ar_aging(conn: sqlite3.Connection, as_of: date) -> dict[str, int]:
    """Outstanding receivables by days past due: current, 1-30, 31-60, 61-90, 90+."""
    buckets = {"current": 0, "1-30": 0, "31-60": 0, "61-90": 0, "90+": 0}
    for r in conn.execute(
        "SELECT i.total - COALESCE((SELECT SUM(amount) FROM payments p WHERE p.invoice_id = i.id), 0)"
        " AS open, i.due_on FROM invoices i WHERE i.status = 'issued'"
    ):
        if r["open"] <= 0:
            continue
        late = (as_of - date.fromisoformat(r["due_on"])).days
        key = (
            "current"
            if late <= 0
            else "1-30"
            if late <= 30
            else "31-60"
            if late <= 60
            else "61-90"
            if late <= 90
            else "90+"
        )
        buckets[key] += r["open"]
    return buckets
