//! Agent identities and the credential broker.
//!
//! **Agent identities.** An MCP client says who it is in `initialize`, and anything can say
//! anything. A person registers each agent instead (`fabric agent add close-bot`), which issues
//! a token shown once; the agent's gateway configuration passes it to `fabric mcp-proxy` in
//! `FABRIC_AGENT_TOKEN`. The proxy authenticates the token and acts for that agent: mandates
//! naming an agent cover only calls authenticated as it, its scopes limit which tools it may
//! call at all, and the evidence records who it was. Only a BLAKE3 hash of each token is
//! stored. With `fabric agent require on`, calls without a valid token are refused.
//!
//! **Credential broker.** The agent should never hold the keys to the systems it acts on.
//! `fabric secret set stripe-live` stores a credential in this installation's data directory
//! (outside every project, readable only by the user's account), and `fabric mcp-proxy
//! --secret-env STRIPE_SECRET_KEY=stripe-live` gives it to the upstream server's environment
//! only. Secret values are never printed, logged or recorded; there is no command that reads
//! one back. The guard denies agents commands and reads that touch the secrets directory.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write as _};
use std::path::{Path, PathBuf};

use globset::GlobSet;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::approvals::in_agent_environment;
use crate::changes::globs;
use crate::error::{CliError, exit};
use crate::evidence;
use crate::home::{Home, Project, display_path, write_durably};
use crate::pack::hex;
use crate::timefmt;

const AGENTS_FILE: &str = "agents.json";
const AGENTS_FORMAT: &str = "fabric-agents";
const AGENTS_VERSION: u32 = 1;
/// The environment variable that carries an agent's token to `fabric mcp-proxy`.
pub const TOKEN_ENV: &str = "FABRIC_AGENT_TOKEN";
const TOKEN_PREFIX: &str = "fat_";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    /// BLAKE3 of the token; the token itself is never stored.
    pub token_hash: String,
    /// Tool calls (`server/tool` globs) the agent may make at all; empty means any.
    #[serde(default)]
    pub tools: Vec<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    /// Calls through the proxy must carry a valid agent token.
    pub required: bool,
    pub agents: Vec<Agent>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    format: String,
    version: u32,
    required: bool,
    agents: Vec<Agent>,
}

pub fn registry(project: &Project) -> Result<Registry, CliError> {
    let path = project.dir.join(AGENTS_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let file: RegistryFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_agents",
                    format!("{}: {e}", display_path(&path)),
                )
            })?;
            if file.format != AGENTS_FORMAT || file.version != AGENTS_VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_agents",
                    format!("{}: unsupported agents file", display_path(&path)),
                ));
            }
            Ok(Registry {
                required: file.required,
                agents: file.agents,
            })
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(CliError::io(&path, e)),
    }
}

fn save(project: &Project, registry: &Registry) -> Result<(), CliError> {
    let file = RegistryFile {
        format: AGENTS_FORMAT.to_owned(),
        version: AGENTS_VERSION,
        required: registry.required,
        agents: registry.agents.clone(),
    };
    write_durably(
        &project.dir.join(AGENTS_FILE),
        &serde_json::to_vec_pretty(&file).expect("agents always serialize"),
    )
}

fn person_only(what: &str) -> Result<(), CliError> {
    match in_agent_environment() {
        Some(marker) => Err(CliError::new(
            exit::ERROR,
            "from_agent",
            format!(
                "{what} must be done by a person in their own terminal, not from inside an agent session ({marker} is set)"
            ),
        )),
        None => Ok(()),
    }
}

fn token_hash(token: &str) -> String {
    evidence::hash_hex(token.trim().as_bytes())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// An authenticated agent, as the proxy acts for it.
#[derive(Debug, Clone)]
pub struct Authenticated {
    pub name: String,
    scopes: Option<GlobSet>,
}

impl Authenticated {
    /// Whether the agent may call `tool` (`server/tool`) at all.
    pub fn may_call(&self, tool: &str) -> bool {
        self.scopes.as_ref().is_none_or(|s| s.is_match(tool))
    }
}

/// Authenticates `token` against the project's agents. `Ok(None)` without a token.
pub fn authenticate(
    project: &Project,
    token: Option<&str>,
) -> Result<Option<Authenticated>, CliError> {
    let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let hash = token_hash(token);
    let registry = registry(project)?;
    let agent = registry
        .agents
        .iter()
        .find(|a| a.revoked_at.is_none() && a.token_hash == hash)
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "unknown_agent",
                format!("{TOKEN_ENV} is not a valid token for an active agent of this project"),
            )
        })?;
    let scopes = if agent.tools.is_empty() {
        None
    } else {
        Some(globs(&agent.tools).map_err(|e| CliError::new(exit::ERROR, "invalid_agents", e))?)
    };
    Ok(Some(Authenticated {
        name: agent.name.clone(),
        scopes,
    }))
}

