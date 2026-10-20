"""Money as integer minor units (cents) with an ISO currency code.

Floats never touch amounts. Rounding to minor units is half-even (banker's rounding), the
rule the finance team agreed with the auditors; see ``round_minor``.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import ROUND_HALF_EVEN, Decimal

CURRENCIES = {"USD": 2, "EUR": 2, "GBP": 2, "JPY": 0, "INR": 2}


class MoneyError(ValueError):
    pass


def round_minor(value: Decimal) -> int:
    """Rounds a Decimal amount of minor units to a whole minor unit, half-even."""
    return int(value.quantize(Decimal(1), rounding=ROUND_HALF_EVEN))


@dataclass(frozen=True, order=True)
class Money:
    currency: str
    minor: int

    def __post_init__(self) -> None:
        if self.currency not in CURRENCIES:
            raise MoneyError(f"unsupported currency {self.currency!r}")
        if not isinstance(self.minor, int) or isinstance(self.minor, bool):
            raise MoneyError("amounts are integer minor units")

    @classmethod
    def zero(cls, currency: str) -> Money:
        return cls(currency, 0)

    @classmethod
    def parse(cls, text: str, currency: str) -> Money:
        """Parses a major-unit string such as ``"12.34"``."""
        places = CURRENCIES[currency]
        value = Decimal(text) * (10**places)
        if value != value.to_integral_value():
            raise MoneyError(f"{text} has more than {places} decimal places")
        return cls(currency, int(value))

    def _same(self, other: Money) -> None:
        if self.currency != other.currency:
            raise MoneyError(f"cannot combine {self.currency} and {other.currency}")

    def __add__(self, other: Money) -> Money:
        self._same(other)
        return Money(self.currency, self.minor + other.minor)

    def __sub__(self, other: Money) -> Money:
        self._same(other)
        return Money(self.currency, self.minor - other.minor)

    def __neg__(self) -> Money:
        return Money(self.currency, -self.minor)

    def times(self, factor: Decimal | int | str) -> Money:
        """Multiplies by an exact factor, rounding half-even to a minor unit."""
        return Money(self.currency, round_minor(Decimal(self.minor) * Decimal(factor)))

    def allocate(self, weights: list[int]) -> list[Money]:
        """Splits into parts proportional to ``weights``; the parts always sum to the whole.

        Remainders go one minor unit at a time to the earliest parts.
        """
        if not weights or any(w < 0 for w in weights) or sum(weights) == 0:
            raise MoneyError("weights must be non-negative and not all zero")
        total = sum(weights)
        parts = [self.minor * w // total for w in weights]
        remainder = self.minor - sum(parts)
        for i in range(abs(remainder)):
            parts[i % len(parts)] += 1 if remainder > 0 else -1
        return [Money(self.currency, p) for p in parts]

    def is_negative(self) -> bool:
        return self.minor < 0

    def format(self) -> str:
        places = CURRENCIES[self.currency]
        sign = "-" if self.minor < 0 else ""
        whole, frac = divmod(abs(self.minor), 10**places) if places else (abs(self.minor), 0)
        body = f"{whole:,}" + (f".{frac:0{places}d}" if places else "")
        return f"{sign}{body} {self.currency}"
