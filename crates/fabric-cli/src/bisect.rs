//! `fabric bisect`: find the checkpoint at which a test command started failing.
//!
//! Each candidate checkpoint is restored in place — so ignored files such as installed
//! dependencies stay available to the tests — and the test command is run against it. The
//! project is always returned to the state it was in before bisecting, which is recorded as a
//! checkpoint first so nothing can be lost.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use fabric_recovery::{RecoveryError, apply_restore, plan_restore};
use fabric_snapshot::{Manifest, capture, diff};
use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::checkpoint::{Checkpoint, CheckpointKind, Meta, read_log, record_manifest, resolve};
use crate::commands::{Ctx, changes_text, diff_lines};
use crate::error::{CliError, exit};
use crate::home::Project;
use crate::timefmt;

const RESULTS_FILE: &str = "test-results.jsonl";
const OUTPUT_FILE: &str = "last-test-output.log";

/// A recorded test outcome: the history of which states passed which test command.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestResult {
    snapshot: ObjectId,
    command: String,
    passed: bool,
    exit_code: Option<i32>,
    tested_at: String,
}

struct Tester<'a> {
    ctx: &'a Ctx,
    store: &'a Store,
    project: &'a Project,
    command: &'a str,
    known: HashMap<ObjectId, bool>,
    runs: usize,
}

impl Tester<'_> {
    /// Whether the test command passes on `checkpoint`'s state.
    fn passes(&mut self, checkpoint: &Checkpoint) -> Result<bool, CliError> {
        if let Some(&passed) = self.known.get(&checkpoint.snapshot) {
            self.progress(checkpoint, passed, true);
            return Ok(passed);
        }
        goto(self.store, &self.project.root, &checkpoint.snapshot)?;
        let (passed, exit_code) = run_test(self.project, self.command)?;
        self.runs += 1;
        self.known.insert(checkpoint.snapshot, passed);
        append_result(
            self.project,
            &TestResult {
                snapshot: checkpoint.snapshot,
                command: self.command.to_owned(),
                passed,
                exit_code,
                tested_at: timefmt::now_rfc3339(),
            },
        )?;
        self.progress(checkpoint, passed, false);
        Ok(passed)
    }

    fn progress(&self, checkpoint: &Checkpoint, passed: bool, cached: bool) {
        if !self.ctx.json {
            eprintln!(
                "  #{:<4} {}{}  {}",
                checkpoint.seq,
                if passed { "pass" } else { "FAIL" },
                if cached { " (cached)" } else { "" },
                checkpoint.label
            );
        }
    }
}

pub fn bisect(
    ctx: &Ctx,
    command: &str,
    good: Option<String>,
    bad: Option<String>,
) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;

    // Record where we are, so the project can always be put back exactly.
    let start = capture(&store, &project.root)?;
    record_manifest(
        &store,
        &project,
        start.id,
        &start.manifest,
        Meta::new(CheckpointKind::Manual, "before bisect"),
        true,
    )?;
    let log = read_log(&project)?;

    let index_of = |reference: &str| -> Result<usize, CliError> {
        let (_, seq) = resolve(&log, reference)?;
        seq.and_then(|seq| log.iter().position(|c| c.seq == seq))
            .ok_or_else(|| {
                CliError::new(
                    exit::ERROR,
                    "unknown_checkpoint",
                    "bisect needs checkpoint numbers from `fabric log`",
                )
            })
    };
    let good_index = good.as_deref().map_or(Ok(0), index_of)?;
    let bad_index = bad.as_deref().map_or(Ok(log.len() - 1), index_of)?;
    if good_index >= bad_index {
        return Err(precondition(
            "--good must be an earlier checkpoint than --bad",
        ));
    }

    // Consecutive checkpoints with identical state behave identically; keep the first of
    // each run, which is the step that produced that state.
    let mut candidates: Vec<&Checkpoint> = Vec::new();
    for checkpoint in &log[good_index..=bad_index] {
        if candidates
            .last()
            .is_none_or(|c| c.snapshot != checkpoint.snapshot)
        {
            candidates.push(checkpoint);
        }
    }

    let mut tester = Tester {
        ctx,
        store: &store,
        project: &project,
        command,
        // Earlier results are kept as history but not reused: tests can depend on things
        // outside the snapshot, such as installed dependencies, that may have changed since.
        known: HashMap::new(),
        runs: 0,
    };
    if !ctx.json {
        eprintln!(
            "Bisecting {} checkpoints with `{command}`:",
            candidates.len()
        );
    }
    let search = search(&mut tester, &candidates);

    // Always go back, whatever happened.
    let returned = goto(&store, &project.root, &start.id);
    let (last_good, first_bad) = search?;
    returned.map_err(|e| {
        let mut e = CliError::from(e);
        e.message = format!(
            "bisect finished but could not return the project to where it was ({}); the earlier state is the checkpoint labelled \"before bisect\"",
            e.message
        );
        e
    })?;

    let culprit = candidates[first_bad];
    let before = Manifest::load(&store, &candidates[last_good].snapshot)?;
    let after = Manifest::load(&store, &culprit.snapshot)?;
    let step_diff = diff(&before, &after);
    let runs = tester.runs;

    ctx.emit(
        "fabric.bisect/v1",
        json!({
            "command": command,
            "first_bad": culprit,
            "last_good": candidates[last_good].seq,
            "diff": step_diff,
            "test_runs": runs,
            "candidates": candidates.len(),
        }),
        || {
            let mut out = format!(
                "\nFirst failing checkpoint: #{}  {}\n",
                culprit.seq, culprit.label
            );
            if let Some(agent) = &culprit.agent {
                let _ = writeln!(out, "  by {agent} at {}", culprit.created_at);
            }
            let _ = writeln!(
                out,
                "  changes since #{} (last passing): {}",
                candidates[last_good].seq,
                changes_text(&(&step_diff).into())
            );
            out.push_str(&diff_lines(&step_diff));
            let _ = writeln!(
                out,
                "Found in {runs} test run(s). The project is back where it was.\nTo undo just this step: fabric revert {} --yes",
                culprit.seq
            );
            out
        },
    );
    Ok(exit::OK)
}