/// Whether an agent with this name is registered and active.
pub fn is_registered(project: &Project, name: &str) -> Result<bool, CliError> {
    Ok(registry(project)?
        .agents
        .iter()
        .any(|a| a.name == name && a.revoked_at.is_none()))
}

fn print(json_output: bool, value: Value, human: &str) {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("output always serializes")
        );
    } else {
        println!("{human}");
    }
}

pub fn add_cmd(
    project: &Project,
    name: &str,
    tools: &[String],
    json_output: bool,
) -> Result<u8, CliError> {
    person_only("registering an agent")?;
    if !valid_name(name) {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_name",
            "agent names use lowercase letters, digits, - and _ (up to 64)",
        ));
    }
    globs(tools).map_err(|e| CliError::new(exit::ERROR, "invalid_arguments", e))?;
    let mut registry = registry(project)?;
    if registry
        .agents
        .iter()
        .any(|a| a.name == name && a.revoked_at.is_none())
    {
        return Err(CliError::new(
            exit::ERROR,
            "agent_exists",
            format!("agent {name} already exists; revoke it first to issue a new token"),
        ));
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| {
        CliError::new(
            exit::ERROR,
            "no_randomness",
            format!("cannot generate a token: {e}"),
        )
    })?;
    let token = format!("{TOKEN_PREFIX}{}", hex(&bytes));
    let agent = Agent {
        name: name.to_owned(),
        token_hash: token_hash(&token),
        tools: tools.to_vec(),
        created_by: evidence::os_user(),
        created_at: timefmt::now_rfc3339(),
        revoked_at: None,
    };
    registry.agents.push(agent.clone());
    save(project, &registry)?;
    evidence::append(
        project,
        json!({
            "kind": "agent_registered",
            "agent": agent.name,
            "tools": agent.tools,
            "token_hash": agent.token_hash,
            "by": agent.created_by,
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.agent.add/v1", "agent": agent.name, "tools": agent.tools, "token": token }),
        &format!(
            "Registered agent {name}. Its token (shown once; store it in the agent's gateway configuration as {TOKEN_ENV}):\n{token}"
        ),
    );
    Ok(exit::OK)
}

pub fn revoke_cmd(project: &Project, name: &str, json_output: bool) -> Result<u8, CliError> {
    person_only("revoking an agent")?;
    let mut registry = registry(project)?;
    let agent = registry
        .agents
        .iter_mut()
        .find(|a| a.name == name && a.revoked_at.is_none())
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "unknown_agent",
                format!("no active agent {name}; see `fabric agent list`"),
            )
        })?;
    agent.revoked_at = Some(timefmt::now_rfc3339());
    save(project, &registry)?;
    evidence::append(
        project,
        json!({ "kind": "agent_revoked", "agent": name, "by": evidence::os_user() }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.agent.revoke/v1", "agent": name }),
        &format!("Revoked agent {name}: its token no longer works."),
    );
    Ok(exit::OK)
}

pub fn require_cmd(project: &Project, on: bool, json_output: bool) -> Result<u8, CliError> {
    person_only("changing the agent requirement")?;
    let mut registry = registry(project)?;
    registry.required = on;
    save(project, &registry)?;
    evidence::append(
        project,
        json!({
            "kind": "policy_changed",
            "change": if on { "agent tokens required" } else { "agent tokens not required" },
            "by": evidence::os_user(),
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.agent.require/v1", "required": on }),
        if on {
            "Tool calls through the proxy now need a valid agent token."
        } else {
            "Agent tokens are no longer required."
        },
    );
    Ok(exit::OK)
}

pub fn list_cmd(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let registry = registry(project)?;
    let mut human = format!(
        "Agent tokens {}required.\n",
        if registry.required { "" } else { "not " }
    );
    for a in &registry.agents {
        human.push_str(&format!(
            "{} {} tools: {}  added {}\n",
            a.name,
            if a.revoked_at.is_some() {
                "revoked"
            } else {
                "active"
            },
            if a.tools.is_empty() {
                "any".to_owned()
            } else {
                a.tools.join(", ")
            },
            a.created_at
        ));
    }
    let agents: Vec<Value> = registry
        .agents
        .iter()
        .map(|a| json!({ "name": a.name, "tools": a.tools, "created_at": a.created_at, "revoked_at": a.revoked_at }))
        .collect();
    print(
        json_output,
        json!({ "schema": "fabric.agent.list/v1", "required": registry.required, "agents": agents }),
        human.trim_end(),
    );
    Ok(exit::OK)
}

// ---- Credential broker ------------------------------------------------------------------

pub fn secrets_dir(home: &Home) -> PathBuf {
    home.root.join("secrets")
}

fn secret_path(home: &Home, name: &str) -> Result<PathBuf, CliError> {
    if !valid_name(name) {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_name",
            "secret names use lowercase letters, digits, - and _ (up to 64)",
        ));
    }
    Ok(secrets_dir(home).join(name))
}

