//! `fabric`: a flight recorder for AI coding agents.
//!
//! Every command accepts `--json` for versioned machine-readable output and uses the exit
//! codes in [`error::exit`]. See `docs/CLI.md`.

mod bisect;
mod checkpoint;
mod claude;
mod codex;
mod commands;
mod error;
mod home;
mod mcp;
mod query;
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
    about = "Flight recorder for AI coding agents"
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
