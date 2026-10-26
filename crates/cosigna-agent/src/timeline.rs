//! The story of one money operation, for the people who answer for it: who asked, what was
//! decided and why, who approved, what the payment system said, where it stands now and what
//! can happen next. Built only from the operation's evidence records, each cited by sequence
//! number and hash, so the story is exactly what the tamper-evident chain says.
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;

use crate::{Actor, Result, Role, Service, operations::OperationStage, storage};

pub const API_VERSION: &str = "cosigna.timeline/v1";

#[derive(Debug, Serialize)]
pub struct Who {
    /// `agent`, `owner`, `approver` or `gateway`.
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    /// The evidence record this step is read from.
    pub seq: i64,
    pub hash: String,
    pub at: String,
    pub step: String,
    pub who: Who,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub struct Timeline {
    pub operation: String,
    pub kind: String,
    pub amount_minor: i64,
    pub currency: String,
    /// The payment refunded, or the payee paid.
    pub counterparty: Option<String>,
    pub case_id: Option<String>,
    pub stage: OperationStage,
    pub entries: Vec<Entry>,
    /// Where it stands now, in one sentence.
    pub now: String,
    /// What can happen next, and who can do it.
    pub next: Vec<String>,
    /// The whole chain verifies and current state matches it (`evidence verify`).
    pub evidence_ok: bool,
}

/// A rule, in words an operator can act on.
pub fn reason(rule: &str) -> &'static str {
    match rule {
        "within_agent_auto_limit" => "within the agent's automatic limit",
        "needs_owner_approval" => "above the agent's automatic limit",
        "needs_two_approvals" => "above the two-person limit, so two different people must approve",
        "case_mismatch" => "it does not match the customer's case (payment, customer or currency)",
        "exceeds_case_owed" => "it is more than the customer's case says is still owed",
        "case_required" => "it names no case, and this agent must name one",
        "case_not_found" => "the case it names does not exist",
        "not_granted" => "this agent is not allowed to make this kind of payment",
        "currency_not_in_budget" => "it is in a different currency from the budget",
        "over_per_operation_limit" => "it is over the budget's limit for a single payment",
        "budget_exhausted" => "the budget does not have enough left",
        "source_not_found" => "the payment to refund does not exist",
        "exceeds_refundable" => "it is more than is left to refund on that payment",
        "currency_mismatch" => "its currency differs from the payment's",
        "payee_unknown" => "the payee is not registered",
        _ => "a policy rule",
    }
}

fn money(minor: i64, currency: &str) -> String {
    format!("{currency} {}.{:02}", minor / 100, (minor % 100).abs())
}

/// The first 12 characters of an id or digest, after any `prefix_`.
fn short(value: &str) -> &str {
    let tail = value.split_once('_').map_or(value, |(_, rest)| rest);
    tail.get(..12).unwrap_or(tail)
}

