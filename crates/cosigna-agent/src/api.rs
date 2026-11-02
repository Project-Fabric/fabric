//! The gateway's HTTP API: loopback by default, or behind a TLS proxy. Tokens are role-scoped,
//! not a production identity service.
use std::{fs::OpenOptions, io::Write, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    if let Error::Identity(error) = error {
        return error.response();
    }
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
    /// Behind the proxy, also serve clients connecting from this machine's own loopback
    /// address under the local name, without the proxy's headers: a console sharing the
    /// gateway's network namespace. No one outside that namespace can connect from there.
    trust_local: bool,
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
                "listening beyond this machine needs --public-host: the name clients use through \
                 the TLS proxy"
                    .into(),
            ));
        }
        Ok(Self {
            address,
            public_host,
            trust_local: false,
        })
    }

    /// See `trust_local`. Only meaningful behind the proxy; refused without a public host.
    pub fn trust_local_clients(mut self) -> Result<Self> {
        if self.public_host.is_none() {
            return Err(Error::Invalid(
                "--trust-local-clients only applies with --public-host".into(),
            ));
        }
        self.trust_local = true;
        Ok(self)
    }

    fn local_host(&self) -> String {
        format!("127.0.0.1:{}", self.address.port())
    }

    /// Whether a request may proceed, judged by its `Host` and `X-Forwarded-Proto` headers and
    /// whether it came from this machine's loopback address. Health checks may also come from
    /// the machine itself (container probes).
    fn admits(
        &self,
        host: Option<&str>,
        forwarded_proto: Option<&str>,
        health: bool,
        loopback_peer: bool,
    ) -> bool {
        let local = host == Some(self.local_host().as_str());
        match &self.public_host {
            None => local,
            Some(_) if local && (health || (self.trust_local && loopback_peer)) => true,
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

/// Limits for the HTTP server.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Requests handled at once; more get 503 `busy` at once instead of waiting in a queue.
    /// Writes still serialize in SQLite; reads and external calls overlap.
    pub concurrent: usize,
    /// How long a client may take to send its request and wait for the answer. Work already
    /// started (a payment being sent) is not abandoned: it finishes and is recorded, and the
    /// client gets 504 and should look the operation up.
    pub request_timeout: std::time::Duration,
    /// Requests per second for each credential (bursts up to twice that). A client without a
    /// credential is counted by its address.
    pub per_credential: u32,
    /// Refused requests (401 or 403) a source address may make per minute before it is blocked for
    /// the rest of that minute.
    pub refusals_per_minute: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            concurrent: 32,
            request_timeout: std::time::Duration::from_secs(120),
            per_credential: 20,
            refusals_per_minute: 30,
        }
    }
}

impl Limits {
    /// Slots kept for people (owner, approvers and other non-agent credentials), so a flood
    /// of agent requests cannot stop someone from approving, rejecting or revoking.
    fn reserved_for_people(&self) -> usize {
        if self.concurrent < 2 {
            0
        } else {
            (self.concurrent / 8).max(1)
        }
    }
}

struct App {
    service: Service,
    planner: Box<dyn Planner>,
    credentials: Credentials,
    listen: Listen,
    limits: Limits,
    /// Slots anyone may use, and slots only people may use.
    slots: std::sync::Arc<tokio::sync::Semaphore>,
    people_slots: std::sync::Arc<tokio::sync::Semaphore>,
    rates: crate::limits::RateLimiter,
    refusals: crate::limits::FailureLimiter,
}

/// Serves until Ctrl-C or SIGTERM, then stops accepting connections and lets requests in
/// progress finish, so a payment being sent is recorded before the process exits.
pub fn serve(
    service: Service,
    planner: Box<dyn Planner>,
    credentials: Credentials,
    listen: Listen,
    limits: Limits,
) -> Result<()> {
    if limits.concurrent == 0
        || limits.request_timeout.is_zero()
        || limits.per_credential == 0
        || limits.refusals_per_minute == 0
    {
        return Err(Error::Invalid(
            "concurrency, request timeout and rate limits must be nonzero".into(),
        ));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(limits.concurrent)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen.address)
            .await
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
        let reserved = limits.reserved_for_people();
        let app = std::sync::Arc::new(App {
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(
                limits.concurrent - reserved,
            )),
            people_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(reserved)),
            rates: crate::limits::RateLimiter::new(limits.per_credential),
            refusals: crate::limits::FailureLimiter::new(limits.refusals_per_minute),
            service,
            planner,
            credentials,
            listen,
            limits,
        });
        let router = axum::Router::new().fallback(handle).with_state(app);
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown())
            .await?;
        Ok(())
    })
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(_) => return std::future::pending().await,
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
    eprintln!("Cosigna agent stopping: finishing requests in progress");
}

