//! Evidence reports: one page per change, written for a reviewer or auditor — what changed,
//! which agent and prompt it came from, which rules applied, who approved it, and how to
//! reverse it. Also finds *exceptions*: controlled files an agent changed without an
//! approval (for example through a shell command the guard could not attribute to a path).

use std::fmt::Write as _;

use fabric_snapshot::{Manifest, diff};
use fabric_store::Store;
use serde::Serialize;
use serde_json::Value;

use crate::checkpoint::{Checkpoint, CheckpointKind, read_log};
use crate::commands::{changes_text, kind_name};
use crate::error::{CliError, exit};
use crate::evidence::{self, Record};
use crate::guard::Policy;
use crate::home::Project;
use crate::query;

/// A controlled file an agent changed without an approval in the same step.
#[derive(Debug, Clone, Serialize)]
pub struct Exception {
    pub checkpoint: u64,
    pub path: String,
    pub rules: Vec<String>,
}

/// The evidence records between the previous checkpoint's record and this checkpoint's, i.e.
/// what happened while this step was being made.
fn window(records: &[Record], seq: u64) -> &[Record] {
    let is_checkpoint = |r: &Record, n: u64| {
        r.body.event["kind"] == "checkpoint" && r.body.event["checkpoint"].as_u64() == Some(n)
    };
    let Some(end) = records.iter().position(|r| is_checkpoint(r, seq)) else {
        return &[];
    };
    let start = records[..end]
        .iter()
        .rposition(|r| r.body.event["kind"] == "checkpoint")
        .map_or(0, |i| i + 1);
    &records[start..end]
}

