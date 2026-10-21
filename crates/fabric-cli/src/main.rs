//! `fabric`: a flight recorder for AI coding agents.
//!
//! Every command accepts `--json` for versioned machine-readable output and uses the exit
//! codes in [`error::exit`]. See `docs/CLI.md`.

mod agentauth;
mod approvals;
mod bisect;
mod changes;
mod checkpoint;
mod claude;
mod codex;
mod commands;
mod enforce;
mod error;
mod evidence;
mod guard;
mod home;
mod identity;
mod lessons;
mod mandates;
mod mcp;
mod mcp_proxy;
mod pack;
mod query;
mod report;
mod results;
mod signoff;
mod timefmt;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::json;

use crate::commands::Ctx;
use crate::error::CliError;
use crate::home::Home;

#[derive(Parser)]
#[command(
    name = "fabric",
    version,
    about = "Change control for AI coding agents: record, approve, undo and evidence every change"
)]
struct Cli {
    /// Print versioned JSON instead of human-readable output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start recording a project (default: the current directory).
    Init { path: Option<PathBuf> },

    /// Record a checkpoint of the project as it is now.
    Checkpoint {
        /// What this checkpoint is for.
        #[arg(short, long)]
        message: Option<String>,
        /// Which agent or tool made the changes.
        #[arg(long)]
        agent: Option<String>,
        /// Record nothing if the project is unchanged since the latest checkpoint.
        #[arg(long)]
        if_changed: bool,
    },

    /// List checkpoints, newest first.
    Log {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },

    /// Show what changed between checkpoints, or from a checkpoint to now.
    Diff {
        /// Checkpoint to compare from (default: the latest).
        from: Option<String>,
        /// Checkpoint to compare to (default: the project as it is now).
        to: Option<String>,
    },

    /// Restore the project to a checkpoint. Shows the plan; applies only with --yes.
    Restore {
        /// Checkpoint number (as in `fabric log`), `latest`, or a snapshot id prefix.
        checkpoint: String,
        /// Apply the restore. The current state is checkpointed first, so it can be undone.
        #[arg(long)]
        yes: bool,
    },

    /// Undo one checkpoint's changes, keeping everything recorded after it.
    Revert {
        /// Checkpoint number (as in `fabric log`) whose changes to undo.
        #[arg(required_unless_present = "change", conflicts_with = "change")]
        checkpoint: Option<String>,
        /// Undo a whole authorized change instead (as in `fabric changes --all`): every file
        /// its agent session touched goes back to how it was when the change started.
        #[arg(long)]
        change: Option<u64>,
        /// Apply the revert. The current state is checkpointed first, so it can be undone.
        #[arg(long)]
        yes: bool,
    },

    /// Find the checkpoint where a test command started failing.
    Bisect {
        /// Command that exits 0 when the project is good, e.g. "pytest -x" or "npm test".
        #[arg(long)]
        test: String,
        /// A checkpoint where the tests pass (default: the oldest).
        #[arg(long)]
        good: Option<String>,
        /// A checkpoint where the tests fail (default: the project as it is now).
        #[arg(long)]
        bad: Option<String>,
    },

    /// Show what one checkpoint changed, with diffs.
    Show {
        /// Checkpoint number from `fabric log`.
        checkpoint: u64,
    },

    /// Show every recorded change to one file, and which agent made it.
    History {
        /// File path, relative to the current directory or absolute.
        path: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },

    /// Finish a restore that was interrupted.
    Resume,

    /// Run the MCP server that lets agents query this project's history (started by agents).
    Mcp,

    /// Put this project's rules and evidence in front of another MCP server: configure your
    /// agent to start this instead of the server (started by agents, not people).
    McpProxy {
        /// The server's name as rules see it: tools are matched as `<name>/<tool>`.
        #[arg(long)]
        name: String,
        /// The server's command and arguments, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
        /// Give the server a stored credential as an environment variable, VAR=name
        /// (repeatable; see `fabric secret`). The agent never sees it.
        #[arg(long = "secret-env", value_name = "VAR=NAME")]
        secret_env: Vec<String>,
        /// The stored consent (an AgentAuth delegation token, see `fabric secret`) that
        /// payment tools are authorized against.
        #[arg(long, value_name = "NAME")]
        consent: Option<String>,
    },

