"""Ratable revenue recognition.

A line with a service period is billed into deferred revenue (2100) and recognised month by
month in proportion to the service days in each month. The schedule's amounts always sum to
the line's amount exactly. Recognising a period posts, per line:

    Dr 2100 Deferred revenue
        Cr <the line's revenue account>
"""

from __future__ import annotations

import sqlite3
from datetime import date, timedelta

from ..ledger import Line, post
from ..ledger.periods import period_name
from ..money import Money

DEFERRED_REVENUE = "2100"


def _days_per_period(start: date, end: date) -> dict[str, int]:
    days: dict[str, int] = {}
    day = start
    while day <= end:
        name = period_name(day)
        days[name] = days.get(name, 0) + 1
        day += timedelta(days=1)
    return days


def build_schedule(conn: sqlite3.Connection, invoice_id: int) -> int:
    """Creates schedule rows for each service-period line of an issued invoice."""
    invoice = conn.execute("SELECT * FROM invoices WHERE id = ?", (invoice_id,)).fetchone()
    if invoice is None or invoice["status"] not in ("issued", "paid"):
        raise RuntimeError("revenue schedules are built for issued invoices")
    created = 0
    lines = conn.execute(
        "SELECT * FROM invoice_lines WHERE invoice_id = ? AND service_start IS NOT NULL",
        (invoice_id,),
    ).fetchall()
    for line in lines:
        if conn.execute(
            "SELECT 1 FROM revenue_schedule WHERE invoice_line_id = ?", (line["id"],)
        ).fetchone():
            continue
        amount = Money(invoice["currency"], line["quantity"] * line["unit_price"])
        days = _days_per_period(
            date.fromisoformat(line["service_start"]), date.fromisoformat(line["service_end"])
        )
        parts = amount.allocate(list(days.values()))
        for period, part in zip(days, parts):
            conn.execute(
                "INSERT INTO revenue_schedule (invoice_line_id, period, amount) VALUES (?, ?, ?)",
                (line["id"], period, part.minor),
            )
            created += 1
    conn.commit()
    return created


def recognise_period(
    conn: sqlite3.Connection, period: str, posted_on: date, *, created_by: str
) -> list[int]:
    """Recognises every scheduled amount for ``period`` not yet recognised."""
    rows = conn.execute(
        "SELECT s.id, s.amount, l.revenue_account, i.currency, i.number"
        " FROM revenue_schedule s"
        " JOIN invoice_lines l ON l.id = s.invoice_line_id"
        " JOIN invoices i ON i.id = l.invoice_id"
        " WHERE s.period = ? AND s.recognised_entry_id IS NULL AND i.status != 'void'"
        " ORDER BY s.id",
        (period,),
    ).fetchall()
    entries = []
    for row in rows:
        if row["amount"] == 0:
            continue
        amount = Money(row["currency"], row["amount"])
        entry_id = post(
            conn,
            posted_on,
            f"Recognise {period} revenue for invoice {row['number']}",
            [Line.dr(DEFERRED_REVENUE, amount), Line.cr(row["revenue_account"], amount)],
            source=f"revenue:{row['id']}",
            created_by=created_by,
        )
        conn.execute(
            "UPDATE revenue_schedule SET recognised_entry_id = ? WHERE id = ?",
            (entry_id, row["id"]),
        )
        entries.append(entry_id)
    conn.commit()
    return entries
