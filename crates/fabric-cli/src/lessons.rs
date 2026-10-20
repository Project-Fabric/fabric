//! Lessons: what Fabric learned from steps that broke things.
//!
//! A lesson is built from evidence Fabric already has, not invented: the step that caused a
//! failure (found by `bisect` or reverted by a person), the files it changed, the lines it added
//! and removed, the tests that failed, and the developer's correction in their own words.
//!
//! Lessons are used two ways, both **experimental and off by default**:
//! - `FABRIC_EXPERIMENT_LESSONS=1`: each prompt to a hooked agent is given the active lessons as
//!   context, and the `fabric_lessons` MCP tool returns them on request.
//! - `FABRIC_EXPERIMENT_LEARNED_GUARD=1`: before an edit that resembles a lesson's failing change
//!   (re-adding a line like one it added, or removing a line it removed, in the same file), the
//!   agent is stopped once with the lesson as the reason. Retrying the edit lets it through.
//!
//! See `docs/EXPERIMENT_LESSONS.md` for the experiment these support.

use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use fabric_snapshot::{Entry, Manifest, RelPath, diff};
use fabric_store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};

use crate::checkpoint::read_log;
use crate::error::{CliError, exit};
use crate::home::{Home, Project, project_relative, write_durably};
use crate::timefmt;

const LESSONS_FILE: &str = "lessons.json";
const WARNINGS_FILE: &str = "learned-guard.jsonl";
const LESSONS_FORMAT: &str = "fabric-lessons";
const LESSONS_VERSION: u32 = 1;
const TEST_OUTPUT_FILE: &str = "last-test-output.log";
/// Most evidence lines kept per lesson; the guard only needs a few distinctive ones.
const MAX_LINES: usize = 20;
/// Lines shorter than this (after trimming) are too generic to recognize a change by.
const MIN_LINE_CHARS: usize = 8;
/// How alike two lines must be (token overlap) to count as "the same change again".
const SIMILARITY: f64 = 0.7;
/// Most lessons given to an agent with each prompt.
const MAX_INJECTED: usize = 10;

pub fn lessons_enabled() -> bool {
    env::var("FABRIC_EXPERIMENT_LESSONS").is_ok_and(|v| v == "1")
}

pub fn learned_guard_enabled() -> bool {
    env::var("FABRIC_EXPERIMENT_LEARNED_GUARD").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: u64,
    pub created_at: String,
    /// The correction, in the developer's words.
    pub rule: String,
    /// The checkpoint whose change caused the failure.
    pub from_checkpoint: u64,
    pub step_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Files the failing step changed.
    pub files: Vec<String>,
    /// Lines the failing step added (evidence for recognizing the change again).
    pub added_lines: Vec<String>,
    /// Lines the failing step removed.
    pub removed_lines: Vec<String>,
    /// Tests that failed, when known.
    pub failing_tests: Vec<String>,
    pub active: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LessonsFile {
    format: String,
    version: u32,
    lessons: Vec<Lesson>,
}

pub fn load(project: &Project) -> Result<Vec<Lesson>, CliError> {
    let path = project.dir.join(LESSONS_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    let file: LessonsFile = serde_json::from_slice(&bytes).map_err(|e| {
        CliError::new(
            exit::INTEGRITY,
            "invalid_lessons",
            format!("lessons file: {e}"),
        )
    })?;
    if file.format != LESSONS_FORMAT || file.version != LESSONS_VERSION {
        return Err(CliError::new(
            exit::INTEGRITY,
            "invalid_lessons",
            "unsupported lessons file",
        ));
    }
    Ok(file.lessons)
}

fn save(project: &Project, lessons: Vec<Lesson>) -> Result<(), CliError> {
    let file = LessonsFile {
        format: LESSONS_FORMAT.to_owned(),
        version: LESSONS_VERSION,
        lessons,
    };
    let bytes = serde_json::to_vec_pretty(&file).expect("lessons always serialize");
    write_durably(&project.dir.join(LESSONS_FILE), &bytes)
}

/// Records a lesson from checkpoint `seq`, using its change as evidence.
pub fn add(store: &Store, project: &Project, seq: u64, rule: &str) -> Result<Lesson, CliError> {
    let log = read_log(project)?;
    let index = log.iter().position(|c| c.seq == seq).ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_checkpoint",
            format!("no checkpoint #{seq}; see `fabric log`"),
        )
    })?;
    if index == 0 {
        return Err(CliError::new(
            exit::ERROR,
            "nothing_before",
            "the first checkpoint has no change to learn from",
        ));
    }
    let step = &log[index];
    let before = Manifest::load(store, &log[index - 1].snapshot)?;
    let after = Manifest::load(store, &step.snapshot)?;
    let changes = diff(&before, &after);

    let mut files = Vec::new();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let changed = changes
        .modified
        .iter()
        .chain(&changes.added)
        .chain(&changes.removed);
    for path in changed {
        let old = file_text(store, before.get(path))?;
        let new = file_text(store, after.get(path))?;
        if old.is_none() && new.is_none() {
            continue;
        }
        files.push(path.to_string());
        let (old, new) = (old.unwrap_or_default(), new.unwrap_or_default());
        for change in TextDiff::from_lines(&old, &new).iter_all_changes() {
            let line = change.value().trim();
            if line.chars().count() < MIN_LINE_CHARS {
                continue;
            }
            match change.tag() {
                ChangeTag::Insert => added.push(line.to_owned()),
                ChangeTag::Delete => removed.push(line.to_owned()),
                ChangeTag::Equal => {}
            }
        }
    }
    added.truncate(MAX_LINES);
    removed.truncate(MAX_LINES);

    let mut lessons = load(project)?;
    let lesson = Lesson {
        id: lessons.iter().map(|l| l.id).max().unwrap_or(0) + 1,
        created_at: timefmt::now_rfc3339(),
        rule: rule.trim().to_owned(),
        from_checkpoint: seq,
        step_label: step.label.clone(),
        agent: step.agent.clone(),
        files,
        added_lines: added,
        removed_lines: removed,
        failing_tests: failing_tests(project),
        active: true,
    };
    lessons.push(lesson.clone());
    save(project, lessons)?;
    Ok(lesson)
}

