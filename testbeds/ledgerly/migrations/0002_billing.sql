-- Customers, invoices and their lines.

CREATE TABLE customers (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    country     TEXT NOT NULL,
    region      TEXT,
    currency    TEXT NOT NULL
);

CREATE TABLE invoices (
    id          INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customers(id),
    number      TEXT NOT NULL UNIQUE,
    issued_on   TEXT,
    due_on      TEXT,
    status      TEXT NOT NULL CHECK (status IN ('draft', 'issued', 'paid', 'void')),
    currency    TEXT NOT NULL,
    subtotal    INTEGER NOT NULL DEFAULT 0,
    discount    INTEGER NOT NULL DEFAULT 0,
    tax         INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL DEFAULT 0,
    entry_id    INTEGER REFERENCES journal_entries(id)
);

CREATE TABLE invoice_lines (
    id          INTEGER PRIMARY KEY,
    invoice_id  INTEGER NOT NULL REFERENCES invoices(id),
    description TEXT NOT NULL,
    quantity    INTEGER NOT NULL CHECK (quantity > 0),
    unit_price  INTEGER NOT NULL CHECK (unit_price >= 0),
    revenue_account TEXT NOT NULL REFERENCES accounts(code),
    service_start TEXT,
    service_end   TEXT
);
