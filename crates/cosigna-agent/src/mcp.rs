//! `cosigna-agent mcp`: the gateway's money operations as Model Context Protocol tools, so any
//! MCP client (Claude, Codex, others) can request refunds and payouts.
//!
//! The server runs beside the agent with only that agent's own token (`COSIGNA_AGENT_TOKEN`)
//! and forwards each call to the gateway's HTTP API. It holds no owner credential, no rail
//! credential and no ledger access, so it can do nothing the agent's grant does not allow.
//! Messages are newline-delimited JSON-RPC 2.0 over stdio.
use std::{
    io::{self, BufRead, Write},
    time::Duration,
};

use serde_json::{Value, json};

use crate::{Error, Result};

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Cosigna controls the money this agent can move. Request refunds \
and payouts only through these tools. Each request is checked against the payment system's \
records and your limits: small ones within your limit are paid at once, larger ones wait for a \
person, and requests outside your authority are denied. Never try to get around a denial or a \
hold (for example by splitting an amount or changing the reason). When you retry the same \
request, reuse its idempotency_key so it cannot be paid twice.";

pub struct Gateway {
    url: String,
    token: String,
}

impl Gateway {
    /// The gateway is loopback-only in this version; the token is the agent's own.
    pub fn new(url: &str, token: &str) -> Result<Self> {
        let url = url.trim_end_matches('/');
        let loopback = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok())
            .is_some_and(|port| port > 0);
        if !loopback {
            return Err(Error::Invalid(
                "the gateway URL must be http://127.0.0.1:<port>".into(),
            ));
        }
        let token = token.trim();
        if !token.starts_with("agentkey_") {
            return Err(Error::Invalid(
                "COSIGNA_AGENT_TOKEN must be an agent token (agentkey_...)".into(),
            ));
        }
        Ok(Self {
            url: url.into(),
            token: token.into(),
        })
    }

    fn call(&self, path: &str, body: Option<&Value>) -> (u16, Value) {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent();
        let url = format!("{}/gateway/v1{path}", self.url);
        let auth = format!("Bearer {}", self.token);
        let response = match body {
            Some(body) => agent
                .post(&url)
                .header("Authorization", &auth)
                .send_json(body),
            None => agent.get(&url).header("Authorization", &auth).call(),
        };
        match response {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let value = response
                    .body_mut()
                    .with_config()
                    .limit(256 * 1024)
                    .read_json()
                    .unwrap_or(Value::Null);
                (status, value)
            }
            Err(_) => (0, Value::Null),
        }
    }
}

