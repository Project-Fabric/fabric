//! A real HTTP boundary to the local test merchant. Deliberately cannot reach live sellers.
use std::{io::Write, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Offer, Quote, Receipt, Result,
    provider::{FixtureProvider, Outcome, Scenario},
};

pub const MERCHANT_SCHEMA: &str = "cosigna.test-merchant/v1";

#[derive(Clone)]
pub enum Provider {
    Fixture(FixtureProvider),
    TestMerchant(HttpTestMerchant),
}

impl Provider {
    pub fn offers(&self) -> Result<Vec<Offer>> {
        match self {
            Self::Fixture(provider) => provider.offers(),
            Self::TestMerchant(provider) => provider.offers(),
        }
    }
    pub fn submit(&self, key: &str, quote: &Quote, scenario: Scenario, at: i64) -> Result<Outcome> {
        match self {
            Self::Fixture(provider) => provider.submit(key, quote, scenario, at),
            Self::TestMerchant(provider) => provider.submit(key, quote, scenario),
        }
    }
    pub fn lookup(&self, key: &str) -> Result<Option<Receipt>> {
        match self {
            Self::Fixture(provider) => provider.lookup(key),
            Self::TestMerchant(provider) => provider.lookup(key),
        }
    }
    pub fn change_price(&self, id: &str, minor: i64) -> Result<()> {
        match self {
            Self::Fixture(provider) => provider.change_price(id, minor),
            _ => Err(Error::Forbidden),
        }
    }
    pub fn order_count(&self) -> Result<i64> {
        match self {
            Self::Fixture(provider) => provider.order_count(),
            _ => Err(Error::Forbidden),
        }
    }
}

#[derive(Clone)]
pub struct HttpTestMerchant {
    url: String,
    token: String,
    instance: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    format: String,
    version: u32,
    url: String,
    instance: String,
}

impl HttpTestMerchant {
    pub fn connect(directory: &Path, url: &str, token: String) -> Result<Self> {
        // Numeric loopback only, no paths, redirects, DNS, embedded credentials or public URLs.
        let port = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|value| value.parse::<u16>().ok());
        if !port.is_some_and(|port| port > 0) || token.len() < 32 || !token.is_ascii() {
            return Err(Error::Invalid(
                "test merchant requires a numeric loopback URL and a private token".into(),
            ));
        }
        let mut provider = Self {
            url: url.into(),
            token,
            instance: String::new(),
        };
        let info = provider.get("/merchant/v1/info")?;
        let instance = info["data"]["instance_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 100)
            .ok_or(Error::Merchant)?;
        if info["data"]["mode"] != "test" {
            return Err(Error::Merchant);
        }
        provider.instance = instance.to_string();
        std::fs::create_dir_all(directory)?;
        let marker = directory.join("test-merchant-connection.json");
        if marker.exists() {
            let saved: Connection = serde_json::from_slice(&std::fs::read(&marker)?)?;
            if saved.format != "cosigna-test-merchant-connection"
                || saved.version != 1
                || saved.url != provider.url
                || saved.instance != provider.instance
            {
                return Err(Error::Conflict(
                    "test merchant connection differs from this ledger's binding".into(),
                ));
            }
        } else {
            if directory.join("transactions.sqlite").exists() {
                return Err(Error::Conflict(
                    "use a fresh data directory for the HTTP test merchant".into(),
                ));
            }
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(marker)?;
            file.write_all(&serde_json::to_vec(&Connection {
                format: "cosigna-test-merchant-connection".into(),
                version: 1,
                url: provider.url.clone(),
                instance: provider.instance.clone(),
            })?)?;
            file.sync_all()?;
        }
        Ok(provider)
    }

    fn client(&self) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .build()
            .new_agent()
    }

    fn get(&self, path: &str) -> Result<Value> {
        let mut response = self
            .client()
            .get(format!("{}{path}", self.url))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("X-Cosigna-Merchant-Instance", &self.instance)
            .call()
            .map_err(|_| Error::Merchant)?;
        self.read(&mut response)
    }

    fn read(&self, response: &mut ureq::http::Response<ureq::Body>) -> Result<Value> {
        let value: Value = response
            .body_mut()
            .with_config()
            .limit(256 * 1024)
            .read_json()
            .map_err(|_| Error::Merchant)?;
        if value["schema"] != MERCHANT_SCHEMA {
            return Err(Error::Merchant);
        }
        Ok(value)
    }

    pub fn offers(&self) -> Result<Vec<Offer>> {
        let value = self.get("/merchant/v1/catalog")?;
        let offers: Vec<Offer> =
            serde_json::from_value(value["data"].clone()).map_err(|_| Error::Merchant)?;
        if offers.len() > 100
            || offers.iter().any(|offer| {
                !offer.simulated
                    || offer.merchant_id != "cosigna-test-cafe"
                    || offer.currency != "USD"
                    || !(1..=crate::MAX_MINOR).contains(&offer.unit_total_minor)
                    || offer.id.len() > 100
                    || offer.title.len() > 500
                    || offer.terms.len() > 2000
                    || offer.fulfillment.len() > 1000
                    || offer.features.len() > 30
            })
        {
            return Err(Error::Merchant);
        }
        Ok(offers)
    }

    fn submit(&self, key: &str, quote: &Quote, scenario: Scenario) -> Result<Outcome> {
        let mut response = self.client().post(format!("{}/merchant/v1/orders", self.url))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("X-Cosigna-Merchant-Instance", &self.instance)
            .send_json(json!({"attempt_key":key,"quote_digest":quote.digest()?,"quote":quote,"scenario":scenario}))
            .map_err(|_| Error::Merchant)?;
        let value = self.read(&mut response)?;
        match value["data"]["status"].as_str() {
            Some("observed") => Ok(Outcome::Observed(
                serde_json::from_value(value["data"]["receipt"].clone())
                    .map_err(|_| Error::Merchant)?,
            )),
            Some("rejected") => Ok(Outcome::Rejected),
            _ => Ok(Outcome::Unknown),
        }
    }

    fn lookup(&self, key: &str) -> Result<Option<Receipt>> {
        if !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return Err(Error::Merchant);
        }
        let value = self.get(&format!("/merchant/v1/attempts/{key}"))?;
        // Only an accepted/declined receipt resolves uncertainty. Absent/rejected lookup
        // does not release a possibly in-flight reservation.
        if value["data"]["status"] != "observed" {
            return Ok(None);
        }
        Ok(Some(
            serde_json::from_value(value["data"]["receipt"].clone())
                .map_err(|_| Error::Merchant)?,
        ))
    }
}
