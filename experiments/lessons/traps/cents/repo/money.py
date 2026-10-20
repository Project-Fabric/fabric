"""Order amounts for the billing service."""

PRICES = {"coffee": 450, "bagel": 325, "juice": 599}


def order_total(items):
    """items is a list of (product, quantity) pairs."""
    return sum(PRICES[product] * qty for product, qty in items)


def format_amount(amount):
    return f"{amount // 100}.{amount % 100:02d}"
