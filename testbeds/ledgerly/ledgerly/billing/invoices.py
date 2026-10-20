"""Invoices: drafted, issued (which posts to the ledger), paid or voided.

Issuing posts one balanced entry:

    Dr 1100 Accounts receivable      total
    Dr 4900 Discounts and allowances discount
        Cr 4xxx revenue (or 2100 Deferred revenue for service periods)  subtotal
        Cr 2000 Sales tax payable                                          tax

An issued invoice is never edited. Voiding an unpaid invoice posts a reversal.
"""

from __future__ import annotations

import sqlite3
from datetime import date, timedelta
from decimal import Decimal

from ..ledger import Line, post, reverse
from ..money import Money
from .customers import get_customer
from .tax import tax_on

AR = "1100"
TAX_PAYABLE = "2000"
DEFERRED_REVENUE = "2100"
DISCOUNTS = "4900"
PAYMENT_TERMS_DAYS = 30


class InvoiceError(RuntimeError):
    pass


def create_invoice(conn: sqlite3.Connection, customer_id: int, number: str) -> int:
    customer = get_customer(conn, customer_id)
    cur = conn.execute(
        "INSERT INTO invoices (customer_id, number, status, currency) VALUES (?, ?, 'draft', ?)",
        (customer_id, number, customer["currency"]),
    )
    conn.commit()
    assert cur.lastrowid is not None
    return cur.lastrowid


def get_invoice(conn: sqlite3.Connection, invoice_id: int) -> sqlite3.Row:
    row = conn.execute("SELECT * FROM invoices WHERE id = ?", (invoice_id,)).fetchone()
    if row is None:
        raise KeyError(f"no invoice {invoice_id}")
    return row


def _require_draft(invoice: sqlite3.Row) -> None:
    if invoice["status"] != "draft":
        raise InvoiceError(f"invoice {invoice['number']} is {invoice['status']}, not draft")


def add_line(
    conn: sqlite3.Connection,
    invoice_id: int,
    description: str,
    quantity: int,
    unit_price: Money,
    revenue_account: str = "4100",
    *,
    service_start: date | None = None,
    service_end: date | None = None,
) -> int:
    invoice = get_invoice(conn, invoice_id)
    _require_draft(invoice)
    if unit_price.currency != invoice["currency"]:
        raise InvoiceError("line currency must match the invoice")
    if (service_start is None) != (service_end is None):
        raise InvoiceError("a service period needs both a start and an end")
    if service_start and service_end and service_end < service_start:
        raise InvoiceError("service period ends before it starts")
    cur = conn.execute(
        "INSERT INTO invoice_lines (invoice_id, description, quantity, unit_price, revenue_account,"
        " service_start, service_end) VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            invoice_id,
            description,
            quantity,
            unit_price.minor,
            revenue_account,
            service_start.isoformat() if service_start else None,
            service_end.isoformat() if service_end else None,
        ),
    )
    _recalculate(conn, invoice_id)
    assert cur.lastrowid is not None
    return cur.lastrowid


def apply_discount_bp(conn: sqlite3.Connection, invoice_id: int, basis_points: int) -> None:
    """Discounts the subtotal by ``basis_points`` (1000 = 10%)."""
    if not 0 <= basis_points <= 10_000:
        raise InvoiceError("discount must be between 0 and 10000 basis points")
    invoice = get_invoice(conn, invoice_id)
    _require_draft(invoice)
    subtotal = Money(invoice["currency"], invoice["subtotal"])
    discount = subtotal.times(Decimal(basis_points) / Decimal(10_000))
    conn.execute("UPDATE invoices SET discount = ? WHERE id = ?", (discount.minor, invoice_id))
    _recalculate(conn, invoice_id)


def _recalculate(conn: sqlite3.Connection, invoice_id: int) -> None:
    invoice = get_invoice(conn, invoice_id)
    customer = get_customer(conn, invoice["customer_id"])
    currency = invoice["currency"]
    subtotal = sum(
        r["quantity"] * r["unit_price"]
        for r in conn.execute(
            "SELECT quantity, unit_price FROM invoice_lines WHERE invoice_id = ?", (invoice_id,)
        )
    )
    discount = min(invoice["discount"], subtotal)
    taxable = Money(currency, subtotal - discount)
    tax = tax_on(taxable, customer["country"], customer["region"])
    conn.execute(
        "UPDATE invoices SET subtotal = ?, discount = ?, tax = ?, total = ? WHERE id = ?",
        (subtotal, discount, tax.minor, taxable.minor + tax.minor, invoice_id),
    )
    conn.commit()


def issue_invoice(
    conn: sqlite3.Connection, invoice_id: int, issued_on: date, *, created_by: str
) -> int:
    invoice = get_invoice(conn, invoice_id)
    _require_draft(invoice)
    lines = conn.execute(
        "SELECT * FROM invoice_lines WHERE invoice_id = ? ORDER BY id", (invoice_id,)
    ).fetchall()
    if not lines:
        raise InvoiceError("cannot issue an invoice with no lines")
    currency = invoice["currency"]
    entry: list[Line] = [Line.dr(AR, Money(currency, invoice["total"]))]
    if invoice["discount"]:
        entry.append(Line.dr(DISCOUNTS, Money(currency, invoice["discount"])))
    credits: dict[str, int] = {}
    for line in lines:
        account = DEFERRED_REVENUE if line["service_start"] else line["revenue_account"]
        credits[account] = credits.get(account, 0) + line["quantity"] * line["unit_price"]
    entry += [Line.cr(acct, Money(currency, amt)) for acct, amt in credits.items() if amt]
    if invoice["tax"]:
        entry.append(Line.cr(TAX_PAYABLE, Money(currency, invoice["tax"])))
    entry = [l for l in entry if l.amount.minor]
    entry_id = post(
        conn,
        issued_on,
        f"Invoice {invoice['number']}",
        entry,
        source=f"invoice:{invoice_id}",
        created_by=created_by,
    )
    conn.execute(
        "UPDATE invoices SET status = 'issued', issued_on = ?, due_on = ?, entry_id = ? WHERE id = ?",
        (
            issued_on.isoformat(),
            (issued_on + timedelta(days=PAYMENT_TERMS_DAYS)).isoformat(),
            entry_id,
            invoice_id,
        ),
    )
    conn.commit()
    return entry_id


def void_invoice(
    conn: sqlite3.Connection, invoice_id: int, on: date, *, reason: str, created_by: str
) -> int | None:
    invoice = get_invoice(conn, invoice_id)
    if invoice["status"] == "paid":
        raise InvoiceError("a paid invoice is refunded, not voided")
    if invoice["status"] == "void":
        raise InvoiceError("invoice is already void")
    reversal = None
    if invoice["status"] == "issued":
        reversal = reverse(conn, invoice["entry_id"], on, reason=reason, created_by=created_by)
    conn.execute("UPDATE invoices SET status = 'void' WHERE id = ?", (invoice_id,))
    conn.commit()
    return reversal


def outstanding(conn: sqlite3.Connection, invoice_id: int) -> Money:
    invoice = get_invoice(conn, invoice_id)
    paid = conn.execute(
        "SELECT COALESCE(SUM(amount), 0) AS paid FROM payments WHERE invoice_id = ?",
        (invoice_id,),
    ).fetchone()["paid"]
    return Money(invoice["currency"], invoice["total"] - paid)
