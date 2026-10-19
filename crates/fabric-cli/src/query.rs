//! Answers to "what happened?", written for agents and people to read: the project timeline,
//! the history of one file, and what one step changed. Used by the MCP server and by
//! `fabric show`.

use std::collections::HashMap;
use std::fmt::Write as _;

use fabric_snapshot::{Entry, Manifest, RelPath, diff};
use fabric_store::{ObjectId, Store};
use similar::TextDiff;

use crate::checkpoint::{Checkpoint, read_log};
use crate::commands::{changes_text, diff_lines, kind_name};
use crate::error::{CliError, exit};
use crate::home::{Project, display_path, project_relative};
use crate::results::{self, TestResult};

/// Most files whose content is shown for one step, and most diff lines per file.
const MAX_FILES_SHOWN: usize = 10;
const MAX_LINES_PER_FILE: usize = 200;
/// Larger files are summarized rather than diffed.
const MAX_DIFF_BYTES: u64 = 512 * 1024;

/// Loads each manifest at most once.
struct Manifests<'a> {
    store: &'a Store,
    cache: HashMap<ObjectId, Manifest>,
}

impl<'a> Manifests<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            cache: HashMap::new(),
        }
    }

    fn get(&mut self, id: &ObjectId) -> Result<&Manifest, CliError> {
        if !self.cache.contains_key(id) {
            let manifest = Manifest::load(self.store, id)?;
            self.cache.insert(*id, manifest);
        }
        Ok(&self.cache[id])
    }
}

fn header(c: &Checkpoint) -> String {
    format!(
        "#{} {} {} {} {}",
        c.seq,
        c.created_at,
        c.agent.as_deref().unwrap_or("-"),
        kind_name(c.kind),
        c.label
    )
}

fn test_note(results: &[TestResult], snapshot: &ObjectId) -> String {
    results::latest_for(results, snapshot)
        .map(|r| {
            format!(
                "  tests: {} `{}`",
                if r.passed { "pass" } else { "FAIL" },
                r.command
            )
        })
        .unwrap_or_default()
}

/// The most recent checkpoints, newest first, optionally only one agent's.
pub fn timeline(project: &Project, limit: usize, agent: Option<&str>) -> Result<String, CliError> {
    let log = read_log(project)?;
    let results = results::read(project)?;
    let mut out = format!(
        "Project {} — {} checkpoints (newest first; format: #seq time agent kind label (+added ~modified -removed))\n",
        display_path(&project.root),
        log.len()
    );
    let shown = log
        .iter()
        .rev()
        .filter(|c| agent.is_none_or(|a| c.agent.as_deref() == Some(a)))
        .take(limit);
    for c in shown {
        let _ = writeln!(
            out,
            "{} ({}){}",
            header(c),
            changes_text(&c.changes),
            test_note(&results, &c.snapshot)
        );
    }
    Ok(out)
}

/// Every checkpoint that changed `path`, newest first: who changed it, how, and whether the
/// tests passed afterwards.
pub fn file_history(
    store: &Store,
    project: &Project,
    path: &str,
    limit: usize,
) -> Result<String, CliError> {
    let relative = project_relative(path, &project.root);
    let rel = RelPath::new(relative.clone()).map_err(|e| {
        CliError::new(
            exit::ERROR,
            "invalid_path",
            format!("{path:?} is not a path inside the project: {e}"),
        )
    })?;
    let log = read_log(project)?;
    let results = results::read(project)?;
    let mut manifests = Manifests::new(store);

    let mut changes = Vec::new();
    let mut previous: Option<Entry> = None;
    for c in &log {
        let entry = manifests.get(&c.snapshot)?.get(&rel).cloned();
        if entry != previous {
            let what = match (&previous, &entry) {
                (None, Some(_)) => "created",
                (Some(_), None) => "deleted",
                _ => "modified",
            };
            changes.push((c, what));
            previous = entry;
        }
    }

    let mut out = format!(
        "History of {relative} ({} changes, newest first)\n",
        changes.len()
    );
    if changes.is_empty() {
        out.push_str("Never recorded in this project.\n");
    }
    for (c, what) in changes.iter().rev().take(limit) {
        let _ = writeln!(
            out,
            "{} — {what}{}",
            header(c),
            test_note(&results, &c.snapshot)
        );
    }
    if previous.is_none() && !changes.is_empty() {
        out.push_str("The file does not exist in the latest checkpoint.\n");
    }
    Ok(out)
}

