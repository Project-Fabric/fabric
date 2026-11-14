//! Stablecoin refunds on an EVM test network as a money rail (`evm_test`). Test networks only:
//! the chain must be a known test chain, and the node must agree, before anything is signed.
//!
//! A refund goes back to whoever paid: the source is the payment's transaction, and the
//! recipient is the address its token transfer came from, bound into the approved proposal.
//!
//! At most once: a transaction with a given account nonce can be included only once, and only
//! one transaction per nonce can be included. Each attempt's transaction is signed with its own
//! nonce and stored durably before it is broadcast. After that, only those exact signed bytes
//! are ever sent again, so a lost answer or a crash can delay a refund but never pay it twice.
//! An attempt whose nonce was taken by another transaction can never be included, and is void.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use k256::ecdsa::{RecoveryId, SigningKey};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha3::{Digest, Keccak256};

use crate::{
    Error, Result,
    operations::{OperationKind, OperationReceipt, OperationRequest, Proposal},
    rails::{Check, RailOutcome},
};

/// The configuration file's schema.
pub const CONFIG_SCHEMA: &str = "cosigna.evm-rail/v1";
/// Chains this rail will use: local development chains and public test networks.
pub const TEST_CHAINS: [(u64, &str); 5] = [
    (31_337, "local"),
    (11_155_111, "sepolia"),
    (84_532, "base-sepolia"),
    (421_614, "arbitrum-sepolia"),
    (11_155_420, "optimism-sepolia"),
];
const STORE_APP: i64 = 0x4353_4556;
const STORE_VERSION: i64 = 1;
const STORE_SCHEMA: &str = "
    CREATE TABLE evm_attempts (
        attempt_key TEXT PRIMARY KEY, digest TEXT NOT NULL, chain_id INTEGER NOT NULL,
        sender TEXT NOT NULL, nonce INTEGER NOT NULL, source TEXT NOT NULL,
        recipient TEXT NOT NULL, amount_units TEXT NOT NULL, tx_hash TEXT NOT NULL UNIQUE,
        raw_tx TEXT NOT NULL, created_at INTEGER NOT NULL,
        outcome TEXT CHECK(outcome IN ('succeeded','failed','voided')),
        UNIQUE(chain_id, sender, nonce));
    CREATE INDEX evm_attempts_source ON evm_attempts(chain_id, source);
    CREATE TABLE evm_voids (attempt_key TEXT PRIMARY KEY, digest TEXT NOT NULL);";
/// keccak256("Transfer(address,address,uint256)")
const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
/// Gas for one ERC-20 transfer, with room to spare; unused gas is not charged.
const GAS_LIMIT: u128 = 120_000;
const MAX_BODY: u64 = 512 * 1024;
const MAX_CONFIG: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    schema: String,
    chain_id: u64,
    /// Node addresses often carry an API key, so the address comes from the environment.
    rpc_url_env: String,
    /// The stablecoin contract: `0x` and 40 hex digits.
    token: String,
    /// The token's decimals; USDC has 6. At least 2, since amounts are in cents.
    #[serde(default)]
    decimals: Option<u8>,
    /// Blocks a transaction needs on top before it counts as final. Default 3.
    #[serde(default)]
    confirmations: Option<u64>,
    /// Environment variable holding the paying account's private key (hex).
    private_key_env: String,
}

#[derive(Clone)]
pub struct EvmRail {
    chain_id: u64,
    rpc: String,
    token: String,
    decimals: u8,
    confirmations: u64,
    key: SigningKey,
    /// The paying account, lowercase `0x` hex.
    address: String,
    store: PathBuf,
}

fn bad(message: &str) -> Error {
    Error::Invalid(format!("evm rail configuration: {message}"))
}