const BODY_LIMIT: usize = 16_384;
/// How long a client has to send its (at most 16 KiB) body. Kept short and separate from the
/// request timeout, so a client that stalls its upload cannot hold a slot for long.
const BODY_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Who a request's credential belongs to, as far as choosing a slot is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    /// The owner, a registered approver, or a verified identity token.
    Person,
    /// A valid role credential that is not a person's (planner, executor).
    Machine,
    /// No credential, or one that is not valid: refused later if it got that far.
    Unknown,
}

/// Only people may use the slots kept for them. Reads headers only, and is asked only when the
/// shared slots are full.
fn caller(app: &App, url: &str, authorization: Option<&str>) -> Caller {
    let Some(authorization) = authorization else {
        return Caller::Unknown;
    };
    if url.starts_with("/identity/") {
        return if crate::identity::Verifier::from_environment()
            .is_ok_and(|verifier| verifier.verify(authorization, now()).is_ok())
        {
            Caller::Person
        } else {
            Caller::Unknown
        };
    }
    if let Ok(actor) = app.credentials.authenticate(authorization) {
        return if actor.require(&[Role::Owner]).is_ok() {
            Caller::Person
        } else {
            Caller::Machine
        };
    }
    if authorization.starts_with("Bearer approverkey_")
        && app.service.authenticate_approver(authorization).is_ok()
    {
        return Caller::Person;
    }
    Caller::Unknown
}

