def subtotal(items):
    """items is a list of (unit_price, quantity) pairs."""
    return sum(price * qty for price, qty in items)


def total(items, discount=0.0):
    return round(subtotal(items) * (1 - discount), 2)