pub fn serve(gateway: &Gateway) -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle(gateway, &line) {
            let mut text = response.to_string();
            text.push('\n');
            stdout.write_all(text.as_bytes())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle(gateway: &Gateway, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line.trim_start_matches('\u{feff}')) {
        Ok(message) => message,
        Err(_) => return Some(rpc_error(Value::Null, -32700, "parse error")),
    };
    let id = message.get("id")?.clone();
    let method = message["method"].as_str()?;
    let params = &message["params"];
    let result = match method {
        "initialize" => {
            let requested = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == requested)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            json!({"protocolVersion": version, "capabilities": {"tools": {}},
                "serverInfo": {"name": "cosigna", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => call(gateway, params),
        _ => return Some(rpc_error(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tools() -> Value {
    let money = |reference: &str, description: &str| {
        json!({"type": "object", "additionalProperties": false,
            "required": ["budget_id", "rail", reference, "amount_minor", "currency", "reason", "idempotency_key"],
            "properties": {
                "budget_id": {"type": "string", "description": "The budget you were given."},
                "rail": {"type": "string", "description": "Payment system, e.g. stripe_test or fixture."},
                reference: {"type": "string", "description": description},
                "amount_minor": {"type": "integer", "minimum": 1,
                    "description": "Amount in the currency's smallest unit (cents for USD)."},
                "currency": {"type": "string", "description": "Three-letter code, e.g. USD."},
                "reason": {"type": "string", "maxLength": 500,
                    "description": "Why, in one sentence. Shown to the approver."},
                "idempotency_key": {"type": "string", "maxLength": 128,
                    "description": "A unique key for this request. Reuse it when retrying the same request."}}})
    };
    let mut refund = money(
        "source",
        "The payment to refund (e.g. a Stripe PaymentIntent ID, pi_...).",
    );
    refund["properties"]["case_id"] = json!({"type": "string", "maxLength": 100,
        "description": "The support case or order this refund settles. Cosigna checks the refund against what the case says is owed."});
    json!([
        {"name": "cosigna_request_refund",
         "description": "Request a refund of a customer payment. Returns whether it was paid, is waiting for a person, or was denied, and why.",
         "inputSchema": refund},
        {"name": "cosigna_request_payout",
         "description": "Request a payout to a registered payee. Returns whether it was paid, is waiting for a person, or was denied, and why.",
         "inputSchema": money("payee", "The registered payee to pay.")},
        {"name": "cosigna_operation_status",
         "description": "Check a refund or payout you requested earlier, for example one that was waiting for approval.",
         "inputSchema": {"type": "object", "additionalProperties": false, "required": ["operation_id"],
            "properties": {"operation_id": {"type": "string"}}}},
        {"name": "cosigna_budget",
         "description": "See how much of your budget is left.",
         "inputSchema": {"type": "object", "additionalProperties": false, "required": ["budget_id"],
            "properties": {"budget_id": {"type": "string"}}}}
    ])
}

fn text(value: Value, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": value.to_string()}], "isError": is_error})
}

fn identifier(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| {
        !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// What the agent sees: the outcome and what to do next, without approval digests.
fn summary(envelope: &Value) -> Value {
    let op = &envelope["data"];
    let stage = op["stage"].as_str().unwrap_or_default();
    let rule = op["decision"]["rule"].as_str();
    let next = match stage {
        "succeeded" => "Paid. Tell the user it is done.",
        "held" => {
            "Waiting for a person to approve. Do not retry with a new key; check status later."
        }
        "approved" | "submitting" | "unknown" => {
            "Being settled. Check status later; do not request it again."
        }
        "denied" if rule == Some("case_required") => {
            "Denied: name the case this refund settles (case_id) and request again."
        }
        "denied" => {
            "Denied. Do not try to achieve this another way; explain the reason to the user."
        }
        "rejected" => "A person declined this request.",
        "failed" => "The payment system refused it; no money moved.",
        _ => "Check status later.",
    };
    json!({"operation_id": op["id"], "stage": stage, "decision": op["decision"]["outcome"],
        "reason_code": rule, "amount_minor": op["request"]["amount_minor"],
        "currency": op["request"]["currency"], "reference": op["receipt"]["rail_reference"],
        "next": next})
}

fn call(gateway: &Gateway, params: &Value) -> Value {
    let args = &params["arguments"];
    let request = |kind: &str, reference: &str| -> Value {
        let optional = if kind == "refund" { "case_id" } else { "" };
        let allowed = [
            "budget_id",
            "rail",
            reference,
            "amount_minor",
            "currency",
            "reason",
            "idempotency_key",
            optional,
        ];
        // Same rule as the gateway: unknown fields are refused, never silently dropped.
        if args
            .as_object()
            .is_none_or(|map| map.keys().any(|k| !allowed.contains(&k.as_str())))
        {
            return text(
                json!({"error": "invalid_request", "message": "unknown or missing arguments",
                "next": "This request was not carried out."}),
                true,
            );
        }
        let mut body = json!({"kind": kind});
        for field in [
            "budget_id",
            "rail",
            reference,
            "amount_minor",
            "currency",
            "reason",
            "idempotency_key",
        ] {
            body[field] = args[field].clone();
        }
        if !optional.is_empty() && !args[optional].is_null() {
            body[optional] = args[optional].clone();
        }
        let (status, value) = gateway.call("/operations", Some(&body));
        reply(status, &value, true)
    };
    match params["name"].as_str().unwrap_or_default() {
        "cosigna_request_refund" => request("refund", "source"),
        "cosigna_request_payout" => request("payout", "payee"),
        "cosigna_operation_status" => match identifier(&args["operation_id"]) {
            Some(id) => {
                let (status, value) = gateway.call(&format!("/operations/{id}"), None);
                reply(status, &value, true)
            }
            None => text(json!({"error": "operation_id is invalid"}), true),
        },
        "cosigna_budget" => match identifier(&args["budget_id"]) {
            Some(id) => {
                let (status, value) = gateway.call(&format!("/budgets/{id}"), None);
                if status == 200 {
                    let data = &value["data"];
                    text(
                        json!({"currency": data["budget"]["terms"]["currency"],
                        "available_minor": data["available_minor"], "reserved_minor": data["reserved_minor"],
                        "spent_minor": data["spent_minor"],
                        "per_operation_limit_minor": data["budget"]["terms"]["per_purchase_minor"]}),
                        false,
                    )
                } else {
                    reply(status, &value, false)
                }
            }
            None => text(json!({"error": "budget_id is invalid"}), true),
        },
        _ => text(json!({"error": "unknown tool"}), true),
    }
}

fn reply(status: u16, value: &Value, operation: bool) -> Value {
    match status {
        200 if operation => {
            let summary = summary(value);
            let denied = summary["stage"] == "denied" || summary["stage"] == "failed";
            text(summary, denied)
        }
        200 => text(value["data"].clone(), false),
        0 => text(
            json!({"error": "Cosigna is unreachable. Do not move money another way; try again later."}),
            true,
        ),
        _ => text(
            json!({"error": value["error"]["code"], "message": value["error"]["message"],
            "next": "This request was not carried out. Do not try to achieve it another way."}),
            true,
        ),
    }
}
