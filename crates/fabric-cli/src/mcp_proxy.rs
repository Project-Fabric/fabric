//! `fabric mcp-proxy`: puts Fabric's controls in front of any MCP server, for any agent.
//!
//! Agents increasingly act on systems through tools rather than files: an agent closing the
//! month calls `post_journal_entry` on an ERP's MCP server. The proxy sits between the agent
//! (any MCP client: Claude Code, Codex, an in-house finance agent) and the real server:
//!
//! ```text
//! agent  <-- stdio -->  fabric mcp-proxy --name ledgerly -- <server command>  <-- stdio -->  server
//! ```
//!
//! Every `tools/call` is judged by the project's rules as `server/tool` (see the
//! `finance-ops` pack). Allowed calls are forwarded. Calls under an `approve` rule are held
//! exactly as file edits are: an approval request is recorded and the agent gets an error
//! result telling it to ask for `fabric approve N`; the same call made again after approval is
//! forwarded once. Denied calls are refused. Every call is recorded in the evidence with its
//! arguments, the decision, and the server's result, whether forwarded, held or refused.
//!
//! Tools under a rule are marked in `tools/list` so the agent knows before it calls.
//! Everything else passes through unchanged. Messages are newline-delimited JSON-RPC (the
//! MCP stdio transport); a line that is not JSON is forwarded as is.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::approvals;
use crate::error::{CliError, exit};
use crate::evidence;
use crate::guard::{self, Action, Decision, Policy};
use crate::home::{Home, Project};
use crate::identity;

/// A call forwarded to the server, waiting for its response.
struct Pending {
    tool: String,
    digest: String,
    decision: Decision,
    rule: Option<String>,
}

struct Shared {
    project: Project,
    server: String,
    session: String,
    /// The agent, from the client's `initialize`: what it says it is.
    client: Mutex<Option<String>>,
    /// The agent as authenticated by its token (`FABRIC_AGENT_TOKEN`), if it gave one.
    authenticated: Option<identity::Authenticated>,
    /// Calls without a valid agent token are refused (`fabric agent require on`).
    identity_required: bool,
    pending: Mutex<HashMap<String, Pending>>,
    out: Mutex<io::Stdout>,
}

impl Shared {
    fn send(&self, message: &Value) {
        self.send_line(&message.to_string());
    }

    fn send_line(&self, line: &str) {
        let mut out = self.out.lock().expect("stdout lock");
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    /// Who the evidence says made a call: the authenticated agent, else the name the client
    /// gave (recorded as unauthenticated).
    fn agent(&self) -> String {
        if let Some(a) = &self.authenticated {
            return a.name.clone();
        }
        self.client
            .lock()
            .expect("client lock")
            .clone()
            .unwrap_or_else(|| "mcp-client".to_owned())
    }

    /// Records a tool call in the evidence. Failing to record is reported on stderr, never
    /// to the agent: the agent's work must not depend on Fabric's bookkeeping.
    fn record(&self, call: &Value) {
        let mut event = call.clone();
        event["kind"] = "tool_call".into();
        event["server"] = self.server.clone().into();
        event["agent"] = self.agent().into();
        event["agent_authenticated"] = self.authenticated.is_some().into();
        event["session"] = self.session.clone().into();
        if let Err(e) = evidence::append(&self.project, event) {
            eprintln!("fabric mcp-proxy: could not record a tool call: {e}");
        }
    }
}

/// The text of a tool result, for the evidence.
fn result_text(result: &Value) -> String {
    let text: Vec<&str> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["text"].as_str())
        .collect();
    if text.is_empty() {
        result.to_string()
    } else {
        text.join("\n")
    }
}

fn tool_error(id: &Value, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "content": [{ "type": "text", "text": text }], "isError": true },
    })
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