/// Returns the indices of the last passing and first failing candidate.
fn search(tester: &mut Tester<'_>, candidates: &[&Checkpoint]) -> Result<(usize, usize), CliError> {
    let (mut good, mut bad) = (0, candidates.len() - 1);
    if tester.passes(candidates[bad])? {
        return Err(precondition(
            "the tests pass at the newest checkpoint, so there is nothing to find (use --bad to pick a failing one)",
        ));
    }
    if !tester.passes(candidates[good])? {
        return Err(precondition(
            "the tests already fail at the oldest checkpoint (use --good to pick one where they pass)",
        ));
    }
    while bad - good > 1 {
        let mid = good + (bad - good) / 2;
        if tester.passes(candidates[mid])? {
            good = mid;
        } else {
            bad = mid;
        }
    }
    Ok((good, bad))
}

fn precondition(message: &str) -> CliError {
    CliError::new(exit::ERROR, "bisect_precondition", message)
}

/// Puts the project into `target`'s state through the normal, verified restore path.
fn goto(store: &Store, root: &Path, target: &ObjectId) -> Result<(), RecoveryError> {
    let plan = plan_restore(store, root, target)?;
    if !plan.conflicts.is_empty() {
        return Err(RecoveryError::Conflicts(plan.conflicts));
    }
    if !plan.operations.is_empty() {
        apply_restore(store, root, &plan)?;
    }
    Ok(())
}

/// Runs the test command in the project root through the platform shell. Its output goes to
/// a log file, so it never mixes with Fabric's own output.
fn run_test(project: &Project, command: &str) -> Result<(bool, Option<i32>), CliError> {
    let output_path = project.dir.join(OUTPUT_FILE);
    let output = File::create(&output_path).map_err(|e| CliError::io(&output_path, e))?;
    let errors = output
        .try_clone()
        .map_err(|e| CliError::io(&output_path, e))?;
    let mut shell = shell(command);
    let status = shell
        .current_dir(&project.root)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .status()
        .map_err(|e| {
            CliError::new(
                exit::ERROR,
                "test_command",
                format!("could not run `{command}`: {e}"),
            )
        })?;
    Ok((status.success(), status.code()))
}

#[cfg(windows)]
fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut shell = Command::new("cmd");
    // Passed verbatim so quoting inside the command reaches cmd unchanged.
    shell.arg("/C").raw_arg(command);
    shell
}

#[cfg(not(windows))]
fn shell(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}

fn append_result(project: &Project, result: &TestResult) -> Result<(), CliError> {
    let path = project.dir.join(RESULTS_FILE);
    let mut line = serde_json::to_string(result).expect("results always serialize");
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|e| CliError::io(&path, e))
}
