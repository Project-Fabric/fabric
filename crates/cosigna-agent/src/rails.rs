//! Money-movement rails. A rail checks a request against its own records before anything is
//! decided, and executes an approved proposal at most once per attempt key. The fixture rail
//! is synthetic; no live payment system is linked here.
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::{
    Error, Result, new_id,
    operations::{OperationKind, OperationReceipt, OperationRequest, Proposal},
    provider::Scenario,
    storage,
};

/// What a rail reports about an attempt.
pub enum RailOutcome {
    Done(OperationReceipt),
    /// The rail refused the proposal before moving money (it no longer matches its records).
    Rejected,
    /// The rail has no result for the attempt and has recorded that it never will.
    Voided,
    Unknown,
}

/// The rail's verdict on a request: verified facts, or why it cannot be carried out.
pub enum Check {
    Valid(Value),
    Invalid(&'static str),
}

#[derive(Clone)]
pub struct Rails {
    fixture: Option<FixtureRail>,
}

impl Rails {
    pub(crate) fn fixture(path: &Path) -> Result<Self> {
        Ok(Self {
            fixture: Some(FixtureRail::open(path)?),
        })
    }

    pub(crate) fn none() -> Self {
        Self { fixture: None }
    }

    fn get(&self, rail: &str) -> Result<&FixtureRail> {
        match (rail, &self.fixture) {
            ("fixture", Some(fixture)) => Ok(fixture),
            _ => Err(Error::Invalid("this rail is not connected".into())),
        }
    }

    pub fn check(&self, request: &OperationRequest) -> Result<Check> {
        self.get(&request.rail)?.check(request)
    }

    pub fn submit(
        &self,
        key: &str,
        proposal: &Proposal,
        scenario: Scenario,
    ) -> Result<RailOutcome> {
        self.get(&proposal.rail)?.submit(key, proposal, scenario)
    }

    pub fn lookup(&self, rail: &str, key: &str) -> Result<Option<OperationReceipt>> {
        self.get(rail)?.lookup(key)
    }

    pub fn void(&self, key: &str, proposal: &Proposal) -> Result<RailOutcome> {
        self.get(&proposal.rail)?.void(key, &proposal.digest()?)
    }

    /// Fixture control for tests: synthetic payments and payees, never exposed to agents.
    pub fn fixture_rail(&self) -> Option<&FixtureRail> {
        self.fixture.as_ref()
    }
}

#[derive(Clone)]
pub struct FixtureRail {
    path: PathBuf,
}

pub(crate) const FIXTURE_SCHEMA: &str = "CREATE TABLE fixture_payments (
    id TEXT PRIMARY KEY, currency TEXT NOT NULL, amount_minor INTEGER NOT NULL CHECK(amount_minor>0),
    refunded_minor INTEGER NOT NULL DEFAULT 0 CHECK(refunded_minor>=0), customer TEXT NOT NULL);
    CREATE TABLE fixture_payees (id TEXT PRIMARY KEY, currency TEXT NOT NULL, added_at INTEGER NOT NULL);
    CREATE TABLE money_attempts (attempt_key TEXT PRIMARY KEY, digest TEXT NOT NULL, data TEXT NOT NULL);
    CREATE TABLE money_voids (attempt_key TEXT PRIMARY KEY, digest TEXT NOT NULL);";