/// The client's address: the connection's peer, or behind the TLS proxy the address the proxy
/// appended last to `X-Forwarded-For` (earlier entries come from the client and prove nothing).
fn source(
    listen: &Listen,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> std::net::IpAddr {
    if listen.public_host.is_some()
        && let Some(forwarded) = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .next_back()
            .and_then(|last| last.trim().parse().ok())
    {
        return forwarded;
    }
    peer.ip()
}

fn too_many(url: &str, wait: std::time::Duration, why: &str) -> axum::response::Response {
    let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
    let mut response = reply(
        url,
        429,
        json!({"schema":API_VERSION,"error":{"code":"rate_limited","message":format!("{why}; retry in {seconds} s")}}),
    );
    if let Ok(value) = axum::http::HeaderValue::from_str(&seconds.max(1).to_string()) {
        response.headers_mut().insert("retry-after", value);
    }
    response
}

async fn handle(
    axum::extract::State(app): axum::extract::State<std::sync::Arc<App>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let url = request.uri().path_and_query().map_or_else(
        || request.uri().path().to_string(),
        |p| p.as_str().to_string(),
    );
    // Health checks never count as load, so a busy gateway is not reported unhealthy and
    // restarted in the middle of a payment.
    let health = request.method() == axum::http::Method::GET && url == "/v1/health";
    let from = source(&app.listen, peer, request.headers());
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    // Every request from the trusted local console arrives from loopback, so its refusals are
    // counted per credential: one person's refusals must not block the owner's console.
    let refused: crate::limits::Source = match &authorization {
        Some(value)
            if app.listen.trust_local
                && peer.ip().is_loopback()
                && !request.headers().contains_key("x-forwarded-for") =>
        {
            crate::limits::Source::Credential(*blake3::hash(value.as_bytes()).as_bytes())
        }
        _ => from.into(),
    };
    if !health {
        let now = std::time::Instant::now();
        if let Some(wait) = app.refusals.blocked(refused, now) {
            return too_many(&url, wait, "too many refused requests from this address");
        }
        // Each credential has its own budget; a client without one is counted by address.
        let key = match &authorization {
            Some(value) => value.as_bytes().to_vec(),
            None => from.to_string().into_bytes(),
        };
        if let Err(wait) = app.rates.check(&key, now) {
            return too_many(&url, wait, "request rate limit reached for this credential");
        }
    }
    // The token's prefix chooses a pool. When the shared slots are full, a request that is not
    // an agent's may use a slot kept for people only once its credential is shown to be a
    // person's; full authentication still comes later.
    let agent = authorization
        .as_deref()
        .is_some_and(|value| value.starts_with("Bearer agentkey_"));
    let permit = match app.slots.clone().try_acquire_owned() {
        Ok(permit) => Some(permit),
        Err(_) if !agent && !health => {
            let checked = app.clone();
            let path = url.clone();
            let credential = authorization.clone();
            let who =
                tokio::task::spawn_blocking(move || caller(&checked, &path, credential.as_deref()))
                    .await
                    .unwrap_or(Caller::Unknown);
            // A valid machine credential is only told the gateway is busy; a wrong one is a
            // refusal, as it would have been with a free slot.
            if who == Caller::Unknown && authorization.is_some() {
                app.refusals.record(refused, std::time::Instant::now());
            }
            if who == Caller::Person {
                app.people_slots.clone().try_acquire_owned().ok()
            } else {
                None
            }
        }
        Err(_) => None,
    };
    let permit = match permit {
        Some(permit) => Some(permit),
        None if health => None,
        None => {
            return reply(
                &url,
                503,
                json!({"schema":API_VERSION,"error":{"code":"busy","message":"too many requests in progress; retry shortly"}}),
            );
        }
    };
    let deadline = app.limits.request_timeout;
    let (parts, body) = request.into_parts();
    let work = {
        let app = app.clone();
        let url = url.clone();
        async move {
            // At most one byte over the limit is read, within a short deadline. The size is
            // judged after the authentication checks, in the same order as before. A body that
            // does not arrive in time counts as a refusal from its source: nothing was done.
            let read = tokio::time::timeout(
                deadline.min(BODY_READ_TIMEOUT),
                axum::body::to_bytes(body, BODY_LIMIT + 1),
            )
            .await;
            let body = match read {
                Ok(Ok(bytes)) if bytes.len() <= BODY_LIMIT => Some(bytes.to_vec()),
                Ok(_) => None,
                Err(_) => {
                    drop(permit);
                    app.refusals.record(refused, std::time::Instant::now());
                    return Ok((
                        504,
                        json!({"schema":API_VERSION,"error":{"code":"timed_out","message":"the request body did not arrive in time; nothing was done"}}),
                    ));
                }
            };
            // The permit moves into the blocking task: the slot stays taken until the work
            // really ends, even if the client has already been told it timed out.
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                respond(
                    &app,
                    parts.method.as_str(),
                    &url,
                    &parts.headers,
                    body,
                    peer.ip().is_loopback(),
                )
            })
            .await
        }
    };
    match tokio::time::timeout(deadline, work).await {
        Ok(Ok((status, value))) => {
            // 403 from the gateway's own credentials, 401 from identity tokens: both are
            // refusals a token guesser collects.
            if status == 401 || status == 403 {
                app.refusals.record(refused, std::time::Instant::now());
            }
            reply(&url, status, value)
        }
        Ok(Err(_)) => {
            let (status, value) = error_response(&Error::Invalid("request failed".into()));
            reply(&url, status, value)
        }
        Err(_) => reply(
            &url,
            504,
            json!({"schema":API_VERSION,"error":{"code":"timed_out","message":"no answer in time; work already started still completes, so look the operation up before retrying"}}),
        ),
    }
}

