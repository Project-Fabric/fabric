//! Where case records come from: the business's own record of what each customer is owed.
//!
//! One source is connected per gateway, chosen by a configuration file (`cosigna.cases/v1`):
//! the synthetic case book (the default), a CSV export from any system, or a help desk or store
//! read over HTTPS with a field mapping. A case that cannot be read is never guessed: an
//! unreachable source or an unreadable record fails the request, and only a definite "no such
//! record" is reported as a missing case.
//!
//! The mapped fields must be ones the paying agent cannot write. If the agent can edit the
//! ticket field that says what is owed, the case check only repeats the agent's own claim.
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result, operations::Case};

pub const CONFIG_SCHEMA: &str = "cosigna.cases/v1";
const MAX_BODY: u64 = 512 * 1024;
const MAX_MINOR: i64 = 1_000_000_000_000;
const UNREACHABLE: &str = "the case records system did not answer; nothing was decided";
const UNREADABLE: &str =
    "the case record lacks a mapped field or has an invalid value; nothing was decided";

/// A connected source of case records.
#[derive(Clone)]
pub enum CaseSource {
    /// A fixed list read once at startup (a CSV export).
    List { name: String, cases: Vec<Case> },
    /// A system read per request over HTTPS.
    Http(Box<HttpCases>),
}

impl From<HttpCases> for CaseSource {
    fn from(source: HttpCases) -> Self {
        Self::Http(Box::new(source))
    }
}

#[derive(Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
enum SourceConfig {
    /// Columns: id, customer, source, currency, owed_minor.
    Csv {
        path: String,
    },
    Http(Box<HttpConfig>),
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpConfig {
    /// A short name for logs and evidence, such as `zendesk`.
    name: String,
    /// The record's address with `{id}` in place of the case number, such as
    /// `https://acme.zendesk.com/api/v2/tickets/{id}.json`.
    url: String,
    auth: Auth,
    /// The prefix agents put before the system's own id (`zd_` for `zd_123`). Case ids
    /// without it are not this system's and are reported as not found.
    #[serde(default)]
    id_prefix: String,
    fields: Fields,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "scheme", rename_all = "snake_case", deny_unknown_fields)]
enum Auth {
    /// HTTP basic: `username` and a secret (for Zendesk, `you@company.com/token` and the API token).
    Basic {
        username: String,
        secret_env: String,
    },
    /// `Authorization: Bearer <secret>`.
    Bearer { secret_env: String },
    /// A named header carrying the secret, such as `X-Shopify-Access-Token`.
    Header { name: String, secret_env: String },
}

/// Where each value is in the record. Paths are JSON pointers; a segment `[key=value]` picks
/// the array element whose `key` equals `value` (Zendesk keeps custom fields as `{id, value}`).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    /// The payment system's customer id, compared with the payment's customer.
    customer: String,
    /// The payment the refund must be made on.
    source: String,
    owed: String,
    /// `minor` (cents as an integer) or `major` (a decimal such as `12.50`).
    owed_unit: Unit,
    /// A path to the currency, or `fixed` for one currency for every case.
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    currency_fixed: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Unit {
    Minor,
    Major,
}

#[derive(Clone)]
pub struct HttpCases {
    config: HttpConfig,
    /// The value sent for authentication, read once from the environment; never logged.
    credential: (String, String),
}

