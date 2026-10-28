"""Northwind 2.0 on Stripe test mode: test payments, refunds read back from Stripe, and the
loopback service that holds the key for the agents' tools.

The Stripe key is read from COSIGNA_STRIPE_TEST_SECRET_KEY by the runner only. It goes to this
module's client and to the gateway process, never to the agents, their MCP configuration files
or the Claude CLI's environment. Only `sk_test_` keys are accepted; every request goes to
api.stripe.com (or a loopback stand-in set with NORTHWIND_STRIPE_BASE, for the offline check).
"""
import json
import os
import re
import secrets
import threading
import urllib.error
import urllib.parse
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

STRIPE = "https://api.stripe.com"


class StripeError(Exception):
    def __init__(self, status, body):
        error = body.get("error", {}) if isinstance(body, dict) else {}
        # Stripe's message is shown to the agent like the provider's own reply; never the key.
        super().__init__(error.get("message") or f"Stripe answered HTTP {status}")
        self.status = status


class Stripe:
    def __init__(self, key, base=None):
        if not re.fullmatch(r"sk_test_[A-Za-z0-9_]+", key or ""):
            raise SystemExit("Set COSIGNA_STRIPE_TEST_SECRET_KEY to a Stripe TEST secret key (sk_test_...). Live keys are refused.")
        base = base or STRIPE
        if base != STRIPE and not re.fullmatch(r"http://127\.0\.0\.1:\d+", base):
            raise SystemExit("NORTHWIND_STRIPE_BASE may only name a loopback stand-in.")
        self.key, self.base = key, base

    def call(self, method, path, form=None, idempotency=None):
        data = urllib.parse.urlencode(form).encode() if form is not None else None
        headers = {"Authorization": "Bearer " + self.key}
        if form is not None:
            headers["Content-Type"] = "application/x-www-form-urlencoded"
        if idempotency:
            headers["Idempotency-Key"] = idempotency
        request = urllib.request.Request(self.base + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as error:
            try:
                body = json.loads(error.read())
            except ValueError:
                body = {}
            raise StripeError(error.code, body) from None

    def refunds(self, payment):
        page = self.call("GET", "/v1/refunds?" + urllib.parse.urlencode({"payment_intent": payment, "limit": 100}))
        if page.get("has_more"):
            raise SystemExit(f"More than 100 refunds on {payment}; this run cannot be measured.")
        return page["data"]


def committed(refunds):
    """Refunded or on its way: what the customer gets back."""
    return sum(r["amount"] for r in refunds if r["status"] in ("succeeded", "pending", "requires_action"))


def seed(stripe, orders, run_id, world):
    """One test customer and one confirmed test-card payment per order; returns their ids by order."""
    paid = {}
    for order in orders:
        key = f"northwind2-{run_id}-{world}-{order['order']}"
        customer = stripe.call("POST", "/v1/customers", {"name": order["customer"],
                               "description": f"Northwind 2.0 synthetic customer ({world})",
                               "metadata[northwind_run]": run_id}, key + "-customer")
        intent = stripe.call("POST", "/v1/payment_intents", {
            "amount": str(order["amount_minor"]), "currency": "usd", "customer": customer["id"],
            "payment_method": "pm_card_visa", "confirm": "true",
            "automatic_payment_methods[enabled]": "true", "automatic_payment_methods[allow_redirects]": "never",
            "description": f"Northwind order #{order['order']} ({world})",
            "metadata[northwind_run]": run_id, "metadata[northwind_world]": world,
            "metadata[northwind_order]": str(order["order"])}, key + "-payment")
        if intent.get("livemode") is not False or intent.get("status") != "succeeded" \
                or intent.get("amount_received") != order["amount_minor"]:
            raise SystemExit(f"Stripe did not confirm the test payment for order {order['order']}.")
        paid[order["order"]] = {"payment": intent["id"], "customer": customer["id"]}
    return paid


def refunded_by_order(stripe, orders):
    return {o["order"]: committed(stripe.refunds(o["payment"])) for o in orders}


class Backend:
    """What the agents' Northwind tools reach: refund history from Stripe and, in the world
    without Cosigna, refunds straight through Stripe with only Stripe's own rules. Limited to
    this run's payments, so a run cannot touch anything else in the Stripe account."""

    def __init__(self, stripe, payments, direct):
        self.stripe, self.payments, self.direct = stripe, set(payments), direct
        self.token = secrets.token_hex(32)
        backend = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                if not secrets.compare_digest(self.headers.get("Authorization", ""), "Bearer " + backend.token):
                    return self.answer(403, {"error": "forbidden"})
                length = int(self.headers.get("Content-Length") or 0)
                if length > 16384:
                    return self.answer(413, {"error": "request too large"})
                try:
                    args = json.loads(self.rfile.read(length) or b"{}")
                except ValueError:
                    return self.answer(400, {"error": "invalid JSON"})
                status, value = backend.handle(self.path, args)
                self.answer(status, value)

            def answer(self, status, value):
                body = json.dumps(value).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def handle(self, path, args):
        payment = args.get("payment_id")
        if not isinstance(payment, str) or payment not in self.payments:
            return 404, {"error": "no such payment"}
        try:
            if path == "/history":
                intent = self.stripe.call("GET", f"/v1/payment_intents/{payment}")
                return 200, {"payment_id": payment, "paid_minor": intent["amount_received"],
                             "refunded_minor": committed(self.stripe.refunds(payment)),
                             "currency": intent["currency"].upper()}
            if path == "/refund" and self.direct:
                amount = args.get("amount_minor")
                if type(amount) is not int or amount <= 0:
                    return 400, {"error": "amount_minor must be a positive integer"}
                refund = self.stripe.call("POST", "/v1/refunds", {"payment_intent": payment, "amount": str(amount)},
                                          "northwind2-direct-" + uuid.uuid4().hex)
                return 200, {"refund": refund["id"], "status": refund["status"], "amount_minor": refund["amount"]}
        except StripeError as error:
            return 400, {"error": str(error)}
        return 404, {"error": "unknown tool"}

    def close(self):
        self.server.shutdown()
        self.server.server_close()


def without_key(env):
    """The environment for the Claude CLI and its tools: no Stripe key."""
    return {k: v for k, v in env.items() if k not in ("COSIGNA_STRIPE_TEST_SECRET_KEY", "COSIGNA_STRIPE_TEST_SECRET_KEY_FILE")}


def key_found(paths, key):
    """Whether the key appears in any file under these folders (it must not)."""
    needle = key.encode()
    for root in paths:
        for folder, _, files in os.walk(root):
            for name in files:
                try:
                    with open(os.path.join(folder, name), "rb") as handle:
                        if needle in handle.read():
                            return os.path.join(folder, name)
                except OSError:
                    continue
    return None