/// Controlled files changed by agent step `checkpoint` without an approval covering them.
pub fn exceptions_for(
    store: &Store,
    policy: &Policy,
    log: &[Checkpoint],
    records: &[Record],
    checkpoint: &Checkpoint,
) -> Result<Vec<Exception>, CliError> {
    if checkpoint.kind != CheckpointKind::AgentStep {
        return Ok(Vec::new());
    }
    let Some(previous) = log.iter().take_while(|c| c.seq < checkpoint.seq).last() else {
        return Ok(Vec::new());
    };
    let d = diff(
        &Manifest::load(store, &previous.snapshot)?,
        &Manifest::load(store, &checkpoint.snapshot)?,
    );
    let events = window(records, checkpoint.seq);
    let used: Vec<&Value> = events
        .iter()
        .map(|r| &r.body.event)
        .filter(|e| e["kind"] == "approval_used")
        .collect();
    // Files Fabric already rolled back after this step are resolved, not exceptions.
    let rolled_back: Vec<&str> = records
        .iter()
        .map(|r| &r.body.event)
        .filter(|e| e["kind"] == "rollback" && e["step"].as_u64() == Some(checkpoint.seq))
        .flat_map(|e| e["paths"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let covered = |path: &str| {
        rolled_back.contains(&path)
            || used
                .iter()
                .any(|e| e["tool"] == "Bash" || e["target"].as_str() == Some(path))
    };
    let changed = d
        .added
        .iter()
        .chain(&d.modified)
        .chain(&d.removed)
        .map(|p| p.as_str().to_owned())
        .chain(
            d.renamed
                .iter()
                .flat_map(|r| [r.from.as_str().to_owned(), r.to.as_str().to_owned()]),
        );
    let mut out = Vec::new();
    for path in changed {
        let rules = policy.controlling_rules(&path);
        if !rules.is_empty() && !covered(&path) {
            out.push(Exception {
                checkpoint: checkpoint.seq,
                path,
                rules: rules.iter().map(|r| r.id.clone()).collect(),
            });
        }
    }
    Ok(out)
}

fn describe(event: &Value) -> Option<String> {
    let s = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
    Some(match event["kind"].as_str()? {
        "guard" => format!(
            "Guard decided **{}** ({}) for {} `{}`",
            s("decision"),
            s("rule"),
            s("tool"),
            s("target")
        ),
        "approval_requested" => format!(
            "Approval request #{} recorded ({}) for {} `{}`",
            event["request"],
            s("rule"),
            s("tool"),
            s("target")
        ),
        "approval_granted" => format!(
            "Request #{} **approved** by {}{}{}",
            event["request"],
            s("approver"),
            if event["same_os_account_as_agent"] == true {
                " (same OS account as the agent)"
            } else {
                ""
            },
            event["note"]
                .as_str()
                .map(|n| format!(": \"{n}\""))
                .unwrap_or_default()
        ),
        "approval_rejected" => format!(
            "Request #{} **rejected** by {}",
            event["request"],
            s("approver")
        ),
        "approval_used" => format!(
            "Approved request #{} used for {} `{}`",
            event["request"],
            s("tool"),
            s("target")
        ),
        "policy_changed" => format!("Guard rules changed ({}) by {}", s("change"), s("by")),
        "rollback" => format!(
            "Fabric **rolled back** unapproved changes from step #{} to {}",
            event["step"],
            event["paths"]
                .as_array()
                .map(|p| p
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default()
        ),
        _ => return None,
    })
}

/// The report for checkpoint `seq`, in Markdown, and its exceptions.
pub fn checkpoint_report(
    store: &Store,
    project: &Project,
    seq: u64,
) -> Result<(String, Vec<Exception>), CliError> {
    let log = read_log(project)?;
    let (_, records) = evidence::verify_chain(&evidence::read_chain(project)?);
    let policy = Policy::load(project)?;
    render(store, project, &policy, &log, &records, seq)
}

pub fn render(
    store: &Store,
    project: &Project,
    policy: &Policy,
    log: &[Checkpoint],
    records: &[Record],
    seq: u64,
) -> Result<(String, Vec<Exception>), CliError> {
    let checkpoint = log.iter().find(|c| c.seq == seq).ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_checkpoint",
            format!("no checkpoint #{seq}; see `fabric log`"),
        )
    })?;
    let exceptions = exceptions_for(store, policy, log, records, checkpoint)?;
    let prompt = log
        .iter()
        .take_while(|c| c.seq <= seq)
        .filter(|c| c.kind == CheckpointKind::Prompt)
        .last()
        .map(|c| c.label.trim_start_matches("prompt: ").to_owned());
    let record = records.iter().find(|r| {
        r.body.event["kind"] == "checkpoint" && r.body.event["checkpoint"].as_u64() == Some(seq)
    });

    let mut out = format!("# Change #{seq}: {}\n\n", checkpoint.label);
    out.push_str("| | |\n|---|---|\n");
    let _ = writeln!(out, "| Type | {} |", kind_name(checkpoint.kind));
    let _ = writeln!(out, "| Recorded | {} |", checkpoint.created_at);
    let _ = writeln!(
        out,
        "| Agent | {} |",
        checkpoint.agent.as_deref().unwrap_or("person (no agent)")
    );
    if let Some(session) = &checkpoint.session {
        let _ = writeln!(out, "| Session | `{session}` |");
    }
    if let Some(tool) = &checkpoint.tool {
        let _ = writeln!(out, "| Tool | {tool} |");
    }
    if let Some(prompt) = &prompt
        && checkpoint.agent.is_some()
    {
        let _ = writeln!(out, "| Prompt | {} |", prompt.replace('|', "\\|"));
    }
    let _ = writeln!(out, "| Changes | {} |", changes_text(&checkpoint.changes));
    let _ = writeln!(out, "| Snapshot | `{}` |", checkpoint.snapshot);
    match record {
        Some(r) => {
            let _ = writeln!(out, "| Evidence record | #{} `{}` |", r.body.seq, r.hash);
        }
        None => out.push_str("| Evidence record | none (recorded before evidence began) |\n"),
    }

    out.push_str("\n## Controls\n\n");
    let events: Vec<String> = window(records, seq)
        .iter()
        .filter_map(|r| describe(&r.body.event))
        .collect();
    if events.is_empty() {
        out.push_str("No guard decisions or approvals during this change.\n");
    }
    for event in events {
        let _ = writeln!(out, "- {event}");
    }
    if exceptions.is_empty() {
        out.push_str("\nNo exceptions: no controlled file changed without an approval.\n");
    } else {
        out.push_str("\n**Exceptions: controlled files changed without an approval**\n\n");
        for e in &exceptions {
            let _ = writeln!(out, "- `{}` ({})", e.path, e.rules.join(", "));
        }
    }

    out.push_str("\n## What changed\n\n```diff\n");
    out.push_str(&query::step(store, project, seq)?);
    out.push_str("```\n\n## Reversal\n\n");
    let _ = writeln!(
        out,
        "`fabric revert {seq} --yes` undoes only this change and keeps later work; \
         `fabric restore {} --yes` returns the project to the state before it. Both are verified \
         and recorded as new checkpoints.",
        seq.saturating_sub(1).max(1)
    );
    Ok((out, exceptions))
}