pub(crate) fn keccak(bytes: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytes).into()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::from("0x");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() % 2 != 0 {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `0x` and exactly `len` hex digits, lowercased.
fn hex_id(text: &str, len: usize) -> Option<String> {
    let digits = text.strip_prefix("0x")?;
    (digits.len() == len && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("0x{}", digits.to_ascii_lowercase()))
}

/// A JSON-RPC quantity (`0x`-prefixed, no leading zeros required).
fn quantity(value: &Value) -> Option<u128> {
    let digits = value.as_str()?.strip_prefix("0x")?;
    if digits.is_empty() || digits.len() > 32 {
        return None;
    }
    u128::from_str_radix(digits, 16).ok()
}

/// An address in a 32-byte log topic, lowercase.
fn topic_address(value: &Value) -> Option<String> {
    let digits = value.as_str()?.strip_prefix("0x")?;
    (digits.len() == 64 && digits[..24].bytes().all(|b| b == b'0'))
        .then(|| hex_id(&format!("0x{}", &digits[24..]), 40))
        .flatten()
}

/// A uint256 in log data, if it fits in u128.
fn word(value: &Value) -> Option<u128> {
    let digits = value.as_str()?.strip_prefix("0x")?;
    if digits.len() != 64 || !digits[..32].bytes().all(|b| b == b'0') {
        return None;
    }
    u128::from_str_radix(&digits[32..], 16).ok()
}

/// Recursive length prefix encoding, the format Ethereum transactions are signed in.
mod rlp {
    fn length(prefix: u8, len: usize) -> Vec<u8> {
        if len < 56 {
            return vec![prefix + len as u8];
        }
        let bytes: Vec<u8> = len
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        let mut out = vec![prefix + 55 + bytes.len() as u8];
        out.extend(bytes);
        out
    }

    pub fn bytes(value: &[u8]) -> Vec<u8> {
        if value.len() == 1 && value[0] < 0x80 {
            return value.to_vec();
        }
        let mut out = length(0x80, value.len());
        out.extend_from_slice(value);
        out
    }

    /// An integer: big-endian with no leading zeros; zero is the empty string.
    pub fn uint(value: u128) -> Vec<u8> {
        let be = value.to_be_bytes();
        let start = be.iter().position(|b| *b != 0).unwrap_or(be.len());
        bytes(&be[start..])
    }

    pub fn list(items: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = items.concat();
        let mut out = length(0xc0, body.len());
        out.extend(body);
        out
    }
}

/// An EIP-1559 transaction calling `token.transfer(to, amount)`, signed. Returns the raw bytes
/// and the transaction hash.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sign_transfer(
    key: &SigningKey,
    chain_id: u64,
    nonce: u64,
    max_priority_fee: u128,
    max_fee: u128,
    token: &[u8; 20],
    to: &[u8; 20],
    amount: u128,
) -> Result<(Vec<u8>, [u8; 32])> {
    let mut data = vec![0xa9, 0x05, 0x9c, 0xbb]; // transfer(address,uint256)
    data.extend([0u8; 12]);
    data.extend_from_slice(to);
    data.extend([0u8; 16]);
    data.extend(amount.to_be_bytes());
    let mut fields = vec![
        rlp::uint(chain_id.into()),
        rlp::uint(nonce.into()),
        rlp::uint(max_priority_fee),
        rlp::uint(max_fee),
        rlp::uint(GAS_LIMIT),
        rlp::bytes(token),
        rlp::uint(0),
        rlp::bytes(&data),
        rlp::list(&[]),
    ];
    let mut unsigned = vec![0x02];
    unsigned.extend(rlp::list(&fields));
    let (mut signature, mut recovery) = key
        .sign_prehash_recoverable(&keccak(&unsigned))
        .map_err(|_| Error::Merchant)?;
    // Ethereum accepts only the lower of the two equivalent `s` values.
    if let Some(normalized) = signature.normalize_s() {
        signature = normalized;
        recovery = RecoveryId::new(!recovery.is_y_odd(), recovery.is_x_reduced());
    }
    let (r, s) = signature.split_bytes();
    let trim = |b: &[u8]| {
        b.iter()
            .skip_while(|x| **x == 0)
            .copied()
            .collect::<Vec<u8>>()
    };
    fields.push(rlp::uint(u128::from(recovery.is_y_odd())));
    fields.push(rlp::bytes(&trim(&r)));
    fields.push(rlp::bytes(&trim(&s)));
    let mut raw = vec![0x02];
    raw.extend(rlp::list(&fields));
    let hash = keccak(&raw);
    Ok((raw, hash))
}

/// The account a private key controls, lowercase `0x` hex.
pub fn address_of(key: &SigningKey) -> String {
    let point = key.verifying_key().to_encoded_point(false);
    hex(&keccak(&point.as_bytes()[1..])[12..])
}

