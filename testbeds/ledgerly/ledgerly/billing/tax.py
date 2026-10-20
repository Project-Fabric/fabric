"""Sales tax rates by jurisdiction, in basis points (1 bp = 0.01%).

Tax is charged on the discounted subtotal and rounded once per invoice, half-even.
"""

from __future__ import annotations

from decimal import Decimal

from ..money import Money

# (country, region) -> basis points. A region of None is the country-wide rate.
RATES_BP: dict[tuple[str, str | None], int] = {
    ("US", "CA"): 725,
    ("US", "NY"): 400,
    ("US", "TX"): 625,
    ("US", "OR"): 0,
    ("US", None): 0,
    ("GB", None): 2000,
    ("DE", None): 1900,
    ("IN", None): 1800,
}


class TaxError(LookupError):
    pass


def rate_bp(country: str, region: str | None) -> int:
    if (country, region) in RATES_BP:
        return RATES_BP[(country, region)]
    if (country, None) in RATES_BP:
        return RATES_BP[(country, None)]
    raise TaxError(f"no tax rate for {country}/{region}")


def tax_on(taxable: Money, country: str, region: str | None) -> Money:
    return taxable.times(Decimal(rate_bp(country, region)) / Decimal(10_000))
