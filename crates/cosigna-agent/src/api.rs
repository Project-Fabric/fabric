//! Loopback-only development API. Tokens are role-scoped, not a production identity service.
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Response, Server};

use crate::{
    API_VERSION, Actor, BudgetInput, Error, Result, Role, Service, TaskInput, new_id, now,
    planner::Planner, provider::Scenario,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    schema: String,
    owner: String,
    planner: String,
    executor: String,
}

impl Credentials {
    pub fn initialize(path: &Path) -> Result<Self> {
        if path.exists() {
            return Self::load(path);
        }
        let credentials = Self {
            schema: API_VERSION.into(),
            owner: new_id("owner")?,
            planner: new_id("planner")?,
            executor: new_id("executor")?,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&serde_json::to_vec(&credentials)?)?;
        file.sync_all()?;
        Ok(credentials)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let credentials: Self = serde_json::from_slice(&bytes)?;
        if credentials.schema != API_VERSION {
            return Err(Error::StoreFormat);
        }
        for (token, prefix) in [
            (&credentials.owner, "owner_"),
            (&credentials.planner, "planner_"),
            (&credentials.executor, "executor_"),
        ] {
            let suffix = token.strip_prefix(prefix).ok_or(Error::StoreFormat)?;
            if suffix.len() != 64 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::StoreFormat);
            }
        }
        Ok(credentials)
    }

    pub fn authenticate(&self, authorization: &str) -> Result<Actor> {
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(Error::Forbidden)?;
        let input = blake3::hash(token.as_bytes());
        for (stored, role) in [
            (&self.owner, Role::Owner),
            (&self.planner, Role::Planner),
            (&self.executor, Role::Executor),
        ] {
            let expected = blake3::hash(stored.as_bytes());
            let difference = input
                .as_bytes()
                .iter()
                .zip(expected.as_bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b));
            if difference == 0 {
                return Ok(Actor {
                    owner: "local-user".into(),
                    role,
                });
            }
        }
        Err(Error::Forbidden)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    quote_digest: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    #[serde(default)]
    scenario: Scenario,
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body)
        .map_err(|_| Error::Invalid("invalid JSON body or unknown fields".into()))
}

fn empty(body: &[u8]) -> Result<()> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    let _: Empty = decode(body)?;
    Ok(())
}