    /// Connect this project to an AgentAuth API, which authorizes payment tools.
    Connect {
        #[arg(value_parser = ["agentauth"])]
        service: String,
        /// The API's base URL (https; http only for localhost).
        #[arg(long)]
        url: String,
        /// The stored credential holding the API key (see `fabric secret set`).
        #[arg(long)]
        api_key_secret: String,
    },

    /// Tool calls that move money, authorized by AgentAuth before they run.
    Payments {
        #[command(subcommand)]
        command: PaymentsCommand,
    },

    /// Agents' identities: tokens issued by a person, scoped to tools.
    #[command(name = "agent")]
    AgentIdentity {
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Credentials the gateway gives to MCP servers; agents never see them.
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },

    /// Lessons learned from steps that broke things (experimental).
    Lesson {
        #[command(subcommand)]
        command: LessonCommand,
    },

    /// Consent rules: which agent actions need your approval or are blocked.
    Guard {
        #[command(subcommand)]
        command: GuardCommand,
    },

    /// Approval requests waiting for a person's sign-off.
    Approvals {
        /// Show every request, not only those waiting.
        #[arg(long)]
        all: bool,
        #[command(subcommand)]
        command: Option<ApprovalsCommand>,
    },

    /// Mandates: bounded authority granted to agents ahead of time (limits per call and per
    /// day, an expiry); calls within one need no approval each.
    Mandate {
        #[command(subcommand)]
        command: MandateCommand,
    },

    /// Approvers who sign decisions with their own key, possibly on another machine.
    Approver {
        #[command(subcommand)]
        command: ApproverCommand,
    },

    /// Review an agent's held change and approve (or reject) it. Run this in your own
    /// terminal; agents cannot approve their own changes.
    Approve {
        /// Request numbers, as in `fabric approvals`; several are reviewed together.
        #[arg(required_unless_present = "signed")]
        ids: Vec<u64>,
        /// Apply a decision signed by a registered approver (`fabric approver sign`).
        #[arg(long, value_name = "FILE")]
        signed: Option<PathBuf>,
        /// Reject the change instead.
        #[arg(long)]
        reject: bool,
        /// Who is approving (default: your Git email, then your OS account).
        #[arg(long = "as")]
        approver: Option<String>,
        /// A note recorded with the decision, e.g. a change ticket.
        #[arg(long)]
        note: Option<String>,
        /// Record the decision without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Also authorize the whole task as a change with this title: the agent session's
        /// further edits to the change's paths proceed, recorded, until `fabric review`.
        #[arg(long, value_name = "TITLE")]
        change: Option<String>,
        /// Paths the change covers, as globs (repeatable; default: the requested files).
        #[arg(long = "path", value_name = "GLOB", requires = "change")]
        paths: Vec<String>,
    },

    /// Changes authorized with `fabric approve --change`, waiting for review.
    Changes {
        /// Show every change, not only open ones.
        #[arg(long)]
        all: bool,
    },

    /// Review the result of an authorized change and accept (or reject) it. Run this in your
    /// own terminal.
    Review {
        /// The change number, as in `fabric changes`.
        id: u64,
        /// Reject the result instead.
        #[arg(long)]
        reject: bool,
        /// Who is reviewing (default: your Git email, then your OS account).
        #[arg(long = "as")]
        reviewer: Option<String>,
        /// A note recorded with the decision.
        #[arg(long)]
        note: Option<String>,
        /// Record the decision without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Write the review to a file for an approver to sign elsewhere, deciding nothing here.
        #[arg(long, value_name = "FILE", conflicts_with = "signed")]
        export: Option<PathBuf>,
        /// Apply a review signed by a registered approver (`fabric approver sign`).
        #[arg(long, value_name = "FILE")]
        signed: Option<PathBuf>,
    },

    /// Evidence for reviewers and auditors: verify the record, report on a change, export a
    /// signed pack.
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },

    /// Connect a coding agent: record its work and let it query what other agents did.
    Integrate {
        #[command(subcommand)]
        agent: Agent,
    },

    /// Entry point for agent hooks (called by the agent, not by people).
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        agent: Agent,
    },
}

