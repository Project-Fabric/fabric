//! The evidence chain: a hash-chained, append-only record of everything that matters to an
//! auditor — every checkpoint, every guard decision, every approval and every policy change.
//!
//! Each line of `<project dir>/evidence.jsonl` is `<hash> <body>`, where `body` is compact JSON
//! and `hash` is the BLAKE3 hash of exactly those bytes, in hex. Each body names the previous
//! record's hash, so changing, removing or reordering any record breaks every link after it,
//! and the chain's last hash (its *head*) commits to the whole history. See
//! `docs/STORAGE_FORMAT.md`.
//!
//! What this proves, and what it does not: anyone holding only an earlier head (from an
//! export, a ticket or an email) can detect any later change to the records it covers. It
//! cannot stop someone with write access to Fabric's data directory from rewriting the whole
//! chain before a head has been shared; exports are signed and heads should be shared for that
//! reason.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use fabric_snapshot::{Entry, Manifest};
use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::checkpoint::{Checkpoint, read_log};
use crate::error::{CliError, exit};
use crate::home::{Project, display_path};
use crate::timefmt;

pub const FILE: &str = "evidence.jsonl";
const LOCK_FILE: &str = "evidence.lock";
pub const VERSION: u32 = 1;
/// The `prev` of the first record.
pub const GENESIS_PREV: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// Longest text kept from a command or path in an event; the full content is in snapshots.
const MAX_TEXT: usize = 2_000;
/// How much of the end of the chain is read to find the last record before appending.
const TAIL_BYTES: u64 = 64 * 1024;

/// The hashed part of a record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub v: u32,
    pub seq: u64,
    pub prev: String,
    pub at: String,
    pub event: Value,
}

#[derive(Debug, Clone)]
pub struct Record {
    pub hash: String,
    pub body: Body,
}

pub fn hash_hex(bytes: &[u8]) -> String {
    ObjectId::of(bytes).to_hex()
}

/// Cuts text kept in events to a bounded length.
pub fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX_TEXT).collect();
    format!("{cut}… (truncated)")
}

/// Appends `event` to the project's chain, starting the chain with a genesis record if it
/// does not exist yet. Appends from concurrent processes (agent hooks, the CLI) are
/// serialized by a lock file.
pub fn append(project: &Project, event: Value) -> Result<Record, CliError> {
    let lock_path = project.dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| CliError::io(&lock_path, e))?;
    lock.lock().map_err(|e| CliError::io(&lock_path, e))?;

    let path = project.dir.join(FILE);
    drop_interrupted_tail(&path)?;
    let mut last = last_record(&path)?;
    if last.is_none() {
        let genesis = json!({
            "kind": "genesis",
            "project": project.id,
            "root": display_path(&project.root),
            "fabric_version": env!("CARGO_PKG_VERSION"),
        });
        last = Some(write_record(&path, None, genesis)?);
    }
    let record = write_record(&path, last.as_ref(), event)?;
    drop(lock);
    Ok(record)
}

fn write_record(path: &Path, last: Option<&Record>, event: Value) -> Result<Record, CliError> {
    let body = Body {
        v: VERSION,
        seq: last.map_or(1, |r| r.body.seq + 1),
        prev: last.map_or_else(|| GENESIS_PREV.to_owned(), |r| r.hash.clone()),
        at: timefmt::now_rfc3339(),
        event,
    };
    let bytes = serde_json::to_vec(&body).expect("evidence always serializes");
    let hash = hash_hex(&bytes);
    let mut line = Vec::with_capacity(bytes.len() + 66);
    line.extend_from_slice(hash.as_bytes());
    line.push(b' ');
    line.extend_from_slice(&bytes);
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| CliError::io(path, e))?;
    file.write_all(&line)
        .and_then(|()| file.sync_data())
        .map_err(|e| CliError::io(path, e))?;
    Ok(Record { hash, body })
}

/// A record whose write was interrupted never completed; remove it so the next record starts
/// on its own line. (`verify` reports such a tail if it is still present.)
fn drop_interrupted_tail(path: &Path) -> Result<(), CliError> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(CliError::io(path, e)),
    };
    let len = file.metadata().map_err(|e| CliError::io(path, e))?.len();
    if len == 0 {
        return Ok(());
    }
    let mut last = [0u8; 1];
    file.seek(SeekFrom::End(-1))
        .and_then(|_| file.read_exact(&mut last))
        .map_err(|e| CliError::io(path, e))?;
    if last[0] == b'\n' {
        return Ok(());
    }
    let text = fs::read(path).map_err(|e| CliError::io(path, e))?;
    let keep = text.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    file.set_len(keep as u64).map_err(|e| CliError::io(path, e))
}

/// The last complete record, reading only the end of the file.
fn last_record(path: &Path) -> Result<Option<Record>, CliError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CliError::io(path, e)),
    };
    let len = file.metadata().map_err(|e| CliError::io(path, e))?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    let mut tail = Vec::new();
    file.seek(SeekFrom::Start(start))
        .and_then(|_| file.read_to_end(&mut tail))
        .map_err(|e| CliError::io(path, e))?;
    let text = String::from_utf8_lossy(&tail);
    let mut lines = text.lines().filter(|l| !l.is_empty());
    let Some(line) = lines.next_back() else {
        return Ok(None);
    };
    // A record longer than the tail window is never written (events are clipped), so a
    // partial first line can only be one we skip.
    parse_line(line).map(Some).map_err(|problem| {
        CliError::new(
            exit::INTEGRITY,
            "corrupt_evidence",
            format!(
                "the last evidence record is unreadable ({problem}); run `fabric evidence verify`"
            ),
        )
    })
}

