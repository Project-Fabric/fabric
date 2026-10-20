//! `fabric`: a flight recorder for AI coding agents.
//!
//! Every command accepts `--json` for versioned machine-readable output and uses the exit
//! codes in [`error::exit`]. See `docs/CLI.md`.

mod approvals;
mod bisect;
mod checkpoint;
mod claude;
mod codex;
mod commands;
mod error;
mod evidence;
mod guard;
mod home;
mod lessons;
mod mcp;
mod pack;
mod query;
mod report;
mod results;
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
        checkpoint: String,
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
    },

    /// Review an agent's held change and approve (or reject) it. Run this in your own
    /// terminal; agents cannot approve their own changes.
    Approve {
        /// Request number, as in `fabric approvals`.
        id: u64,
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
        checkpoint: u64,
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
        Command::Revert { checkpoint, yes } => commands::revert(&ctx, &checkpoint, yes),
        Command::Bisect { test, good, bad } => bisect::bisect(&ctx, &test, good, bad),
        Command::Show { checkpoint } => commands::show(&ctx, checkpoint),
        Command::History { path, limit } => commands::history(&ctx, &path, limit),
        Command::Resume => commands::resume(&ctx),
        Command::Mcp => mcp::serve(&ctx.home).map(|()| error::exit::OK),
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
                        committed_only,
                    },
                    json,
                )
            }),
            GuardCommand::RemoveRule { id } => guard::remove_rule(&project, &id, json),
        }),
        Command::Approvals { all } => ctx
            .project()
            .and_then(|project| approvals::list_cmd(&project, all, json)),
        Command::Approve {
            id,
            reject,
            approver,
            note,
            yes,
        } => ctx.project().and_then(|project| {
            approvals::approve_cmd(
                &project,
                &approvals::Decision {
                    id,
                    reject,
                    approver: approver.as_deref(),
                    note: note.as_deref(),
                    yes,
                },
                json,
            )
        }),
        Command::Evidence { command } => match command {
            EvidenceCommand::Verify { deep } => pack::verify_cmd(&ctx, deep),
            EvidenceCommand::Report { checkpoint } => pack::report_cmd(&ctx, checkpoint),
            EvidenceCommand::Export { out } => pack::export_cmd(&ctx, &out),
            EvidenceCommand::VerifyPack { dir, key } => {
                pack::verify_pack_cmd(&ctx, &dir, key.as_deref())
            }
            EvidenceCommand::Key => pack::key_cmd(&ctx),
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
