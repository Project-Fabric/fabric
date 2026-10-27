use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

use super::{IdentityError as E, IdentityResult};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;

const MAX_TOKEN: usize = 16_384;
const MAX_KEYS: u64 = 65_536;
const CONFIG: [&str; 4] = [
    "COSIGNA_IDENTITY_ISSUER",
    "COSIGNA_IDENTITY_AUDIENCE",
    "COSIGNA_IDENTITY_CLIENT_IDS",
    "COSIGNA_IDENTITY_JWKS_PATH",
];

/// Incoming requests obtain this only from verification; callers cannot supply owner/role.
/// The sibling approval module also uses recorded subjects to recheck durable grants.
#[derive(Clone)]
pub struct Subject {
    pub(super) issuer: String,
    pub(super) subject: String,
    pub(super) expires_at: i64,
    pub(super) can_approve: bool,
}

pub struct Verifier {
    issuer: String,
    audience: String,
    clients: Vec<String>,
    keys: BTreeMap<String, DecodingKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    typ: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: i64,
    iat: i64,
    jti: String,
    client_id: String,
    scope: String,
    // Missing is allowed; an explicitly null, fractional or string date is not.
    #[serde(default, deserialize_with = "present_integer")]
    nbf: Option<i64>,
}

fn present_integer<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<i64>, D::Error> {
    i64::deserialize(d).map(Some)
}

pub(super) fn text(s: &str, max: usize, spaces: bool) -> bool {
    !s.is_empty()
        && s.len() <= max
        && !s
            .chars()
            .any(|c| c.is_control() || (!spaces && c.is_whitespace()))
}

pub(super) fn issuer_valid(s: &str) -> bool {
    text(s, 2048, false)
        && url::Url::parse(s).is_ok_and(|u| {
            u.scheme() == "https"
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none()
        })
}

impl Verifier {
    pub fn from_environment() -> IdentityResult<Self> {
        Self::from_settings(CONFIG.map(|name| std::env::var(name).ok()))
    }

    /// Loads operator settings, never a URL or path supplied by a token or API caller.
    pub fn from_settings(settings: [Option<String>; 4]) -> IdentityResult<Self> {
        if settings
            .iter()
            .all(|s| s.as_ref().is_none_or(|s| s.is_empty()))
        {
            return Err(E::Unconfigured);
        }
        let [issuer, audience, clients, path] =
            settings.map(|s| s.filter(|s| !s.is_empty()).ok_or(E::ConfigurationInvalid));
        let (issuer, audience, clients, path) = (issuer?, audience?, clients?, path?);
        if clients.len() > 4096 || path.len() > 4096 || !Path::new(&path).is_absolute() {
            return Err(E::ConfigurationInvalid);
        }
        let file = std::fs::File::open(path).map_err(|_| E::ConfigurationInvalid)?;
        if !file
            .metadata()
            .map_err(|_| E::ConfigurationInvalid)?
            .is_file()
        {
            return Err(E::ConfigurationInvalid);
        }
        let mut bytes = Vec::new();
        file.take(MAX_KEYS + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| E::ConfigurationInvalid)?;
        if bytes.len() > MAX_KEYS as usize {
            return Err(E::ConfigurationInvalid);
        }
        Self::new(
            issuer,
            audience,
            serde_json::from_str(&clients).map_err(|_| E::ConfigurationInvalid)?,
            &bytes,
        )
    }

