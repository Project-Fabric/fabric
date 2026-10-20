use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;

use fabric_recovery::{
    Operation, RestorePlan, RevertConflict, RevertConflictReason, apply_restore, plan_restore,
    resume_restore, revert_step,
};
use fabric_snapshot::{Diff, Manifest, SkipReason, Snapshot, diff};
use fabric_store::{ObjectId, Store};
use serde::Serialize;
use serde_json::{Value, json};

use crate::changes;
use crate::checkpoint::{
    ChangeCounts, CheckpointKind, Meta, read_log, record, record_manifest, resolve,
};
use crate::error::{CliError, exit};
use crate::evidence;
use crate::home::{Home, Project, display_path};
use crate::query;

/// Most operations listed in human-readable restore output; JSON always has all of them.
const MAX_LISTED: usize = 30;

pub struct Ctx {
    pub home: Home,
    pub json: bool,
}

impl Ctx {
    /// Prints `value` (tagged with its versioned schema) as JSON, or the human rendering.
    pub fn emit(&self, schema: &str, mut value: Value, human: impl FnOnce() -> String) {
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

    pub fn project(&self) -> Result<Project, CliError> {
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
        if !ctx.json {
            // Every file is read and stored durably once; later checkpoints skip unchanged
            // files. On large projects this first one takes noticeably longer.
            eprintln!("Taking the first checkpoint (later ones only read what changed)...");
        }
        let snap = project.capture(&store)?;
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
        || {
            let mut out = match (&checkpoint, &snapshot) {
                (Some(c), Some(s)) => format!(
                    "Recording {root}\nCheckpoint #{}: {} files, {} directories\n",
                    c.seq, s.stats.files, s.stats.dirs
                ),
                _ => format!("Already recording {root}\n"),
            };
            out.push_str(&next_steps());
            out
        },
    );
    Ok(exit::OK)
}

/// Suggests connecting the agents that appear to be installed.
fn next_steps() -> String {
    let on_path = |names: &[&str]| {
        env::var_os("PATH").is_some_and(|path| {
            env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
        })
    };
    let mut steps = Vec::new();
    if on_path(&["claude", "claude.exe", "claude.cmd"]) {
        steps.push(
            "fabric integrate claude-code   # checkpoint every Claude Code prompt and change",
        );
    }
    if on_path(&["codex", "codex.exe", "codex.cmd"])
        || dirs::home_dir().is_some_and(|home| home.join(".codex").is_dir())
    {
        steps.push("fabric integrate codex         # let Codex see what other agents did");
    }
    if steps.is_empty() {
        return "Next: run `fabric integrate claude-code` or `fabric integrate codex` to connect an agent.\n"
            .to_owned();
    }
    let mut out = String::from("\nConnect your agents:\n");
    for step in steps {
        let _ = writeln!(out, "  {step}");
    }
    out
}

pub fn checkpoint(
    ctx: &Ctx,
    message: Option<String>,
    agent: Option<String>,
    if_changed: bool,
) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let snapshot = project.capture(&store)?;
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
            let snapshot = project.capture(&store)?;
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

/// Describes a command that changes the project through a restore plan (`restore`, `revert`),
/// for its messages and its log entries.
struct Action {
    schema: &'static str,
    /// Heads the plan, e.g. "Restore to #3".
    title: String,
    /// The command that applies the plan, shown when confirmation is needed.
    confirm: String,
    /// Shown on success, e.g. "Restored #3".
    done: String,
    /// The checkpoint recorded for the result.
    kind: CheckpointKind,
    log_label: String,
    /// Extra fields for JSON output.
    extra: Value,
}

pub fn restore(ctx: &Ctx, reference: &str, yes: bool) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let log = read_log(&project)?;
    let (target, seq) = resolve(&log, reference)?;
    let label = checkpoint_label(&target, seq);
    let plan = plan_restore(&store, &project.root, &target)?;
    let action = Action {
        schema: "fabric.restore/v1",
        title: format!("Restore to {label}"),
        confirm: format!("fabric restore {reference} --yes"),
        done: format!("Restored {label}"),
        kind: CheckpointKind::Restore,
        log_label: format!("restored {label}"),
        extra: json!({ "target": label }),
    };
    run_plan(ctx, &store, &project, plan, yes, action)
}