impl FixtureRail {
    fn open(path: &Path) -> Result<Self> {
        let mut db = storage::connect(path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (id, currency, amount, customer) in [
            ("pay_fixture_2041", "USD", 2000, "cus_fixture_ana"),
            ("pay_fixture_2045", "USD", 45000, "cus_fixture_ben"),
            ("pay_fixture_2047", "USD", 6000, "cus_fixture_chloe"),
            ("pay_fixture_2049", "USD", 64000, "cus_fixture_dev"),
            ("pay_fixture_2051", "USD", 9900, "cus_fixture_eli"),
            ("pay_fixture_3001", "INR", 150000, "cus_fixture_farah"),
        ] {
            tx.execute(
                "INSERT OR IGNORE INTO fixture_payments(id,currency,amount_minor,customer) VALUES (?1,?2,?3,?4)",
                params![id, currency, amount, customer],
            )?;
        }
        for (id, currency) in [
            ("payee_fixture_bean_supply", "USD"),
            ("payee_fixture_cup_co", "USD"),
            ("payee_fixture_chai_wholesale", "INR"),
        ] {
            tx.execute(
                "INSERT OR IGNORE INTO fixture_payees(id,currency,added_at) VALUES (?1,?2,0)",
                params![id, currency],
            )?;
        }
        tx.commit()?;
        Ok(Self {
            path: path.to_owned(),
        })
    }

    fn check(&self, request: &OperationRequest) -> Result<Check> {
        let db = storage::connect(&self.path, true)?;
        Ok(match request.kind {
            OperationKind::Refund => {
                let source = request.source.as_deref().unwrap_or_default();
                let payment: Option<(String, i64, i64, String)> = db
                    .query_row(
                        "SELECT currency,amount_minor,refunded_minor,customer FROM fixture_payments WHERE id=?1",
                        [source],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?;
                match payment {
                    None => Check::Invalid("source_not_found"),
                    Some((currency, ..)) if currency != request.currency => {
                        Check::Invalid("currency_mismatch")
                    }
                    Some((_, amount, refunded, _)) if request.amount_minor > amount - refunded => {
                        Check::Invalid("exceeds_refundable")
                    }
                    Some((_, amount, refunded, customer)) => Check::Valid(json!({
                        "payment_amount_minor": amount, "refunded_minor": refunded,
                        "refundable_minor": amount - refunded, "customer": customer})),
                }
            }
            OperationKind::Payout => {
                let payee = request.payee.as_deref().unwrap_or_default();
                let found: Option<(String, i64)> = db
                    .query_row(
                        "SELECT currency,added_at FROM fixture_payees WHERE id=?1",
                        [payee],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                match found {
                    None => Check::Invalid("payee_unknown"),
                    Some((currency, _)) if currency != request.currency => {
                        Check::Invalid("currency_mismatch")
                    }
                    Some((_, added_at)) => {
                        Check::Valid(json!({"payee_known": true, "payee_added_at": added_at}))
                    }
                }
            }
        })
    }

    fn submit(&self, key: &str, proposal: &Proposal, scenario: Scenario) -> Result<RailOutcome> {
        let digest = proposal.digest()?;
        let mut db = storage::connect(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = existing(&tx, key, &digest)? {
            return Ok(outcome);
        }
        if tx
            .query_row(
                "SELECT 1 FROM money_voids WHERE attempt_key=?1",
                [key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some()
        {
            return Ok(RailOutcome::Voided);
        }
        // Re-check against current records: a proposal approved earlier may no longer hold.
        let still_valid = match proposal.kind {
            OperationKind::Refund => tx
                .query_row(
                    "SELECT currency,amount_minor-refunded_minor FROM fixture_payments WHERE id=?1",
                    [proposal.source.as_deref().unwrap_or_default()],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?
                .is_some_and(|(currency, refundable)| {
                    currency == proposal.currency && proposal.amount_minor <= refundable
                }),
            OperationKind::Payout => tx
                .query_row(
                    "SELECT currency FROM fixture_payees WHERE id=?1",
                    [proposal.payee.as_deref().unwrap_or_default()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .is_some_and(|currency| currency == proposal.currency),
        };
        if !still_valid {
            return Ok(RailOutcome::Rejected);
        }
        if matches!(scenario, Scenario::TimeoutBeforeAccept) {
            return Ok(RailOutcome::Unknown);
        }
        let succeeded = !matches!(scenario, Scenario::Decline);
        let receipt = OperationReceipt {
            attempt_key: key.to_owned(),
            proposal_digest: digest.clone(),
            rail_reference: new_id(match proposal.kind {
                OperationKind::Refund => "re_fixture",
                OperationKind::Payout => "po_fixture",
            })?,
            amount_minor: proposal.amount_minor,
            currency: proposal.currency.clone(),
            succeeded,
            simulated: true,
        };
        if succeeded && proposal.kind == OperationKind::Refund {
            tx.execute(
                "UPDATE fixture_payments SET refunded_minor=refunded_minor+?1 WHERE id=?2",
                params![proposal.amount_minor, proposal.source],
            )?;
        }
        tx.execute(
            "INSERT INTO money_attempts(attempt_key,digest,data) VALUES (?1,?2,?3)",
            params![key, digest, serde_json::to_string(&receipt)?],
        )?;
        tx.commit()?;
        Ok(if matches!(scenario, Scenario::TimeoutAfterAccept) {
            RailOutcome::Unknown
        } else {
            RailOutcome::Done(receipt)
        })
    }

    fn lookup(&self, key: &str) -> Result<Option<OperationReceipt>> {
        let db = storage::connect(&self.path, true)?;
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM money_attempts WHERE attempt_key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(data.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    fn void(&self, key: &str, digest: &str) -> Result<RailOutcome> {
        let mut db = storage::connect(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = existing(&tx, key, digest)? {
            return Ok(outcome);
        }
        tx.execute(
            "INSERT OR IGNORE INTO money_voids(attempt_key,digest) VALUES (?1,?2)",
            params![key, digest],
        )?;
        tx.commit()?;
        Ok(RailOutcome::Voided)
    }

    /// Test control: the refunded total of a synthetic payment.
    pub fn refunded_minor(&self, payment: &str) -> Result<i64> {
        let db = storage::connect(&self.path, true)?;
        db.query_row(
            "SELECT refunded_minor FROM fixture_payments WHERE id=?1",
            [payment],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)
    }

    /// Test control: the number of recorded attempts (money actually moved or declined).
    pub fn attempt_count(&self) -> Result<i64> {
        let db = storage::connect(&self.path, true)?;
        Ok(db.query_row("SELECT COUNT(*) FROM money_attempts", [], |r| r.get(0))?)
    }
}

fn existing(db: &rusqlite::Connection, key: &str, digest: &str) -> Result<Option<RailOutcome>> {
    let found: Option<(String, String)> = db
        .query_row(
            "SELECT digest,data FROM money_attempts WHERE attempt_key=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match found {
        Some((prior, _)) if prior != digest => {
            Err(Error::Conflict("rail idempotency payload changed".into()))
        }
        Some((_, data)) => Ok(Some(RailOutcome::Done(serde_json::from_str(&data)?))),
        None => Ok(None),
    }
}
