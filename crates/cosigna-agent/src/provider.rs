//! A durable simulated merchant. No live payment/booking implementation is linked.
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Offer, Quote, Receipt, Result, new_id, storage};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    #[default]
    Success,
    Decline,
    TimeoutAfterAccept,
    TimeoutBeforeAccept,
}

pub enum Outcome {
    Observed(Receipt),
    Rejected,
    /// The provider has no result for the attempt and has recorded that it never will:
    /// a late copy of the submission is refused. Only an owner's `resolve` asks for this.
    Voided,
    Unknown,
}

#[derive(Clone)]
pub struct FixtureProvider {
    path: PathBuf,
}

impl FixtureProvider {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = storage::connect(path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (id, title, price, features) in [
            (
                "oat-latte",
                "Example oat latte",
                700,
                vec!["oat_milk", "pickup"],
            ),
            (
                "black-coffee",
                "Example black coffee",
                450,
                vec!["black_coffee", "pickup"],
            ),
        ] {
            let offer = Offer {
                id: id.into(),
                merchant_id: "fixture-cafe".into(),
                title: title.into(),
                currency: "USD".into(),
                unit_total_minor: price,
                features: features.into_iter().map(str::to_string).collect(),
                fulfillment: "Pickup at Example Cafe, location fixture-001".into(),
                terms:
                    "Synthetic item and pickup. All fees included; no real fulfillment or charge."
                        .into(),
                revision: 1,
                simulated: true,
            };
            tx.execute(
                "INSERT OR IGNORE INTO offers(id,data) VALUES (?1,?2)",
                params![id, serde_json::to_string(&offer)?],
            )?;
        }
        tx.commit()?;
        Ok(Self {
            path: path.to_owned(),
        })
    }

    pub fn offers(&self) -> Result<Vec<Offer>> {
        let db = storage::connect(&self.path, true)?;
        let mut stmt = db.prepare("SELECT data FROM offers ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// Fixture control for testing changed quotes. Never exposed to model tools.
    pub fn change_price(&self, id: &str, minor: i64) -> Result<()> {
        if !(1..=crate::MAX_MINOR).contains(&minor) {
            return Err(Error::Invalid("invalid price".into()));
        }
        let mut db = storage::connect(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data: String = tx
            .query_row("SELECT data FROM offers WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound)?;
        let mut offer: Offer = serde_json::from_str(&data)?;
        offer.unit_total_minor = minor;
        offer.revision += 1;
        tx.execute(
            "UPDATE offers SET data=?1 WHERE id=?2",
            params![serde_json::to_string(&offer)?, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn submit(
        &self,
        key: &str,
        quote: &Quote,
        scenario: Scenario,
        now: i64,
    ) -> Result<Outcome> {
        let mut db = storage::connect(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest = quote.digest()?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT digest,data FROM orders WHERE attempt_key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((prior_digest, data)) = existing {
            if digest != prior_digest {
                return Err(Error::Conflict(
                    "provider idempotency payload changed".into(),
                ));
            }
            return Ok(Outcome::Observed(serde_json::from_str(&data)?));
        }
        if voided(&tx, key)? {
            return Ok(Outcome::Voided);
        }
        let data: Option<String> = tx
            .query_row(
                "SELECT data FROM offers WHERE id=?1",
                [&quote.offer.id],
                |r| r.get(0),
            )
            .optional()?;
        let current: Option<Offer> = data.map(|s| serde_json::from_str(&s)).transpose()?;
        if current.as_ref() != Some(&quote.offer)
            || quote.expires_at <= now
            || !(1..=10).contains(&quote.quantity)
            || quote
                .offer
                .unit_total_minor
                .checked_mul(i64::from(quote.quantity))
                != Some(quote.total_minor)
            || !quote.offer.simulated
        {
            return Ok(Outcome::Rejected);
        }
        if matches!(scenario, Scenario::TimeoutBeforeAccept) {
            return Ok(Outcome::Unknown);
        }
        let receipt = Receipt {
            attempt_key: key.to_owned(),
            quote_digest: digest,
            order_id: new_id("fixture_order")?,
            total_minor: quote.total_minor,
            currency: quote.offer.currency.clone(),
            accepted: !matches!(scenario, Scenario::Decline),
            simulated: true,
        };
        tx.execute(
            "INSERT INTO orders(attempt_key,digest,data) VALUES (?1,?2,?3)",
            params![key, receipt.quote_digest, serde_json::to_string(&receipt)?],
        )?;
        tx.commit()?;
        Ok(if matches!(scenario, Scenario::TimeoutAfterAccept) {
            Outcome::Unknown
        } else {
            Outcome::Observed(receipt)
        })
    }

    /// Settles an attempt with no known result: returns its receipt if it was accepted,
    /// otherwise records that it never will be, so a late submission is refused.
    pub fn void(&self, key: &str, digest: &str) -> Result<Outcome> {
        let mut db = storage::connect(&self.path, true)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT digest,data FROM orders WHERE attempt_key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((prior_digest, data)) = existing {
            if prior_digest != digest {
                return Err(Error::Conflict(
                    "provider idempotency payload changed".into(),
                ));
            }
            return Ok(Outcome::Observed(serde_json::from_str(&data)?));
        }
        tx.execute(
            "INSERT OR IGNORE INTO voids(attempt_key,digest) VALUES (?1,?2)",
            params![key, digest],
        )?;
        tx.commit()?;
        Ok(Outcome::Voided)
    }

    pub fn lookup(&self, key: &str) -> Result<Option<Receipt>> {
        let db = storage::connect(&self.path, true)?;
        let data: Option<String> = db
            .query_row("SELECT data FROM orders WHERE attempt_key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(data.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub fn void_count(&self) -> Result<i64> {
        let db = storage::connect(&self.path, true)?;
        Ok(db.query_row("SELECT COUNT(*) FROM voids", [], |r| r.get(0))?)
    }

    pub fn order_count(&self) -> Result<i64> {
        let db = storage::connect(&self.path, true)?;
        Ok(db.query_row("SELECT COUNT(*) FROM orders", [], |r| r.get(0))?)
    }
}

fn voided(db: &rusqlite::Connection, key: &str) -> Result<bool> {
    Ok(db
        .query_row("SELECT 1 FROM voids WHERE attempt_key=?1", [key], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
        .is_some())
}