impl Service {
    /// The operation's story. Owner, or a named approver of the same owner.
    pub fn timeline(&self, actor: &Actor, id: &str) -> Result<Timeline> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner, Role::Approver])?;
        let op = self.operation(actor, id)?;
        let db = storage::connect(&self.ledger, false)?;
        let name = |table: &str, id: &str| -> Option<String> {
            db.query_row(
                &format!("SELECT name FROM {table} WHERE id=?1"),
                [id],
                |r| r.get(0),
            )
            .ok()
        };
        // Operation ids are `op_` + 64 hex characters, so this pattern cannot match another one.
        let mut statement =
            db.prepare("SELECT seq,hash,body FROM evidence WHERE body LIKE ?1 ORDER BY seq")?;
        let rows = statement
            .query_map(params![format!("%\"operation\":\"{id}\"%")], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let request = &op.request;
        let amount = money(request.amount_minor, &request.currency);
        let target = request
            .source
            .as_deref()
            .or(request.payee.as_deref())
            .unwrap_or("?");
        let what = match request.kind {
            crate::operations::OperationKind::Refund => format!("a refund of {amount} on {target}"),
            crate::operations::OperationKind::Payout => format!("a payout of {amount} to {target}"),
        };
        let mut entries = Vec::new();
        for (seq, hash, body) in rows {
            let record: Value = serde_json::from_str(&body)?;
            let event = &record["event"];
            if event["operation"] != id {
                continue;
            }
            let step = event["kind"]
                .as_str()
                .and_then(|k| k.strip_prefix("gateway.operation."))
                .unwrap_or("")
                .to_string();
            let by = &event["by"];
            let who = match by["role"].as_str() {
                Some("approver") => {
                    let id = by["approver"].as_str().map(str::to_string);
                    Who {
                        role: "approver",
                        name: id.as_deref().and_then(|a| name("approvers", a)),
                        id,
                    }
                }
                Some("owner") => Who {
                    role: "owner",
                    id: None,
                    name: None,
                },
                _ if by["agent"].is_string() => {
                    let id = by["agent"].as_str().map(str::to_string);
                    Who {
                        role: "agent",
                        name: id.as_deref().and_then(|a| name("agents", a)),
                        id,
                    }
                }
                _ => Who {
                    role: "gateway",
                    id: None,
                    name: None,
                },
            };
            let person = who.name.clone().unwrap_or_else(|| match who.role {
                "owner" => "the owner".into(),
                "agent" => "the agent".into(),
                "approver" => "an approver".into(),
                _ => "the gateway".into(),
            });
            let rule = event["decision"]["rule"].as_str().unwrap_or("");
            let case = request
                .case_id
                .as_deref()
                .map(|c| format!(" for case {c}"))
                .unwrap_or_default();
            let summary = match step.as_str() {
                "auto_approved" => format!(
                    "{person} asked for {what}{case}. Approved by policy: {}.",
                    reason(rule)
                ),
                "held" => format!(
                    "{person} asked for {what}{case}. Held for a person: {}.",
                    reason(rule)
                ),
                "denied" => format!("{person} asked for {what}{case}. Denied: {}.", reason(rule)),
                "first_approved" => format!(
                    "First approval by {person}. A second, different person must approve it."
                ),
                "owner_approved" | "approver_approved" => format!(
                    "Approved by {person}, for exactly this payment (proposal {}).",
                    short(event["approved_digest"].as_str().unwrap_or(""))
                ),
                "owner_rejected" | "approver_rejected" => {
                    format!("Rejected by {person}. No money moved.")
                }
                "submission_started" => format!(
                    "Sent to the payment system (attempt {}). It can be paid at most once.",
                    short(event["attempt_key"].as_str().unwrap_or(""))
                ),
                "rail_result" if event["receipt"]["succeeded"] == true => format!(
                    "The payment system confirmed it ({}).",
                    event["receipt"]["rail_reference"].as_str().unwrap_or("no reference")
                ),
                "rail_result" => {
                    "The payment system declined it. No money moved; the hold was released.".into()
                }
                "rail_rejected" => "The payment system refused it on re-checking its records. No money moved; the hold was released.".into(),
                "rail_voided" => "Settled through the payment system: it confirmed this attempt will never go through. The hold was released.".into(),
                "submission_unknown" => "No answer from the payment system yet. The money stays held and the payment will not be sent again.".into(),
                other => format!("Recorded step: {other}."),
            };
            entries.push(Entry {
                seq,
                hash,
                at: record["at"].as_str().unwrap_or("").to_string(),
                step,
                who,
                summary,
            });
        }
        let waiting_second = op.first_approval.is_some();
        let (now, next): (String, Vec<String>) = match op.stage {
            OperationStage::Held if waiting_second => (
                "Waiting for a second person to approve.".into(),
                vec![
                    "A different person approves the exact proposal, or anyone rejects it.".into(),
                ],
            ),
            OperationStage::Held => (
                format!("Waiting for a person: {}.", reason(op.decision.rule())),
                vec![
                    "An approver within their limit, or the owner, approves the exact proposal."
                        .into(),
                    "Anyone authorized rejects it; no money moves.".into(),
                ],
            ),
            OperationStage::Approved => (
                "Approved; not yet sent to the payment system.".into(),
                vec!["The gateway sends it once (execute), or the owner rejects it first.".into()],
            ),
            OperationStage::Submitting | OperationStage::Unknown => (
                "Sent, but the payment system's answer is not known. The money stays held.".into(),
                vec![
                    "Look up the result (reconcile); nothing is sent again.".into(),
                    "The owner settles it through the payment system (resolve).".into(),
                ],
            ),
            OperationStage::Succeeded => ("Paid.".into(), Vec::new()),
            OperationStage::Failed => (
                "Not paid: the payment system did not complete it.".into(),
                Vec::new(),
            ),
            OperationStage::Denied => ("Denied by policy. No money moved.".into(), Vec::new()),
            OperationStage::Rejected => {
                ("Rejected by a person. No money moved.".into(), Vec::new())
            }
        };
        Ok(Timeline {
            operation: op.id.clone(),
            kind: format!("{:?}", request.kind).to_lowercase(),
            amount_minor: request.amount_minor,
            currency: request.currency.clone(),
            counterparty: request.source.clone().or(request.payee.clone()),
            case_id: request.case_id.clone(),
            stage: op.stage,
            entries,
            now,
            next,
            evidence_ok: self.verify_evidence()?.ok(),
        })
    }
}