#[derive(Subcommand)]
enum LessonCommand {
    /// Record a lesson from a checkpoint whose change broke something.
    Add {
        /// The checkpoint that caused the failure (as in `fabric log` or `fabric bisect`).
        #[arg(long)]
        step: u64,
        /// The correction, in your words, e.g. "discount is a fraction, never dollars".
        #[arg(long)]
        rule: String,
        /// A test the change broke (repeatable). Default: failures in the last test run.
        #[arg(long = "failing-test")]
        failing_tests: Vec<String>,
    },
    /// List lessons.
    List,
    /// Stop using a lesson.
    Retire { id: u64 },
}

#[derive(Subcommand)]
enum GuardCommand {
    /// List the rules and what each decides for this project.
    Rules,
    /// Show what the guard would decide for a command, or a file write with --file.
    Check {
        /// A shell command, e.g. "rm -rf build".
        command: Option<String>,
        /// A file an agent wants to write.
        #[arg(long)]
        file: Option<String>,
    },
    /// Change a rule's decision for this project: allow, ask, approve or deny.
    Set { rule: String, decision: String },
    /// Install a rule pack for this project. Available: finance.
    Pack {
        name: String,
        /// Paths in scope, as globs (repeatable), e.g. "src/billing/**". The finance pack
        /// defaults to common ledger, billing and payments directories.
        #[arg(long)]
        scope: Vec<String>,
    },
    /// Add (or replace) a project rule matching file paths or commands.
    AddRule {
        /// Lowercase id, e.g. "billing-code".
        id: String,
        /// allow, ask, approve or deny.
        #[arg(long)]
        decision: String,
        /// A glob over project-relative paths (repeatable).
        #[arg(long = "path")]
        paths: Vec<String>,
        /// A case-insensitive pattern over shell commands (repeatable).
        #[arg(long = "command")]
        commands: Vec<String>,
        /// A glob over MCP tool calls as `server/tool`, e.g. "*/post_journal_entry" (repeatable).
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Only files already committed to Git (e.g. applied migrations).
        #[arg(long)]
        committed_only: bool,
        #[arg(long)]
        description: Option<String>,
    },
    /// Remove a project rule.
    RemoveRule { id: String },
    /// Actions that needed approval or were blocked, newest first.
    Log {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum ApprovalsCommand {
    /// Write held requests to a file an approver signs elsewhere (`fabric approver sign`).
    Export {
        /// Request numbers, as in `fabric approvals`.
        #[arg(required = true)]
        ids: Vec<u64>,
        /// Where to write the request (default: print it).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PaymentsCommand {
    /// Declare a payment tool and where its amount, currency and counterparty are.
    Add {
        /// The tool, exactly, as server/tool (e.g. stripe/create_refund).
        tool: String,
        /// The argument holding the amount (dotted for nested fields).
        #[arg(long)]
        amount: String,
        /// Amounts are in the currency's smallest unit (cents), as Stripe sends them.
        #[arg(long)]
        minor_units: bool,
        /// The argument holding the currency.
        #[arg(long)]
        currency: Option<String>,
        /// The currency when the call has none, e.g. USD.
        #[arg(long)]
        default_currency: Option<String>,
        /// The argument naming the counterparty (merchant, customer, payee).
        #[arg(long)]
        merchant: Option<String>,
    },
    /// Declared payment tools.
    List,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Register an agent and print its token (once).
    Add {
        name: String,
        /// Tool calls it may make, as `server/tool` globs (repeatable; default: any).
        #[arg(long = "tool")]
        tools: Vec<String>,
    },
    /// Revoke an agent's token.
    Revoke { name: String },
    /// Registered agents.
    List,
    /// Require (on) or stop requiring (off) a valid agent token for calls through the proxy.
    Require {
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
}

#[derive(Subcommand)]
enum SecretCommand {
    /// Store a credential read from standard input, e.g. `fabric secret set stripe-test < key.txt`.
    Set { name: String },
    /// Remove a stored credential.
    Remove { name: String },
    /// Names of stored credentials (never their values).
    List,
}

// Parsed once per run; the size difference between variants does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum MandateCommand {
    /// Grant a mandate (or, with --out, write it for an approver to sign; with --signed, apply
    /// a signed one).
    Grant {
        /// What the mandate is for, e.g. "Small customer refunds".
        #[arg(required_unless_present = "signed")]
        title: Option<String>,
        /// Tool calls it covers, as `server/tool` globs (repeatable), e.g. "*/issue_refund".
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Only this agent (an MCP client's name, or claude-code).
        #[arg(long)]
        agent: Option<String>,
        /// Largest amount per call, e.g. 500 or 500.00.
        #[arg(long)]
        max_per_call: Option<String>,
        /// Largest total per day (UTC).
        #[arg(long)]
        max_per_day: Option<String>,
        /// Most calls per day (UTC).
        #[arg(long)]
        max_calls_per_day: Option<u64>,
        /// The day it ends, YYYY-MM-DD (it stops at the start of that day, UTC).
        #[arg(long)]
        expires: Option<String>,
        /// Write the mandate to this file for an approver to sign instead of granting it.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Apply a mandate signed by a registered approver.
        #[arg(long, value_name = "FILE", conflicts_with_all = ["title", "out"])]
        signed: Option<PathBuf>,
    },
    /// Revoke a mandate: calls it covered are held for a person again.
    Revoke { id: u64 },
    /// Mandates, their limits and today's use.
    List,
}

#[derive(Subcommand)]
enum ApproverCommand {
    /// This installation's approver key: give the public key to projects you approve for.
    Key,
    /// Register an approver for this project by their public key.
    Add {
        name: String,
        /// Their public key, as `fabric approver key` prints it.
        #[arg(long)]
        key: String,
    },
    /// Remove a registered approver.
    Remove { name: String },
    /// Registered approvers, and whether signed approvals are required.
    List,
    /// Require (on) or stop requiring (off) signed approvals and reviews from another
    /// installation.
    Require {
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Review an exported request or review and sign your decision. Works without the project.
    Sign {
        /// The file from `fabric approvals export` or `fabric review --export`.
        file: PathBuf,
        /// Reject instead.
        #[arg(long)]
        reject: bool,
        /// Also authorize the whole task as a change with this title.
        #[arg(long, value_name = "TITLE")]
        change: Option<String>,
        /// Paths the change covers, as globs (repeatable; default: the requested files).
        #[arg(long = "path", value_name = "GLOB", requires = "change")]
        paths: Vec<String>,
        /// A note recorded with the decision.
        #[arg(long)]
        note: Option<String>,
        /// Sign without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Where to write the signed decision (default: next to the request).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum EvidenceCommand {
    /// Check that the evidence chain is intact and list exceptions.
    Verify {
        /// Also re-hash every file content the recorded checkpoints refer to.
        #[arg(long)]
        deep: bool,
    },
    /// A reviewer's report on one change.
    Report {
        /// Checkpoint number, as in `fabric log`.
        #[arg(required_unless_present = "change", conflicts_with = "change")]
        checkpoint: Option<u64>,
        /// Report on a whole authorized change instead (as in `fabric changes --all`).
        #[arg(long)]
        change: Option<u64>,
    },
    /// Write a signed evidence pack to a new directory.
    Export {
        #[arg(long)]
        out: PathBuf,
    },
    /// Check an evidence pack (does not need the project or its history).
    VerifyPack {
        dir: PathBuf,
        /// The signing key fingerprint you expect, obtained separately.
        #[arg(long)]
        key: Option<String>,
    },
    /// Show this installation's evidence signing key.
    Key,
    /// Tool calls agents made through `fabric mcp-proxy`, newest first: what was called with
    /// which arguments, the decision, and what came back.
    Calls {
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum Agent {
    /// Anthropic's Claude Code.
    ClaudeCode,
    /// OpenAI's Codex.
    Codex,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    let home = match Home::locate() {
        Ok(home) => home,
        Err(e) => return fail(&e, json),
    };

    // Hooks must never fail from the agent's point of view.
    if let Command::Hook { agent } = &cli.command {
        if matches!(agent, Agent::ClaudeCode) {
            claude::hook(&home);
        }
        return ExitCode::SUCCESS;
    }

    let ctx = Ctx { home, json };
    let result = match cli.command {
        Command::Init { path } => commands::init(&ctx, path),
        Command::Checkpoint {
            message,
            agent,
            if_changed,
        } => commands::checkpoint(&ctx, message, agent, if_changed),
        Command::Log { limit } => commands::log(&ctx, limit),
        Command::Diff { from, to } => commands::diff_cmd(&ctx, from, to),
        Command::Restore { checkpoint, yes } => commands::restore(&ctx, &checkpoint, yes),
        Command::Revert {
            checkpoint,
            change,
            yes,
        } => match (checkpoint, change) {
            (_, Some(id)) => commands::revert_change(&ctx, id, yes),
            (Some(checkpoint), None) => commands::revert(&ctx, &checkpoint, yes),
            (None, None) => unreachable!("clap requires a checkpoint or --change"),
        },
        Command::Bisect { test, good, bad } => bisect::bisect(&ctx, &test, good, bad),
        Command::Show { checkpoint } => commands::show(&ctx, checkpoint),
        Command::History { path, limit } => commands::history(&ctx, &path, limit),
        Command::Resume => commands::resume(&ctx),
        Command::Mcp => mcp::serve(&ctx.home).map(|()| error::exit::OK),
        Command::McpProxy {
            name,
            command,
            secret_env,
            consent,
        } => ctx.project().and_then(|project| {
            mcp_proxy::run(
                &ctx.home,
                project,
                &name,
                &command,
                &secret_env,
                consent.as_deref(),
            )
        }),
        Command::Connect {
            service: _,
            url,
            api_key_secret,
        } => ctx.project().and_then(|project| {
            agentauth::connect_cmd(&ctx.home, &project, &url, &api_key_secret, json)
        }),
        Command::Payments { command } => ctx.project().and_then(|project| match command {
            PaymentsCommand::Add {
                tool,
                amount,
                minor_units,
                currency,
                default_currency,
                merchant,
            } => agentauth::payments_add_cmd(
                &project,
                agentauth::PaymentTool {
                    tool,
                    amount,
                    minor_units,
                    currency,
                    default_currency: default_currency.map(|c| c.to_ascii_uppercase()),
                    merchant,
                },
                json,
            ),
            PaymentsCommand::List => agentauth::payments_list_cmd(&project, json),
        }),
        Command::AgentIdentity { command } => ctx.project().and_then(|project| match command {
            AgentCommand::Add { name, tools } => identity::add_cmd(&project, &name, &tools, json),
            AgentCommand::Revoke { name } => identity::revoke_cmd(&project, &name, json),
            AgentCommand::List => identity::list_cmd(&project, json),
            AgentCommand::Require { state } => identity::require_cmd(&project, state == "on", json),
        }),
        Command::Secret { command } => match command {
            SecretCommand::Set { name } => identity::secret_set_cmd(&ctx.home, &name, json),
            SecretCommand::Remove { name } => identity::secret_remove_cmd(&ctx.home, &name, json),
            SecretCommand::List => identity::secret_list_cmd(&ctx.home, json),
        },
        Command::Lesson { command } => ctx.project().and_then(|project| match command {
            LessonCommand::Add {
                step,
                rule,
                failing_tests,
            } => lessons::add_cmd(&ctx.home, &project, step, &rule, &failing_tests, json),
            LessonCommand::List => lessons::list_cmd(&project, json),
            LessonCommand::Retire { id } => lessons::retire_cmd(&project, id, json),
        }),
        Command::Guard { command } => ctx.project().and_then(|project| match command {
            GuardCommand::Rules => guard::rules(&project, json),
            GuardCommand::Check { command, file } => guard::check(
                &ctx.home,
                &project,
                command.as_deref(),
                file.as_deref(),
                json,
            ),
            GuardCommand::Set { rule, decision } => guard::set(&project, &rule, &decision, json),
            GuardCommand::Log { limit } => guard::log(&project, limit, json),
            GuardCommand::Pack { name, scope } => guard::pack(&project, &name, &scope, json),
            GuardCommand::AddRule {
                id,
                decision,
                paths,
                commands,
                tools,
                committed_only,
                description,
            } => guard::parse_decision(&decision).and_then(|decision| {
                guard::add_rule(
                    &project,
                    guard::CustomRule {
                        description: description.unwrap_or_else(|| format!("Project rule {id}")),
                        id,
                        decision,
                        paths,
                        commands,
                        tools,
                        committed_only,
                    },
                    json,
                )
            }),
            GuardCommand::RemoveRule { id } => guard::remove_rule(&project, &id, json),
        }),
        Command::Approvals { all, command } => ctx.project().and_then(|project| match command {
            None => approvals::list_cmd(&project, all, json),
            Some(ApprovalsCommand::Export { ids, out }) => {
                approvals::export_cmd(&project, &ids, out.as_deref(), json)
            }
        }),
        Command::Mandate { command } => ctx.project().and_then(|project| match command {
            MandateCommand::Grant {
                title,
                tools,
                agent,
                max_per_call,
                max_per_day,
                max_calls_per_day,
                expires,
                out,
                signed,
            } => {
                let terms = title.map(|title| mandates::Terms {
                    title,
                    tools,
                    agent,
                    max_per_call,
                    max_per_day,
                    max_calls_per_day,
                    expires: expires.unwrap_or_default(),
                });
                mandates::grant_cmd(
                    &ctx.home,
                    &project,
                    terms,
                    out.as_deref(),
                    signed.as_deref(),
                    json,
                )
            }
            MandateCommand::Revoke { id } => mandates::revoke_cmd(&project, id, json),
            MandateCommand::List => mandates::list_cmd(&project, json),
        }),
        Command::Approver { command } => match command {
            ApproverCommand::Key => signoff::key_cmd(&ctx.home, json),
            ApproverCommand::Sign {
                file,
                reject,
                change,
                paths,
                note,
                yes,
                out,
            } => signoff::sign_cmd(
                &ctx.home,
                &file,
                &signoff::SignOptions {
                    reject,
                    change: change.as_deref(),
                    paths: &paths,
                    note: note.as_deref(),
                    yes,
                    out: out.as_deref(),
                },
                json,
            ),
            command => ctx.project().and_then(|project| match command {
                ApproverCommand::Add { name, key } => signoff::add_cmd(&project, &name, &key, json),
                ApproverCommand::Remove { name } => signoff::remove_cmd(&project, &name, json),
                ApproverCommand::List => signoff::list_cmd(&project, json),
                ApproverCommand::Require { state } => {
                    signoff::require_cmd(&project, state == "on", json)
                }
                ApproverCommand::Key | ApproverCommand::Sign { .. } => {
                    unreachable!("handled above")
                }
            }),
        },
        Command::Approve {
            ids,
            signed,
            reject,
            approver,
            note,
            yes,
            change,
            paths,
        } => ctx.project().and_then(|project| {
            approvals::approve_cmd(
                &ctx.home,
                &project,
                &approvals::Decision {
                    ids: &ids,
                    reject,
                    approver: approver.as_deref(),
                    note: note.as_deref(),
                    yes,
                    change: change.as_deref(),
                    paths: &paths,
                    signed: signed.as_deref(),
                },
                json,
            )
        }),
        Command::Changes { all } => ctx
            .project()
            .and_then(|project| changes::list_cmd(&project, all, json)),
        Command::Review {
            id,
            reject,
            reviewer,
            note,
            yes,
            export,
            signed,
        } => ctx.project().and_then(|project| {
            let store = ctx.home.open_store()?;
            changes::review_cmd(
                &ctx.home,
                &store,
                &project,
                &changes::Review {
                    id,
                    reject,
                    reviewer: reviewer.as_deref(),
                    note: note.as_deref(),
                    yes,
                    export: export.as_deref(),
                    signed: signed.as_deref(),
                },
                json,
            )
        }),
        Command::Evidence { command } => match command {
            EvidenceCommand::Verify { deep } => pack::verify_cmd(&ctx, deep),
            EvidenceCommand::Report { checkpoint, change } => match (checkpoint, change) {
                (_, Some(id)) => pack::change_report_cmd(&ctx, id),
                (Some(seq), None) => pack::report_cmd(&ctx, seq),
                (None, None) => unreachable!("clap requires a checkpoint or --change"),
            },
            EvidenceCommand::Export { out } => pack::export_cmd(&ctx, &out),
            EvidenceCommand::VerifyPack { dir, key } => {
                pack::verify_pack_cmd(&ctx, &dir, key.as_deref())
            }
            EvidenceCommand::Key => pack::key_cmd(&ctx),
            EvidenceCommand::Calls { limit } => pack::calls_cmd(&ctx, limit),
        },
        Command::Integrate {
            agent: Agent::ClaudeCode,
        } => claude::integrate(&ctx.home, json),
        Command::Integrate {
            agent: Agent::Codex,
        } => codex::integrate(json),
        Command::Hook { .. } => unreachable!("handled above"),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => fail(&e, json),
    }
}

fn fail(error: &CliError, json: bool) -> ExitCode {
    if json {
        let value = json!({
            "schema": "fabric.error/v1",
            "error": { "kind": error.kind, "message": error.message },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("output always serializes")
        );
    } else {
        eprintln!("error: {error}");
    }
    ExitCode::from(error.code)
}
