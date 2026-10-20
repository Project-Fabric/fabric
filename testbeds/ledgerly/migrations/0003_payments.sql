-- Payments, refunds and recognised revenue schedules.

CREATE TABLE payments (
    id              INTEGER PRIMARY KEY,
    invoice_id      INTEGER NOT NULL REFERENCES invoices(id),
    idempotency_key TEXT NOT NULL UNIQUE,
    amount          INTEGER NOT NULL CHECK (amount > 0),
    fee             INTEGER NOT NULL DEFAULT 0 CHECK (fee >= 0),
    currency        TEXT NOT NULL,
    received_on     TEXT NOT NULL,
    entry_id        INTEGER REFERENCES journal_entries(id)
);

CREATE TABLE refunds (
    id          INTEGER PRIMARY KEY,
    payment_id  INTEGER NOT NULL REFERENCES payments(id),
    amount      INTEGER NOT NULL CHECK (amount > 0),
    reason      TEXT NOT NULL,
    refunded_on TEXT NOT NULL,
    entry_id    INTEGER REFERENCES journal_entries(id)
);

CREATE TABLE revenue_schedule (
    id          INTEGER PRIMARY KEY,
    invoice_line_id INTEGER NOT NULL REFERENCES invoice_lines(id),
    period      TEXT NOT NULL,
    amount      INTEGER NOT NULL,
    recognised_entry_id INTEGER REFERENCES journal_entries(id)
);
