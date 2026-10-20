import sqlite3
from pathlib import Path

MIGRATIONS = Path(__file__).parent / "migrations"


def connect():
    """An in-memory database with every migration applied in order."""
    db = sqlite3.connect(":memory:")
    for migration in sorted(MIGRATIONS.glob("*.sql")):
        db.executescript(migration.read_text())
    return db


def columns(db, table):
    return [row[1] for row in db.execute(f"PRAGMA table_info({table})")]