/// Undoes one checkpoint's changes and keeps everything recorded after it.
pub fn revert(ctx: &Ctx, reference: &str, yes: bool) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let log = read_log(&project)?;
    let (_, seq) = resolve(&log, reference)?;
    let index = seq
        .and_then(|seq| log.iter().position(|c| c.seq == seq))
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "unknown_checkpoint",
                "revert needs a checkpoint number from `fabric log`",
            )
        })?;
    if index == 0 {
        return Err(CliError::new(
            exit::ERROR,
            "nothing_before",
            "the first checkpoint has nothing before it to revert to; use `fabric restore`",
        ));
    }
    let step = &log[index];
    let before = Manifest::load(&store, &log[index - 1].snapshot)?;
    let after = Manifest::load(&store, &step.snapshot)?;
    let current = project.capture(&store)?;
    ctx.warn_skipped(&current);
    let outcome = revert_step(&before, &after, &current.manifest)?;
    let label = format!("#{} ({})", step.seq, step.label);

    if !outcome.conflicts.is_empty() {
        return Ok(emit_revert_conflicts(
            ctx,
            &label,
            json!({ "checkpoint": step.seq }),
            &outcome.conflicts,
        ));
    }

    let target = outcome.manifest.store(&store)?;
    let plan = plan_restore(&store, &project.root, &target)?;
    let action = Action {
        schema: "fabric.revert/v1",
        title: format!("Revert {label}, keeping later changes"),
        confirm: format!("fabric revert {} --yes", step.seq),
        done: format!("Reverted {label}"),
        kind: CheckpointKind::Revert,
        log_label: format!("reverted #{}: {}", step.seq, step.label),
        extra: json!({
            "checkpoint": step.seq,
            "reverted": outcome.reverted,
            "already_reverted": outcome.already_reverted,
        }),
    };
    run_plan(ctx, &store, &project, plan, yes, action)
}

/// Undoes a whole authorized change: every file its agent session touched between the
/// change's start and its review (or now, while it is open) goes back to how it was when the
/// change started. Later work on other files is kept; files changed again after the change
/// are conflicts, as for `revert`.
pub fn revert_change(ctx: &Ctx, id: u64, yes: bool) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let log = read_log(&project)?;
    let (_, records) = evidence::verify_chain(&evidence::read_chain(&project)?);
    let span = changes::span(&records, log.last().map_or(0, |c| c.seq), id)?;
    let manifest_at = |seq: u64| -> Result<Manifest, CliError> {
        let checkpoint = log.iter().find(|c| c.seq == seq).ok_or_else(|| {
            CliError::new(
                exit::INTEGRITY,
                "missing_checkpoint",
                format!("change #{id} refers to checkpoint #{seq}, which is not in the log"),
            )
        })?;
        Ok(Manifest::load(&store, &checkpoint.snapshot)?)
    };
    let before = manifest_at(span.base)?;
    let end = manifest_at(span.end)?;

    // Every path the session's steps changed, including Fabric's rollbacks within them.
    let mut touched = std::collections::BTreeSet::new();
    for pair in log.windows(2) {
        let (previous, step) = (&pair[0], &pair[1]);
        if step.seq <= span.base || step.seq > span.end || step.session != span.session {
            continue;
        }
        let d = diff(
            &Manifest::load(&store, &previous.snapshot)?,
            &Manifest::load(&store, &step.snapshot)?,
        );
        touched.extend(d.added.into_iter().chain(d.modified).chain(d.removed));
        touched.extend(d.renamed.into_iter().flat_map(|r| [r.from, r.to]));
    }
    // The change's own result: the starting state with the touched paths as they ended.
    let mut entries: std::collections::BTreeMap<_, _> = before
        .entries()
        .iter()
        .map(|e| (e.path().clone(), e.clone()))
        .collect();
    for path in &touched {
        match end.get(path) {
            Some(entry) => entries.insert(path.clone(), entry.clone()),
            None => entries.remove(path),
        };
    }
    let after = Manifest::new(entries.into_values().collect())?;

    let current = project.capture(&store)?;
    ctx.warn_skipped(&current);
    let outcome = revert_step(&before, &after, &current.manifest)?;
    let label = format!("change #{id} ({})", span.title);
    if !outcome.conflicts.is_empty() {
        return Ok(emit_revert_conflicts(
            ctx,
            &label,
            json!({ "checkpoint": null, "change": id }),
            &outcome.conflicts,
        ));
    }
    let target = outcome.manifest.store(&store)?;
    let plan = plan_restore(&store, &project.root, &target)?;
    let action = Action {
        schema: "fabric.revert/v1",
        title: format!("Revert {label}, keeping other work"),
        confirm: format!("fabric revert --change {id} --yes"),
        done: format!("Reverted {label}"),
        kind: CheckpointKind::Revert,
        log_label: format!("reverted change #{id}: {}", span.title),
        extra: json!({
            "checkpoint": null,
            "change": id,
            "reverted": outcome.reverted,
            "already_reverted": outcome.already_reverted,
        }),
    };
    run_plan(ctx, &store, &project, plan, yes, action)
}

