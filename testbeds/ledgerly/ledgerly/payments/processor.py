"""Customer payments through the card processor.

A payment of ``amount`` with processor ``fee`` posts:

    Dr 1200 Processor clearing        amount - fee
    Dr 6000 Payment processing fees   fee
        Cr 1100 Accounts receivable   amount

Payments are idempotent by key: retrying the same request returns the original payment, and
reusing a key for a different request is an error. A payment may not exceed what is owed.
"""

from __future__ import annotations

import sqlite3
from datetime import date

from ..billing.invoices import AR, get_invoice, outstanding
from ..ledger import Line, post
from ..money import Money

CLEARING = "1200"
CASH = "1000"
FEES = "6000"
REFUNDS = "4900"


class PaymentError(RuntimeError):
    pass


def record_payment(
    conn: sqlite3.Connection,
    invoice_id: int,
    amount: Money,
    *,
    idempotency_key: str,
    received_on: date,
    fee: Money | None = None,
    created_by: str,
) -> int:
    existing = conn.execute(
        "SELECT * FROM payments WHERE idempotency_key = ?", (idempotency_key,)
    ).fetchone()
    if existing is not None:
        same = (
            existing["invoice_id"] == invoice_id
            and existing["amount"] == amount.minor
            and existing["currency"] == amount.currency
        )
        if not same:
            raise PaymentError(f"idempotency key {idempotency_key!r} was used for another payment")
        return existing["id"]

    invoice = get_invoice(conn, invoice_id)
    if invoice["status"] != "issued":
        raise PaymentError(f"invoice {invoice['number']} is {invoice['status']}")
    if amount.currency != invoice["currency"]:
        raise PaymentError("payment currency must match the invoice")
    if amount.minor <= 0:
        raise PaymentError("payment must be positive")
    owed = outstanding(conn, invoice_id)
    if amount.minor > owed.minor:
        raise PaymentError(f"payment {amount.format()} exceeds the {owed.format()} owed")
    fee = fee or Money.zero(amount.currency)
    if fee.minor < 0 or fee.minor > amount.minor:
        raise PaymentError("fee must be between zero and the payment")

    lines = [Line.dr(CLEARING, amount - fee)] if amount.minor > fee.minor else []
    if fee.minor:
        lines.append(Line.dr(FEES, fee))
    lines.append(Line.cr(AR, amount))
    entry_id = post(
        conn,
        received_on,
        f"Payment for invoice {invoice['number']}",
        lines,
        source=f"payment:{idempotency_key}",
        created_by=created_by,
    )
    cur = conn.execute(
        "INSERT INTO payments (invoice_id, idempotency_key, amount, fee, currency, received_on, entry_id)"
        " VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            invoice_id,
            idempotency_key,
            amount.minor,
            fee.minor,
            amount.currency,
            received_on.isoformat(),
            entry_id,
        ),
    )
    if outstanding(conn, invoice_id).minor == 0:
        conn.execute("UPDATE invoices SET status = 'paid' WHERE id = ?", (invoice_id,))
    conn.commit()
    assert cur.lastrowid is not None
    return cur.lastrowid


def refunded(conn: sqlite3.Connection, payment_id: int) -> int:
    return conn.execute(
        "SELECT COALESCE(SUM(amount), 0) AS total FROM refunds WHERE payment_id = ?",
        (payment_id,),
    ).fetchone()["total"]


def refund(
    conn: sqlite3.Connection,
    payment_id: int,
    amount: Money,
    *,
    reason: str,
    on: date,
    created_by: str,
) -> int:
    """Refunds part or all of a payment. Total refunds never exceed the payment."""
    payment = conn.execute("SELECT * FROM payments WHERE id = ?", (payment_id,)).fetchone()
    if payment is None:
        raise PaymentError(f"no payment {payment_id}")
    if amount.currency != payment["currency"]:
        raise PaymentError("refund currency must match the payment")
    if amount.minor <= 0:
        raise PaymentError("refund must be positive")
    if not reason.strip():
        raise PaymentError("a refund needs a reason")
    if refunded(conn, payment_id) + amount.minor > payment["amount"]:
        raise PaymentError("refunds would exceed the original payment")
    entry_id = post(
        conn,
        on,
        f"Refund of payment {payment_id}: {reason}",
        [Line.dr(REFUNDS, amount), Line.cr(CLEARING, amount)],
        source=f"refund:{payment_id}",
        created_by=created_by,
    )
    cur = conn.execute(
        "INSERT INTO refunds (payment_id, amount, reason, refunded_on, entry_id) VALUES (?, ?, ?, ?, ?)",
        (payment_id, amount.minor, reason, on.isoformat(), entry_id),
    )
    conn.commit()
    assert cur.lastrowid is not None
    return cur.lastrowid


def settle(conn: sqlite3.Connection, amount: Money, on: date, *, created_by: str) -> int:
    """Moves a processor payout from clearing into cash."""
    return post(
        conn,
        on,
        "Processor payout",
        [Line.dr(CASH, amount), Line.cr(CLEARING, amount)],
        source="settlement",
        created_by=created_by,
    )