fn address_bytes(address: &str) -> Result<[u8; 20]> {
    unhex(address)
        .and_then(|b| b.try_into().ok())
        .ok_or(Error::StoreFormat)
}

/// One stored attempt.
struct Attempt {
    digest: String,
    nonce: u64,
    recipient: String,
    amount_units: u128,
    tx_hash: String,
    raw_tx: String,
}

impl EvmRail {
    /// Loads the configuration; the key and node address come from the environment (or its
    /// `_FILE` form). `store` is the rail's own database next to the ledger.
    pub fn load(path: &Path, store: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|_| bad("cannot read the file"))?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, MAX_CONFIG + 1), &mut text)
            .map_err(|_| bad("cannot read the file"))?;
        if text.len() as u64 > MAX_CONFIG {
            return Err(bad("the file is larger than 64 KiB"));
        }
        Self::from_json(&text, store, crate::env_secret)
    }

    pub fn from_json(
        text: &str,
        store: &Path,
        secret: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let config: ConfigFile =
            serde_json::from_str(text).map_err(|_| bad("invalid JSON or unknown field"))?;
        if config.schema != CONFIG_SCHEMA {
            return Err(bad("schema must be cosigna.evm-rail/v1"));
        }
        if !TEST_CHAINS.iter().any(|(id, _)| *id == config.chain_id) {
            return Err(bad(
                "only test chains are accepted (31337, Sepolia, Base/Arbitrum/Optimism Sepolia)",
            ));
        }
        let rpc = secret(&config.rpc_url_env)
            .ok_or_else(|| bad("the node address variable is not set"))?;
        let rpc = rpc.trim().to_string();
        let loopback = rpc
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split('/').next())
            .and_then(|port| port.parse::<u16>().ok())
            .is_some_and(|port| port > 0);
        let https = rpc
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|host| !host.is_empty() && !host.contains('@'));
        if !(https || loopback) {
            return Err(bad(
                "the node address must be https:// (or a loopback test address)",
            ));
        }
        let token =
            hex_id(&config.token, 40).ok_or_else(|| bad("token must be 0x and 40 hex digits"))?;
        let decimals = config.decimals.unwrap_or(6);
        if !(2..=18).contains(&decimals) {
            return Err(bad("decimals must be between 2 and 18"));
        }
        let confirmations = config.confirmations.unwrap_or(3);
        if !(1..=64).contains(&confirmations) {
            return Err(bad("confirmations must be between 1 and 64"));
        }
        let key_text = secret(&config.private_key_env)
            .ok_or_else(|| bad("the private key variable is not set"))?;
        let key_hex = key_text.trim();
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);
        let key = (key_hex.len() == 64)
            .then(|| unhex(&format!("0x{key_hex}")))
            .flatten()
            .and_then(|bytes| SigningKey::from_slice(&bytes).ok())
            .ok_or_else(|| bad("the private key must be 64 hex digits"))?;
        let rail = Self {
            chain_id: config.chain_id,
            rpc,
            token,
            decimals,
            confirmations,
            address: address_of(&key),
            key,
            store: store.to_owned(),
        };
        rail.open_store()?;
        Ok(rail)
    }

    /// The paying account, so the operator can fund it. Not a secret.
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn open_store(&self) -> Result<Connection> {
        let mut db = Connection::open(&self.store)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let app: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match (app, version) {
            (0, 0) => {
                let tables: i64 =
                    tx.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))?;
                if tables != 0 {
                    return Err(Error::StoreFormat);
                }
                tx.execute_batch(STORE_SCHEMA)?;
                tx.pragma_update(None, "application_id", STORE_APP)?;
                tx.pragma_update(None, "user_version", STORE_VERSION)?;
            }
            (STORE_APP, STORE_VERSION) => {}
            _ => return Err(Error::StoreFormat),
        }
        tx.commit()?;
        db.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(db)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = agent
            .post(&self.rpc)
            .send_json(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .map_err(|_| Error::Merchant)?;
        if !response.status().is_success() {
            return Err(Error::Merchant);
        }
        let body: Value = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_json()
            .map_err(|_| Error::Merchant)?;
        if body.get("error").is_some_and(|e| !e.is_null()) {
            return Err(Error::Merchant);
        }
        Ok(body["result"].clone())
    }

    /// Refuses a node on any other chain than the configured test chain.
    pub fn verify_chain(&self) -> Result<()> {
        match quantity(&self.call("eth_chainId", json!([]))?) {
            Some(id) if id == u128::from(self.chain_id) => Ok(()),
            _ => Err(Error::Invalid(
                "the node is not on the configured test chain".into(),
            )),
        }
    }

    fn latest_block(&self) -> Result<u64> {
        quantity(&self.call("eth_blockNumber", json!([]))?)
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(Error::Merchant)
    }

    fn receipt(&self, hash: &str) -> Result<Option<Value>> {
        let receipt = self.call("eth_getTransactionReceipt", json!([hash]))?;
        Ok((!receipt.is_null()).then_some(receipt))
    }

    /// Token units per cent.
    fn per_cent(&self) -> u128 {
        10u128.pow(u32::from(self.decimals) - 2)
    }

    /// The payment `source`: who paid this account in the token, and how much (in units), once
    /// it has enough confirmations. `Err(code)` explains why it cannot be refunded.
    fn payment(&self, source: &str) -> Result<std::result::Result<(String, u128), &'static str>> {
        let Some(hash) = hex_id(source, 64) else {
            return Ok(Err("source_not_found"));
        };
        let latest = self.latest_block()?;
        let Some(receipt) = self.receipt(&hash)? else {
            return Ok(Err("source_not_found"));
        };
        let block = quantity(&receipt["blockNumber"])
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(Error::Merchant)?;
        if receipt["status"] != "0x1" {
            return Ok(Err("source_not_found"));
        }
        if latest.saturating_sub(block) + 1 < self.confirmations {
            return Ok(Err("payment_not_settled"));
        }
        let mut payer: Option<String> = None;
        let mut received: u128 = 0;
        for log in receipt["logs"].as_array().into_iter().flatten() {
            let topics = log["topics"].as_array();
            let is_transfer_to_us = log["address"]
                .as_str()
                .map(str::to_ascii_lowercase)
                .as_deref()
                == Some(self.token.as_str())
                && topics.is_some_and(|t| {
                    t.len() == 3
                        && t[0] == TRANSFER_TOPIC
                        && topic_address(&t[2]).as_deref() == Some(self.address.as_str())
                });
            if !is_transfer_to_us {
                continue;
            }
            let from = topics
                .and_then(|t| topic_address(&t[1]))
                .ok_or(Error::Merchant)?;
            // One payer per payment: a transaction paying in for several people is not a
            // payment this rail can refund to a single sender.
            if payer.as_ref().is_some_and(|p| *p != from) {
                return Ok(Err("source_not_found"));
            }
            payer = Some(from);
            received = received
                .checked_add(word(&log["data"]).ok_or(Error::Merchant)?)
                .ok_or(Error::Merchant)?;
        }
        Ok(match payer {
            Some(payer) if received > 0 => Ok((payer, received)),
            _ => Err("source_not_found"),
        })
    }

    /// Units already refunded on this payment, or on their way: anything not known to have
    /// failed or been voided counts.
    fn refunded_units(db: &Connection, chain_id: u64, source: &str) -> Result<u128> {
        let mut statement = db.prepare(
            "SELECT amount_units FROM evm_attempts WHERE chain_id=?1 AND source=?2
             AND (outcome IS NULL OR outcome='succeeded')",
        )?;
        let amounts = statement
            .query_map(params![chain_id as i64, source], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        amounts.iter().try_fold(0u128, |total, units| {
            total
                .checked_add(units.parse().map_err(|_| Error::StoreFormat)?)
                .ok_or(Error::StoreFormat)
        })
    }

    /// What can still be refunded on `source`, in cents, and who to: `Err(code)` if nothing.
    fn refundable(
        &self,
        db: &Connection,
        source: &str,
    ) -> Result<std::result::Result<(String, i64, i64), &'static str>> {
        let (payer, received) = match self.payment(source)? {
            Ok(found) => found,
            Err(code) => return Ok(Err(code)),
        };
        let source = hex_id(source, 64).ok_or(Error::StoreFormat)?;
        let refunded = Self::refunded_units(db, self.chain_id, &source)?;
        // Rounded against refunding: what was received rounds down, what was refunded up.
        let cents = |c: u128| i64::try_from(c).map_err(|_| Error::Merchant);
        Ok(Ok((
            payer,
            cents(received / self.per_cent())?,
            cents(refunded.div_ceil(self.per_cent()))?,
        )))
    }

    pub(crate) fn check(&self, request: &OperationRequest) -> Result<Check> {
        if request.kind != OperationKind::Refund {
            return Ok(Check::Invalid("kind_not_supported_on_rail"));
        }
        // Amounts are cents of a US dollar stablecoin.
        if request.currency != "USD" {
            return Ok(Check::Invalid("currency_mismatch"));
        }
        self.verify_chain()?;
        let db = self.open_store()?;
        let source = request.source.as_deref().unwrap_or_default();
        let (payer, received, refunded) = match self.refundable(&db, source)? {
            Ok(found) => found,
            Err(code) => return Ok(Check::Invalid(code)),
        };
        if request.amount_minor > received - refunded {
            return Ok(Check::Invalid("exceeds_refundable"));
        }
        Ok(Check::Valid(json!({
            "payment_amount_minor": received, "refunded_minor": refunded,
            "refundable_minor": received - refunded, "customer": payer,
            "chain_id": self.chain_id, "token": self.token, "testnet": true})))
    }

    fn attempt(db: &Connection, key: &str) -> Result<Option<Attempt>> {
        db.query_row(
            "SELECT digest,nonce,recipient,amount_units,tx_hash,raw_tx FROM evm_attempts
                 WHERE attempt_key=?1",
            [key],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .map(|(digest, nonce, recipient, units, tx_hash, raw_tx)| {
            Ok::<_, Error>(Attempt {
                digest,
                nonce: u64::try_from(nonce).map_err(|_| Error::StoreFormat)?,
                recipient,
                amount_units: units.parse().map_err(|_| Error::StoreFormat)?,
                tx_hash,
                raw_tx,
            })
        })
        .transpose()
    }

    fn broadcast(&self, raw_tx: &str) {
        // An error here can mean it was already received; lookups settle what happened.
        let _ = self.call("eth_sendRawTransaction", json!([raw_tx]));
    }

    pub(crate) fn submit(&self, key: &str, proposal: &Proposal, now: i64) -> Result<RailOutcome> {
        let digest = proposal.digest()?;
        let mut db = self.open_store()?;
        if let Some(attempt) = Self::attempt(&db, key)? {
            if attempt.digest != digest {
                return Err(Error::StoreFormat);
            }
            return Ok(self.settled(key, proposal)?.unwrap_or(RailOutcome::Unknown));
        }
        if db
            .query_row("SELECT 1 FROM evm_voids WHERE attempt_key=?1", [key], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .is_some()
        {
            return Ok(RailOutcome::Voided);
        }
        self.verify_chain()?;
        // Re-check against the chain now: the payment, its payer and what is left to refund.
        let source = proposal.source.as_deref().unwrap_or_default();
        let still_valid = proposal.kind == OperationKind::Refund
            && proposal.currency == "USD"
            && match self.refundable(&db, source)? {
                Ok((payer, received, refunded)) => {
                    proposal.facts["customer"].as_str() == Some(payer.as_str())
                        && proposal.amount_minor <= received - refunded
                }
                Err(_) => false,
            };
        if !still_valid {
            return Ok(RailOutcome::Rejected);
        }
        let recipient = proposal.facts["customer"]
            .as_str()
            .ok_or(Error::StoreFormat)?
            .to_string();
        let units = u128::try_from(proposal.amount_minor)
            .ok()
            .and_then(|cents| cents.checked_mul(self.per_cent()))
            .ok_or(Error::Merchant)?;
        let pending =
            quantity(&self.call("eth_getTransactionCount", json!([self.address, "pending"]))?)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or(Error::Merchant)?;
        let block = self.call("eth_getBlockByNumber", json!(["latest", false]))?;
        let base_fee = quantity(&block["baseFeePerGas"]).ok_or(Error::Merchant)?;
        let priority =
            quantity(&self.call("eth_maxPriorityFeePerGas", json!([]))?).ok_or(Error::Merchant)?;
        let max_fee = base_fee
            .checked_mul(2)
            .and_then(|fee| fee.checked_add(priority))
            .ok_or(Error::Merchant)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = hex_id(source, 64).ok_or(Error::StoreFormat)?;
        // Concurrent attempts each take the next nonce; none is reused.
        let next_local: Option<i64> = tx.query_row(
            "SELECT MAX(nonce)+1 FROM evm_attempts WHERE chain_id=?1 AND sender=?2",
            params![self.chain_id as i64, self.address],
            |r| r.get(0),
        )?;
        let nonce = pending.max(next_local.map_or(0, |n| n as u64));
        let (raw, hash) = sign_transfer(
            &self.key,
            self.chain_id,
            nonce,
            priority,
            max_fee,
            &address_bytes(&self.token)?,
            &address_bytes(&recipient)?,
            units,
        )?;
        let (raw, hash) = (hex(&raw), hex(&hash));
        // Durable before it leaves this machine: from here on only these bytes are sent.
        tx.execute(
            "INSERT INTO evm_attempts(attempt_key,digest,chain_id,sender,nonce,source,recipient,
             amount_units,tx_hash,raw_tx,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                key,
                digest,
                self.chain_id as i64,
                self.address,
                nonce as i64,
                source,
                recipient,
                units.to_string(),
                hash,
                raw,
                now
            ],
        )?;
        tx.commit()?;
        self.broadcast(&raw);
        // A local chain includes it at once; a public one takes seconds. Unsettled stays unknown
        // and is settled by lookup.
        for _ in 0..4 {
            if let Some(outcome) = self.settled(key, proposal)? {
                return Ok(outcome);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(RailOutcome::Unknown)
    }

    /// The attempt's final result, if it has one: included with enough confirmations.
    fn settled(&self, key: &str, proposal: &Proposal) -> Result<Option<RailOutcome>> {
        let db = self.open_store()?;
        let Some(attempt) = Self::attempt(&db, key)? else {
            return Ok(None);
        };
        let digest = proposal.digest()?;
        if attempt.digest != digest {
            return Ok(None);
        }
        let latest = self.latest_block()?;
        let Some(receipt) = self.receipt(&attempt.tx_hash)? else {
            return Ok(None);
        };
        let block = quantity(&receipt["blockNumber"])
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(Error::Merchant)?;
        if latest.saturating_sub(block) + 1 < self.confirmations {
            return Ok(None);
        }
        // Success means the token contract logged exactly this transfer.
        let transferred = receipt["status"] == "0x1"
            && receipt["logs"].as_array().into_iter().flatten().any(|log| {
                let topics = log["topics"].as_array();
                log["address"]
                    .as_str()
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                    == Some(self.token.as_str())
                    && topics.is_some_and(|t| {
                        t.len() == 3
                            && t[0] == TRANSFER_TOPIC
                            && topic_address(&t[1]).as_deref() == Some(self.address.as_str())
                            && topic_address(&t[2]).as_deref() == Some(attempt.recipient.as_str())
                    })
                    && word(&log["data"]) == Some(attempt.amount_units)
            });
        db.execute(
            "UPDATE evm_attempts SET outcome=?2 WHERE attempt_key=?1",
            params![key, if transferred { "succeeded" } else { "failed" }],
        )?;
        Ok(Some(RailOutcome::Done(OperationReceipt {
            attempt_key: key.into(),
            proposal_digest: digest,
            rail_reference: attempt.tx_hash,
            amount_minor: proposal.amount_minor,
            currency: proposal.currency.clone(),
            succeeded: transferred,
            // Test networks only: the tokens have no value.
            simulated: true,
        })))
    }

    pub(crate) fn lookup(
        &self,
        key: &str,
        proposal: &Proposal,
    ) -> Result<Option<OperationReceipt>> {
        Ok(match self.settled(key, proposal)? {
            Some(RailOutcome::Done(receipt)) => Some(receipt),
            _ => None,
        })
    }

    /// Settles an unknown attempt: its result if final; void if it was never signed or its nonce
    /// went to another transaction (it can then never be included); otherwise the same signed
    /// bytes are sent again and it stays unknown.
    pub(crate) fn settle_unknown(&self, key: &str, proposal: &Proposal) -> Result<RailOutcome> {
        let digest = proposal.digest()?;
        let db = self.open_store()?;
        let Some(attempt) = Self::attempt(&db, key)? else {
            db.execute(
                "INSERT OR IGNORE INTO evm_voids(attempt_key,digest) VALUES (?1,?2)",
                params![key, digest],
            )?;
            return Ok(RailOutcome::Voided);
        };
        if attempt.digest != digest {
            return Err(Error::StoreFormat);
        }
        // Order matters: the block height is read before the receipt, so a transaction in any
        // block at or below it is visible to the receipt query.
        let latest = self.latest_block()?;
        if let Some(outcome) = self.settled(key, proposal)? {
            return Ok(outcome);
        }
        if self.receipt(&attempt.tx_hash)?.is_some() {
            return Ok(RailOutcome::Unknown); // included, not yet final
        }
        let final_block = latest.saturating_sub(self.confirmations - 1);
        let used = quantity(&self.call(
            "eth_getTransactionCount",
            json!([self.address, format!("0x{final_block:x}")]),
        )?)
        .and_then(|n| u64::try_from(n).ok())
        .ok_or(Error::Merchant)?;
        if used > attempt.nonce {
            db.execute(
                "UPDATE evm_attempts SET outcome='voided' WHERE attempt_key=?1",
                [key],
            )?;
            return Ok(RailOutcome::Voided);
        }
        self.broadcast(&attempt.raw_tx);
        Ok(RailOutcome::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlp_matches_the_published_examples() {
        // From the Ethereum RLP specification.
        assert_eq!(rlp::bytes(b"dog"), [0x83, b'd', b'o', b'g']);
        assert_eq!(
            rlp::list(&[rlp::bytes(b"cat"), rlp::bytes(b"dog")]),
            [0xc8, 0x83, b'c', b'a', b't', 0x83, b'd', b'o', b'g']
        );
        assert_eq!(rlp::bytes(b""), [0x80]);
        assert_eq!(rlp::list(&[]), [0xc0]);
        assert_eq!(rlp::uint(0), [0x80]);
        assert_eq!(rlp::uint(15), [0x0f]);
        assert_eq!(rlp::uint(1024), [0x82, 0x04, 0x00]);
        let long = b"Lorem ipsum dolor sit amet, consectetur adipisicing elit";
        let encoded = rlp::bytes(long);
        assert_eq!(&encoded[..2], &[0xb8, 0x38]);
        assert_eq!(&encoded[2..], long);
    }

    #[test]
    fn keys_map_to_their_published_addresses() {
        // The first well-known development account (Hardhat and Anvil).
        let key = SigningKey::from_slice(
            &unhex("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap(),
        )
        .unwrap();
        assert_eq!(
            address_of(&key),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
        assert_eq!(
            hex(&keccak(b"Transfer(address,address,uint256)")),
            TRANSFER_TOPIC
        );
        assert_eq!(
            &keccak(b"transfer(address,uint256)")[..4],
            [0xa9, 0x05, 0x9c, 0xbb]
        );
    }

    #[test]
    fn signing_the_same_attempt_gives_the_same_transaction() {
        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let (raw, hash) = sign_transfer(
            &key,
            31_337,
            5,
            1,
            2_000_000_000,
            &[1; 20],
            &[2; 20],
            1_000_000,
        )
        .unwrap();
        assert_eq!(raw[0], 0x02);
        assert_eq!(hash, keccak(&raw));
        let again = sign_transfer(
            &key,
            31_337,
            5,
            1,
            2_000_000_000,
            &[1; 20],
            &[2; 20],
            1_000_000,
        )
        .unwrap();
        // RFC 6979 signing is deterministic: the same attempt always has the same hash.
        assert_eq!(again, (raw, hash));
    }

    #[test]
    fn hex_parsing_refuses_what_it_should() {
        assert_eq!(quantity(&json!("0x0")), Some(0));
        assert_eq!(quantity(&json!("0x1a")), Some(26));
        assert_eq!(quantity(&json!("1a")), None);
        assert_eq!(quantity(&json!("0x")), None);
        assert!(hex_id("0xABC", 3).is_some());
        assert!(hex_id("0xABG", 3).is_none());
        assert!(hex_id("0xab", 3).is_none());
        let padded = format!("0x{}{}", "0".repeat(24), "ab".repeat(20));
        assert_eq!(
            topic_address(&json!(padded)),
            Some(format!("0x{}", "ab".repeat(20)))
        );
        let dirty = format!("0x{}{}", "1".repeat(24), "ab".repeat(20));
        assert_eq!(topic_address(&json!(dirty)), None);
    }
}
