//! Optional application layer. No model or merchant can grant itself spending authority.
//! This first increment executes synthetic merchant orders only; see this crate's README.
mod agents;
pub mod api;
pub mod approvers;
pub mod cases;
pub mod domain;
pub mod evidence;
pub mod exposure;
pub mod gateway;
pub mod limits;
pub mod mcp;
pub mod merchant;
pub mod operations;
pub mod pipeline;
pub mod planner;
pub mod provider;
pub mod rails;
pub mod replay;
pub mod service;
mod storage;
pub mod stripe_rail;
pub mod timeline;
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
    /// The connected case records could not be read; see `cases`.
    #[error("case records unavailable: {0}")]
    CaseSource(&'static str),
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

/// A secret from the environment variable `name`, or else from the file named by `name_FILE`
/// (how Docker and Kubernetes mount secrets). Surrounding whitespace is removed; an empty,
/// unreadable or oversized (over 64 KiB) value counts as unset.
pub fn env_secret(name: &str) -> Option<String> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(_) => {
            let path = std::env::var_os(format!("{name}_FILE"))?;
            let file = std::fs::File::open(path).ok()?;
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::Read::take(file, 65_537), &mut text)
                .ok()?;
            if text.len() > 65_536 {
                return None;
            }
            text
        }
    };
    clean_secret(&value)
}

/// Editors and `docker secret` end files with a newline, which would break a request header.
fn clean_secret(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_secs().min(i64::MAX as u64) as i64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn secrets_lose_surrounding_whitespace_and_blank_ones_are_unset() {
        assert_eq!(super::clean_secret("tok_1\n").as_deref(), Some("tok_1"));
        assert_eq!(super::clean_secret("  tok_1\r\n").as_deref(), Some("tok_1"));
        assert_eq!(super::clean_secret("a b").as_deref(), Some("a b"));
        assert_eq!(super::clean_secret(" \n\t"), None);
        assert_eq!(super::clean_secret(""), None);
    }
}
