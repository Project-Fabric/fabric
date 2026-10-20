"""SQLite storage. The schema comes only from the files in ``migrations/``, applied in order.

Applied migrations are recorded in ``schema_migrations`` with a checksum; a migration that
changed after being applied is refused, because production databases already ran it.
"""

from __future__ import annotations

import hashlib
import sqlite3
from pathlib import Path

MIGRATIONS = Path(__file__).resolve().parent.parent / "migrations"


class MigrationError(RuntimeError):
    pass


def connect(path: str = ":memory:") -> sqlite3.Connection:
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    migrate(conn)
    return conn


def migrate(conn: sqlite3.Connection, directory: Path = MIGRATIONS) -> list[str]:
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations ("
        " name TEXT PRIMARY KEY, checksum TEXT NOT NULL)"
    )
    applied = {
        row["name"]: row["checksum"]
        for row in conn.execute("SELECT name, checksum FROM schema_migrations")
    }
    ran = []
    for file in sorted(directory.glob("*.sql")):
        sql = file.read_text(encoding="utf-8")
        checksum = hashlib.sha256(sql.encode()).hexdigest()
        if file.name in applied:
            if applied[file.name] != checksum:
                raise MigrationError(f"{file.name} changed after it was applied")
            continue
        conn.executescript(sql)
        conn.execute(
            "INSERT INTO schema_migrations (name, checksum) VALUES (?, ?)",
            (file.name, checksum),
        )
        ran.append(file.name)
    conn.commit()
    return ran
