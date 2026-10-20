"""Foreign exchange: converting amounts at a stated rate.

Rates are quoted as units of ``to`` per one unit of ``from`` in major units, and conversion
rounds half-even to the target currency's minor unit.
"""

from __future__ import annotations

from decimal import Decimal

from .money import CURRENCIES, Money, round_minor


def convert(amount: Money, to: str, rate: Decimal | str) -> Money:
    rate = Decimal(rate)
    if rate <= 0:
        raise ValueError("rate must be positive")
    if amount.currency == to:
        return amount
    scale = Decimal(10) ** (CURRENCIES[to] - CURRENCIES[amount.currency])
    return Money(to, round_minor(Decimal(amount.minor) * rate * scale))
