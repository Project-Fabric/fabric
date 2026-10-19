use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;

use fabric_recovery::{Operation, RestorePlan, apply_restore, plan_restore, resume_restore};
use fabric_snapshot::{Diff, Manifest, SkipReason, Snapshot, capture, diff};
use fabric_store::ObjectId;
use serde::Serialize;
use serde_json::{Value, json};

use crate::checkpoint::{
    ChangeCounts, CheckpointKind, Meta, read_log, record, record_manifest, resolve,
};
use crate::error::{CliError, exit};
use crate::home::{Home, Project, display_path};

/// Most operations listed in human-readable restore output; JSON always has all of them.
const MAX_LISTED: usize = 30;

pub struct Ctx {
    pub home: Home,
    pub json: bool,
}

impl Ctx {
    /// Prints `value` (tagged with its versioned schema) as JSON, or the human rendering.
    fn emit(&self, schema: &str, mut value: Value, human: impl FnOnce() -> String) {
        if self.json {
            value["schema"] = schema.into();
            println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("output always serializes")
            );
        } else {
            print!("{}", human());
        }
    }

    fn project(&self) -> Result<Project, CliError> {
        let cwd = env::current_dir().map_err(|e| CliError::io(&PathBuf::from("."), e))?;
        self.home.find_project(&cwd)
    }

    fn warn_skipped(&self, snapshot: &Snapshot) {
        if self.json || snapshot.skipped.is_empty() {
            return;
        }
        eprintln!(
            "warning: {} path(s) could not be recorded and are not in this checkpoint:",
            snapshot.skipped.len()
        );
        for skipped in snapshot.skipped.iter().take(10) {
            eprintln!("  {}  ({})", skipped.path, describe_skip(&skipped.reason));
        }
    }
}

pub fn init(ctx: &Ctx, path: Option<PathBuf>) -> Result<u8, CliError> {
    let root = match path {
        Some(path) => path,
        None => env::current_dir().map_err(|e| CliError::io(&PathBuf::from("."), e))?,
    };
    let store = ctx.home.open_store()?;
    let (project, newly_registered) = ctx.home.register(&root)?;
    let mut snapshot = None;
    let checkpoint = if read_log(&project)?.is_empty() {
        let snap = capture(&store, &project.root)?;
        ctx.warn_skipped(&snap);
        let checkpoint = record(
            &store,
            &project,
            &snap,
            Meta::new(CheckpointKind::Init, "start recording"),
            false,
        )?;
        snapshot = Some(snap);
        checkpoint
    } else {
        None
    };

    let root = display_path(&project.root);
    ctx.emit(
        "fabric.init/v1",
        json!({
            "project": { "id": project.id, "root": root },
            "store": display_path(&ctx.home.store_path()),
            "newly_registered": newly_registered,
            "checkpoint": checkpoint,
            "skipped": snapshot.as_ref().map(|s| &s.skipped),
        }),
        || match (&checkpoint, &snapshot) {
            (Some(c), Some(s)) => format!(
                "Recording {root}\nCheckpoint #{}: {} files, {} directories\n",
                c.seq, s.stats.files, s.stats.dirs
            ),
            _ => format!("Already recording {root}\n"),
        },
    );
    Ok(exit::OK)
}

pub fn checkpoint(
    ctx: &Ctx,
    message: Option<String>,
    agent: Option<String>,
    if_changed: bool,
) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let snapshot = capture(&store, &project.root)?;
    ctx.warn_skipped(&snapshot);
    let mut meta = Meta::new(
        CheckpointKind::Manual,
        message.unwrap_or_else(|| "checkpoint".to_owned()),
    );
    meta.agent = agent;
    let checkpoint = record(&store, &project, &snapshot, meta, if_changed)?;

    ctx.emit(
        "fabric.checkpoint/v1",
        json!({
            "recorded": checkpoint.is_some(),
            "checkpoint": checkpoint,
            "skipped": snapshot.skipped,
        }),
        || match &checkpoint {
            Some(c) => format!(
                "Checkpoint #{}  {}  {}\n",
                c.seq,
                changes_text(&c.changes),
                c.label
            ),
            None => "No changes since the latest checkpoint; nothing recorded\n".to_owned(),
        },
    );
    Ok(exit::OK)
}