/// Router shared by the HTTP server and authorization tests. Caller must authenticate first.
pub fn dispatch(
    service: &Service,
    planner: &dyn Planner,
    actor: &Actor,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> Result<Value> {
    actor.require(&[Role::Owner, Role::Planner, Role::Executor])?;
    let parts: Vec<_> = path.split('/').collect();
    let data = match (method, parts.as_slice()) {
        ("GET", ["", "v1", "catalog"]) => serde_json::to_value(service.provider.offers()?)?,
        ("POST", ["", "v1", "budgets"]) => {
            serde_json::to_value(service.create_budget(actor, decode::<BudgetInput>(body)?, at)?)?
        }
        ("GET", ["", "v1", "budgets", id]) => serde_json::to_value(service.balance(actor, id)?)?,
        ("POST", ["", "v1", "budgets", id, "revoke"]) => {
            empty(body)?;
            serde_json::to_value(service.revoke_budget_at(actor, id, at)?)?
        }
        ("POST", ["", "v1", "tasks"]) => {
            serde_json::to_value(service.create_task(actor, decode::<TaskInput>(body)?, at)?)?
        }
        ("GET", ["", "v1", "tasks", id]) => serde_json::to_value(service.task(actor, id)?)?,
        ("GET", ["", "v1", "tasks", id, "events"]) => {
            serde_json::to_value(service.events(actor, id)?)?
        }
        ("POST", ["", "v1", "tasks", id, "plan"]) => {
            empty(body)?;
            serde_json::to_value(service.plan(actor, id, planner, at)?)?
        }
        ("POST", ["", "v1", "tasks", id, "approve"]) => {
            let approval: Approval = decode(body)?;
            serde_json::to_value(service.approve(actor, id, &approval.quote_digest, at)?)?
        }
        ("POST", ["", "v1", "tasks", id, "execute"]) => {
            let execution: Execution = decode(body)?;
            serde_json::to_value(service.execute(actor, id, execution.scenario, at)?)?
        }
        ("POST", ["", "v1", "tasks", id, "reconcile"]) => {
            empty(body)?;
            serde_json::to_value(service.reconcile(actor, id, at)?)?
        }
        ("POST", ["", "v1", "tasks", id, "resolve"]) => {
            empty(body)?;
            serde_json::to_value(service.resolve(actor, id, at)?)?
        }
        ("POST", ["", "v1", "tasks", id, "cancel"]) => {
            empty(body)?;
            serde_json::to_value(service.cancel(actor, id, at)?)?
        }
        _ => return Err(Error::NotFound),
    };
    let approval_digest = data
        .get("quote")
        .filter(|q| !q.is_null())
        .map(|q| -> Result<String> { serde_json::from_value::<crate::Quote>(q.clone())?.digest() })
        .transpose()?;
    Ok(
        json!({"schema":API_VERSION,"simulated_provider":true,"approval_digest":approval_digest,"data":data}),
    )
}

pub fn error_response(error: &Error) -> (u16, Value) {
    let (status, code) = match error {
        Error::Invalid(_) => (400, "invalid_request"),
        Error::Forbidden => (403, "forbidden"),
        Error::NotFound => (404, "not_found"),
        Error::Conflict(_) => (409, "conflict"),
        Error::BudgetExceeded => (409, "budget_exceeded"),
        Error::Planner => (502, "planner_unavailable"),
        Error::Merchant => (503, "merchant_unavailable"),
        Error::CaseSource(_) => (503, "case_records_unavailable"),
        _ => (500, "internal_error"),
    };
    (
        status,
        json!({"schema":API_VERSION,"error":{"code":code,"message":error.to_string()}}),
    )
}

/// Where the API listens. Without a public host it serves this machine only, at
/// `127.0.0.1:<port>`. With one, it serves behind a TLS-terminating reverse proxy: requests
/// must name that host and arrive with `X-Forwarded-Proto: https`.
#[derive(Debug, Clone)]
pub struct Listen {
    address: std::net::SocketAddr,
    public_host: Option<String>,
}

impl Listen {
    pub fn loopback(port: u16) -> Result<Self> {
        Self::new((std::net::Ipv4Addr::LOCALHOST, port).into(), None)
    }

    pub fn new(address: std::net::SocketAddr, public_host: Option<String>) -> Result<Self> {
        if address.port() == 0 {
            return Err(Error::Invalid("port must be nonzero".into()));
        }
        if let Some(host) = &public_host {
            let valid = (1..=253).contains(&host.len())
                && host.split('.').all(|label| {
                    (1..=63).contains(&label.len())
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                });
            if !valid {
                return Err(Error::Invalid(
                    "--public-host must be a lowercase host name, without scheme or port".into(),
                ));
            }
        } else if !address.ip().is_loopback() {
            return Err(Error::Invalid(
                "listening beyond this machine needs --public-host: the name clients use through                  the TLS proxy"
                    .into(),
            ));
        }
        Ok(Self {
            address,
            public_host,
        })
    }

    fn local_host(&self) -> String {
        format!("127.0.0.1:{}", self.address.port())
    }

    /// Whether a request may proceed, judged by its `Host` and `X-Forwarded-Proto` headers.
    /// Health checks may also come from the machine itself (container probes).
    fn admits(&self, host: Option<&str>, forwarded_proto: Option<&str>, health: bool) -> bool {
        let local = host == Some(self.local_host().as_str());
        match &self.public_host {
            None => local,
            Some(_) if health && local => true,
            Some(public) => {
                // The proxy's published port may be any port; the name is what is checked.
                let named = host.is_some_and(|h| {
                    h == public
                        || h.strip_prefix(public.as_str())
                            .and_then(|rest| rest.strip_prefix(':'))
                            .is_some_and(|port| port.parse::<u16>().is_ok_and(|p| p > 0))
                });
                named && forwarded_proto.is_some_and(|p| p.eq_ignore_ascii_case("https"))
            }
        }
    }
}

pub fn serve(
    service: &Service,
    planner: &dyn Planner,
    credentials: &Credentials,
    listen: &Listen,
) -> Result<()> {
    let server = Server::http(listen.address)
        .map_err(|_| Error::Invalid("cannot bind the service address".into()))?;
    match &listen.public_host {
        None => println!(
            "Cosigna agent · http://{} · planner: {} · SIMULATED merchant",
            listen.local_host(),
            planner.name()
        ),
        Some(host) => println!(
            "Cosigna agent · listening on {} for https://{host} (behind a TLS proxy) · planner: {}",
            listen.address,
            planner.name()
        ),
    }
    // A small pool: a slow rail or planner call holds one worker, not the whole gateway, so
    // status, approvals and revocation stay responsive. Each request opens its own connection.
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                while let Ok(request) = server.recv() {
                    handle(service, planner, credentials, listen, request);
                }
            });
        }
    });
    Ok(())
}