fn file_text(store: &Store, entry: Option<&Entry>) -> Result<Option<String>, CliError> {
    match entry {
        Some(Entry::File { id, .. }) => Ok(String::from_utf8(store.read(id)?).ok()),
        _ => Ok(None),
    }
}

/// Test names from the last recorded test run (unittest and pytest output formats).
fn failing_tests(project: &Project) -> Vec<String> {
    let Ok(text) = fs::read_to_string(project.dir.join(TEST_OUTPUT_FILE)) else {
        return Vec::new();
    };
    let mut tests = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        for prefix in ["FAIL: ", "ERROR: ", "FAILED "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name = rest.split(" - ").next().unwrap_or(rest).trim();
                if !name.is_empty() {
                    tests.insert(name.to_owned());
                }
            }
        }
    }
    tests.into_iter().collect()
}

pub fn retire(project: &Project, id: u64) -> Result<(), CliError> {
    let mut lessons = load(project)?;
    let lesson = lessons
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or_else(|| CliError::new(exit::ERROR, "unknown_lesson", format!("no lesson {id}")))?;
    lesson.active = false;
    save(project, lessons)
}

/// The active lessons as text an agent can act on, most recent first.
pub fn briefing(lessons: &[Lesson], paths: &[String]) -> String {
    let relevant: Vec<&Lesson> = lessons
        .iter()
        .rev()
        .filter(|l| l.active)
        .filter(|l| paths.is_empty() || l.files.iter().any(|f| paths.contains(f)))
        .take(MAX_INJECTED)
        .collect();
    if relevant.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "Lessons Fabric recorded in this project from changes that broke things (verified by tests and reverted). Follow them:\n",
    );
    for l in relevant {
        let _ = write!(out, "- {}", l.rule);
        let mut evidence = Vec::new();
        if !l.files.is_empty() {
            evidence.push(format!("files: {}", l.files.join(", ")));
        }
        if !l.failing_tests.is_empty() {
            evidence.push(format!("broke: {}", l.failing_tests.join(", ")));
        }
        evidence.push(format!("learned from checkpoint #{}", l.from_checkpoint));
        let _ = writeln!(out, " ({})", evidence.join("; "));
    }
    out
}

/// Context to add to a prompt, as Claude Code's `UserPromptSubmit` hook output.
pub fn prompt_context(project: &Project) -> Result<Option<Value>, CliError> {
    let text = briefing(&load(project)?, &[]);
    Ok((!text.is_empty()).then(|| {
        json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": text,
            }
        })
    }))
}

// ---- learned guard ------------------------------------------------------------------------