pub fn log(ctx: &Ctx, limit: usize) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let log = read_log(&project)?;
    let shown: Vec<_> = log.iter().rev().take(limit).collect();

    ctx.emit(
        "fabric.log/v1",
        json!({
            "project_root": display_path(&project.root),
            "total": log.len(),
            "checkpoints": shown,
        }),
        || {
            let mut out = String::new();
            for c in &shown {
                let _ = writeln!(
                    out,
                    "#{:<4} {}  {:<11} {:<12} {:<16} {}",
                    c.seq,
                    c.created_at,
                    kind_name(c.kind),
                    c.agent.as_deref().unwrap_or("-"),
                    changes_text(&c.changes),
                    c.label
                );
            }
            if log.len() > shown.len() {
                let _ = writeln!(out, "... {} older (use -n)", log.len() - shown.len());
            }
            if log.is_empty() {
                out.push_str("No checkpoints yet\n");
            }
            out
        },
    );
    Ok(exit::OK)
}

pub fn diff_cmd(ctx: &Ctx, from: Option<String>, to: Option<String>) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let log = read_log(&project)?;

    let (from_id, from_seq) = resolve(&log, from.as_deref().unwrap_or("latest"))?;
    let from_manifest = Manifest::load(&store, &from_id)?;
    let (to_manifest, to_label) = match to.as_deref() {
        None | Some("current") => {
            let snapshot = capture(&store, &project.root)?;
            ctx.warn_skipped(&snapshot);
            (snapshot.manifest, "current".to_owned())
        }
        Some(reference) => {
            let (id, seq) = resolve(&log, reference)?;
            (Manifest::load(&store, &id)?, checkpoint_label(&id, seq))
        }
    };
    let d = diff(&from_manifest, &to_manifest);
    let from_label = checkpoint_label(&from_id, from_seq);

    ctx.emit(
        "fabric.diff/v1",
        json!({ "from": from_label, "to": to_label, "diff": d }),
        || {
            let mut out = format!(
                "{from_label} -> {to_label}: {}\n",
                changes_text(&(&d).into())
            );
            out.push_str(&diff_lines(&d));
            out
        },
    );
    Ok(exit::OK)
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RestoreStatus {
    Conflicts,
    AlreadyAtTarget,
    RequiresConfirmation,
    Applied,
}

pub fn restore(ctx: &Ctx, reference: &str, yes: bool) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let log = read_log(&project)?;
    let (target, seq) = resolve(&log, reference)?;
    let label = checkpoint_label(&target, seq);
    let plan = plan_restore(&store, &project.root, &target)?;

    let (status, code) = if !plan.conflicts.is_empty() {
        (RestoreStatus::Conflicts, exit::CONFLICTS)
    } else if plan.operations.is_empty() {
        (RestoreStatus::AlreadyAtTarget, exit::OK)
    } else if !yes {
        (
            RestoreStatus::RequiresConfirmation,
            exit::NEEDS_CONFIRMATION,
        )
    } else {
        (RestoreStatus::Applied, exit::OK)
    };

    if !matches!(status, RestoreStatus::Applied) {
        ctx.emit(
            "fabric.restore/v1",
            json!({ "status": status, "target": label, "plan": plan }),
            || {
                let mut out = plan_text(&label, &plan);
                match status {
                    RestoreStatus::Conflicts => out.push_str(
                        "Nothing was changed. Move or delete the paths above (they were never recorded), then try again.\n",
                    ),
                    RestoreStatus::AlreadyAtTarget => {
                        out = format!("The project already matches {label}\n");
                    }
                    _ => {
                        let _ = writeln!(
                            out,
                            "Nothing was changed. Run `fabric restore {reference} --yes` to apply.\nThe current state is saved first, so the restore can be undone."
                        );
                    }
                }
                out
            },
        );
        return Ok(code);
    }

    // Record the undo point before touching anything, so it is in the log even if the
    // restore is interrupted.
    let base = Manifest::load(&store, &plan.base)?;
    let mut undo = record_manifest(
        &store,
        &project,
        plan.base,
        &base,
        Meta::new(
            CheckpointKind::PreRestore,
            format!("before restoring {label}"),
        ),
        true,
    )?
    .map(|c| c.seq);
    if undo.is_none() {
        undo = read_log(&project)?.last().map(|c| c.seq);
    }

    let report = apply_restore(&store, &project.root, &plan)?;
    let target_manifest = Manifest::load(&store, &report.target)?;
    let restored = record_manifest(
        &store,
        &project,
        report.target,
        &target_manifest,
        Meta::new(CheckpointKind::Restore, format!("restored {label}")),
        false,
    )?;

    ctx.emit(
        "fabric.restore/v1",
        json!({
            "status": status,
            "target": label,
            "plan": plan,
            "report": report,
            "undo_checkpoint": undo,
            "checkpoint": restored,
        }),
        || {
            let mut out = plan_text(&label, &plan);
            let _ = writeln!(out, "Restored {label} and verified the result.");
            if !report.untracked_kept.is_empty() {
                let _ = writeln!(out, "Left untouched (never recorded):");
                for path in &report.untracked_kept {
                    let _ = writeln!(out, "  {path}");
                }
            }
            if let Some(undo) = undo {
                let _ = writeln!(out, "To undo: fabric restore {undo} --yes");
            }
            out
        },
    );
    Ok(exit::OK)
}