    pub fn new(
        issuer: String,
        audience: String,
        clients: Vec<String>,
        jwks: &[u8],
    ) -> IdentityResult<Self> {
        if !issuer_valid(&issuer)
            || !text(&audience, 255, false)
            || !(1..=10).contains(&clients.len())
            || clients.iter().any(|s| !text(s, 255, false))
            || clients.iter().collect::<BTreeSet<_>>().len() != clients.len()
            || jwks.len() > MAX_KEYS as usize
        {
            return Err(E::ConfigurationInvalid);
        }
        let raw: Value = serde_json::from_slice(jwks).map_err(|_| E::ConfigurationInvalid)?;
        let entries = raw
            .get("keys")
            .and_then(Value::as_array)
            .ok_or(E::ConfigurationInvalid)?;
        if !(1..=10).contains(&entries.len()) {
            return Err(E::ConfigurationInvalid);
        }
        let mut keys = BTreeMap::new();
        for entry in entries {
            let map = entry.as_object().ok_or(E::ConfigurationInvalid)?;
            let kid = entry["kid"].as_str().ok_or(E::ConfigurationInvalid)?;
            if entry["kty"] != "RSA"
                || kid.is_empty()
                || kid.len() > 128
                || !kid
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || keys.contains_key(kid)
                || map.get("alg").is_some_and(|v| v != "RS256")
                || map.get("use").is_some_and(|v| v != "sig")
                || map
                    .get("key_ops")
                    .is_some_and(|v| v != &serde_json::json!(["verify"]))
                || ["d", "p", "q", "dp", "dq", "qi", "oth", "k"]
                    .iter()
                    .any(|p| map.contains_key(*p))
            {
                return Err(E::ConfigurationInvalid);
            }
            let n = entry["n"].as_str().ok_or(E::ConfigurationInvalid)?;
            let e = entry["e"].as_str().ok_or(E::ConfigurationInvalid)?;
            if n.len() > 1024 || e != "AQAB" {
                return Err(E::ConfigurationInvalid);
            }
            let modulus = URL_SAFE_NO_PAD
                .decode(n)
                .map_err(|_| E::ConfigurationInvalid)?;
            let bits = modulus
                .first()
                .map_or(0, |b| modulus.len() * 8 - b.leading_zeros() as usize);
            if !(2048..=4096).contains(&bits)
                || modulus.first() == Some(&0)
                || modulus.last().is_none_or(|b| b & 1 == 0)
            {
                return Err(E::ConfigurationInvalid);
            }
            let key =
                DecodingKey::from_rsa_components(n, e).map_err(|_| E::ConfigurationInvalid)?;
            keys.insert(kid.to_string(), key);
        }
        Ok(Self {
            issuer,
            audience,
            clients,
            keys,
        })
    }

    pub fn verify(&self, authorization: &str, at: i64) -> IdentityResult<Subject> {
        if authorization.len() > MAX_TOKEN {
            return Err(E::InvalidToken);
        }
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(E::InvalidToken)?;
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|p| {
                p.is_empty()
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            || parts[0].len() > 2048
        {
            return Err(E::InvalidToken);
        }
        let header: Header = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(parts[0])
                .map_err(|_| E::InvalidToken)?,
        )
        .map_err(|_| E::InvalidToken)?;
        if header.alg != "RS256"
            || !matches!(
                header.typ.to_ascii_lowercase().as_str(),
                "at+jwt" | "application/at+jwt"
            )
        {
            return Err(E::InvalidToken);
        }
        let key = self.keys.get(&header.kid).ok_or(E::InvalidToken)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = 0;
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.validate_nbf = true;
        let claims = jsonwebtoken::decode::<Claims>(token, key, &validation)
            .map_err(|_| E::InvalidToken)?
            .claims;
        let audience_valid = match &claims.aud {
            Audience::One(s) => s == &self.audience,
            Audience::Many(items) => {
                (1..=10).contains(&items.len())
                    && items.iter().all(|s| text(s, 255, false))
                    && items.contains(&self.audience)
            }
        };
        if claims.iss != self.issuer
            || !audience_valid
            || !text(&claims.sub, 255, false)
            || !text(&claims.jti, 255, false)
            || !self.clients.contains(&claims.client_id)
            || claims.iat < 0
            || claims.iat > at
            || claims.exp <= at
            || claims.exp <= claims.iat
            || claims.exp.saturating_sub(claims.iat) > 3600
            || claims
                .nbf
                .is_some_and(|n| n < 0 || n > at || n >= claims.exp)
            || !text(&claims.scope, 1024, true)
        {
            return Err(E::InvalidToken);
        }
        if !claims.scope.split(' ').any(|s| s == "cosigna:read") {
            return Err(E::InsufficientScope);
        }
        Ok(Subject {
            issuer: claims.iss,
            subject: claims.sub,
            expires_at: claims.exp,
            can_approve: claims.scope.split(' ').any(|s| s == "cosigna:approve"),
        })
    }
}
