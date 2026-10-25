//! Money-movement rails. A rail checks a request against its own records before anything is
//! decided, and executes an approved proposal at most once per attempt key. The fixture rail
//! is synthetic; no live payment system is linked here.
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::{
    Error, Result, new_id,
    operations::{Case, OperationKind, OperationReceipt, OperationRequest, Proposal},
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
    stripe_test: Option<crate::stripe_rail::StripeRail>,
}

enum Rail<'a> {
    Fixture(&'a FixtureRail),
    StripeTest(&'a crate::stripe_rail::StripeRail),
}

impl Rails {
    pub(crate) fn fixture(path: &Path) -> Result<Self> {
        Ok(Self {
            fixture: Some(FixtureRail::open(path)?),
            stripe_test: None,
        })
    }

    pub(crate) fn none() -> Self {
        Self {
            fixture: None,
            stripe_test: None,
        }
    }

    /// Connects Stripe test-mode refunds as the `stripe_test` rail.
    pub fn with_stripe_test(mut self, rail: crate::stripe_rail::StripeRail) -> Self {
        self.stripe_test = Some(rail);
        self
    }

    fn get(&self, rail: &str) -> Result<Rail<'_>> {
        match (rail, &self.fixture, &self.stripe_test) {
            ("fixture", Some(fixture), _) => Ok(Rail::Fixture(fixture)),
            ("stripe_test", _, Some(stripe)) => Ok(Rail::StripeTest(stripe)),
            _ => Err(Error::Invalid("this rail is not connected".into())),
        }
    }

    pub fn check(&self, request: &OperationRequest) -> Result<Check> {
        match self.get(&request.rail)? {
            Rail::Fixture(rail) => rail.check(request),
            Rail::StripeTest(rail) => rail.check(request),
        }
    }

    /// `scenario` only affects the synthetic fixture rail.
    pub fn submit(
        &self,
        key: &str,
        proposal: &Proposal,
        scenario: Scenario,
    ) -> Result<RailOutcome> {
        match self.get(&proposal.rail)? {
            Rail::Fixture(rail) => rail.submit(key, proposal, scenario),
            Rail::StripeTest(rail) => rail.submit(key, proposal),
        }
    }

    pub fn lookup(&self, key: &str, proposal: &Proposal) -> Result<Option<OperationReceipt>> {
        match self.get(&proposal.rail)? {
            Rail::Fixture(rail) => rail.lookup(key),
            Rail::StripeTest(rail) => rail.lookup(key, proposal),
        }
    }

    /// Settles an unknown attempt through the rail: its result if it has one, otherwise
    /// whatever the rail can guarantee (a void, or an idempotent completion on Stripe).
    pub fn settle_unknown(
        &self,
        key: &str,
        proposal: &Proposal,
        submitted_at: Option<i64>,
        now: i64,
    ) -> Result<RailOutcome> {
        match self.get(&proposal.rail)? {
            Rail::Fixture(rail) => rail.void(key, &proposal.digest()?),
            Rail::StripeTest(rail) => rail.settle_unknown(key, proposal, submitted_at, now),
        }
    }

    /// The case a refund names, from the connected system of record. Today that is the
    /// synthetic case book next to the fixture rail; a help-desk or store connector replaces it.
    pub fn case(&self, id: &str) -> Result<Option<Case>> {
        match &self.fixture {
            Some(fixture) => fixture.case(id),
            None => Err(Error::Invalid("no case records are connected".into())),
        }
    }

    /// Every case in the connected system of record. None connected: no case can be paid.
    pub fn cases(&self) -> Result<Vec<Case>> {
        match &self.fixture {
            Some(fixture) => fixture.cases(),
            None => Ok(Vec::new()),
        }
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

/// Synthetic case records (provider version 4): what each customer is owed, on which payment.
pub(crate) const CASE_SCHEMA: &str = "CREATE TABLE fixture_cases (
    id TEXT PRIMARY KEY, customer TEXT NOT NULL, source TEXT NOT NULL, currency TEXT NOT NULL,
    owed_minor INTEGER NOT NULL CHECK(owed_minor>0));";

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
        // Each case is what the business's records say the customer is owed.
        for (id, customer, source, currency, owed) in [
            (
                "case_fixture_2041",
                "cus_fixture_ana",
                "pay_fixture_2041",
                "USD",
                2000,
            ),
            (
                "case_fixture_2045",
                "cus_fixture_ben",
                "pay_fixture_2045",
                "USD",
                45000,
            ),
            (
                "case_fixture_2047",
                "cus_fixture_chloe",
                "pay_fixture_2047",
                "USD",
                500,
            ),
            (
                "case_fixture_2051",
                "cus_fixture_eli",
                "pay_fixture_2051",
                "USD",
                2500,
            ),
        ] {
            tx.execute(
                "INSERT OR IGNORE INTO fixture_cases(id,customer,source,currency,owed_minor) VALUES (?1,?2,?3,?4,?5)",
                params![id, customer, source, currency, owed],
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

    fn case(&self, id: &str) -> Result<Option<Case>> {
        let db = storage::connect(&self.path, true)?;
        Ok(db
            .query_row(
                "SELECT id,customer,source,currency,owed_minor FROM fixture_cases WHERE id=?1",
                [id],
                |r| {
                    Ok(Case {
                        id: r.get(0)?,
                        customer: r.get(1)?,
                        source: r.get(2)?,
                        currency: r.get(3)?,
                        owed_minor: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    fn cases(&self) -> Result<Vec<Case>> {
        let db = storage::connect(&self.path, true)?;
        let mut statement = db.prepare(
            "SELECT id,customer,source,currency,owed_minor FROM fixture_cases ORDER BY id",
        )?;
        Ok(statement
            .query_map([], |r| {
                Ok(Case {
                    id: r.get(0)?,
                    customer: r.get(1)?,
                    source: r.get(2)?,
                    currency: r.get(3)?,
                    owed_minor: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?)
    }

    /// Test control: adds a synthetic case record.
    pub fn add_case(&self, case: &Case) -> Result<()> {
        storage::connect(&self.path, true)?.execute(
            "INSERT INTO fixture_cases(id,customer,source,currency,owed_minor) VALUES (?1,?2,?3,?4,?5)",
            params![case.id, case.customer, case.source, case.currency, case.owed_minor],
        )?;
        Ok(())
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
