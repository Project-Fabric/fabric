//! Optional application layer. No model or merchant can grant itself spending authority.
//! This first increment executes synthetic merchant orders only; see this crate's README.
mod agents;
pub mod api;
pub mod domain;
pub mod evidence;
pub mod gateway;
pub mod merchant;
pub mod pipeline;
pub mod planner;
pub mod provider;
pub mod service;
mod storage;
pub mod worker;

pub use domain::*;
pub use service::Service;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("not authorized for this operation")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("budget does not cover this purchase")]
    BudgetExceeded,
    #[error("planner unavailable or returned an invalid response")]
    Planner,
    #[error("test merchant is unavailable or returned an invalid response")]
    Merchant,
    #[error("unsupported transaction store format")]
    StoreFormat,
    #[error("storage operation failed")]
    Sql(#[from] rusqlite::Error),
    #[error("data encoding failed")]
    Json(#[from] serde_json::Error),
    #[error("local I/O operation failed")]
    Io(#[from] std::io::Error),
    #[error("secure randomness unavailable")]
    Random,
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn new_id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
    Ok(format!("{prefix}_{}", blake3::Hash::from(bytes).to_hex()))
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_secs().min(i64::MAX as u64) as i64)
}
