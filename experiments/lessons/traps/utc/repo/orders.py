"""Order records for the fulfilment service."""

ORDERS = []


def create_order(customer, items):
    order = {"id": len(ORDERS) + 1, "customer": customer, "items": items}
    ORDERS.append(order)
    return order