pub fn parse_line(line: &str) -> Result<Record, String> {
    let (hash, body) = line
        .split_once(' ')
        .ok_or_else(|| "not `<hash> <body>`".to_owned())?;
    if hash_hex(body.as_bytes()) != hash {
        return Err("its hash does not match its content".to_owned());
    }
    let body: Body = serde_json::from_str(body).map_err(|e| format!("invalid body: {e}"))?;
    Ok(Record {
        hash: hash.to_owned(),
        body,
    })
}

/// The result of checking a chain.
#[derive(Debug, Default, Serialize)]
pub struct ChainReport {
    pub records: u64,
    pub head: Option<String>,
    pub problems: Vec<String>,
    /// An interrupted final write, which is not tampering but is reported.
    pub incomplete_tail: bool,
}

impl ChainReport {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Checks every record's hash and every link. Pure: needs only the chain's text.
pub fn verify_chain(text: &str) -> (ChainReport, Vec<Record>) {
    let mut report = ChainReport::default();
    let mut records = Vec::new();
    let complete = text.rfind('\n').map_or("", |end| &text[..=end]);
    report.incomplete_tail = complete.len() < text.len();
    let mut prev = GENESIS_PREV.to_owned();
    for (n, line) in complete.lines().enumerate() {
        let line_no = n + 1;
        let record = match parse_line(line) {
            Ok(record) => record,
            Err(problem) => {
                report
                    .problems
                    .push(format!("record on line {line_no}: {problem}"));
                // Keep checking links from this line's claimed hash, so one bad record is
                // reported once rather than breaking every report after it.
                if let Some((hash, _)) = line.split_once(' ') {
                    prev = hash.to_owned();
                }
                continue;
            }
        };
        let expected_seq = line_no as u64;
        if record.body.seq != expected_seq {
            report.problems.push(format!(
                "record on line {line_no} has number {} (expected {expected_seq}): records were removed or reordered",
                record.body.seq
            ));
        }
        if record.body.prev != prev {
            report.problems.push(format!(
                "record {} does not follow the record before it: the chain was changed",
                record.body.seq
            ));
        }
        if record.body.v != VERSION {
            report.problems.push(format!(
                "record {} has unsupported version {}",
                record.body.seq, record.body.v
            ));
        }
        prev = record.hash.clone();
        report.head = Some(record.hash.clone());
        report.records += 1;
        records.push(record);
    }
    (report, records)
}

pub fn read_chain(project: &Project) -> Result<String, CliError> {
    let path = project.dir.join(FILE);
    match fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            CliError::new(
                exit::INTEGRITY,
                "corrupt_evidence",
                "the evidence chain is not valid UTF-8",
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(CliError::io(&path, e)),
    }
}

/// Checks the chain, that every recorded snapshot is present and intact in the store
/// (`deep` also re-hashes every file the snapshots contain), and that the checkpoint log
/// agrees with the chain.
pub fn verify_project(
    store: &Store,
    project: &Project,
    deep: bool,
) -> Result<(ChainReport, Vec<Record>), CliError> {
    let (mut report, records) = verify_chain(&read_chain(project)?);
    let mut evidenced: BTreeMap<u64, ObjectId> = BTreeMap::new();
    for record in &records {
        let event = &record.body.event;
        if event["kind"] != "checkpoint" {
            continue;
        }
        let (Some(seq), Some(snapshot)) = (
            event["checkpoint"].as_u64(),
            event["snapshot"]
                .as_str()
                .and_then(|s| s.parse::<ObjectId>().ok()),
        ) else {
            report.problems.push(format!(
                "record {} is a malformed checkpoint event",
                record.body.seq
            ));
            continue;
        };
        evidenced.insert(seq, snapshot);
        match Manifest::load(store, &snapshot) {
            Ok(manifest) if deep => {
                for entry in manifest.entries() {
                    if let Entry::File { path, id, .. } = entry
                        && store.verify(id).is_err()
                    {
                        report.problems.push(format!(
                            "checkpoint #{seq}: the content of {path} is missing or damaged in the store"
                        ));
                    }
                }
            }
            Ok(_) => {}
            Err(e) => report.problems.push(format!(
                "checkpoint #{seq}: snapshot {} cannot be read from the store ({e})",
                snapshot.to_hex()
            )),
        }
    }
    if let Some(first) = evidenced.keys().next().copied() {
        for checkpoint in read_log(project)?.iter().filter(|c| c.seq >= first) {
            match evidenced.get(&checkpoint.seq) {
                Some(id) if *id == checkpoint.snapshot => {}
                Some(_) => report.problems.push(format!(
                    "checkpoint #{} in the log names a different snapshot than the evidence",
                    checkpoint.seq
                )),
                None => report.problems.push(format!(
                    "checkpoint #{} is in the log but not in the evidence",
                    checkpoint.seq
                )),
            }
        }
    }
    Ok((report, records))
}

// ---- events ------------------------------------------------------------------------------

pub fn checkpoint_event(checkpoint: &Checkpoint, parent: Option<&ObjectId>) -> Value {
    json!({
        "kind": "checkpoint",
        "checkpoint": checkpoint.seq,
        "snapshot": checkpoint.snapshot,
        "parent": parent,
        "type": checkpoint.kind,
        "label": clip(&checkpoint.label),
        "agent": checkpoint.agent,
        "session": checkpoint.session,
        "tool": checkpoint.tool,
        "changes": checkpoint.changes,
    })
}

/// The OS account a process runs as. Recorded with requests and approvals so a reviewer can
/// see whether an approval came from a different account than the agent's.
pub fn os_user() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.is_empty())
}