/// Decides on one `tools/call` request. Returns the response to send back when the call is
/// not forwarded.
fn judge_call(home: &Home, shared: &Shared, message: &Value) -> Option<Value> {
    let id = &message["id"];
    let name = message["params"]["name"].as_str().unwrap_or_default();
    let arguments = message["params"]["arguments"].clone();
    let tool = format!("{}/{name}", shared.server);
    // Named as Claude Code names MCP tools, so an approval is the same whichever way the
    // call arrives.
    let tool_name = format!("mcp__{}__{name}", shared.server);
    let digest = approvals::digest(&tool_name, &arguments);
    let arguments_text = evidence::clip(&arguments.to_string());
    let policy = match Policy::load(&shared.project) {
        Ok(policy) => policy,
        Err(e) => {
            // Fail closed: without the rules, a call that might move money is not forwarded.
            return Some(tool_error(
                id,
                &format!(
                    "Fabric could not read this project's rules ({e}); the call was not made."
                ),
            ));
        }
    };
    let mut verdict = policy.evaluate(home, &shared.project, &Action::Tool(&tool));
    // Identity comes before rules: an unauthenticated agent where tokens are required, or an
    // agent calling outside its scopes, is refused whatever the rules say.
    if shared.identity_required && shared.authenticated.is_none() {
        verdict = guard::Verdict {
            decision: Decision::Deny,
            rule: Some("agent-identity".to_owned()),
            reason: format!(
                "this gateway requires an agent token ({})",
                identity::TOKEN_ENV
            ),
        };
    } else if let Some(agent) = &shared.authenticated
        && !agent.may_call(&tool)
    {
        verdict = guard::Verdict {
            decision: Decision::Deny,
            rule: Some("agent-scope".to_owned()),
            reason: format!("{tool} is outside agent {}'s scopes", agent.name),
        };
    }
    let event = json!({
        "tool_name": tool_name,
        "tool_input": arguments,
        "session_id": shared.session,
        "agent": shared.agent(),
        "agent_authenticated": shared.authenticated.is_some(),
    });
    if verdict.decision != Decision::Allow
        && let Err(e) = guard::audit(&shared.project, &event, &verdict, &tool)
    {
        eprintln!("fabric mcp-proxy: could not record a guard decision: {e}");
    }
    let held = |reason: String, outcome: &str| {
        shared.record(&json!({
            "tool": tool,
            "digest": digest,
            "arguments": arguments_text,
            "decision": verdict.decision,
            "rule": verdict.rule,
            "outcome": outcome,
            "result": evidence::clip(&reason),
        }));
        Some(tool_error(id, &reason))
    };
    match verdict.decision {
        Decision::Allow => {}
        Decision::Deny => {
            return held(
                format!(
                    "Blocked by Fabric ({}: {}). Do not try to work around this; ask the user if it is really needed.",
                    verdict.rule.as_deref().unwrap_or_default(),
                    verdict.reason
                ),
                "blocked",
            );
        }
        // Built-in `ask` rules do not match tools; treat one as needing a recorded approval,
        // since the proxy cannot ask anyone itself.
        Decision::Approve | Decision::Ask => {
            match approvals::gate(&shared.project, &event, &verdict, &tool, "proxy") {
                Ok(None) => {}
                Ok(Some(answer)) => {
                    let reason = answer["hookSpecificOutput"]["permissionDecisionReason"]
                        .as_str()
                        .unwrap_or("Fabric held this call for approval.")
                        .to_owned();
                    return held(reason, "held");
                }
                Err(e) => {
                    return held(
                        format!(
                            "Fabric could not check this call's approval ({e}); the call was not made."
                        ),
                        "held",
                    );
                }
            }
        }
    }
    shared.record(&json!({
        "tool": tool,
        "digest": digest,
        "arguments": arguments_text,
        "decision": verdict.decision,
        "rule": verdict.rule,
        "outcome": "forwarded",
    }));
    shared.pending.lock().expect("pending lock").insert(
        id_key(id),
        Pending {
            tool,
            digest,
            decision: verdict.decision,
            rule: verdict.rule,
        },
    );
    None
}

/// Marks the tools under a rule in a `tools/list` result, so the agent knows before calling.
fn annotate_tools(home: &Home, shared: &Shared, message: &mut Value) {
    let Ok(policy) = Policy::load(&shared.project) else {
        return;
    };
    for tool in message["result"]["tools"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let name = tool["name"].as_str().unwrap_or_default();
        let verdict = policy.evaluate(
            home,
            &shared.project,
            &Action::Tool(&format!("{}/{name}", shared.server)),
        );
        let note = match verdict.decision {
            Decision::Allow => continue,
            Decision::Deny => " [Fabric: blocked in this project]",
            _ => " [Fabric: needs a recorded approval from a person before it runs]",
        };
        let description = tool["description"].as_str().unwrap_or_default().to_owned();
        tool["description"] = format!("{description}{note}").into();
    }
}

/// Each injected credential, as it appears raw and JSON-escaped, with its placeholder.
/// Values shorter than 8 characters are not redacted: too likely to match ordinary text.
fn redactions(pairs: &[String], credentials: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for ((_, value), pair) in credentials.iter().zip(pairs) {
        let name = pair.split_once('=').map_or("", |(_, n)| n);
        let value = value.trim();
        if value.len() < 8 {
            continue;
        }
        let placeholder = format!("[redacted credential {name}]");
        let escaped = serde_json::to_string(value).expect("strings serialize");
        out.push((
            escaped[1..escaped.len() - 1].to_owned(),
            placeholder.clone(),
        ));
        out.push((value.to_owned(), placeholder));
    }
    out
}

fn redact(line: &str, redactions: &[(String, String)]) -> String {
    let mut line = line.to_owned();
    for (value, placeholder) in redactions {
        if line.contains(value.as_str()) {
            line = line.replace(value.as_str(), placeholder);
        }
    }
    line
}

