"""The double-entry journal.

Every entry balances (total debits equal total credits, per currency) and is immutable once
posted: a mistake is corrected by posting a reversing entry, never by editing or deleting.
"""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import date, datetime, timezone

from ..money import Money
from .accounts import get_account
from .periods import period_for


class JournalError(RuntimeError):
    pass


@dataclass(frozen=True)
class Line:
    account: str
    debit: Money | None = None
    credit: Money | None = None

    @classmethod
    def dr(cls, account: str, amount: Money) -> Line:
        return cls(account, debit=amount)

    @classmethod
    def cr(cls, account: str, amount: Money) -> Line:
        return cls(account, credit=amount)

    @property
    def amount(self) -> Money:
        value = self.debit if self.debit is not None else self.credit
        assert value is not None
        return value


def _validate(lines: list[Line]) -> None:
    if len(lines) < 2:
        raise JournalError("an entry needs at least two lines")
    totals: dict[str, int] = {}
    for line in lines:
        if (line.debit is None) == (line.credit is None):
            raise JournalError(f"line for {line.account} must be a debit or a credit")
        if line.amount.minor <= 0:
            raise JournalError(f"line for {line.account} must be a positive amount")
        signed = line.amount.minor if line.debit is not None else -line.amount.minor
        totals[line.amount.currency] = totals.get(line.amount.currency, 0) + signed
    unbalanced = {c: t for c, t in totals.items() if t != 0}
    if unbalanced:
        raise JournalError(f"entry does not balance: {unbalanced}")


def post(
    conn: sqlite3.Connection,
    posted_on: date,
    memo: str,
    lines: list[Line],
    *,
    source: str,
    created_by: str,
    reverses: int | None = None,
) -> int:
    """Posts a balanced entry into the open period containing ``posted_on``."""
    _validate(lines)
    for line in lines:
        account = get_account(conn, line.account)
        if account.currency != line.amount.currency:
            raise JournalError(
                f"account {account.code} is in {account.currency}, line is in {line.amount.currency}"
            )
    period = period_for(conn, posted_on)
    cur = conn.execute(
        "INSERT INTO journal_entries (posted_on, period, memo, source, reverses, created_by, created_at)"
        " VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            posted_on.isoformat(),
            period,
            memo,
            source,
            reverses,
            created_by,
            datetime.now(timezone.utc).isoformat(),
        ),
    )
    entry_id = cur.lastrowid
    assert entry_id is not None
    for line in lines:
        conn.execute(
            "INSERT INTO journal_lines (entry_id, account, debit, credit, currency) VALUES (?, ?, ?, ?, ?)",
            (
                entry_id,
                line.account,
                line.debit.minor if line.debit else 0,
                line.credit.minor if line.credit else 0,
                line.amount.currency,
            ),
        )
    conn.commit()
    return entry_id


def lines_of(conn: sqlite3.Connection, entry_id: int) -> list[Line]:
    rows = conn.execute(
        "SELECT account, debit, credit, currency FROM journal_lines WHERE entry_id = ? ORDER BY id",
        (entry_id,),
    ).fetchall()
    return [
        Line.dr(r["account"], Money(r["currency"], r["debit"]))
        if r["debit"]
        else Line.cr(r["account"], Money(r["currency"], r["credit"]))
        for r in rows
    ]


def reverse(
    conn: sqlite3.Connection, entry_id: int, posted_on: date, *, reason: str, created_by: str
) -> int:
    """Posts the mirror image of an entry. The original stays in the journal unchanged."""
    if conn.execute(
        "SELECT 1 FROM journal_entries WHERE reverses = ?", (entry_id,)
    ).fetchone():
        raise JournalError(f"entry {entry_id} has already been reversed")
    original = lines_of(conn, entry_id)
    if not original:
        raise JournalError(f"no entry {entry_id}")
    mirrored = [
        Line.cr(l.account, l.amount) if l.debit is not None else Line.dr(l.account, l.amount)
        for l in original
    ]
    return post(
        conn,
        posted_on,
        f"Reversal of entry {entry_id}: {reason}",
        mirrored,
        source=f"reversal:{entry_id}",
        created_by=created_by,
        reverses=entry_id,
    )


def balance(conn: sqlite3.Connection, account: str, *, through: date | None = None) -> Money:
    """The account's balance on its normal side (positive means normal)."""
    acct = get_account(conn, account)
    sql = (
        "SELECT COALESCE(SUM(l.debit), 0) AS dr, COALESCE(SUM(l.credit), 0) AS cr"
        " FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id WHERE l.account = ?"
    )
    args: list[object] = [account]
    if through is not None:
        sql += " AND e.posted_on <= ?"
        args.append(through.isoformat())
    row = conn.execute(sql, args).fetchone()
    net = row["dr"] - row["cr"]
    return Money(acct.currency, net if acct.debit_normal else -net)
