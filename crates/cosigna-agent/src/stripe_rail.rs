//! Stripe test-mode refunds as a money rail. The gateway holds the key; agents never see it.
//!
//! Live keys are refused. Requests go only to `https://api.stripe.com` (or a numeric
//! loopback address for tests), so no configuration or request can redirect the key.
//! Every create uses an idempotency key derived from the attempt, and every result is
//! confirmed by retrieving the refund and checking it against the approved proposal.
use std::time::Duration;

use serde_json::{Value, json};

use crate::{
    Error, Result,
    operations::{OperationKind, OperationReceipt, OperationRequest, Proposal},
    rails::{Check, RailOutcome},
};

const STRIPE_API: &str = "https://api.stripe.com";
/// Stripe keeps idempotency keys for 24 hours; stay inside that with a margin.
pub const RETRY_WINDOW: i64 = 23 * 3600;
const MAX_BODY: u64 = 512 * 1024;
/// Pages of 100 refunds read per comparison with the ledger.
const MAX_RECONCILE_PAGES: usize = 10;

#[derive(Clone)]
pub struct StripeRail {
    key: String,
    base: String,
}

enum Reply {
    Ok(Value),
    NotFound,
    /// Stripe refused the request as invalid (for example, more than is refundable).
    Refused,
    /// No reliable answer: network failure, rate limit, server error or a concurrent retry.
    Unclear,
}