/// Identifiers, numbers and single punctuation characters, in order; whitespace ignored.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in line.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if !c.is_whitespace() {
                out.push(c.to_string());
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Two lines are alike when their adjacent token pairs mostly overlap. Pairs keep the order
/// that matters: `x * (1 - d)` and `x - d` share every token but not their structure.
fn similar_lines(a: &str, b: &str) -> bool {
    let (a, b) = (tokens(a), tokens(b));
    if a.len() < 3 || b.len() < 3 {
        return a == b && !a.is_empty();
    }
    let pairs = |t: &[String]| -> BTreeSet<(String, String)> {
        t.windows(2).map(|w| (w[0].clone(), w[1].clone())).collect()
    };
    let (a, b) = (pairs(&a), pairs(&b));
    let shared = a.intersection(&b).count() as f64;
    shared / a.union(&b).count() as f64 >= SIMILARITY
}

fn meaningful(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.chars().count() >= MIN_LINE_CHARS)
        .collect()
}

/// Whether an edit replacing `old` with `new` in `path` repeats a lesson's failing change.
pub fn repeats(lesson: &Lesson, path: &str, old: &str, new: &str) -> bool {
    if !lesson.active || !lesson.files.iter().any(|f| f == path) {
        return false;
    }
    let new_lines = meaningful(new);
    let old_lines = meaningful(old);
    let re_adds = lesson
        .added_lines
        .iter()
        .any(|bad| new_lines.iter().any(|l| similar_lines(bad, l)));
    let re_removes = lesson.removed_lines.iter().any(|gone| {
        old_lines.iter().any(|l| similar_lines(gone, l))
            && !new_lines.iter().any(|l| similar_lines(gone, l))
    });
    re_adds || re_removes
}

/// The old and new text of a Claude Code file-editing tool call.
fn edit_texts(tool: &str, input: &Value, current: &str) -> Option<(String, String)> {
    match tool {
        "Write" => Some((current.to_owned(), input["content"].as_str()?.to_owned())),
        "Edit" => Some((
            input["old_string"].as_str()?.to_owned(),
            input["new_string"].as_str()?.to_owned(),
        )),
        "MultiEdit" => {
            let edits = input["edits"].as_array()?;
            let join = |key: &str| {
                edits
                    .iter()
                    .filter_map(|e| e[key].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Some((join("old_string"), join("new_string")))
        }
        _ => None,
    }
}

/// The learned guard's answer to a `PreToolUse` event: a one-time block, or `None`.
pub fn learned_guard(project: &Project, event: &Value) -> Result<Option<Value>, CliError> {
    let tool = event["tool_name"].as_str().unwrap_or_default();
    let input = &event["tool_input"];
    let Some(file) = input["file_path"].as_str() else {
        return Ok(None);
    };
    let path = project_relative(file, &project.root);
    let current = fs::read_to_string(RelPath::new(path.clone()).map_or_else(
        |_| project.root.join(&path),
        |rel| rel.to_fs_path(&project.root),
    ))
    .unwrap_or_default();
    let Some((old, new)) = edit_texts(tool, input, &current) else {
        return Ok(None);
    };
    let session = event["session_id"].as_str().unwrap_or("-").to_owned();
    let warned = warned_before(project, &session);
    let lessons = load(project)?;
    let Some(lesson) = lessons
        .iter()
        .find(|l| !warned.contains(&l.id) && repeats(l, &path, &old, &new))
    else {
        return Ok(None);
    };
    record_warning(project, &session, lesson, &path, tool)?;
    let reason = format!(
        "Fabric learned guard: this edit to {path} resembles the change in checkpoint #{} ({}), which {} and was reverted. Lesson: {} If you have checked this change follows the lesson, retry it and it will be allowed.",
        lesson.from_checkpoint,
        lesson.step_label,
        if lesson.failing_tests.is_empty() {
            "broke the tests".to_owned()
        } else {
            format!("broke {}", lesson.failing_tests.join(", "))
        },
        lesson.rule
    );
    Ok(Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })))
}

fn warned_before(project: &Project, session: &str) -> BTreeSet<u64> {
    fs::read_to_string(project.dir.join(WARNINGS_FILE))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|w| w["session"] == session)
        .filter_map(|w| w["lesson"].as_u64())
        .collect()
}

fn record_warning(
    project: &Project,
    session: &str,
    lesson: &Lesson,
    path: &str,
    tool: &str,
) -> Result<(), CliError> {
    let path_on_disk = project.dir.join(WARNINGS_FILE);
    let mut line = json!({
        "at": timefmt::now_rfc3339(),
        "session": session,
        "lesson": lesson.id,
        "file": path,
        "tool": tool,
    })
    .to_string();
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path_on_disk)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .map_err(|e| CliError::io(&path_on_disk, e))
}

