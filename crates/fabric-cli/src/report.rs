//! Evidence reports: one page per change, written for a reviewer or auditor — what changed,
//! which agent and prompt it came from, which rules applied, who approved it, and how to
//! reverse it. Also finds *exceptions*: controlled files an agent changed without an
//! approval (for example through a shell command the guard could not attribute to a path).

use std::fmt::Write as _;

use fabric_snapshot::{Manifest, RelPath, diff};
use fabric_store::Store;
use globset::GlobSet;
use serde::Serialize;
use serde_json::Value;

use crate::changes;
use crate::checkpoint::{Checkpoint, CheckpointKind, read_log};
use crate::commands::{changes_text, kind_name};
use crate::error::{CliError, exit};
use crate::evidence::{self, Record};
use crate::guard::{Decision, Policy, committed};
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
    project: &Project,
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
    let before = Manifest::load(store, &previous.snapshot)?;
    let d = diff(&before, &Manifest::load(store, &checkpoint.snapshot)?);
    let events = window(records, checkpoint.seq);
    // Changes authorized for this step's session before the step was recorded.
    let end = records
        .iter()
        .position(|r| {
            r.body.event["kind"] == "checkpoint"
                && r.body.event["checkpoint"].as_u64() == Some(checkpoint.seq)
        })
        .unwrap_or(records.len());
    let authorized: Vec<GlobSet> = changes::open_in_evidence(records, end)
        .into_iter()
        .filter(|(session, _)| *session == checkpoint.session)
        .map(|(_, paths)| paths)
        .collect();
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
        // A file the step created was not committed before it.
        let existed = RelPath::new(path.clone()).is_ok_and(|rel| before.get(&rel).is_some());
        let rules =
            policy.controlling_rules(&path, &mut || existed && committed(&project.root, &path));
        // A change authorizes edits under `approve` rules; `deny` rules still apply.
        let in_change = rules.iter().all(|r| r.decision == Decision::Approve)
            && authorized.iter().any(|set| set.is_match(&path));
        if !rules.is_empty() && !covered(&path) && !in_change {
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
        "change_opened" => format!(
            "Change #{} \"{}\" **authorized** by {} for {}{}",
            event["change"],
            s("title"),
            s("approver"),
            event["paths"]
                .as_array()
                .map(|p| p
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default(),
            event["note"]
                .as_str()
                .map(|n| format!(": \"{n}\""))
                .unwrap_or_default()
        ),
        "change_edit" => format!(
            "{} `{}` made under change #{}",
            s("tool"),
            s("target"),
            event["change"]
        ),
        "change_reviewed" => format!(
            "Change #{} **{}** by {} at checkpoint #{}",
            event["change"],
            s("outcome"),
            s("reviewer"),
            event["checkpoint"]
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
    let exceptions = exceptions_for(store, project, policy, log, records, checkpoint)?;
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

/// The report for change `id` (authorized with `fabric approve --change`), in Markdown, and the
/// exceptions in its steps: one page per change ticket, as an auditor samples them. Built from
/// the evidence chain and the snapshots alone.
pub fn change_report(
    store: &Store,
    project: &Project,
    policy: &Policy,
    log: &[Checkpoint],
    records: &[Record],
    id: u64,
) -> Result<(String, Vec<Exception>), CliError> {
    let find = |kind: &str| {
        records.iter().position(|r| {
            r.body.event["kind"] == kind && r.body.event["change"].as_u64() == Some(id)
        })
    };
    let opened_at = find("change_opened").ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_change",
            format!("no change #{id} in the evidence; see `fabric changes --all`"),
        )
    })?;
    let reviewed_at = find("change_reviewed");
    let opened = &records[opened_at];
    let o = &opened.body.event;
    let review = reviewed_at.map(|i| &records[i]);
    let session = o["session"].as_str().map(str::to_owned);
    let paths: Vec<String> = o["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect();
    let scope =
        changes::globs(&paths).map_err(|e| CliError::new(exit::ERROR, "invalid_change", e))?;
    let base_seq = o["base_checkpoint"].as_u64().unwrap_or(0);
    let end_seq = match review {
        Some(r) => r.body.event["checkpoint"].as_u64().unwrap_or(0),
        None => log.last().map_or(0, |c| c.seq),
    };
    let checkpoint = |seq: u64| {
        log.iter().find(|c| c.seq == seq).ok_or_else(|| {
            CliError::new(
                exit::INTEGRITY,
                "missing_checkpoint",
                format!("change #{id} refers to checkpoint #{seq}, which is not in the log"),
            )
        })
    };
    let (base, end) = (checkpoint(base_seq)?, checkpoint(end_seq)?);
    let steps: Vec<&Checkpoint> = log
        .iter()
        .filter(|c| c.seq > base_seq && c.seq <= end_seq && c.session == session)
        .filter(|c| c.kind != CheckpointKind::Prompt)
        .collect();
    let prompt = log
        .iter()
        .filter(|c| c.seq <= end_seq && c.session == session && c.kind == CheckpointKind::Prompt)
        .map(|c| c.label.trim_start_matches("prompt: ").replace('|', "\\|"))
        .next();
    let mut exceptions = Vec::new();
    for step in &steps {
        exceptions.extend(exceptions_for(store, project, policy, log, records, step)?);
    }

    let text = |value: &Value| value.as_str().unwrap_or_default().replace('|', "\\|");
    let approver = text(&o["approver"]);
    let mut out = format!(
        "# Change #{id}: {}\n\n| | |\n|---|---|\n",
        text(&o["title"])
    );
    let status = match review {
        Some(r) => format!("**{}**", text(&r.body.event["outcome"])),
        None => "**open: not yet reviewed**".to_owned(),
    };
    let _ = writeln!(out, "| Status | {status} |");
    let reverted = format!("reverted change #{id}:");
    for c in log.iter().filter(|c| c.label.starts_with(&reverted)) {
        let _ = writeln!(
            out,
            "| Reverted | {} at checkpoint #{} (`fabric revert --change {id}`) |",
            c.created_at, c.seq
        );
    }
    let covers: Vec<String> = paths.iter().map(|p| format!("`{p}`")).collect();
    let _ = writeln!(out, "| Covers | {} |", covers.join(", "));
    let _ = writeln!(
        out,
        "| Agent session | `{}` |",
        session.as_deref().unwrap_or("-")
    );
    if let Some(prompt) = &prompt {
        // The session's first prompt: the task the agent was given.
        let _ = writeln!(out, "| Task | {prompt} |");
    }
    let requests: Vec<String> = o["requests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| format!("#{n}"))
        .collect();
    let _ = writeln!(
        out,
        "| Authorized | {} by {approver}{}, with request {} (evidence record #{}) |",
        opened.body.at,
        if o["same_os_account_as_agent"] == true {
            " (same OS account as the agent)"
        } else {
            ""
        },
        requests.join(", "),
        opened.body.seq,
    );
    if o["note"].is_string() {
        let _ = writeln!(out, "| Authorization note | {} |", text(&o["note"]));
    }
    if let Some(r) = review {
        let e = &r.body.event;
        let reviewer = text(&e["reviewer"]);
        let _ = writeln!(
            out,
            "| Reviewed | {} by {reviewer}{} at checkpoint #{end_seq} (evidence record #{}) |",
            r.body.at,
            if reviewer == approver {
                " (the same person who authorized it)"
            } else {
                ""
            },
            r.body.seq,
        );
        if e["note"].is_string() {
            let _ = writeln!(out, "| Review note | {} |", text(&e["note"]));
        }
    }

    let before = Manifest::load(store, &base.snapshot)?;
    let after = Manifest::load(store, &end.snapshot)?;
    let files = changes::changed_files(&before, &after, &scope);
    if let Some(r) = review {
        // The review recorded each file's content id; check them against the snapshot of the
        // reviewed checkpoint.
        let matches = r.body.event["files"].as_object().is_some_and(|recorded| {
            recorded.len() == files.len()
                && files.iter().all(|(path, id)| {
                    recorded.get(path.as_str()).map(Value::as_str) == Some(id.as_deref())
                })
        });
        out.push_str(if matches {
            "\nThe content recorded at review matches the reviewed checkpoint's snapshot.\n"
        } else {
            "\n**The content recorded at review does not match the reviewed checkpoint's snapshot.**\n"
        });
    }

    out.push_str("\n## Steps\n\n");
    if steps.is_empty() {
        out.push_str("No steps recorded.\n");
    }
    for step in &steps {
        let _ = writeln!(
            out,
            "- #{} {} {} ({})",
            step.seq,
            step.created_at,
            step.label,
            changes_text(&step.changes)
        );
    }

    out.push_str("\n## Controls\n\n");
    let last = reviewed_at.map_or(records.len(), |i| i + 1);
    for record in &records[opened_at..last] {
        let e = &record.body.event;
        let other_change = e["change"].as_u64().is_some_and(|c| c != id);
        let other_session = e["session"]
            .as_str()
            .is_some_and(|s| Some(s) != session.as_deref());
        if other_change || other_session {
            continue;
        }
        if let Some(line) = describe(e) {
            let _ = writeln!(out, "- {line}");
        }
    }
    if exceptions.is_empty() {
        out.push_str(
            "\nNo exceptions: no controlled file changed without an approval or this change.\n",
        );
    } else {
        out.push_str("\n**Exceptions: controlled files changed without an approval**\n\n");
        for e in &exceptions {
            let _ = writeln!(
                out,
                "- `{}` in step #{} ({})",
                e.path,
                e.checkpoint,
                e.rules.join(", ")
            );
        }
    }

    let _ = write!(
        out,
        "\n## What changed under the change's paths (#{base_seq} to #{end_seq})\n\n```diff\n"
    );
    if files.is_empty() {
        out.push_str("No changes.\n");
    }
    out.push_str(&changes::review_diff(store, &before, &after, &files)?);
    out.push_str("```\n\n## Reversal\n\n");
    let _ = writeln!(
        out,
        "`fabric revert N --yes` undoes one step above and keeps later work (newest first to undo \
         them all); `fabric restore {base_seq} --yes` returns the whole project to the state \
         before the change. Both are verified and recorded as new checkpoints."
    );
    Ok((out, exceptions))
}

/// Every change number in the evidence, in order.
pub fn change_ids(records: &[Record]) -> Vec<u64> {
    records
        .iter()
        .filter(|r| r.body.event["kind"] == "change_opened")
        .filter_map(|r| r.body.event["change"].as_u64())
        .collect()
}
