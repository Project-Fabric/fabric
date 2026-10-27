use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use cosigna_agent::{
    Actor, BudgetInput, Error, Result, Role, Service, TaskInput,
    api::{self, Credentials},
    now,
    planner::{FixturePlanner, GroqTransport, HostedPlanner, OpenAITransport, Planner},
    provider::Scenario,
};
use serde_json::json;

#[derive(Parser)]
#[command(
    version,
    about = "Cosigna intelligence and transaction development service (simulated purchases)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a separate local ledger and role credentials. Existing credentials are preserved.
    Init {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        test_merchant_url: Option<String>,
    },
    /// Serve the authenticated API: on 127.0.0.1 by default, or with `--listen` and
    /// `--public-host` behind a TLS-terminating reverse proxy. No real merchant is connected.
    Serve {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 3092)]
        port: u16,
        /// Listen on this address instead of 127.0.0.1:<port>, e.g. 0.0.0.0:3092 in a
        /// container. A non-loopback address requires --public-host.
        #[arg(long, conflicts_with = "port")]
        listen: Option<std::net::SocketAddr>,
        /// The host name clients use through the TLS proxy. Requests must carry it as Host and
        /// arrive with X-Forwarded-Proto: https; anything else is refused.
        #[arg(long)]
        public_host: Option<String>,
        /// Requests handled at once; beyond this, clients get 503 `busy` immediately.
        #[arg(long, default_value_t = 32)]
        max_concurrent: usize,
        /// Seconds a request may take before the client gets 504. Work already started, such
        /// as a payment being sent, still finishes and is recorded.
        #[arg(long, default_value_t = 120)]
        request_timeout: u64,
        /// Requests per second allowed for each credential (bursts up to twice that); beyond
        /// it, 429 `rate_limited` with Retry-After. Other credentials are not affected.
        #[arg(long, default_value_t = 20)]
        rate_limit: u32,
        /// Refused requests (401 or 403, such as a wrong token) one address may make per minute
        /// before it is blocked with 429 for the rest of that minute.
        #[arg(long, default_value_t = 30)]
        refusals_per_minute: u32,
        #[arg(long, value_enum, default_value_t = PlannerMode::Fixtures)]
        planner: PlannerMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        test_merchant_url: Option<String>,
        /// Connect Stripe test-mode refunds as the `stripe_test` rail (key from
        /// COSIGNA_STRIPE_TEST_SECRET_KEY or the engine's stored `stripe-test` credential).
        #[arg(long)]
        stripe_test: bool,
        /// Read case records from this `cosigna.cases/v1` file (a CSV export or a help desk
        /// over HTTPS) instead of the synthetic case book. See examples/cases.
        #[arg(long)]
        cases: Option<PathBuf>,
    },
    /// Check that a gateway on this machine answers its health endpoint (container probes).
    /// Exit code 0 when healthy, 1 otherwise.
    Probe {
        #[arg(long, default_value_t = 3092)]
        port: u16,
    },
    /// Run the opt-in durable scheduler. Only explicitly owner-enrolled tasks are processed.
    Worker {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = PlannerMode::Fixtures)]
        planner: PlannerMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        test_merchant_url: Option<String>,
        /// Process at most one due job then exit (also useful for scheduler checks).
        #[arg(long)]
        once: bool,
        #[arg(long)]
        stripe_test: bool,
        #[arg(long)]
        cases: Option<PathBuf>,
    },
    /// Run an isolated, deterministic end-to-end demo; no API key or real purchase.
    Demo,
    /// Exercise registered agent permissions and exact approval; fixtures only, no secrets logged.
    GatewayDemo,
    /// Run a restartable transaction pipeline with synthetic owner approval and no real payment.
    PipelineDemo,
    /// Serve the gateway's money operations as MCP tools over stdio, for one agent.
    /// Reads the agent's own token from COSIGNA_AGENT_TOKEN.
    Mcp {
        #[arg(long)]
        gateway_url: String,
    },
    /// Replay a proposed policy over past refunds and payouts (a CSV), locally. Prints JSON
    /// (`cosigna.replay/v1`), or a short summary with --text.
    Replay {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        policy: PathBuf,
        /// Optional case records CSV: id, customer, source, currency, owed_minor.
        #[arg(long)]
        cases: Option<PathBuf>,
        #[arg(long)]
        text: bool,
    },
    /// Check or export the ledger's tamper-evident record of decisions.
    Evidence {
        #[command(subcommand)]
        action: EvidenceAction,
    },
}