// ---- `fabric lesson` commands ---------------------------------------------------------------

pub fn add_cmd(
    home: &Home,
    project: &Project,
    seq: u64,
    rule: &str,
    json_output: bool,
) -> Result<u8, CliError> {
    let store = home.open_store()?;
    let lesson = add(&store, project, seq, rule)?;
    if json_output {
        print_json(&json!({ "schema": "fabric.lesson/v1", "lesson": lesson }));
    } else {
        println!(
            "Lesson {} recorded from checkpoint #{} ({} file(s), {} failing test(s)): {}",
            lesson.id,
            lesson.from_checkpoint,
            lesson.files.len(),
            lesson.failing_tests.len(),
            lesson.rule
        );
    }
    Ok(exit::OK)
}

pub fn list_cmd(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let lessons = load(project)?;
    if json_output {
        print_json(&json!({ "schema": "fabric.lessons/v1", "lessons": lessons }));
    } else if lessons.is_empty() {
        println!("No lessons yet. Record one with `fabric lesson add --step N --rule \"...\"`.");
    } else {
        for l in &lessons {
            println!(
                "{:>3} {} from #{} [{}] {}",
                l.id,
                if l.active { "active " } else { "retired" },
                l.from_checkpoint,
                l.files.join(", "),
                l.rule
            );
        }
    }
    Ok(exit::OK)
}

pub fn retire_cmd(project: &Project, id: u64, json_output: bool) -> Result<u8, CliError> {
    retire(project, id)?;
    if json_output {
        print_json(&json!({ "schema": "fabric.lesson/v1", "retired": id }));
    } else {
        println!("Lesson {id} retired");
    }
    Ok(exit::OK)
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("output always serializes")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(added: &[&str], removed: &[&str]) -> Lesson {
        Lesson {
            id: 1,
            created_at: String::new(),
            rule: "discount is a fraction".to_owned(),
            from_checkpoint: 4,
            step_label: "Edit cart.py".to_owned(),
            agent: None,
            files: vec!["cart.py".to_owned()],
            added_lines: added.iter().map(|s| (*s).to_owned()).collect(),
            removed_lines: removed.iter().map(|s| (*s).to_owned()).collect(),
            failing_tests: vec![],
            active: true,
        }
    }

    #[test]
    fn recognizes_the_same_change_written_slightly_differently() {
        let l = lesson(&["return round(subtotal(items) - discount, 2)"], &[]);
        assert!(repeats(
            &l,
            "cart.py",
            "",
            "    return round(subtotal(items) - discount, 2)\n"
        ));
        assert!(repeats(
            &l,
            "cart.py",
            "",
            "return round( subtotal(items)-discount , 2 )"
        ));
        // A different change to the same file is not flagged.
        assert!(!repeats(
            &l,
            "cart.py",
            "",
            "return round(subtotal(items) * (1 - discount), 2)\n"
        ));
        // The same change in another file is not this lesson's concern.
        assert!(!repeats(
            &l,
            "other.py",
            "",
            "return round(subtotal(items) - discount, 2)"
        ));
    }

    #[test]
    fn recognizes_removing_what_the_failing_step_removed() {
        let l = lesson(&[], &["def get_user(user_id):"]);
        assert!(repeats(
            &l,
            "cart.py",
            "def get_user(user_id):\n    ...",
            "def fetch_user(user_id):\n    ..."
        ));
        // Keeping the line is fine.
        assert!(!repeats(
            &l,
            "cart.py",
            "def get_user(user_id):\n",
            "def get_user(user_id):\n    # doc\n"
        ));
    }

    #[test]
    fn retired_lessons_never_fire() {
        let mut l = lesson(&["return round(subtotal(items) - discount, 2)"], &[]);
        l.active = false;
        assert!(!repeats(
            &l,
            "cart.py",
            "",
            "return round(subtotal(items) - discount, 2)"
        ));
    }

    #[test]
    fn briefing_lists_active_lessons_with_evidence() {
        let mut l = lesson(&[], &[]);
        l.failing_tests = vec!["test_discount".to_owned()];
        let text = briefing(&[l], &[]);
        assert!(text.contains("discount is a fraction"));
        assert!(text.contains("broke: test_discount"));
        assert!(text.contains("checkpoint #4"));
    }
}
