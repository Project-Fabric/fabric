//! `fabric mcp`: a Model Context Protocol server over stdio, so any agent can ask what other
//! agents did in this project — the timeline, a file's history, what a step changed, which
//! tests failed — and record its own checkpoints.
//!
//! Deliberately read-mostly: agents can add checkpoints but cannot restore, revert or delete
//! anything. Destructive operations stay with people, on the command line.
//!
//! Messages are newline-delimited JSON-RPC 2.0. The store is opened per call and released
//! immediately, so the server never blocks hooks or other Fabric commands.

use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::checkpoint::{CheckpointKind, Meta, record};
use crate::error::{CliError, exit};
use crate::home::{Home, Project};
use crate::query;

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Fabric records every change AI agents make to this project, \
whichever agent made it. Before changing a file, call fabric_file_history to see what other \
agents already tried on it and whether the tests passed. Use fabric_timeline for recent \
activity and fabric_step to see exactly what a step changed. After finishing a meaningful \
change, call fabric_checkpoint so the next agent (or person) can see and undo it.";

struct Server<'a> {
    home: &'a Home,
    /// Who is calling, from the client's `initialize`; the default agent for checkpoints.
    client: Option<String>,
}

pub fn serve(home: &Home) -> Result<(), CliError> {
    let mut server = Server { home, client: None };
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let line = line.map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle(&line) {
            let mut text = response.to_string();
            text.push('\n');
            stdout
                .write_all(text.as_bytes())
                .and_then(|()| stdout.flush())
                .map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?;
        }
    }
    Ok(())
}

impl Server<'_> {
    /// Handles one message; returns the response, or `None` for notifications.
    fn handle(&mut self, line: &str) -> Option<Value> {
        // Windows tools (PowerShell among them) often prefix piped text with a byte-order mark.
        let message: Value = match serde_json::from_str(line.trim_start_matches('\u{feff}')) {
            Ok(message) => message,
            Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        // Notifications (no id) and responses to anything we sent need no reply.
        let id = message.get("id")?.clone();
        let method = message["method"].as_str()?;
        let params = &message["params"];
        let result = match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(self.call(params)),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id, code, &message),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        self.client = params["clientInfo"]["name"].as_str().map(str::to_owned);
        let requested = params["protocolVersion"].as_str().unwrap_or_default();
        let version = PROTOCOL_VERSIONS
            .iter()
            .find(|v| **v == requested)
            .unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "fabric", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&self, params: &Value) -> Value {
        let args = &params["arguments"];
        let result = match params["name"].as_str().unwrap_or_default() {
            "fabric_timeline" => self
                .project(args)
                .and_then(|p| query::timeline(&p, limit(args, 30), args["agent"].as_str())),
            "fabric_file_history" => match args["path"].as_str() {
                Some(path) => self.with_store(args, |store, project| {
                    query::file_history(store, project, path, limit(args, 20))
                }),
                None => Err(invalid("`path` is required")),
            },
            "fabric_step" => match args["checkpoint"].as_u64() {
                Some(seq) => {
                    self.with_store(args, |store, project| query::step(store, project, seq))
                }
                None => Err(invalid("`checkpoint` must be a checkpoint number")),
            },
            "fabric_test_results" => self
                .project(args)
                .and_then(|p| query::test_results(&p, limit(args, 20))),
            "fabric_lessons" => self.project(args).and_then(|p| {
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .map(|paths| {
                        paths
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|path| crate::home::project_relative(path, &p.root))
                            .collect()
                    })
                    .unwrap_or_default();
                let text = crate::lessons::briefing(&crate::lessons::load(&p)?, &paths);
                Ok(if text.is_empty() {
                    "No lessons recorded for these files.".to_owned()
                } else {
                    text
                })
            }),
            "fabric_checkpoint" => self.checkpoint(args),
            other => Err(invalid(&format!("unknown tool {other:?}"))),
        };
        match result {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(e) => json!({
                "content": [{ "type": "text", "text": format!("{} ({})", e.message, e.kind) }],
                "isError": true,
            }),
        }
    }

    fn project(&self, args: &Value) -> Result<Project, CliError> {
        let dir = match args["project"].as_str() {
            Some(path) => PathBuf::from(path),
            None => {
                env::current_dir().map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?
            }
        };
        self.home.find_project(&dir)
    }

    fn with_store(
        &self,
        args: &Value,
        query: impl FnOnce(&fabric_store::Store, &Project) -> Result<String, CliError>,
    ) -> Result<String, CliError> {
        let project = self.project(args)?;
        let store = self.home.open_store()?;
        query(&store, &project)
    }

    fn checkpoint(&self, args: &Value) -> Result<String, CliError> {
        let message = args["message"]
            .as_str()
            .ok_or_else(|| invalid("`message` is required"))?;
        let project = self.project(args)?;
        let store = self.home.open_store()?;
        let snapshot = project.capture(&store)?;
        let mut meta = Meta::new(CheckpointKind::AgentStep, message);
        meta.agent = args["agent"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| self.client.clone());
        Ok(match record(&store, &project, &snapshot, meta, true)? {
            Some(c) => format!("Recorded checkpoint #{}: {}", c.seq, c.label),
            None => "No changes since the latest checkpoint; nothing recorded.".to_owned(),
        })
    }
}

fn limit(args: &Value, default: usize) -> usize {
    args["limit"].as_u64().map_or(default, |n| {
        usize::try_from(n).unwrap_or(default).clamp(1, 500)
    })
}

fn invalid(message: &str) -> CliError {
    CliError::new(exit::ERROR, "invalid_arguments", message)
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    let project = json!({
        "type": "string",
        "description": "Absolute path of the project (default: the server's working directory)"
    });
    let limit = |default: u64| json!({ "type": "integer", "minimum": 1, "default": default });
    json!([
        {
            "name": "fabric_timeline",
            "title": "Project timeline",
            "description": "Recent checkpoints across all agents, newest first: who changed what, when, and recorded test results. Start here to see what happened recently.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": limit(30),
                    "agent": { "type": "string", "description": "Only this agent's checkpoints, e.g. claude-code" },
                    "project": project,
                },
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "fabric_file_history",
            "title": "File history",
            "description": "Every recorded change to one file: which agent changed it, in which step, and whether tests passed afterwards. Call this before working on a file to learn what was already tried.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path, relative to the project or absolute" },
                    "limit": limit(20),
                    "project": project,
                },
                "required": ["path"],
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "fabric_step",
            "title": "What a step changed",
            "description": "Exactly what one checkpoint changed compared with the previous one, with unified diffs of text files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "checkpoint": { "type": "integer", "minimum": 1, "description": "Checkpoint number from the timeline" },
                    "project": project,
                },
                "required": ["checkpoint"],
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "fabric_test_results",
            "title": "Test results",
            "description": "Recorded test runs (from `fabric bisect`), newest first, with the checkpoint each ran against.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": limit(20), "project": project },
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "fabric_lessons",
            "title": "Lessons for these files",
            "description": "Lessons Fabric recorded from changes that broke tests and were reverted, optionally only those about the given files. Check before changing files that have failed before.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Files you are about to change (default: all lessons)" },
                    "project": project,
                },
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "fabric_checkpoint",
            "title": "Record a checkpoint",
            "description": "Record the project's current state with a short description of what you just did, so other agents and people can see it and undo it. Records nothing if nothing changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "What you changed and why, in one line" },
                    "agent": { "type": "string", "description": "Your name, e.g. codex (default: the MCP client's name)" },
                    "project": project,
                },
                "required": ["message"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
    ])
}