fn emit_revert_conflicts(
    ctx: &Ctx,
    label: &str,
    mut value: Value,
    conflicts: &[RevertConflict],
) -> u8 {
    value["status"] = "conflicts".into();
    value["conflicts"] = json!(conflicts);
    ctx.emit("fabric.revert/v1", value, || {
        let mut out = format!("Cannot revert {label}: later steps changed the same paths.\n");
        for conflict in conflicts {
            let reason = match conflict.reason {
                RevertConflictReason::ChangedLater => "changed again later",
                RevertConflictReason::DirectoryInUse => "later work is inside it",
                RevertConflictReason::ParentIsNotADirectory => {
                    "a file is where its directory belongs"
                }
            };
            let _ = writeln!(out, "  {}  ({reason})", conflict.path);
        }
        out.push_str(
            "Nothing was changed. To go back to before this step entirely, use `fabric restore`.\n",
        );
        out
    });
    exit::CONFLICTS
}

/// Shows a plan, and applies it if confirmed: records the undo point, applies, verifies, and
/// records the result.
fn run_plan(
    ctx: &Ctx,
    store: &Store,
    project: &Project,
    plan: RestorePlan,
    yes: bool,
    action: Action,
) -> Result<u8, CliError> {
    let output = |fields: Value| {
        let mut value = action.extra.clone();
        if let (Some(value), Value::Object(fields)) = (value.as_object_mut(), fields) {
            value.extend(fields);
        }
        value
    };

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
            action.schema,
            output(json!({ "status": status, "plan": plan })),
            || {
                let mut out = plan_text(&action.title, &plan);
                match status {
                    RestoreStatus::Conflicts => out.push_str(
                        "Nothing was changed. Move or delete the paths above (they were never recorded), then try again.\n",
                    ),
                    RestoreStatus::AlreadyAtTarget => {
                        out = "The project already matches; nothing to do\n".to_owned();
                    }
                    _ => {
                        let _ = writeln!(
                            out,
                            "Nothing was changed. Run `{}` to apply.\nThe current state is saved first, so this can be undone.",
                            action.confirm
                        );
                    }
                }
                out
            },
        );
        return Ok(code);
    }

    // Record the undo point before touching anything, so it is in the log even if the
    // operation is interrupted.
    let base = Manifest::load(store, &plan.base)?;
    let mut undo = record_manifest(
        store,
        project,
        plan.base,
        &base,
        Meta::new(
            CheckpointKind::PreRestore,
            format!("before: {}", action.log_label),
        ),
        true,
    )?
    .map(|c| c.seq);
    if undo.is_none() {
        undo = read_log(project)?.last().map(|c| c.seq);
    }

    let report = apply_restore(store, &project.root, &plan)?;
    let target_manifest = Manifest::load(store, &report.target)?;
    let recorded = record_manifest(
        store,
        project,
        report.target,
        &target_manifest,
        Meta::new(action.kind, action.log_label.clone()),
        false,
    )?;

    ctx.emit(
        action.schema,
        output(json!({
            "status": status,
            "plan": plan,
            "report": report,
            "undo_checkpoint": undo,
            "recorded": recorded,
        })),
        || {
            let mut out = plan_text(&action.title, &plan);
            let _ = writeln!(out, "{} and verified the result.", action.done);
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

pub fn show(ctx: &Ctx, seq: u64) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let text = query::step(&store, &project, seq)?;
    ctx.emit(
        "fabric.show/v1",
        json!({ "checkpoint": seq, "text": text }),
        || text.clone(),
    );
    Ok(exit::OK)
}

pub fn history(ctx: &Ctx, path: &str, limit: usize) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    // Paths typed by people are relative to where they are, not to the project root.
    let cwd = env::current_dir().map_err(|e| CliError::io(&PathBuf::from("."), e))?;
    let absolute = cwd.join(path);
    let text = query::file_history(&store, &project, &absolute.to_string_lossy(), limit)?;
    ctx.emit(
        "fabric.history/v1",
        json!({ "path": path, "text": text }),
        || text.clone(),
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

pub fn changes_text(c: &ChangeCounts) -> String {
    let mut text = format!("+{} ~{} -{}", c.added, c.modified, c.removed);
    if c.renamed > 0 {
        let _ = write!(text, " >{}", c.renamed);
    }
    text
}

pub fn diff_lines(d: &Diff) -> String {
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

fn plan_text(title: &str, plan: &RestorePlan) -> String {
    let s = plan.summary;
    let mut out = format!(
        "{title}: create {}, modify {}, delete {}\n",
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
