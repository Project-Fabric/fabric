//! Optional identity resource server. Membership never implies payment authority.
mod approvals;
mod registry;
mod tokens;

pub use approvals::{ApproverBindingInput, GatewayBindingInput};
pub use registry::{Enrollment, MembershipInput, OrganizationInput};
pub use tokens::{Subject, Verifier};

use crate::Service;
use serde_json::{Value, json};

pub const API_VERSION: &str = "cosigna.identity/v1";

/// Fixed public errors: never include tokens, claims, key data or configuration paths.
#[derive(Debug, PartialEq, Eq)]
pub enum IdentityError {
    InvalidToken,
    InsufficientScope,
    InsufficientApprovalScope,
    Unconfigured,
    ConfigurationInvalid,
    Unavailable,
    NotFound,
    InvalidCursor,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("identity request refused")
    }
}
impl std::error::Error for IdentityError {}

impl IdentityError {
    pub fn response(&self) -> (u16, Value) {
        let (status, code) = match self {
            Self::InvalidToken => (401, "invalid_token"),
            Self::InsufficientScope => (403, "insufficient_scope"),
            Self::InsufficientApprovalScope => (403, "insufficient_approval_scope"),
            Self::Unconfigured => (503, "identity_unconfigured"),
            Self::ConfigurationInvalid => (503, "identity_configuration_invalid"),
            Self::Unavailable => (503, "identity_unavailable"),
            Self::NotFound => (404, "not_found"),
            Self::InvalidCursor => (400, "invalid_cursor"),
        };
        (status, json!({"schema": API_VERSION, "error": code}))
    }
}

pub type IdentityResult<T> = std::result::Result<T, IdentityError>;

/// Configuration and public keys are reloaded on every request, including key retirement.
pub(crate) fn dispatch(
    service: &Service,
    authorization: Option<&str>,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> crate::Result<Value> {
    let verifier = Verifier::from_environment()?;
    let subject = verifier.verify(authorization.unwrap_or(""), at)?;
    service.identity_request(&subject, method, path, body, at)
}

pub(crate) use approvals::{
    BINDING_SCHEMA, Context, check_authority, check_first_approval, check_submission_authority,
    has_binding,
};
pub(crate) use registry::admin;
pub(crate) use registry::{SCHEMA, record_digest, record_ids};