/// The request checks and dispatch, in the same order as the earlier server: browser origins,
/// host and HTTPS, authentication, body size, content type, then the API itself.
fn respond(
    app: &App,
    method: &str,
    url: &str,
    headers: &axum::http::HeaderMap,
    body: Option<Vec<u8>>,
    loopback_peer: bool,
) -> (u16, Value) {
    let gateway_request = url.starts_with("/gateway/");
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let result = (|| -> Result<Value> {
        // No browser origins or CORS. Explicit Host blocks DNS rebinding to loopback.
        if headers.contains_key("origin") {
            return Err(Error::Forbidden);
        }
        let health = method == "GET" && url == "/v1/health";
        if !app.listen.admits(
            header("host"),
            header("x-forwarded-proto"),
            health,
            loopback_peer,
        ) {
            return Err(Error::Forbidden);
        }
        if health {
            return Ok(json!({"schema":API_VERSION,"simulated_provider":true,"status":"ok"}));
        }
        if url.starts_with("/identity/") {
            if headers.get_all("authorization").iter().count() > 1 {
                return Err(crate::identity::IdentityError::InvalidToken.into());
            }
            let body = body.ok_or_else(|| Error::Invalid("body exceeds 16 KiB".into()))?;
            if method == "POST"
                && !header("content-type")
                    .is_some_and(|v| v.split(';').next() == Some("application/json"))
            {
                return Err(Error::Invalid(
                    "Content-Type must be application/json".into(),
                ));
            }
            return crate::identity::dispatch(
                &app.service,
                header("authorization"),
                method,
                url,
                &body,
                now(),
            );
        }
        let authorization = header("authorization").ok_or(Error::Forbidden)?;
        let (actor, principal) = if gateway_request {
            (
                None,
                Some(crate::gateway::authenticate(
                    &app.service,
                    &app.credentials,
                    authorization,
                    now(),
                )?),
            )
        } else {
            (Some(app.credentials.authenticate(authorization)?), None)
        };
        let body = body.ok_or_else(|| Error::Invalid("body exceeds 16 KiB".into()))?;
        if method == "POST"
            && !header("content-type")
                .is_some_and(|value| value.split(';').next() == Some("application/json"))
        {
            return Err(Error::Invalid(
                "Content-Type must be application/json".into(),
            ));
        }
        if let Some(principal) = principal {
            return crate::gateway::dispatch_authenticated(
                &app.service,
                app.planner.as_ref(),
                principal,
                method,
                url,
                &body,
                now(),
            );
        }
        dispatch(
            &app.service,
            app.planner.as_ref(),
            &actor.ok_or(Error::Forbidden)?,
            method,
            url,
            &body,
            now(),
        )
    })();
    match result {
        Ok(value) => (200, value),
        Err(error) => error_response(&error),
    }
}

fn reply(url: &str, status: u16, mut value: Value) -> axum::response::Response {
    if url.starts_with("/gateway/") && value.get("error").is_some() {
        let pipeline_request =
            url == "/gateway/v1/pipelines" || url.starts_with("/gateway/v1/pipelines/");
        let worker_request = pipeline_request && url.ends_with("/worker");
        value["schema"] = if worker_request {
            crate::worker::API_VERSION
        } else if pipeline_request {
            crate::pipeline::API_VERSION
        } else {
            crate::gateway::API_VERSION
        }
        .into();
    }
    let mut response = axum::response::Response::new(axum::body::Body::from(value.to_string()));
    *response.status_mut() = axum::http::StatusCode::from_u16(status)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    for (name, value) in [
        ("content-type", "application/json"),
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
    ] {
        headers.insert(name, axum::http::HeaderValue::from_static(value));
    }
    if url.starts_with("/identity/") {
        if status == 401 {
            headers.insert(
                "www-authenticate",
                axum::http::HeaderValue::from_static("Bearer error=\"invalid_token\""),
            );
        } else if status == 403 && value["error"] == "insufficient_scope" {
            headers.insert(
                "www-authenticate",
                axum::http::HeaderValue::from_static(
                    "Bearer error=\"insufficient_scope\", scope=\"cosigna:read\"",
                ),
            );
        } else if status == 403 && value["error"] == "insufficient_approval_scope" {
            headers.insert(
                "www-authenticate",
                axum::http::HeaderValue::from_static(
                    "Bearer error=\"insufficient_scope\", scope=\"cosigna:approve\"",
                ),
            );
        }
    }
    response
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

    #[test]
    fn only_loopback_connections_are_trusted_under_the_local_name() {
        let address: std::net::SocketAddr = "0.0.0.0:3092".parse().unwrap();
        let public = Some("cosigna.example.test".to_string());
        let trusting = Listen::new(address, public.clone())
            .unwrap()
            .trust_local_clients()
            .unwrap();
        let strict = Listen::new(address, public).unwrap();
        let local = Some("127.0.0.1:3092");
        // The console in the gateway's network namespace: loopback peer, local name.
        assert!(trusting.admits(local, None, false, true));
        // The same request arriving from elsewhere (through Docker's network) is refused.
        assert!(!trusting.admits(local, None, false, false));
        assert!(!trusting.admits(local, Some("https"), false, false));
        // Without the option, the local name stays limited to health checks.
        assert!(!strict.admits(local, None, false, true));
        assert!(strict.admits(local, None, true, true));
        // Requests through the proxy are unchanged.
        assert!(trusting.admits(Some("cosigna.example.test"), Some("https"), false, false));
        assert!(!trusting.admits(Some("cosigna.example.test"), None, false, true));
        // The option needs a public host.
        assert!(
            Listen::loopback(3092)
                .unwrap()
                .trust_local_clients()
                .is_err()
        );
    }
}
