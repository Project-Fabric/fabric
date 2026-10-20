# Ledgerly

Billing, payments and the general ledger for a subscription business. Python 3.11+, standard
library only, SQLite.

```sh
python -m unittest discover -s tests -q
```

## Layout

| Path | What | SOX scope |
|---|---|---|
| `ledgerly/ledger/` | Chart of accounts, periods, double-entry journal | In scope |
| `ledgerly/billing/` | Customers, invoices, sales tax | In scope |
| `ledgerly/payments/` | Payments, refunds, settlement | In scope |
| `ledgerly/revenue/` | Revenue recognition | In scope |
| `ledgerly/reports.py`, `reconciliation.py`, `fx.py` | Reporting and reconciliation | In scope |
| `migrations/` | Schema; applied migrations are immutable | In scope |
| `ledgerly/money.py` | Integer minor-unit money | In scope |
| `tools/`, `docs/` | Internal scripts and docs | Out of scope |

## Rules

- Amounts are integer minor units. Never use floats for money.
- Every journal entry balances. Posted entries are never edited or deleted: correct them with
  a reversing entry.
- Nothing is posted into a closed period.
- Applied migrations never change; add a new one.
- Changes to in-scope code need an approved change record (see `CONTRIBUTING.md`).
