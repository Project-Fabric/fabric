-- Chart of accounts, accounting periods and the double-entry journal.

CREATE TABLE accounts (
    code        TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    type        TEXT NOT NULL CHECK (type IN ('asset', 'liability', 'equity', 'revenue', 'expense')),
    currency    TEXT NOT NULL
);

CREATE TABLE periods (
    name        TEXT PRIMARY KEY,          -- e.g. 2026-09
    starts_on   TEXT NOT NULL,
    ends_on     TEXT NOT NULL,
    closed_at   TEXT
);

CREATE TABLE journal_entries (
    id          INTEGER PRIMARY KEY,
    posted_on   TEXT NOT NULL,
    period      TEXT NOT NULL REFERENCES periods(name),
    memo        TEXT NOT NULL,
    source      TEXT NOT NULL,             -- invoice:12, payment:7, manual, ...
    reverses    INTEGER REFERENCES journal_entries(id),
    created_by  TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE journal_lines (
    id          INTEGER PRIMARY KEY,
    entry_id    INTEGER NOT NULL REFERENCES journal_entries(id),
    account     TEXT NOT NULL REFERENCES accounts(code),
    debit       INTEGER NOT NULL DEFAULT 0 CHECK (debit >= 0),
    credit      INTEGER NOT NULL DEFAULT 0 CHECK (credit >= 0),
    currency    TEXT NOT NULL,
    CHECK ((debit = 0) <> (credit = 0))
);

CREATE INDEX journal_lines_account ON journal_lines(account);