pub fn run(
    home: &Home,
    project: Project,
    server: &str,
    command: &[String],
    secret_env: &[String],
) -> Result<u8, CliError> {
    let Some((program, args)) = command.split_first() else {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "give the server's command after --, e.g. fabric mcp-proxy --name erp -- python erp_server.py",
        ));
    };
    if server.is_empty() || server.contains('/') {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "--name is the server's name as rules see it (`<name>/<tool>`); it cannot contain /",
        ));
    }
    // Fail closed before starting anything: a bad token or a missing credential stops here.
    let token = std::env::var(identity::TOKEN_ENV).ok();
    let authenticated = identity::authenticate(&project, token.as_deref())?;
    let identity_required = identity::registry(&project)?.required;
    let credentials = identity::secret_env(home, secret_env)?;
    // Servers echo credentials by accident (`Invalid API key: sk_...`); every value the
    // gateway injected is scrubbed from what the server sends back, before the agent or the
    // evidence sees it.
    let redactions = redactions(secret_env, &credentials);
    let mut child = Command::new(program);
    child
        .args(args)
        .current_dir(&project.root)
        // The agent's token is for the gateway, not the upstream server.
        .env_remove(identity::TOKEN_ENV)
        .envs(credentials)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = child.spawn().map_err(|e| {
        CliError::new(
            exit::ERROR,
            "server_failed",
            format!("cannot start the MCP server {program:?}: {e}"),
        )
    })?;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let session = std::env::var("FABRIC_SESSION")
        .unwrap_or_else(|_| format!("mcp-proxy-{}-{started}", std::process::id()));
    let shared = Arc::new(Shared {
        project,
        server: server.to_owned(),
        session,
        client: Mutex::new(None),
        authenticated,
        identity_required,
        pending: Mutex::new(HashMap::new()),
        out: Mutex::new(io::stdout()),
    });

    // Server to agent: forward everything, recording the results of calls we forwarded.
    let server_out = child.stdout.take().expect("stdout is piped");
    let reader = {
        let shared = Arc::clone(&shared);
        let home = home.clone();
        let tools_list_ids = Arc::new(Mutex::new(Vec::<String>::new()));
        let ids = Arc::clone(&tools_list_ids);
        (
            thread::spawn(move || {
                for line in BufReader::new(server_out).lines() {
                    let Ok(line) = line else { break };
                    let line = redact(&line, &redactions);
                    let Ok(mut message) = serde_json::from_str::<Value>(&line) else {
                        shared.send_line(&line);
                        continue;
                    };
                    let key = id_key(&message["id"]);
                    let is_list = {
                        let mut ids = ids.lock().expect("ids lock");
                        let found = ids.iter().position(|k| *k == key);
                        found.map(|i| ids.remove(i)).is_some()
                    };
                    if is_list {
                        annotate_tools(&home, &shared, &mut message);
                        shared.send(&message);
                        continue;
                    }
                    let pending = shared.pending.lock().expect("pending lock").remove(&key);
                    if let Some(call) = pending {
                        let failed =
                            message["error"].is_object() || message["result"]["isError"] == true;
                        let result = if message["error"].is_object() {
                            message["error"].to_string()
                        } else {
                            result_text(&message["result"])
                        };
                        shared.record(&json!({
                            "tool": call.tool,
                            "digest": call.digest,
                            "decision": call.decision,
                            "rule": call.rule,
                            "outcome": if failed { "error" } else { "ok" },
                            "result": evidence::clip(&result),
                        }));
                    }
                    shared.send(&message);
                }
            }),
            tools_list_ids,
        )
    };
    let (reader, tools_list_ids) = reader;

    // Agent to server.
    let mut server_in = child.stdin.take().expect("stdin is piped");
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let forward = match serde_json::from_str::<Value>(&line) {
            Ok(message) => match message["method"].as_str() {
                Some("initialize") => {
                    *shared.client.lock().expect("client lock") =
                        message["params"]["clientInfo"]["name"]
                            .as_str()
                            .map(str::to_owned);
                    true
                }
                Some("tools/list") => {
                    tools_list_ids
                        .lock()
                        .expect("ids lock")
                        .push(id_key(&message["id"]));
                    true
                }
                Some("tools/call") => match judge_call(home, &shared, &message) {
                    Some(response) => {
                        shared.send(&response);
                        false
                    }
                    None => true,
                },
                _ => true,
            },
            Err(_) => true,
        };
        if forward && (writeln!(server_in, "{line}").is_err() || server_in.flush().is_err()) {
            break;
        }
    }
    drop(server_in);
    let status = child
        .wait()
        .map_err(|e| CliError::new(exit::ERROR, "server_failed", format!("the MCP server: {e}")))?;
    let _ = reader.join();
    Ok(status
        .code()
        .map_or(exit::ERROR, |c| u8::try_from(c).unwrap_or(exit::ERROR)))
}
