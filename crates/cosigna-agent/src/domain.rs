use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const API_VERSION: &str = "cosigna.agent/v1";
pub const MAX_MINOR: i64 = 1_000_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Planner,
    Executor,
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub owner: String,
    pub role: Role,
}

impl Actor {
    pub fn require(&self, roles: &[Role]) -> Result<()> {
        if roles.contains(&self.role) && !self.owner.is_empty() {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
    pub fn owns(&self, owner: &str) -> Result<()> {
        if self.owner == owner {
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetInput {
    pub currency: String,
    pub limit_minor: i64,
    pub per_purchase_minor: i64,
    pub merchants: Vec<String>,
    pub expires_at: i64,
}

impl BudgetInput {
    pub fn validate(&self, now: i64) -> Result<()> {
        if self.currency.len() != 3
            || !self.currency.bytes().all(|b| b.is_ascii_uppercase())
            || !(1..=MAX_MINOR).contains(&self.limit_minor)
            || !(1..=self.limit_minor).contains(&self.per_purchase_minor)
            || self.expires_at <= now
            || self.merchants.is_empty()
            || self.merchants.len() > 20
            || self.merchants.iter().any(|s| s.is_empty() || s.len() > 100)
        {
            return Err(Error::Invalid("invalid budget terms".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Budget {
    pub id: String,
    pub owner: String,
    pub terms: BudgetInput,
    pub revoked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Balance {
    pub budget: Budget,
    pub reserved_minor: i64,
    pub spent_minor: i64,
    pub available_minor: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub budget_id: String,
    pub request: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub required_features: Vec<String>,
}

impl TaskInput {
    pub fn validate(&self) -> Result<()> {
        if self.request.trim().is_empty()
            || self.request.len() > 4096
            || self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 128
            || !self.idempotency_key.is_ascii()
            || self.required_features.len() > 16
            || self
                .required_features
                .iter()
                .any(|f| f.is_empty() || f.len() > 80)
        {
            return Err(Error::Invalid("invalid task fields".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub id: String,
    pub merchant_id: String,
    pub title: String,
    pub currency: String,
    /// All-in unit cost; includes every fee for this fixture's fixed pickup basket.
    pub unit_total_minor: i64,
    pub features: Vec<String>,
    pub fulfillment: String,
    pub terms: String,
    pub revision: u64,
    pub simulated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub offer_id: Option<String>,
    pub quantity: u32,
    pub clarification: Option<String>,
}

impl Plan {
    pub fn validate(&self) -> Result<()> {
        match (&self.offer_id, &self.clarification) {
            (Some(id), None) if !id.is_empty() && (1..=10).contains(&self.quantity) => Ok(()),
            (None, Some(question))
                if !question.trim().is_empty() && question.len() <= 1000 && self.quantity == 0 =>
            {
                Ok(())
            }
            _ => Err(Error::Planner),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quote {
    pub id: String,
    pub task_id: String,
    pub offer: Offer,
    pub quantity: u32,
    pub total_minor: i64,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl Quote {
    pub fn digest(&self) -> Result<String> {
        Ok(blake3::hash(&serde_json::to_vec(self)?)
            .to_hex()
            .to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Draft,
    NeedsInput,
    Quoted,
    Approved,
    Submitting,
    Unknown,
    Confirmed,
    Declined,
    Cancelled,
}

impl Stage {
    pub fn has_attempt(self) -> bool {
        matches!(
            self,
            Self::Submitting | Self::Unknown | Self::Confirmed | Self::Declined
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub attempt_key: String,
    pub quote_digest: String,
    pub order_id: String,
    pub total_minor: i64,
    pub currency: String,
    pub accepted: bool,
    pub simulated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub owner: String,
    pub input: TaskInput,
    pub version: u64,
    pub stage: Stage,
    pub clarification: Option<String>,
    pub quote: Option<Quote>,
    pub approved_digest: Option<String>,
    pub attempt_key: Option<String>,
    pub receipt: Option<Receipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub sequence: i64,
    pub kind: String,
    pub at: i64,
}