/// Stores a secret read from standard input (one line). Never echoed.
pub fn secret_set_cmd(home: &Home, name: &str, json_output: bool) -> Result<u8, CliError> {
    person_only("storing a credential")?;
    let path = secret_path(home, name)?;
    let mut value = String::new();
    io::stdin()
        .lock()
        .read_line(&mut value)
        .map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err(CliError::new(
            exit::ERROR,
            "empty_secret",
            "pipe the credential on standard input, e.g. `fabric secret set stripe-live < key.txt`",
        ));
    }
    let dir = secrets_dir(home);
    fs::create_dir_all(&dir).map_err(|e| CliError::io(&dir, e))?;
    write_private(&path, value.as_bytes())?;
    print(
        json_output,
        json!({ "schema": "fabric.secret.set/v1", "name": name }),
        &format!(
            "Stored credential {name}. Give it to an MCP server with `fabric mcp-proxy --secret-env VAR={name}`."
        ),
    );
    Ok(exit::OK)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let temp = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| CliError::io(&temp, e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| CliError::io(&temp, e))?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| CliError::io(path, e))
}

pub fn secret_remove_cmd(home: &Home, name: &str, json_output: bool) -> Result<u8, CliError> {
    person_only("removing a credential")?;
    let path = secret_path(home, name)?;
    fs::remove_file(&path).map_err(|e| CliError::io(&path, e))?;
    print(
        json_output,
        json!({ "schema": "fabric.secret.remove/v1", "name": name }),
        &format!("Removed credential {name}."),
    );
    Ok(exit::OK)
}

pub fn secret_list_cmd(home: &Home, json_output: bool) -> Result<u8, CliError> {
    let mut names: Vec<String> = match fs::read_dir(secrets_dir(home)) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_name(n))
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(CliError::io(&secrets_dir(home), e)),
    };
    names.sort();
    print(
        json_output,
        json!({ "schema": "fabric.secret.list/v1", "names": names }),
        &if names.is_empty() {
            "No credentials stored.".to_owned()
        } else {
            names.join("\n")
        },
    );
    Ok(exit::OK)
}

/// A stored credential's value, for the gateway's own use (never printed).
pub fn secret_value(home: &Home, name: &str) -> Result<String, CliError> {
    let path = secret_path(home, name)?;
    fs::read_to_string(&path)
        .map(|v| v.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| {
            CliError::new(
                exit::ERROR,
                "missing_secret",
                format!("no stored credential {name}; store it with `fabric secret set {name}`"),
            )
        })
}

/// Resolves `VAR=name` pairs into environment variables for an upstream server. A missing
/// credential is an error: the server is not started without it.
pub fn secret_env(home: &Home, pairs: &[String]) -> Result<Vec<(String, String)>, CliError> {
    pairs
        .iter()
        .map(|pair| {
            let (var, name) = pair.split_once('=').ok_or_else(|| {
                CliError::new(
                    exit::ERROR,
                    "invalid_arguments",
                    format!("--secret-env takes VAR=name, not {pair:?}"),
                )
            })?;
            let path = secret_path(home, name)?;
            let value = fs::read_to_string(&path).map_err(|_| {
                CliError::new(
                    exit::ERROR,
                    "missing_secret",
                    format!(
                        "no stored credential {name}; store it with `fabric secret set {name}`"
                    ),
                )
            })?;
            Ok((var.to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain() {
        assert!(valid_name("close-bot"));
        assert!(valid_name("stripe_live"));
        for bad in ["", "Close", "../x", "a/b", "a b", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn tokens_are_compared_by_hash() {
        assert_eq!(token_hash("fat_abc"), token_hash(" fat_abc\n"));
        assert_ne!(token_hash("fat_abc"), token_hash("fat_abd"));
    }
}