impl CaseSource {
    /// Reads a `cosigna.cases/v1` file. Relative CSV paths are resolved from its folder.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|_| Error::Invalid("cannot read the case source configuration".into()))?;
        let invalid = |e: serde_json::Error| {
            Error::Invalid(format!("invalid case source configuration: {e}"))
        };
        let mut config: Value = serde_json::from_str(&text).map_err(invalid)?;
        if config.get("schema").and_then(Value::as_str) != Some(CONFIG_SCHEMA) {
            return Err(Error::Invalid(format!(
                "case source configuration must have schema {CONFIG_SCHEMA}"
            )));
        }
        if let Some(map) = config.as_object_mut() {
            map.remove("schema");
        }
        let source: SourceConfig = serde_json::from_value(config).map_err(invalid)?;
        match source {
            SourceConfig::Csv { path: csv } => {
                let csv = path.parent().unwrap_or(Path::new(".")).join(csv);
                let text = std::fs::read_to_string(&csv)
                    .map_err(|_| Error::Invalid("cannot read the case records CSV".into()))?;
                let cases = crate::replay::read_cases(&text)?;
                let mut ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
                ids.sort_unstable();
                if ids.windows(2).any(|w| w[0] == w[1]) {
                    return Err(Error::Invalid(
                        "the case records CSV repeats a case id".into(),
                    ));
                }
                Ok(Self::List {
                    name: "csv".into(),
                    cases,
                })
            }
            SourceConfig::Http(http) => {
                Ok(HttpCases::new(*http, |name| std::env::var(name).ok())?.into())
            }
        }
    }

    /// For logs and evidence: which system the case was read from.
    pub fn name(&self) -> &str {
        match self {
            Self::List { name, .. } => name,
            Self::Http(http) => &http.config.name,
        }
    }

    pub fn case(&self, id: &str) -> Result<Option<Case>> {
        match self {
            Self::List { cases, .. } => Ok(cases.iter().find(|c| c.id == id).cloned()),
            Self::Http(http) => http.case(id),
        }
    }

    /// Every case, where the source can list them; `None` for systems read one record at a time.
    pub fn cases(&self) -> Option<Vec<Case>> {
        match self {
            Self::List { cases, .. } => Some(cases.clone()),
            Self::Http(_) => None,
        }
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl HttpCases {
    fn new(config: HttpConfig, env: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let bad = |what: &str| Err(Error::Invalid(format!("case source: {what}")));
        if !identifier(&config.name) {
            return bad("name must be letters, digits, '_' or '-'");
        }
        let Some((base, rest)) = config.url.split_once("{id}") else {
            return bad("url must contain {id}");
        };
        if rest.contains("{id}") || base.contains(['?', '#']) {
            return bad("url must contain {id} once, in its path");
        }
        // The secret only ever goes to the configured system over TLS (or to a loopback test
        // server), so a configuration cannot send it somewhere in the clear.
        let host_ok = match base.strip_prefix("https://") {
            Some(rest) => rest
                .split('/')
                .next()
                .is_some_and(|h| !h.is_empty() && !h.contains('@')),
            None => base
                .strip_prefix("http://127.0.0.1:")
                .and_then(|rest| rest.split('/').next())
                .and_then(|port| port.parse::<u16>().ok())
                .is_some_and(|port| port > 0),
        };
        if !host_ok {
            return bad("url must be https:// (or http://127.0.0.1:<port> for tests)");
        }
        if !(config.id_prefix.is_empty() || identifier(&config.id_prefix)) {
            return bad("id_prefix must be letters, digits, '_' or '-'");
        }
        match (&config.fields.currency, &config.fields.currency_fixed) {
            (Some(_), None) => {}
            (None, Some(c)) if c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()) => {}
            _ => {
                return bad(
                    "set exactly one of fields.currency or fields.currency_fixed (like USD)",
                );
            }
        }
        let secret = |name: &str| {
            env(name)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !s.contains(['\r', '\n']))
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "case source: set the secret in the environment variable {name}"
                    ))
                })
        };
        let credential = match &config.auth {
            Auth::Basic {
                username,
                secret_env,
            } => {
                if username.contains(':') || username.is_empty() {
                    return bad("basic auth username must not be empty or contain ':'");
                }
                let token = base64(format!("{username}:{}", secret(secret_env)?).as_bytes());
                ("Authorization".into(), format!("Basic {token}"))
            }
            Auth::Bearer { secret_env } => (
                "Authorization".into(),
                format!("Bearer {}", secret(secret_env)?),
            ),
            Auth::Header { name, secret_env } => {
                if !identifier(name) {
                    return bad("auth header name must be letters, digits or '-'");
                }
                (name.clone(), secret(secret_env)?)
            }
        };
        Ok(Self { config, credential })
    }

    /// Test construction with an explicit secret instead of the environment.
    pub fn from_json(config: &Value, secret: &str) -> Result<Self> {
        let config: HttpConfig = serde_json::from_value(config.clone())
            .map_err(|e| Error::Invalid(format!("invalid case source configuration: {e}")))?;
        let secret = secret.to_string();
        Self::new(config, move |_| Some(secret.clone()))
    }

    fn case(&self, id: &str) -> Result<Option<Case>> {
        let Some(remote) = id.strip_prefix(self.config.id_prefix.as_str()) else {
            return Ok(None);
        };
        if !identifier(remote) {
            return Ok(None);
        }
        let response = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent()
            .get(self.config.url.replacen("{id}", remote, 1))
            .header(&self.credential.0, &self.credential.1)
            .header("Accept", "application/json")
            .call();
        let Ok(mut response) = response else {
            return Err(Error::CaseSource(UNREACHABLE));
        };
        match response.status().as_u16() {
            200 => {}
            404 => return Ok(None),
            _ => return Err(Error::CaseSource(UNREACHABLE)),
        }
        let record: Value = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_json()
            .map_err(|_| Error::CaseSource(UNREACHABLE))?;
        self.read(id, &record).map(Some)
    }

    /// Maps one record to a case. Anything missing or malformed is an error, not a guess.
    fn read(&self, id: &str, record: &Value) -> Result<Case> {
        let fields = &self.config.fields;
        let text = |path: &str| -> Result<String> {
            match pick(record, path).ok_or(Error::CaseSource(UNREADABLE))? {
                Value::String(s) => Ok(s.trim().to_string()),
                Value::Number(n) => Ok(n.to_string()),
                _ => Err(Error::CaseSource(UNREADABLE)),
            }
        };
        let customer = text(&fields.customer)?;
        let source = text(&fields.source)?;
        if !identifier(&customer) || !identifier(&source) {
            return Err(Error::CaseSource(UNREADABLE));
        }
        let currency = match (&fields.currency, &fields.currency_fixed) {
            (Some(path), _) => text(path)?.to_ascii_uppercase(),
            (None, Some(fixed)) => fixed.clone(),
            (None, None) => return Err(Error::CaseSource(UNREADABLE)),
        };
        if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(Error::CaseSource(UNREADABLE));
        }
        let owed = text(&fields.owed)?;
        let owed_minor = match fields.owed_unit {
            Unit::Minor => owed.parse::<i64>().ok(),
            Unit::Major => major_to_minor(&owed),
        }
        .filter(|a| (1..=MAX_MINOR).contains(a))
        .ok_or(Error::CaseSource(UNREADABLE))?;
        Ok(Case {
            id: id.to_string(),
            customer,
            source,
            owed_minor,
            currency,
        })
    }
}