/// Requests served at once. Writes still serialize in SQLite; reads and external calls overlap.
const WORKERS: usize = 8;

fn handle(
    service: &Service,
    planner: &dyn Planner,
    credentials: &Credentials,
    listen: &Listen,
    mut request: tiny_http::Request,
) {
    let gateway_request = request.url().starts_with("/gateway/");
    let result = (|| -> Result<Value> {
        let headers = request.headers();
        // No browser origins or CORS. Explicit Host blocks DNS rebinding to loopback.
        if headers.iter().any(|h| h.field.equiv("Origin")) {
            return Err(Error::Forbidden);
        }
        let header = |name: &'static str| {
            headers
                .iter()
                .find(|h| h.field.equiv(name))
                .map(|h| h.value.as_str())
        };
        let health = request.method() == &Method::Get && request.url() == "/v1/health";
        if !listen.admits(header("Host"), header("X-Forwarded-Proto"), health) {
            return Err(Error::Forbidden);
        }
        if health {
            return Ok(json!({"schema":API_VERSION,"simulated_provider":true,"status":"ok"}));
        }
        let authorization = headers
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.as_str())
            .ok_or(Error::Forbidden)?;
        let (actor, principal) = if gateway_request {
            (
                None,
                Some(crate::gateway::authenticate(
                    service,
                    credentials,
                    authorization,
                    now(),
                )?),
            )
        } else {
            (Some(credentials.authenticate(authorization)?), None)
        };
        if request.body_length().is_some_and(|length| length > 16_384) {
            return Err(Error::Invalid("body exceeds 16 KiB".into()));
        }
        if request.method() == &Method::Post {
            let content_type = headers
                .iter()
                .find(|h| h.field.equiv("Content-Type"))
                .map(|h| h.value.as_str());
            if !content_type
                .is_some_and(|value| value.split(';').next() == Some("application/json"))
            {
                return Err(Error::Invalid(
                    "Content-Type must be application/json".into(),
                ));
            }
        }
        let mut body = Vec::new();
        request.as_reader().take(16_385).read_to_end(&mut body)?;
        if body.len() > 16_384 {
            return Err(Error::Invalid("body exceeds 16 KiB".into()));
        }
        if let Some(principal) = principal {
            return crate::gateway::dispatch_authenticated(
                service,
                planner,
                principal,
                request.method().as_str(),
                request.url(),
                &body,
                now(),
            );
        }
        dispatch(
            service,
            planner,
            &actor.ok_or(Error::Forbidden)?,
            request.method().as_str(),
            request.url(),
            &body,
            now(),
        )
    })();
    let (status, mut value) = match result {
        Ok(value) => (200, value),
        Err(error) => error_response(&error),
    };
    if gateway_request && value.get("error").is_some() {
        let pipeline_request = request.url() == "/gateway/v1/pipelines"
            || request.url().starts_with("/gateway/v1/pipelines/");
        let worker_request = pipeline_request && request.url().ends_with("/worker");
        value["schema"] = if worker_request {
            crate::worker::API_VERSION
        } else if pipeline_request {
            crate::pipeline::API_VERSION
        } else {
            crate::gateway::API_VERSION
        }
        .into();
    }
    let mut response = Response::from_string(value.to_string()).with_status_code(status);
    for (name, value) in [
        ("Content-Type", "application/json"),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_stable_and_roles_do_not_escalate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        let credentials = Credentials::initialize(&path).unwrap();
        let loaded = Credentials::initialize(&path).unwrap();
        assert_eq!(credentials.owner, loaded.owner);
        assert_eq!(
            credentials
                .authenticate(&format!("Bearer {}", credentials.planner))
                .unwrap()
                .role,
            Role::Planner
        );
        assert!(credentials.authenticate("Bearer garbage").is_err());
        assert!(credentials.authenticate(&credentials.owner).is_err());
        let actor = credentials
            .authenticate(&format!("Bearer {}", credentials.planner))
            .unwrap();
        let service = Service::open(directory.path()).unwrap();
        let body = br#"{"currency":"USD","limit_minor":1000,"per_purchase_minor":1000,"merchants":["fixture-cafe"],"expires_at":2000}"#;
        assert!(matches!(
            dispatch(
                &service,
                &crate::planner::FixturePlanner,
                &actor,
                "POST",
                "/v1/budgets",
                body,
                1000
            ),
            Err(Error::Forbidden)
        ));
    }
}