/// What checkpoint `seq` changed relative to the one before it, with text diffs.
pub fn step(store: &Store, project: &Project, seq: u64) -> Result<String, CliError> {
    let log = read_log(project)?;
    let index = log.iter().position(|c| c.seq == seq).ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_checkpoint",
            format!("no checkpoint #{seq}; see the timeline"),
        )
    })?;
    let checkpoint = &log[index];
    let results = results::read(project)?;
    let mut out = format!("{}\n", header(checkpoint));
    if let Some(session) = &checkpoint.session {
        let _ = writeln!(out, "session: {session}");
    }
    let note = test_note(&results, &checkpoint.snapshot);
    if !note.is_empty() {
        let _ = writeln!(out, "{}", note.trim_start());
    }

    let Some(previous) = index.checked_sub(1).map(|i| &log[i]) else {
        out.push_str("This is the first checkpoint; it recorded the project as it was.\n");
        return Ok(out);
    };
    let before = Manifest::load(store, &previous.snapshot)?;
    let after = Manifest::load(store, &checkpoint.snapshot)?;
    let d = diff(&before, &after);
    if d.is_empty() {
        out.push_str("No changes since the previous checkpoint.\n");
        return Ok(out);
    }
    let _ = writeln!(
        out,
        "Changes since #{}: {}",
        previous.seq,
        changes_text(&(&d).into())
    );
    out.push_str(&diff_lines(&d));

    let changed_files = d
        .modified
        .iter()
        .chain(&d.added)
        .chain(&d.removed)
        .filter(|path| {
            matches!(before.get(path), Some(Entry::File { .. }))
                || matches!(after.get(path), Some(Entry::File { .. }))
        });
    for (shown, path) in changed_files.enumerate() {
        if shown == MAX_FILES_SHOWN {
            out.push_str("(further files omitted)\n");
            break;
        }
        out.push_str(&file_diff(
            store,
            path,
            before.get(path),
            after.get(path),
            previous.seq,
            seq,
        )?);
    }
    Ok(out)
}

fn file_diff(
    store: &Store,
    path: &RelPath,
    before: Option<&Entry>,
    after: Option<&Entry>,
    from_seq: u64,
    to_seq: u64,
) -> Result<String, CliError> {
    let text = |entry: Option<&Entry>| -> Result<Option<String>, CliError> {
        match entry {
            Some(Entry::File { id, size, .. }) if *size <= MAX_DIFF_BYTES => {
                let bytes = store.read(id)?;
                Ok(String::from_utf8(bytes).ok().filter(|s| !s.contains('\0')))
            }
            Some(Entry::File { .. }) => Ok(None),
            _ => Ok(Some(String::new())),
        }
    };
    let (Some(old), Some(new)) = (text(before)?, text(after)?) else {
        return Ok(format!(
            "\n{path}: binary or large file, content not shown\n"
        ));
    };
    let diff = TextDiff::from_lines(&old, &new);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("{path} (#{from_seq})"),
            &format!("{path} (#{to_seq})"),
        )
        .to_string();
    let lines: Vec<&str> = unified.lines().collect();
    let mut out = String::from("\n");
    for line in lines.iter().take(MAX_LINES_PER_FILE) {
        out.push_str(line);
        out.push('\n');
    }
    if lines.len() > MAX_LINES_PER_FILE {
        let _ = writeln!(out, "... {} more lines", lines.len() - MAX_LINES_PER_FILE);
    }
    Ok(out)
}

/// Recorded test runs, newest first.
pub fn test_results(project: &Project, limit: usize) -> Result<String, CliError> {
    let log = read_log(project)?;
    let results = results::read(project)?;
    let seq_of = |snapshot: &ObjectId| {
        log.iter()
            .find(|c| &c.snapshot == snapshot)
            .map_or_else(|| "?".to_owned(), |c| format!("#{}", c.seq))
    };
    let mut out = format!("{} recorded test runs (newest first)\n", results.len());
    for r in results.iter().rev().take(limit) {
        let _ = writeln!(
            out,
            "{} {} {} `{}`",
            r.tested_at,
            seq_of(&r.snapshot),
            if r.passed { "pass" } else { "FAIL" },
            r.command
        );
    }
    Ok(out)
}
