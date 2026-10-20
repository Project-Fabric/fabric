"""Monthly accounting periods. Once a period is closed, nothing may be posted to it."""

from __future__ import annotations

import calendar
import sqlite3
from datetime import date, datetime, timezone


class PeriodError(RuntimeError):
    pass


def period_name(day: date) -> str:
    return f"{day.year:04d}-{day.month:02d}"


def open_period(conn: sqlite3.Connection, year: int, month: int) -> str:
    name = f"{year:04d}-{month:02d}"
    last = calendar.monthrange(year, month)[1]
    conn.execute(
        "INSERT OR IGNORE INTO periods (name, starts_on, ends_on) VALUES (?, ?, ?)",
        (name, date(year, month, 1).isoformat(), date(year, month, last).isoformat()),
    )
    conn.commit()
    return name


def period_for(conn: sqlite3.Connection, day: date) -> str:
    """The open period containing ``day``. Posting into a closed or missing period fails."""
    name = period_name(day)
    row = conn.execute("SELECT closed_at FROM periods WHERE name = ?", (name,)).fetchone()
    if row is None:
        raise PeriodError(f"period {name} is not open")
    if row["closed_at"] is not None:
        raise PeriodError(f"period {name} is closed")
    return name


def close_period(conn: sqlite3.Connection, name: str) -> None:
    row = conn.execute("SELECT closed_at FROM periods WHERE name = ?", (name,)).fetchone()
    if row is None:
        raise PeriodError(f"no period {name}")
    if row["closed_at"] is not None:
        raise PeriodError(f"period {name} is already closed")
    conn.execute(
        "UPDATE periods SET closed_at = ? WHERE name = ?",
        (datetime.now(timezone.utc).isoformat(), name),
    )
    conn.commit()


def is_closed(conn: sqlite3.Connection, name: str) -> bool:
    row = conn.execute("SELECT closed_at FROM periods WHERE name = ?", (name,)).fetchone()
    return row is not None and row["closed_at"] is not None
