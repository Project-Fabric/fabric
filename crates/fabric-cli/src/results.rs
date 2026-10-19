//! Test results recorded by `fabric bisect`: which states passed which test command.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use fabric_store::ObjectId;
use serde::{Deserialize, Serialize};

use crate::error::CliError;
use crate::home::Project;

const RESULTS_FILE: &str = "test-results.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub snapshot: ObjectId,
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub tested_at: String,
}

pub fn append(project: &Project, result: &TestResult) -> Result<(), CliError> {
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

/// All recorded results, oldest first. Unreadable lines (an interrupted write) are skipped:
/// results are history, never used to make decisions.
pub fn read(project: &Project) -> Result<Vec<TestResult>, CliError> {
    let path = project.dir.join(RESULTS_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CliError::io(&path, e)),
    }
}

/// The most recent result for `snapshot`, if it was ever tested.
pub fn latest_for<'a>(results: &'a [TestResult], snapshot: &ObjectId) -> Option<&'a TestResult> {
    results.iter().rev().find(|r| &r.snapshot == snapshot)
}