/// `12`, `12.5` or `12.50` to 1250. No signs, exponents or more than two decimals.
fn major_to_minor(value: &str) -> Option<i64> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, "0"));
    if whole.is_empty()
        || fraction.is_empty()
        || whole.len() > 12
        || fraction.len() > 2
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let cents = format!("{fraction:0<2}").parse::<i64>().ok()?;
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents)
}

/// A JSON pointer whose segments may also be `[key=value]`, selecting the array element
/// whose `key` is `value` (compared as text, so `[id=360001]` matches the number 360001).
fn pick<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.strip_prefix('/')?;
    path.split('/').try_fold(value, |node, segment| {
        if let Some(filter) = segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let (key, want) = filter.split_once('=')?;
            let mut matches = node.as_array()?.iter().filter(|item| match &item[key] {
                Value::String(s) => s == want,
                Value::Number(n) => n.to_string() == want,
                _ => false,
            });
            let found = matches.next()?;
            // Two elements with the same key: ambiguous, so read neither.
            matches.next().is_none().then_some(found)
        } else {
            let segment = segment.replace("~1", "/").replace("~0", "~");
            match node {
                Value::Object(map) => map.get(&segment),
                Value::Array(items) => items.get(segment.parse::<usize>().ok()?),
                _ => None,
            }
        }
    })
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"a@b.co/token:x"), "YUBiLmNvL3Rva2VuOng=");
    }

    #[test]
    fn amounts_are_parsed_exactly() {
        assert_eq!(major_to_minor("12"), Some(1200));
        assert_eq!(major_to_minor("12.5"), Some(1250));
        assert_eq!(major_to_minor("0.07"), Some(7));
        for bad in ["", ".5", "-1", "1e3", "12.505", "1,000", " 1", "12."] {
            assert_eq!(major_to_minor(bad), None, "{bad}");
        }
    }

    #[test]
    fn paths_select_custom_fields_and_refuse_ambiguity() {
        let record = json!({"ticket": {"id": 7, "custom_fields": [
            {"id": 1, "value": "pi_1"}, {"id": 2, "value": "45.00"}, {"id": 3, "value": null}]}});
        assert_eq!(
            pick(&record, "/ticket/custom_fields/[id=1]/value"),
            Some(&json!("pi_1"))
        );
        assert_eq!(pick(&record, "/ticket/id"), Some(&json!(7)));
        assert_eq!(pick(&record, "/ticket/custom_fields/0/id"), Some(&json!(1)));
        assert_eq!(pick(&record, "/ticket/custom_fields/[id=9]/value"), None);
        assert_eq!(pick(&record, "ticket/id"), None);
        let twice = json!([{"id": 1, "v": 1}, {"id": "1", "v": 2}]);
        assert_eq!(pick(&twice, "/[id=1]/v"), None);
    }
}