pub fn resume(ctx: &Ctx) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let Some(report) = resume_restore(&store, &project.root)? else {
        ctx.emit("fabric.resume/v1", json!({ "resumed": false }), || {
            "No interrupted restore to resume\n".to_owned()
        });
        return Ok(exit::OK);
    };
    let manifest = Manifest::load(&store, &report.target)?;
    let checkpoint = record_manifest(
        &store,
        &project,
        report.target,
        &manifest,
        Meta::new(CheckpointKind::Restore, "finished interrupted restore"),
        false,
    )?;
    ctx.emit(
        "fabric.resume/v1",
        json!({ "resumed": true, "report": report, "checkpoint": checkpoint }),
        || "Finished the interrupted restore and verified the result\n".to_owned(),
    );
    Ok(exit::OK)
}

fn checkpoint_label(id: &ObjectId, seq: Option<u64>) -> String {
    match seq {
        Some(seq) => format!("#{seq}"),
        None => id.to_hex()[..12].to_owned(),
    }
}

pub fn kind_name(kind: CheckpointKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn changes_text(c: &ChangeCounts) -> String {
    let mut text = format!("+{} ~{} -{}", c.added, c.modified, c.removed);
    if c.renamed > 0 {
        let _ = write!(text, " >{}", c.renamed);
    }
    text
}

fn diff_lines(d: &Diff) -> String {
    let mut out = String::new();
    for path in &d.added {
        let _ = writeln!(out, "  added     {path}");
    }
    for path in &d.modified {
        let _ = writeln!(out, "  modified  {path}");
    }
    for path in &d.removed {
        let _ = writeln!(out, "  removed   {path}");
    }
    for rename in &d.renamed {
        let _ = writeln!(out, "  renamed   {} -> {}", rename.from, rename.to);
    }
    out
}

fn plan_text(label: &str, plan: &RestorePlan) -> String {
    let s = plan.summary;
    let mut out = format!(
        "Restore to {label}: create {}, modify {}, delete {}\n",
        s.create, s.modify, s.delete
    );
    for op in plan.operations.iter().take(MAX_LISTED) {
        let verb = match op {
            Operation::Remove { .. } => "delete",
            Operation::CreateDir { .. } => "mkdir",
            Operation::WriteFile { .. } => "write",
            Operation::CreateSymlink { .. } => "symlink",
        };
        let _ = writeln!(out, "  {verb:<8} {}", op.path());
    }
    if plan.operations.len() > MAX_LISTED {
        let _ = writeln!(out, "  ... and {} more", plan.operations.len() - MAX_LISTED);
    }
    if !plan.conflicts.is_empty() {
        let _ = writeln!(out, "Blocked by {} conflict(s):", plan.conflicts.len());
        for conflict in &plan.conflicts {
            let kind = serde_json::to_value(&conflict.kind)
                .ok()
                .and_then(|v| v["conflict"].as_str().map(str::to_owned))
                .unwrap_or_default();
            let _ = writeln!(out, "  {}  ({})", conflict.path, kind.replace('_', " "));
        }
    }
    out
}

fn describe_skip(reason: &SkipReason) -> String {
    match reason {
        SkipReason::NonUtf8Name => "name is not valid UTF-8".to_owned(),
        SkipReason::NonPortableName { detail } | SkipReason::Unreadable { detail } => {
            detail.clone()
        }
        SkipReason::UnsupportedFileType => "not a regular file, directory or symlink".to_owned(),
        SkipReason::ChangedDuringCapture => "kept changing while being read".to_owned(),
    }
}