impl StripeRail {
    pub fn new(key: &str, base: Option<&str>) -> Result<Self> {
        let key = key.trim();
        let test_key = ["sk_test_", "rk_test_"].iter().any(|prefix| {
            key.strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty())
        }) && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !test_key {
            return Err(Error::Invalid(
                "only Stripe test-mode secret or restricted keys are accepted".into(),
            ));
        }
        let base = match base {
            None => STRIPE_API.to_string(),
            Some(base)
                if base
                    .strip_prefix("http://127.0.0.1:")
                    .and_then(|port| port.parse::<u16>().ok())
                    .is_some_and(|port| port > 0) =>
            {
                base.to_string()
            }
            Some(_) => {
                return Err(Error::Invalid(
                    "the Stripe API address can only be overridden with a loopback test address"
                        .into(),
                ));
            }
        };
        Ok(Self {
            key: key.into(),
            base,
        })
    }

    fn client() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent()
    }

    fn read(response: Result<ureq::http::Response<ureq::Body>>) -> Reply {
        let Ok(mut response) = response else {
            return Reply::Unclear;
        };
        let status = response.status().as_u16();
        let body: Option<Value> = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_json()
            .ok();
        match (status, body) {
            (200, Some(body)) => Reply::Ok(body),
            (404, _) => Reply::NotFound,
            // A definitive validation refusal. Idempotency and rate errors are not definitive.
            (400 | 402, Some(body))
                if body["error"]["type"] == "invalid_request_error"
                    || body["error"]["type"] == "card_error" =>
            {
                Reply::Refused
            }
            _ => Reply::Unclear,
        }
    }

    fn get(&self, path: &str) -> Reply {
        Self::read(
            Self::client()
                .get(format!("{}{path}", self.base))
                .header("Authorization", format!("Bearer {}", self.key))
                .call()
                .map_err(|_| Error::Merchant),
        )
    }

    fn create_refund(&self, key: &str, proposal: &Proposal, digest: &str) -> Reply {
        let amount = proposal.amount_minor.to_string();
        let source = proposal.source.clone().unwrap_or_default();
        Self::read(
            Self::client()
                .post(format!("{}/v1/refunds", self.base))
                .header("Authorization", format!("Bearer {}", self.key))
                .header("Idempotency-Key", format!("cosigna:{key}"))
                .send_form([
                    ("payment_intent", source.as_str()),
                    ("amount", amount.as_str()),
                    ("metadata[cosigna_attempt]", key),
                    ("metadata[cosigna_proposal_digest]", digest),
                    (
                        "metadata[cosigna_operation]",
                        proposal.operation_id.as_str(),
                    ),
                ])
                .map_err(|_| Error::Merchant),
        )
    }

    pub(crate) fn check(&self, request: &OperationRequest) -> Result<Check> {
        if request.kind != OperationKind::Refund {
            return Ok(Check::Invalid("kind_not_supported_on_rail"));
        }
        let source = request.source.as_deref().unwrap_or_default();
        if !source.starts_with("pi_") {
            return Ok(Check::Invalid("source_not_found"));
        }
        let intent = match self.get(&format!("/v1/payment_intents/{source}")) {
            Reply::Ok(intent) => intent,
            Reply::NotFound => return Ok(Check::Invalid("source_not_found")),
            _ => return Err(Error::Merchant),
        };
        if intent["object"] != "payment_intent" || intent["id"] != source {
            return Err(Error::Merchant);
        }
        if intent["livemode"] != false {
            return Ok(Check::Invalid("live_mode_refused"));
        }
        if intent["status"] != "succeeded" {
            return Ok(Check::Invalid("payment_not_settled"));
        }
        if intent["currency"]
            .as_str()
            .map(str::to_ascii_uppercase)
            .as_deref()
            != Some(request.currency.as_str())
        {
            return Ok(Check::Invalid("currency_mismatch"));
        }
        let received = intent["amount_received"].as_i64().ok_or(Error::Merchant)?;
        let refunded = self.refunded(source)?;
        if request.amount_minor > received - refunded {
            return Ok(Check::Invalid("exceeds_refundable"));
        }
        Ok(Check::Valid(json!({
            "payment_amount_minor": received, "refunded_minor": refunded,
            "refundable_minor": received - refunded,
            "customer": intent["customer"].as_str(), "livemode": false})))
    }

    fn refunds(&self, source: &str) -> Result<Vec<Value>> {
        let list = match self.get(&format!("/v1/refunds?payment_intent={source}&limit=100")) {
            Reply::Ok(list) => list,
            _ => return Err(Error::Merchant),
        };
        // More than one page could hide refunds: refuse to guess what is left to refund.
        if list["object"] != "list" || list["has_more"] != false {
            return Err(Error::Merchant);
        }
        Ok(list["data"].as_array().cloned().unwrap_or_default())
    }

    /// Refunds created at or after `from`, newest first, and whether that is all of them. A very
    /// long history is cut off rather than read without limit; the caller reports that.
    pub(crate) fn refunds_since(&self, from: i64) -> Result<(Vec<Value>, bool)> {
        let mut refunds = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_RECONCILE_PAGES {
            let cursor = after
                .as_ref()
                .map(|id| format!("&starting_after={id}"))
                .unwrap_or_default();
            let path = format!("/v1/refunds?limit=100&created%5Bgte%5D={from}{cursor}");
            let Reply::Ok(list) = self.get(&path) else {
                return Err(Error::Merchant);
            };
            let (Some(page), Some(more)) = (list["data"].as_array(), list["has_more"].as_bool())
            else {
                return Err(Error::Merchant);
            };
            if list["object"] != "list" {
                return Err(Error::Merchant);
            }
            refunds.extend(page.iter().cloned());
            if !more {
                return Ok((refunds, true));
            }
            // The cursor goes into a URL: only a real refund id may continue the listing.
            after = match page.last().and_then(|r| r["id"].as_str()) {
                Some(id)
                    if id.starts_with("re_")
                        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
                {
                    Some(id.to_string())
                }
                _ => return Err(Error::Merchant),
            };
        }
        Ok((refunds, false))
    }

    /// Money already refunded or on its way: pending refunds count against what is left.
    fn refunded(&self, source: &str) -> Result<i64> {
        let mut total = 0i64;
        for refund in self.refunds(source)? {
            if matches!(
                refund["status"].as_str(),
                Some("succeeded" | "pending" | "requires_action")
            ) {
                total = total
                    .checked_add(refund["amount"].as_i64().ok_or(Error::Merchant)?)
                    .ok_or(Error::Merchant)?;
            }
        }
        Ok(total)
    }

    /// Settles a refund object against the approved proposal; anything that does not match
    /// exactly, or is still in flight, stays unknown.
    fn settle(key: &str, proposal: &Proposal, digest: &str, refund: &Value) -> RailOutcome {
        let matches = refund["object"] == "refund"
            && refund["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("re_"))
            && refund["amount"].as_i64() == Some(proposal.amount_minor)
            && refund["currency"]
                .as_str()
                .map(str::to_ascii_uppercase)
                .as_deref()
                == Some(proposal.currency.as_str())
            && refund["payment_intent"].as_str() == proposal.source.as_deref()
            && refund["metadata"]["cosigna_attempt"] == key
            && refund["metadata"]["cosigna_proposal_digest"] == digest;
        if !matches {
            return RailOutcome::Unknown;
        }
        let succeeded = match refund["status"].as_str() {
            Some("succeeded") => true,
            Some("failed" | "canceled") => false,
            _ => return RailOutcome::Unknown,
        };
        RailOutcome::Done(OperationReceipt {
            attempt_key: key.into(),
            proposal_digest: digest.into(),
            rail_reference: refund["id"].as_str().unwrap_or_default().into(),
            amount_minor: proposal.amount_minor,
            currency: proposal.currency.clone(),
            succeeded,
            // Stripe test mode moves no real funds.
            simulated: true,
        })
    }

    pub(crate) fn submit(&self, key: &str, proposal: &Proposal) -> Result<RailOutcome> {
        let digest = proposal.digest()?;
        Ok(match self.create_refund(key, proposal, &digest) {
            Reply::Ok(created) => match created["id"].as_str() {
                // Confirm by retrieving the refund: the create response alone settles nothing.
                Some(id) if id.starts_with("re_") => match self.get(&format!("/v1/refunds/{id}")) {
                    Reply::Ok(refund) => Self::settle(key, proposal, &digest, &refund),
                    _ => RailOutcome::Unknown,
                },
                _ => RailOutcome::Unknown,
            },
            Reply::Refused => RailOutcome::Rejected,
            _ => RailOutcome::Unknown,
        })
    }

    pub(crate) fn lookup(
        &self,
        key: &str,
        proposal: &Proposal,
    ) -> Result<Option<OperationReceipt>> {
        let digest = proposal.digest()?;
        let source = proposal.source.as_deref().unwrap_or_default();
        for refund in self.refunds(source)? {
            if refund["metadata"]["cosigna_attempt"] == key {
                return Ok(match Self::settle(key, proposal, &digest, &refund) {
                    RailOutcome::Done(receipt) => Some(receipt),
                    _ => None,
                });
            }
        }
        Ok(None)
    }

    /// Stripe cannot void a refund that was never created. Within the idempotency window,
    /// repeating the original request with its key returns the refund if it exists, or
    /// completes the approved refund exactly once. After the window it stays unknown.
    pub(crate) fn settle_unknown(
        &self,
        key: &str,
        proposal: &Proposal,
        submitted_at: Option<i64>,
        now: i64,
    ) -> Result<RailOutcome> {
        if let Some(receipt) = self.lookup(key, proposal)? {
            return Ok(RailOutcome::Done(receipt));
        }
        match submitted_at {
            Some(at) if now.saturating_sub(at) <= RETRY_WINDOW => self.submit(key, proposal),
            _ => Ok(RailOutcome::Unknown),
        }
    }
}
