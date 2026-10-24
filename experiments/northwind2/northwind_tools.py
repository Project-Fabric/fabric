"""Northwind helpdesk tools as a minimal MCP server (stdio, newline-delimited JSON-RPC 2.0).

Two modes:
  --mode read     refund history only (both worlds), read-only;
  --mode direct   refund history plus a direct refund tool: the unguarded world, equivalent to
                  an agent holding the payment provider's key. It applies only the provider's own
                  rules (the payment exists; no more than is left to refund), like Stripe does.

The ledger is a SQLite file with a `fixture_payments` table (id, currency, amount_minor,
refunded_minor, customer), the same shape as Cosigna's synthetic fixture rail.
"""
import argparse
import json
import sqlite3
import sys
import uuid


def tools(mode):
    history = {"name": "northwind_refund_history",
               "description": "Show a payment's amount and how much has already been refunded.",
               "inputSchema": {"type": "object", "required": ["payment_id"],
                               "properties": {"payment_id": {"type": "string"}}}}
    if mode == "read":
        return [history]
    return [history, {"name": "northwind_refund",
                      "description": "Refund a payment. Amount in cents.",
                      "inputSchema": {"type": "object", "required": ["payment_id", "amount_minor"],
                                      "properties": {"payment_id": {"type": "string"},
                                                     "amount_minor": {"type": "integer"}}}}]


def call(db_path, mode, name, args):
    db = sqlite3.connect(db_path, timeout=10)
    try:
        payment = args.get("payment_id", "")
        row = db.execute("SELECT amount_minor, refunded_minor, currency FROM fixture_payments WHERE id=?",
                         (payment,)).fetchone()
        if name == "northwind_refund_history":
            if not row:
                return {"error": "no such payment"}, True
            return {"payment_id": payment, "paid_minor": row[0], "refunded_minor": row[1],
                    "currency": row[2]}, False
        if name == "northwind_refund" and mode == "direct":
            amount = args.get("amount_minor")
            if not row:
                return {"error": "no such payment"}, True
            if not isinstance(amount, int) or amount <= 0 or amount > row[0] - row[1]:
                return {"error": "amount is greater than the unrefunded amount"}, True
            db.execute("UPDATE fixture_payments SET refunded_minor=refunded_minor+? WHERE id=?", (amount, payment))
            db.commit()
            return {"refund": "re_direct_" + uuid.uuid4().hex[:12], "status": "succeeded",
                    "amount_minor": amount}, False
        return {"error": "unknown tool"}, True
    finally:
        db.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", required=True)
    parser.add_argument("--mode", choices=["read", "direct"], required=True)
    args = parser.parse_args()
    for line in sys.stdin:
        line = line.strip().lstrip("﻿")
        if not line:
            continue
        message = json.loads(line)
        if "id" not in message:
            continue
        method, params = message.get("method"), message.get("params") or {}
        if method == "initialize":
            result = {"protocolVersion": params.get("protocolVersion", "2025-06-18"),
                      "capabilities": {"tools": {}}, "serverInfo": {"name": "northwind", "version": "1"}}
        elif method == "tools/list":
            result = {"tools": tools(args.mode)}
        elif method == "tools/call":
            value, is_error = call(args.db, args.mode, params.get("name"), params.get("arguments") or {})
            result = {"content": [{"type": "text", "text": json.dumps(value)}], "isError": is_error}
        elif method == "ping":
            result = {}
        else:
            print(json.dumps({"jsonrpc": "2.0", "id": message["id"],
                              "error": {"code": -32601, "message": "method not found"}}), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)


if __name__ == "__main__":
    main()