#[derive(Subcommand)]
enum EvidenceAction {
    /// Verify the chain and that current state matches it. Exit code 5 if not.
    Verify {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Write the chain as Fabric-format evidence.jsonl to a new file.
    Export {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
}

/// The ledger's evidence does not verify (same code as `fabric evidence verify`).
const EVIDENCE_FAILED: u8 = 5;

#[derive(Clone, Copy, ValueEnum)]
enum PlannerMode {
    Fixtures,
    Openai,
    Groq,
}

fn directory(path: Option<PathBuf>) -> Result<PathBuf> {
    path.or_else(|| dirs::data_local_dir().map(|d| d.join("Cosigna").join("agent-dev")))
        .ok_or_else(|| Error::Invalid("specify --data-dir".into()))
}

fn run() -> Result<std::process::ExitCode> {
    let outcome: Result<()> = match Cli::parse().command {
        Command::Evidence { action } => return evidence(action),
        Command::Replay {
            history,
            policy,
            cases,
            text,
        } => {
            use cosigna_agent::replay;
            let policy: replay::Policy =
                serde_json::from_slice(&std::fs::read(&policy)?).map_err(|_| {
                    Error::Invalid("the policy file is not a valid replay policy".into())
                })?;
            let history = replay::read_history(&std::fs::read_to_string(&history)?)?;
            let cases = match cases {
                Some(path) => replay::read_cases(&std::fs::read_to_string(&path)?)?,
                None => Vec::new(),
            };
            let report = replay::replay(policy, history, &cases)?;
            if text {
                print!("{}", replay::summary(&report));
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"schema": replay::API_VERSION, "data": report})
                    )?
                );
            }
            Ok(())
        }
        Command::Mcp { gateway_url } => {
            let token = std::env::var("COSIGNA_AGENT_TOKEN").map_err(|_| {
                Error::Invalid("set COSIGNA_AGENT_TOKEN to the agent's token".into())
            })?;
            cosigna_agent::mcp::serve(&cosigna_agent::mcp::Gateway::new(&gateway_url, &token)?)
        }
        Command::Init {
            data_dir,
            test_merchant_url,
        } => {
            let directory = directory(data_dir)?;
            open_service(&directory, test_merchant_url)?;
            Credentials::initialize(&directory.join("credentials.json"))?;
            println!("Cosigna development store: {}", directory.display());
            println!(
                "Role credentials: {} (keep private; tokens are not logged)",
                directory.join("credentials.json").display()
            );
            Ok(())
        }
        Command::Serve {
            data_dir,
            port,
            listen,
            public_host,
            max_concurrent,
            request_timeout,
            rate_limit,
            refusals_per_minute,
            planner,
            model,
            test_merchant_url,
            stripe_test,
            cases,
        } => {
            let limits = api::Limits {
                concurrent: max_concurrent,
                request_timeout: std::time::Duration::from_secs(request_timeout),
                per_credential: rate_limit,
                refusals_per_minute,
            };
            let listen = match listen {
                Some(address) => api::Listen::new(address, public_host)?,
                None if public_host.is_some() => {
                    api::Listen::new((std::net::Ipv4Addr::LOCALHOST, port).into(), public_host)?
                }
                None => api::Listen::loopback(port)?,
            };
            let directory = directory(data_dir)?;
            let credentials = Credentials::load(&directory.join("credentials.json"))?;
            let service = with_rails(
                open_service(&directory, test_merchant_url)?,
                stripe_test,
                cases.as_deref(),
            )?;
            let planner = make_planner(planner, model)?;
            api::serve(service, planner, credentials, listen, limits)
        }
        Command::Probe { port } => {
            let healthy = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(3)))
                .build()
                .new_agent()
                .get(format!("http://127.0.0.1:{port}/v1/health"))
                .call()
                .is_ok_and(|response| response.status() == 200);
            if !healthy {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Worker {
            data_dir,
            planner,
            model,
            test_merchant_url,
            once,
            stripe_test,
            cases,
        } => {
            let directory = directory(data_dir)?;
            // Refuse an accidental new store: initialize this profile explicitly.
            Credentials::load(&directory.join("credentials.json"))?;
            let service = with_rails(
                open_service(&directory, test_merchant_url)?,
                stripe_test,
                cases.as_deref(),
            )?;
            let mut worker = cosigna_agent::worker::Worker::open(&service)?;
            let planner = make_planner(planner, model)?;
            eprintln!(
                "Cosigna worker · enrolled tasks only · planner: {} · SIMULATED merchant · recovers unknown money operations by lookup only",
                planner.name()
            );
            loop {
                for recovery in worker.recover_operations(now())? {
                    println!(
                        "{}",
                        json!({"schema":cosigna_agent::worker::RECOVERY_API_VERSION,"data":recovery})
                    );
                }
                if let Some(job) = worker.tick(planner.as_ref(), now())? {
                    println!(
                        "{}",
                        json!({"schema":cosigna_agent::worker::API_VERSION,"simulated_provider":true,"data":job})
                    );
                }
                if once {
                    return Ok(std::process::ExitCode::SUCCESS);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        Command::Demo => demo(),
        Command::GatewayDemo => {
            println!(
                "{}",
                serde_json::to_string_pretty(&cosigna_agent::gateway::demo()?)?
            );
            Ok(())
        }
        Command::PipelineDemo => {
            println!(
                "{}",
                serde_json::to_string_pretty(&cosigna_agent::pipeline::demo()?)?
            );
            Ok(())
        }
    };
    outcome.map(|()| std::process::ExitCode::SUCCESS)
}

fn evidence(action: EvidenceAction) -> Result<std::process::ExitCode> {
    match action {
        EvidenceAction::Verify { data_dir } => {
            let report = cosigna_agent::evidence::verify(&directory(data_dir)?)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(if report.ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(EVIDENCE_FAILED)
            })
        }
        EvidenceAction::Export { data_dir, out } => {
            let records = cosigna_agent::evidence::export(&directory(data_dir)?, &out)?;
            println!(
                "{}",
                json!({"schema":"cosigna.ledger-evidence-export/v1","records":records,"out":out})
            );
            Ok(std::process::ExitCode::SUCCESS)
        }
    }
}

fn make_planner(mode: PlannerMode, model: Option<String>) -> Result<Box<dyn Planner>> {
    Ok(match mode {
        PlannerMode::Fixtures => Box::new(FixturePlanner),
        PlannerMode::Openai => Box::new(HostedPlanner::new(
            model.unwrap_or_else(|| "gpt-6.1-sol".into()),
            OpenAITransport::from_env()?,
        )?),
        PlannerMode::Groq => Box::new(HostedPlanner::new(
            model.unwrap_or_else(|| "openai/gpt-oss-120b".into()),
            GroqTransport::from_env()?,
        )?),
    })
}

/// Connects optional live-API rails and case records. Keys are read once and never printed.
fn with_rails(
    mut service: Service,
    stripe_test: bool,
    cases: Option<&std::path::Path>,
) -> Result<Service> {
    if let Some(path) = cases {
        let source = cosigna_agent::cases::CaseSource::load(path)?;
        eprintln!(
            "Cosigna case records: {} (secrets not shown)",
            source.name()
        );
        service.rails = service.rails.clone().with_cases(source);
    }
    if stripe_test {
        let key = match cosigna_agent::env_secret("COSIGNA_STRIPE_TEST_SECRET_KEY") {
            Some(key) => key,
            None => {
                // The credential stored with `fabric secret set stripe-test`.
                let home = std::env::var_os("FABRIC_HOME")
                    .map(PathBuf::from)
                    .or_else(|| dirs::data_local_dir().map(|d| d.join("fabric")))
                    .ok_or_else(|| {
                        Error::Invalid("cannot locate the stored stripe-test credential".into())
                    })?;
                std::fs::read_to_string(home.join("secrets").join("stripe-test")).map_err(|_| {
                    Error::Invalid(
                        "set COSIGNA_STRIPE_TEST_SECRET_KEY or store stripe-test with `fabric secret set`".into(),
                    )
                })?
            }
        };
        let base = std::env::var("COSIGNA_STRIPE_API_BASE").ok();
        let rail = cosigna_agent::stripe_rail::StripeRail::new(&key, base.as_deref())?;
        service.rails = service.rails.clone().with_stripe_test(rail);
        eprintln!("Cosigna rail connected: stripe_test (test mode only; key not shown)");
    }
    Ok(service)
}

fn open_service(directory: &std::path::Path, url: Option<String>) -> Result<Service> {
    match url {
        Some(url) => Service::open_test_merchant(
            directory,
            &url,
            std::env::var("COSIGNA_TEST_MERCHANT_TOKEN").map_err(|_| Error::Merchant)?,
        ),
        None => Service::open(directory),
    }
}

fn demo() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = Service::open(directory.path())?;
    let owner = Actor {
        owner: "demo-user".into(),
        role: Role::Owner,
    };
    let executor = Actor {
        owner: owner.owner.clone(),
        role: Role::Executor,
    };
    let at = now();
    let budget = service.create_budget(
        &owner,
        BudgetInput {
            currency: "USD".into(),
            limit_minor: 2000,
            per_purchase_minor: 1000,
            merchants: vec!["fixture-cafe".into()],
            expires_at: at + 3600,
            second_approval_above_minor: None,
        },
        at,
    )?;
    let mut outcomes = Vec::new();
    for (key, scenario) in [
        ("demo-one", Scenario::Success),
        ("demo-two", Scenario::TimeoutAfterAccept),
        ("demo-over-budget", Scenario::Success),
    ] {
        let task = service.create_task(
            &owner,
            TaskInput {
                budget_id: budget.id.clone(),
                request: "Order one oat latte".into(),
                idempotency_key: key.into(),
                required_features: vec!["oat_milk".into()],
            },
            at,
        )?;
        let quoted = service.plan(&owner, &task.id, &FixturePlanner, at)?;
        let digest = quoted.quote.as_ref().ok_or(Error::Planner)?.digest()?;
        match service.approve(&owner, &task.id, &digest, at) {
            Ok(_) => {
                let submitted = service.execute(&executor, &task.id, scenario, at)?;
                let reconciled = service.reconcile(&owner, &task.id, at)?;
                outcomes.push(json!({"request":key,"after_submit":submitted.stage,"after_reconcile":reconciled.stage}));
            }
            Err(Error::BudgetExceeded) => {
                outcomes.push(json!({"request":key,"blocked":"budget_exceeded"}))
            }
            Err(error) => return Err(error),
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"schema":cosigna_agent::API_VERSION,
        "simulated":true,"planner":"fixture-script (not AI)","outcomes":outcomes,
        "balance":service.balance(&owner, &budget.id)?,"synthetic_orders":service.provider.order_count()?}))?
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Cosigna: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
