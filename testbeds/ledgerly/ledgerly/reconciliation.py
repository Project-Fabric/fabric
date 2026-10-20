"""Bank reconciliation: matching bank statement lines to cash entries in the ledger.

A statement line matches a ledger cash movement with the same amount within
``DATE_TOLERANCE_DAYS``. Each ledger line matches at most one statement line.
"""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass, field
from datetime import date

CASH = "1000"
DATE_TOLERANCE_DAYS = 3


@dataclass(frozen=True)
class StatementLine:
    on: date
    amount: int  # minor units; positive is money in
    reference: str


@dataclass
class Reconciliation:
    matched: list[tuple[StatementLine, int]] = field(default_factory=list)
    unmatched_statement: list[StatementLine] = field(default_factory=list)
    unmatched_ledger: list[int] = field(default_factory=list)

    @property
    def complete(self) -> bool:
        return not self.unmatched_statement and not self.unmatched_ledger


def reconcile(
    conn: sqlite3.Connection, statement: list[StatementLine], start: date, end: date
) -> Reconciliation:
    ledger = [
        (r["id"], date.fromisoformat(r["posted_on"]), r["debit"] - r["credit"])
        for r in conn.execute(
            "SELECT l.id, e.posted_on, l.debit, l.credit FROM journal_lines l"
            " JOIN journal_entries e ON e.id = l.entry_id"
            " WHERE l.account = ? AND e.posted_on BETWEEN ? AND ? ORDER BY e.posted_on, l.id",
            (CASH, start.isoformat(), end.isoformat()),
        )
    ]
    used: set[int] = set()
    result = Reconciliation()
    for line in sorted(statement, key=lambda s: (s.on, s.reference)):
        match = next(
            (
                lid
                for lid, on, amount in ledger
                if lid not in used
                and amount == line.amount
                and abs((on - line.on).days) <= DATE_TOLERANCE_DAYS
            ),
            None,
        )
        if match is None:
            result.unmatched_statement.append(line)
        else:
            used.add(match)
            result.matched.append((line, match))
    result.unmatched_ledger = [lid for lid, _, _ in ledger if lid not in used]
    return result
