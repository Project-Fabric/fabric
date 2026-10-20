//! The guard: agents must get a person's consent before destructive or controlled actions.
//!
//! Agents that support pre-action hooks (Claude Code's `PreToolUse`) ask Fabric about every
//! shell command and file write before running it. Each rule maps an action to a decision:
//!
//! - `allow`: Fabric stays silent and the agent's own permission system applies.
//! - `ask`: the agent pauses and asks the person to approve.
//! - `approve`: the action needs a *recorded* approval (`fabric approve`), given from the
//!   person's own terminal, outside the agent; see `approvals.rs`.
//! - `deny`: the action is blocked and the agent is told why.
//!
//! The strictest matching rule wins. Besides the built-in rules, a project can add its own
//! (`fabric guard add-rule`, or a pack such as `fabric guard pack finance`) that match file
//! paths by glob or commands by pattern. Rules and overrides live in Fabric's data directory,
//! outside the project; every `ask`, `approve` or `deny`, and every change to the rules, is
//! recorded in the guard log and the evidence chain.
//!
//! This is a guard against accidents and overreach, not a sandbox: it recognizes common
//! destructive commands, and a determined agent could phrase one in a way it does not
//! recognize, or change a file through a shell command that names no path. Patterns also
//! match inside quoted strings (so `python -c "shutil.rmtree(..)"` is caught), which means a
//! harmless `grep 'rm -rf'` asks too: for consent, a false alarm is the safer mistake.
//! Checkpoints remain the safety net, and evidence reports flag controlled files that changed
//! without an approval.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CliError, exit};
use crate::home::{Home, Project, display_path, project_relative, resolve_lenient, write_durably};
use crate::{approvals, evidence, timefmt};

const POLICY_FILE: &str = "guard-policy.json";
const AUDIT_FILE: &str = "guard-log.jsonl";
const POLICY_FORMAT: &str = "fabric-guard-policy";
/// Version 2 added project rules (`custom`). Version 1 files are still read.
const POLICY_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Ask,
    Approve,
    Deny,
}

impl Decision {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "approve" => Some(Self::Approve),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Approve => "approve",
            Self::Deny => "deny",
        }
    }
}

/// What a rule looks at.
enum Matcher {
    /// Shell commands matching any of these patterns (case-insensitive).
    Command(&'static [&'static str]),
    /// File writes whose project-relative path satisfies the predicate.
    Path(fn(&PathFacts) -> bool),
}

struct Rule {
    id: &'static str,
    description: &'static str,
    default: Decision,
    matcher: Matcher,
}

/// What the guard knows about a file an agent wants to write.
pub struct PathFacts {
    /// Relative to the project root with `/` separators, or `None` if outside the project.
    relative: Option<String>,
    /// Whether the path is inside Fabric's own data directory.
    in_fabric_home: bool,
    /// Whether the file is already committed to Git (or, outside a Git repository, already
    /// exists). Only worked out when a rule needs it.
    committed: Option<bool>,
}

impl PathFacts {
    fn name(&self) -> &str {
        self.relative
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or_default()
    }
}

const RULES: &[Rule] = &[
    Rule {
        id: "system-destroy",
        description: "Commands that can wipe disks or the whole system",
        default: Decision::Deny,
        matcher: Matcher::Command(&[
            r"\bmkfs(\.\w+)?\b",
            r"\bdd\b.*\bof=/dev/",
            r"\bformat\s+[a-z]:",
            r"\brm\s+(-\S+\s+)*(/|~|\$HOME|/\*|~/\*)(\s|$)",
            r"\b(Remove-Item|rm|del|rd|rmdir)\b.*\s[a-z]:\\?(\s|$|\*)",
            r":\(\)\s*\{.*\};\s*:",
            r"\bchmod\s+-R\s+777\s+/(\s|$)",
        ]),
    },
    Rule {
        id: "fabric-tamper",
        description: "Agents changing Fabric's history, rules or data",
        default: Decision::Deny,
        matcher: Matcher::Command(&[
            r"\bfabric(\.exe)?\s+(restore|revert|resume|gc|approve|review)\b",
            r"\bfabric(\.exe)?\s+guard\s+(set|pack|add-rule|remove-rule)\b",
            r"\bfabric(\.exe)?\s+integrate\b",
        ]),
    },
    Rule {
        id: "recursive-delete",
        description: "Deleting directories recursively",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\brm\s+(-\S*\s+)*-\S*[rR]",
            r"\brm\s+.*--recursive\b",
            r"\bRemove-Item\b.*-Recurse",
            r"\b(rmdir|rd)\s+/s\b",
            r"\bdel\s+.*/s\b",
            r"\bshutil\.rmtree\b",
            r"\bfind\b.*\s-delete\b",
            r"\bfs\.(rm|rmdir)(Sync)?\(.*recursive",
        ]),
    },
    Rule {
        id: "file-delete",
        description: "Deleting files",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"(^|[;&|]\s*)(rm|del|erase|unlink)\s",
            r"\bRemove-Item\b",
            r"\bgit\s+rm\b",
            r"\bos\.(remove|unlink)\(",
        ]),
    },
    Rule {
        id: "git-history-rewrite",
        description: "Discarding work or rewriting Git history",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\bgit\s+push\b.*(\s--force\b|\s-f\b|--force-with-lease)",
            r"\bgit\s+reset\s+.*--hard\b",
            r"\bgit\s+clean\s+.*-\S*f",
            r"\bgit\s+(checkout|restore)\s+(\S+\s+)*\.(\s|$)",
            r"\bgit\s+branch\s+.*-D\b",
            r"\bgit\s+stash\s+(drop|clear)\b",
            r"\bgit\s+(filter-branch|filter-repo|rebase)\b",
        ]),
    },
    Rule {
        id: "git-push",
        description: "Pushing code to a remote",
        default: Decision::Ask,
        matcher: Matcher::Command(&[r"\bgit\s+push\b"]),
    },
    Rule {
        id: "publish",
        description: "Publishing packages, images or releases",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\b(npm|pnpm|yarn)\s+publish\b",
            r"\bcargo\s+publish\b",
            r"\btwine\s+upload\b",
            r"\bgem\s+push\b",
            r"\bdocker\s+push\b",
            r"\bgh\s+release\s+create\b",
        ]),
    },
    Rule {
        id: "database-destructive",
        description: "Dropping, truncating or bulk-deleting database data",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\bdrop\s+(table|database|schema|collection)\b",
            r"\btruncate\s+(table\s+)?\w",
            r"\bdelete\s+from\b",
            r"\.drop\(\)",
            r"\bflushall\b",
        ]),
    },
    Rule {
        id: "infrastructure-destructive",
        description: "Destroying cloud or cluster resources",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\bterraform\s+(destroy|apply)\b",
            r"\bkubectl\s+delete\b",
            r"\bhelm\s+(uninstall|delete)\b",
            r"\b(aws|gcloud|az)\b.*\b(delete|terminate|remove)\b",
            r"\bdocker\s+(system\s+prune|volume\s+(rm|prune)|rm|rmi)\b",
        ]),
    },
    Rule {
        id: "remote-script",
        description: "Downloading and running a script in one step",
        default: Decision::Ask,
        matcher: Matcher::Command(&[
            r"\b(curl|wget|iwr|irm|Invoke-WebRequest|Invoke-RestMethod)\b[^|]*\|\s*(sudo\s+)?(sh|bash|zsh|python3?|iex|Invoke-Expression)\b",
        ]),
    },
    Rule {
        id: "git-internals",
        description: "Writing inside .git directly",
        default: Decision::Deny,
        matcher: Matcher::Path(|p| {
            p.relative
                .as_deref()
                .is_some_and(|r| r == ".git" || r.starts_with(".git/") || r.contains("/.git/"))
        }),
    },
    Rule {
        id: "fabric-data",
        description: "Writing inside Fabric's data directory",
        default: Decision::Deny,
        matcher: Matcher::Path(|p| p.in_fabric_home),
    },
    Rule {
        id: "outside-project",
        description: "Writing files outside the project",
        default: Decision::Ask,
        matcher: Matcher::Path(|p| p.relative.is_none() && !p.in_fabric_home),
    },
    Rule {
        id: "secrets",
        description: "Writing secret files (.env, keys)",
        default: Decision::Ask,
        matcher: Matcher::Path(|p| {
            let name = p.name();
            name == ".env"
                || name.starts_with(".env.")
                || name.ends_with(".pem")
                || name.ends_with(".key")
                || name.starts_with("id_rsa")
                || name.starts_with("id_ed25519")
        }),
    },
    Rule {
        id: "agent-config",
        description: "Changing agent or CI configuration (hooks, MCP, workflows)",
        default: Decision::Ask,
        matcher: Matcher::Path(|p| {
            p.relative.as_deref().is_some_and(|r| {
                r.starts_with(".claude/")
                    || r == ".mcp.json"
                    || r.starts_with(".codex/")
                    || r.starts_with(".cursor/")
                    || r.starts_with(".github/workflows/")
            })
        }),
    },
];

fn compiled() -> &'static Vec<Vec<Regex>> {
    static COMPILED: OnceLock<Vec<Vec<Regex>>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| match rule.matcher {
                Matcher::Command(patterns) => patterns
                    .iter()
                    .map(|p| command_regex(p).expect("built-in guard patterns are valid"))
                    .collect(),
                Matcher::Path(_) => Vec::new(),
            })
            .collect()
    })
}

fn command_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern).case_insensitive(true).build()
}

/// An action an agent wants to take.
pub enum Action<'a> {
    Command(&'a str),
    WriteFile(&'a str),
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub decision: Decision,
    /// The rule that decided, or `None` when nothing matched.
    pub rule: Option<String>,
    pub reason: String,
}

/// A rule a project adds for itself, e.g. "changes under `ledger/` need approval".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomRule {
    pub id: String,
    pub description: String,
    pub decision: Decision,
    /// Globs over project-relative paths, e.g. `src/billing/**`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Case-insensitive regular expressions over shell commands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// Match only files already committed to Git, such as applied migrations; new files
    /// pass. Outside a Git repository, files that already exist count as committed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub committed_only: bool,
}

struct CompiledRule {
    rule: CustomRule,
    paths: GlobSet,
    commands: Vec<Regex>,
}

impl CustomRule {
    fn compile(self) -> Result<CompiledRule, String> {
        let mut paths = GlobSetBuilder::new();
        for pattern in &self.paths {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| format!("invalid path pattern {pattern:?}: {e}"))?;
            paths.add(glob);
        }
        let paths = paths.build().map_err(|e| e.to_string())?;
        let commands = self
            .commands
            .iter()
            .map(|p| command_regex(p).map_err(|e| format!("invalid command pattern {p:?}: {e}")))
            .collect::<Result<_, _>>()?;
        Ok(CompiledRule {
            rule: self,
            paths,
            commands,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    format: String,
    version: u32,
    overrides: BTreeMap<String, Decision>,
    #[serde(default)]
    custom: Vec<CustomRule>,
}

/// Per-project overrides of the built-in decisions and project rules, kept outside the
/// project.
pub struct Policy {
    overrides: BTreeMap<String, Decision>,
    custom: Vec<CompiledRule>,
}

impl Policy {
    pub fn load(project: &Project) -> Result<Self, CliError> {
        let path = project.dir.join(POLICY_FILE);
        let invalid = |message: String| {
            CliError::new(
                exit::ERROR,
                "invalid_policy",
                format!("{}: {message}", display_path(&path)),
            )
        };
        let file = match fs::read(&path) {
            Ok(bytes) => {
                let file: PolicyFile =
                    serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
                if file.format != POLICY_FORMAT || !(1..=POLICY_VERSION).contains(&file.version) {
                    return Err(invalid("unsupported policy file".to_owned()));
                }
                file
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => PolicyFile {
                format: POLICY_FORMAT.to_owned(),
                version: POLICY_VERSION,
                overrides: BTreeMap::new(),
                custom: Vec::new(),
            },
            Err(e) => return Err(CliError::io(&path, e)),
        };
        let custom = file
            .custom
            .into_iter()
            .map(CustomRule::compile)
            .collect::<Result<_, _>>()
            .map_err(invalid)?;
        Ok(Self {
            overrides: file.overrides,
            custom,
        })
    }

    fn save(&self, project: &Project) -> Result<(), CliError> {
        let file = PolicyFile {
            format: POLICY_FORMAT.to_owned(),
            version: POLICY_VERSION,
            overrides: self.overrides.clone(),
            custom: self.custom.iter().map(|c| c.rule.clone()).collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file).expect("policies always serialize");
        write_durably(&project.dir.join(POLICY_FILE), &bytes)
    }

    fn decision(&self, rule: &Rule) -> Decision {
        self.overrides.get(rule.id).copied().unwrap_or(rule.default)
    }

    fn needs_commit_status(&self) -> bool {
        self.custom.iter().any(|c| c.rule.committed_only)
    }

    /// Decides on `action` for `project`. The strictest matching rule wins.
    pub fn evaluate(&self, home: &Home, project: &Project, action: &Action<'_>) -> Verdict {
        let facts = match action {
            Action::WriteFile(path) => {
                Some(path_facts(home, project, path, self.needs_commit_status()))
            }
            Action::Command(_) => None,
        };
        self.judge(action, facts.as_ref())
    }

    fn judge(&self, action: &Action<'_>, facts: Option<&PathFacts>) -> Verdict {
        let mut verdict = Verdict {
            decision: Decision::Allow,
            rule: None,
            reason: String::new(),
        };
        let mut consider = |decision: Decision, id: &str, reason: &str| {
            if verdict.rule.is_none() || decision > verdict.decision {
                verdict = Verdict {
                    decision,
                    rule: Some(id.to_owned()),
                    reason: reason.to_owned(),
                };
            }
        };
        for (rule, patterns) in RULES.iter().zip(compiled()) {
            let matched = match (&rule.matcher, action, facts) {
                (Matcher::Command(_), Action::Command(command), _) => {
                    patterns.iter().any(|p| p.is_match(command))
                }
                (Matcher::Path(test), Action::WriteFile(_), Some(facts)) => test(facts),
                _ => false,
            };
            if matched {
                consider(self.decision(rule), rule.id, rule.description);
            }
        }
        for custom in &self.custom {
            let matched = match (action, facts) {
                (Action::Command(command), _) => {
                    custom.commands.iter().any(|p| p.is_match(command))
                }
                (Action::WriteFile(_), Some(facts)) => {
                    facts
                        .relative
                        .as_deref()
                        .is_some_and(|r| custom.paths.is_match(r))
                        && (!custom.rule.committed_only || facts.committed.unwrap_or(true))
                }
                _ => false,
            };
            if matched {
                let rule = &custom.rule;
                consider(rule.decision, &rule.id, &rule.description);
            }
        }
        verdict
    }

    /// The project rules whose paths match `relative` and that control changes (approve or
    /// deny). Used by evidence reports to flag controlled files changed without approval.
    /// `committed` says whether the file was already committed before the change; it is asked
    /// only for rules that protect committed files, so that a new file such as a new
    /// migration is not mistaken for a change to an applied one.
    pub fn controlling_rules(
        &self,
        relative: &str,
        committed: &mut dyn FnMut() -> bool,
    ) -> Vec<&CustomRule> {
        let mut known = None;
        self.custom
            .iter()
            .filter(|c| c.rule.decision >= Decision::Approve && c.paths.is_match(relative))
            .filter(|c| !c.rule.committed_only || *known.get_or_insert_with(&mut *committed))
            .map(|c| &c.rule)
            .collect()
    }
}

fn path_facts(home: &Home, project: &Project, path: &str, need_commit_status: bool) -> PathFacts {
    let relative = project_relative(path, &project.root);
    let inside = !Path::new(&relative).is_absolute() && !relative.starts_with("../");
    let home_root = resolve_lenient(&home.root).unwrap_or_else(|| home.root.clone());
    let resolved = resolve_lenient(Path::new(path));
    let committed = (need_commit_status && inside).then(|| committed(&project.root, &relative));
    PathFacts {
        in_fabric_home: resolved.is_some_and(|r| r.starts_with(&home_root)),
        relative: inside.then_some(relative),
        committed,
    }
}

/// Whether `relative` is committed in the project's Git repository. Without Git (or outside a
/// repository) a file that exists counts as committed, the cautious answer for rules that
/// protect existing files.
pub fn committed(root: &Path, relative: &str) -> bool {
    let tracked = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--error-unmatch", "--", relative])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let in_repo = || {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    match tracked {
        Ok(status) if status.success() => true,
        Ok(_) if in_repo() => false,
        _ => root.join(relative).exists(),
    }
}

/// Handles Claude Code's `PreToolUse` event. Returns the JSON to print, or `None` to stay
/// silent and let Claude Code's own permission rules apply.
pub fn pre_tool_use(
    home: &Home,
    project: &Project,
    event: &Value,
) -> Result<Option<Value>, CliError> {
    let input = &event["tool_input"];
    let action = match event["tool_name"].as_str() {
        Some("Bash") => input["command"].as_str().map(Action::Command),
        Some("Write" | "Edit" | "MultiEdit") => input["file_path"].as_str().map(Action::WriteFile),
        Some("NotebookEdit") => input["notebook_path"].as_str().map(Action::WriteFile),
        _ => None,
    };
    let Some(action) = action else {
        return Ok(None);
    };
    let verdict = Policy::load(project)?.evaluate(home, project, &action);
    if verdict.decision == Decision::Allow {
        return Ok(None);
    }
    let (subject, target) = match action {
        Action::Command(c) => ("command", c.to_owned()),
        Action::WriteFile(p) => ("file", project_relative(p, &project.root)),
    };
    audit(project, event, &verdict, &target)?;
    let rule = verdict.rule.as_deref().unwrap_or_default();
    let reason = match verdict.decision {
        Decision::Approve => return approvals::gate(project, event, &verdict, &target),
        Decision::Deny => format!(
            "Blocked by Fabric guard ({rule}: {}). Do not try to work around this; ask the user if it is really needed.",
            verdict.reason
        ),
        _ => format!(
            "Fabric guard ({rule}): {} needs the user's approval. {subject}: {target}",
            verdict.reason
        ),
    };
    Ok(Some(hook_decision(verdict.decision.name(), &reason)))
}

pub fn hook_decision(decision: &str, reason: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    })
}

fn audit(
    project: &Project,
    event: &Value,
    verdict: &Verdict,
    target: &str,
) -> Result<(), CliError> {
    let path = project.dir.join(AUDIT_FILE);
    let entry = json!({
        "at": timefmt::now_rfc3339(),
        "agent": "claude-code",
        "session": event["session_id"],
        "tool": event["tool_name"],
        "target": target,
        "rule": verdict.rule,
        "decision": verdict.decision,
    });
    let mut line = entry.to_string();
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .map_err(|e| CliError::io(&path, e))?;
    evidence::append(
        project,
        json!({
            "kind": "guard",
            "decision": verdict.decision,
            "rule": verdict.rule,
            "agent": "claude-code",
            "session": event["session_id"],
            "tool": event["tool_name"],
            "target": evidence::clip(target),
        }),
    )?;
    Ok(())
}

fn policy_changed(project: &Project, change: &str, details: Value) -> Result<(), CliError> {
    evidence::append(
        project,
        json!({
            "kind": "policy_changed",
            "change": change,
            "by": evidence::os_user(),
            "details": details,
        }),
    )
    .map(drop)
}

// ---- `fabric guard` commands -------------------------------------------------------------

fn matcher_name(matcher: &Matcher) -> &'static str {
    match matcher {
        Matcher::Command(_) => "commands",
        Matcher::Path(_) => "file writes",
    }
}

fn custom_applies_to(rule: &CustomRule) -> &'static str {
    match (rule.paths.is_empty(), rule.commands.is_empty()) {
        (false, false) => "both",
        (false, true) => "file writes",
        _ => "commands",
    }
}

pub fn rules(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let policy = Policy::load(project)?;
    if json_output {
        let mut rows: Vec<Value> = RULES
            .iter()
            .map(|rule| {
                json!({
                    "rule": rule.id,
                    "source": "built_in",
                    "description": rule.description,
                    "decision": policy.decision(rule),
                    "default": rule.default,
                    "applies_to": matcher_name(&rule.matcher),
                })
            })
            .collect();
        rows.extend(policy.custom.iter().map(|c| {
            json!({
                "rule": c.rule.id,
                "source": "project",
                "description": c.rule.description,
                "decision": c.rule.decision,
                "applies_to": custom_applies_to(&c.rule),
                "paths": c.rule.paths,
                "commands": c.rule.commands,
                "committed_only": c.rule.committed_only,
            })
        }));
        print_json(&json!({ "schema": "fabric.guard.rules/v2", "rules": rows }));
        return Ok(exit::OK);
    }
    let mut out = String::from("Rule                         Decision  Applies to   Description\n");
    for rule in RULES {
        let decision = policy.decision(rule);
        let changed = if decision == rule.default { "" } else { "*" };
        let _ = writeln!(
            out,
            "{:<28} {:<9} {:<12} {}",
            rule.id,
            format!("{}{changed}", decision.name()),
            matcher_name(&rule.matcher),
            rule.description
        );
    }
    for c in &policy.custom {
        let _ = writeln!(
            out,
            "{:<28} {:<9} {:<12} {} (project rule)",
            c.rule.id,
            c.rule.decision.name(),
            custom_applies_to(&c.rule),
            c.rule.description
        );
        for pattern in c.rule.paths.iter().chain(&c.rule.commands) {
            let _ = writeln!(out, "{:<51} {pattern}", "");
        }
    }
    out.push_str("* changed from the default for this project\n");
    print!("{out}");
    Ok(exit::OK)
}

pub fn check(
    home: &Home,
    project: &Project,
    command: Option<&str>,
    file: Option<&str>,
    json_output: bool,
) -> Result<u8, CliError> {
    let action = match (command, file) {
        (Some(c), None) => Action::Command(c),
        (None, Some(f)) => Action::WriteFile(f),
        _ => {
            return Err(CliError::new(
                exit::ERROR,
                "invalid_arguments",
                "give exactly one of a command or --file PATH",
            ));
        }
    };
    let verdict = Policy::load(project)?.evaluate(home, project, &action);
    if json_output {
        print_json(&json!({ "schema": "fabric.guard.check/v2", "verdict": verdict }));
    } else {
        match &verdict.rule {
            Some(rule) => println!("{}  ({rule}: {})", verdict.decision.name(), verdict.reason),
            None => println!("allow  (no rule matched)"),
        }
    }
    Ok(exit::OK)
}

pub fn parse_decision(text: &str) -> Result<Decision, CliError> {
    Decision::parse(text).ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "decision must be allow, ask, approve or deny",
        )
    })
}

pub fn set(
    project: &Project,
    rule_id: &str,
    decision: &str,
    json_output: bool,
) -> Result<u8, CliError> {
    let decision = parse_decision(decision)?;
    let mut policy = Policy::load(project)?;
    if let Some(rule) = RULES.iter().find(|r| r.id == rule_id) {
        if decision == rule.default {
            policy.overrides.remove(rule.id);
        } else {
            policy.overrides.insert(rule.id.to_owned(), decision);
        }
    } else if let Some(custom) = policy.custom.iter_mut().find(|c| c.rule.id == rule_id) {
        custom.rule.decision = decision;
    } else {
        return Err(CliError::new(
            exit::ERROR,
            "unknown_rule",
            format!("no rule {rule_id:?}; see `fabric guard rules`"),
        ));
    }
    policy.save(project)?;
    policy_changed(
        project,
        "set",
        json!({ "rule": rule_id, "decision": decision }),
    )?;
    if json_output {
        print_json(
            &json!({ "schema": "fabric.guard.set/v2", "rule": rule_id, "decision": decision }),
        );
    } else {
        println!("{rule_id}: {} for this project", decision.name());
    }
    Ok(exit::OK)
}

/// Adds `rule`, replacing a project rule with the same id. Returns whether it replaced one.
fn put_rule(policy: &mut Policy, rule: CustomRule) -> Result<bool, CliError> {
    let valid_id = !rule.id.is_empty()
        && rule
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid_id {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "rule ids use lowercase letters, digits and hyphens",
        ));
    }
    if RULES.iter().any(|r| r.id == rule.id) {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            format!("{} is a built-in rule; use `fabric guard set`", rule.id),
        ));
    }
    if rule.paths.is_empty() && rule.commands.is_empty() {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "a rule needs at least one --path or --command pattern",
        ));
    }
    let compiled = rule
        .compile()
        .map_err(|e| CliError::new(exit::ERROR, "invalid_arguments", e))?;
    if let Some(existing) = policy
        .custom
        .iter_mut()
        .find(|c| c.rule.id == compiled.rule.id)
    {
        *existing = compiled;
        return Ok(true);
    }
    policy.custom.push(compiled);
    Ok(false)
}

pub fn add_rule(project: &Project, rule: CustomRule, json_output: bool) -> Result<u8, CliError> {
    let mut policy = Policy::load(project)?;
    let details = serde_json::to_value(&rule).expect("rules always serialize");
    let id = rule.id.clone();
    let replaced = put_rule(&mut policy, rule)?;
    policy.save(project)?;
    policy_changed(project, "add_rule", details)?;
    if json_output {
        print_json(
            &json!({ "schema": "fabric.guard.add_rule/v1", "rule": id, "replaced": replaced }),
        );
    } else {
        println!(
            "{} project rule {id}",
            if replaced { "Updated" } else { "Added" }
        );
    }
    Ok(exit::OK)
}

pub fn remove_rule(project: &Project, id: &str, json_output: bool) -> Result<u8, CliError> {
    let mut policy = Policy::load(project)?;
    let before = policy.custom.len();
    policy.custom.retain(|c| c.rule.id != id);
    if policy.custom.len() == before {
        return Err(CliError::new(
            exit::ERROR,
            "unknown_rule",
            format!("no project rule {id:?}; see `fabric guard rules`"),
        ));
    }
    policy.save(project)?;
    policy_changed(project, "remove_rule", json!({ "rule": id }))?;
    if json_output {
        print_json(&json!({ "schema": "fabric.guard.remove_rule/v1", "rule": id }));
    } else {
        println!("Removed project rule {id}");
    }
    Ok(exit::OK)
}

/// Default SOX scope when `fabric guard pack finance` is given no `--scope`.
const FINANCE_DEFAULT_SCOPE: &[&str] = &[
    "**/ledger/**",
    "**/billing/**",
    "**/payments/**",
    "**/revenue/**",
    "**/accounting/**",
    "**/invoicing/**",
];

/// Rules for code on financially significant systems (SOX in-scope code).
pub fn finance_pack(scope: &[String]) -> Vec<CustomRule> {
    let scope = if scope.is_empty() {
        FINANCE_DEFAULT_SCOPE
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    } else {
        scope.to_vec()
    };
    let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    vec![
        CustomRule {
            id: "finance-in-scope-code".to_owned(),
            description: "Changing SOX in-scope code needs a recorded approval".to_owned(),
            decision: Decision::Approve,
            paths: scope,
            commands: Vec::new(),
            committed_only: false,
        },
        CustomRule {
            id: "finance-applied-migrations".to_owned(),
            description:
                "Editing a committed migration (applied migrations never change; add a new one)"
                    .to_owned(),
            decision: Decision::Deny,
            paths: strings(&["**/migrations/**", "**/migrate/**"]),
            commands: Vec::new(),
            committed_only: true,
        },
        CustomRule {
            id: "finance-ledger-sql".to_owned(),
            description: "Changing financial tables directly with SQL".to_owned(),
            decision: Decision::Approve,
            paths: Vec::new(),
            commands: strings(&[
                r#"\b(update|delete\s+from|insert\s+into|truncate(\s+table)?|alter\s+table|drop\s+table)\s+["`\[]?\w*(journal|ledger|invoice|payment|refund|account|period|revenue|balance)"#,
            ]),
            committed_only: false,
        },
        CustomRule {
            id: "finance-controls".to_owned(),
            description: "Changing finance controls, code ownership or control tests".to_owned(),
            decision: Decision::Approve,
            paths: strings(&["**/controls/**", "CODEOWNERS", "**/CODEOWNERS"]),
            commands: Vec::new(),
            committed_only: false,
        },
        CustomRule {
            id: "finance-production-config".to_owned(),
            description: "Writing production configuration or credentials".to_owned(),
            decision: Decision::Deny,
            paths: strings(&[
                "**/*prod*.env",
                "**/.env.prod*",
                "**/config/prod*",
                "**/secrets/**",
            ]),
            commands: Vec::new(),
            committed_only: false,
        },
    ]
}

pub fn pack(
    project: &Project,
    name: &str,
    scope: &[String],
    json_output: bool,
) -> Result<u8, CliError> {
    let rules = match name {
        "finance" => finance_pack(scope),
        _ => {
            return Err(CliError::new(
                exit::ERROR,
                "unknown_pack",
                format!("no rule pack {name:?}; available: finance"),
            ));
        }
    };
    let mut policy = Policy::load(project)?;
    let ids: Vec<String> = rules.iter().map(|r| r.id.clone()).collect();
    let details = serde_json::to_value(&rules).expect("rules always serialize");
    for rule in rules {
        put_rule(&mut policy, rule)?;
    }
    policy.save(project)?;
    policy_changed(project, "pack", json!({ "pack": name, "rules": details }))?;
    if json_output {
        print_json(&json!({ "schema": "fabric.guard.pack/v1", "pack": name, "rules": ids }));
    } else {
        println!("Installed the {name} rule pack for this project:");
        for id in ids {
            println!("  {id}");
        }
        println!("See `fabric guard rules`; change any rule with `fabric guard set`.");
    }
    Ok(exit::OK)
}

pub fn log(project: &Project, limit: usize, json_output: bool) -> Result<u8, CliError> {
    let path = project.dir.join(AUDIT_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    let entries: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let shown: Vec<&Value> = entries.iter().rev().take(limit).collect();
    if json_output {
        print_json(
            &json!({ "schema": "fabric.guard.log/v2", "total": entries.len(), "entries": shown }),
        );
    } else if shown.is_empty() {
        println!("No actions have needed approval or been blocked yet");
    } else {
        for e in shown {
            println!(
                "{}  {:<7} {:<26} {}  {}",
                e["at"].as_str().unwrap_or_default(),
                e["decision"].as_str().unwrap_or_default(),
                e["rule"].as_str().unwrap_or_default(),
                e["tool"].as_str().unwrap_or_default(),
                e["target"].as_str().unwrap_or_default(),
            );
        }
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

    fn defaults() -> Policy {
        Policy {
            overrides: Default::default(),
            custom: Vec::new(),
        }
    }

    fn decide(command: &str) -> (Decision, Option<&'static str>) {
        decide_with(&defaults(), command)
    }

    fn decide_with(policy: &Policy, command: &str) -> (Decision, Option<&'static str>) {
        let verdict = policy.judge(&Action::Command(command), None);
        (verdict.decision, verdict.rule.map(|r| &*r.leak()))
    }

    fn decide_write(
        relative: Option<&str>,
        in_fabric_home: bool,
    ) -> (Decision, Option<&'static str>) {
        decide_write_with(&defaults(), relative, in_fabric_home, None)
    }

    fn decide_write_with(
        policy: &Policy,
        relative: Option<&str>,
        in_fabric_home: bool,
        committed: Option<bool>,
    ) -> (Decision, Option<&'static str>) {
        let facts = PathFacts {
            relative: relative.map(str::to_owned),
            in_fabric_home,
            committed,
        };
        let verdict = policy.judge(&Action::WriteFile("unused"), Some(&facts));
        (verdict.decision, verdict.rule.map(|r| &*r.leak()))
    }

    #[test]
    fn file_writes_are_judged_by_where_they_land() {
        assert_eq!(
            decide_write(Some("src/main.rs"), false),
            (Decision::Allow, None)
        );
        assert_eq!(
            decide_write(Some("docs/env.md"), false),
            (Decision::Allow, None)
        );
        assert_eq!(
            decide_write(None, false),
            (Decision::Ask, Some("outside-project"))
        );
        assert_eq!(
            decide_write(Some(".env"), false),
            (Decision::Ask, Some("secrets"))
        );
        assert_eq!(
            decide_write(Some("config/.env.production"), false),
            (Decision::Ask, Some("secrets"))
        );
        assert_eq!(
            decide_write(Some("certs/server.pem"), false),
            (Decision::Ask, Some("secrets"))
        );
        assert_eq!(
            decide_write(Some(".claude/settings.local.json"), false),
            (Decision::Ask, Some("agent-config"))
        );
        assert_eq!(
            decide_write(Some(".github/workflows/ci.yml"), false),
            (Decision::Ask, Some("agent-config"))
        );
        assert_eq!(
            decide_write(Some(".git/config"), false),
            (Decision::Deny, Some("git-internals"))
        );
        assert_eq!(
            decide_write(None, true),
            (Decision::Deny, Some("fabric-data"))
        );
    }

    #[test]
    fn overrides_change_a_rules_decision() {
        let mut policy = defaults();
        policy
            .overrides
            .insert("git-push".to_owned(), Decision::Allow);
        let verdict = policy.judge(&Action::Command("git push origin main"), None);
        assert_eq!(verdict.decision, Decision::Allow);
        // Other rules are unaffected.
        let verdict = policy.judge(&Action::Command("git push --force"), None);
        assert_eq!(verdict.decision, Decision::Ask);
    }

    #[test]
    fn everyday_commands_are_left_alone() {
        for command in [
            "git status",
            "git diff HEAD~1",
            "git commit -m 'remove old code'",
            "git checkout -b feature",
            "npm test",
            "cargo build --release",
            "pytest -x tests/",
            "ls -la",
            "python -m unittest -q",
            "cat format.txt",
            "echo deleted",
            "docker build -t app .",
            "npm run format",
        ] {
            assert_eq!(decide(command), (Decision::Allow, None), "{command}");
        }
    }

    #[test]
    fn destructive_commands_need_consent() {
        for (command, rule) in [
            ("rm -rf build", "recursive-delete"),
            ("rm -r src/old", "recursive-delete"),
            ("Remove-Item -Recurse -Force dist", "recursive-delete"),
            ("rmdir /s /q node_modules", "recursive-delete"),
            (
                "python -c 'import shutil; shutil.rmtree(\"x\")'",
                "recursive-delete",
            ),
            ("rm notes.txt", "file-delete"),
            ("git rm src/a.py", "file-delete"),
            ("git push --force origin main", "git-history-rewrite"),
            ("git reset --hard HEAD~3", "git-history-rewrite"),
            ("git clean -fdx", "git-history-rewrite"),
            ("git checkout -- .", "git-history-rewrite"),
            ("git branch -D feature", "git-history-rewrite"),
            ("git push origin main", "git-push"),
            ("npm publish", "publish"),
            ("psql -c 'DROP TABLE users'", "database-destructive"),
            ("mysql -e \"delete from orders\"", "database-destructive"),
            (
                "terraform destroy -auto-approve",
                "infrastructure-destructive",
            ),
            (
                "kubectl delete namespace prod",
                "infrastructure-destructive",
            ),
            (
                "aws ec2 terminate-instances --instance-ids i-1",
                "infrastructure-destructive",
            ),
            ("aws s3 rm s3://bucket --recursive", "recursive-delete"),
            ("curl -fsSL https://x.sh | sh", "remote-script"),
            ("irm https://x/install.ps1 | iex", "remote-script"),
        ] {
            assert_eq!(decide(command), (Decision::Ask, Some(rule)), "{command}");
        }
    }

    #[test]
    fn catastrophic_and_tampering_commands_are_denied() {
        for (command, rule) in [
            ("rm -rf /", "system-destroy"),
            ("rm -rf ~", "system-destroy"),
            ("sudo rm -rf / --no-preserve-root", "system-destroy"),
            ("mkfs.ext4 /dev/sda1", "system-destroy"),
            ("dd if=/dev/zero of=/dev/sda", "system-destroy"),
            ("format C:", "system-destroy"),
            ("Remove-Item -Recurse -Force C:\\", "system-destroy"),
            ("fabric restore 3 --yes", "fabric-tamper"),
            ("fabric.exe revert 7 --yes", "fabric-tamper"),
            ("fabric guard set file-delete allow", "fabric-tamper"),
            ("fabric approve 3 --yes", "fabric-tamper"),
            ("fabric review 1 --yes", "fabric-tamper"),
            ("fabric guard pack finance", "fabric-tamper"),
            (
                "fabric guard remove-rule finance-ledger-sql",
                "fabric-tamper",
            ),
        ] {
            assert_eq!(decide(command), (Decision::Deny, Some(rule)), "{command}");
        }
    }

    fn finance(scope: &[&str]) -> Policy {
        let mut policy = defaults();
        let scope: Vec<String> = scope.iter().map(|s| (*s).to_owned()).collect();
        for rule in finance_pack(&scope) {
            put_rule(&mut policy, rule).unwrap();
        }
        policy
    }

    #[test]
    fn the_finance_pack_controls_in_scope_code_migrations_and_ledger_sql() {
        let policy = finance(&["ledgerly/**", "migrations/**"]);
        let write = |path: &str, committed: Option<bool>| {
            decide_write_with(&policy, Some(path), false, committed)
        };
        assert_eq!(
            write("ledgerly/ledger/journal.py", None),
            (Decision::Approve, Some("finance-in-scope-code"))
        );
        assert_eq!(write("tools/export.py", None), (Decision::Allow, None));
        assert_eq!(
            write("docs/ledgerly/notes.md", None),
            (Decision::Allow, None)
        );
        // A committed migration is denied (deny is stricter than the scope's approve)...
        assert_eq!(
            write("migrations/0001_ledger.sql", Some(true)),
            (Decision::Deny, Some("finance-applied-migrations"))
        );
        // ...a new one only needs the in-scope approval.
        assert_eq!(
            write("migrations/0004_late_fees.sql", Some(false)),
            (Decision::Approve, Some("finance-in-scope-code"))
        );
        assert_eq!(
            write("config/production.yaml", None),
            (Decision::Deny, Some("finance-production-config"))
        );
        assert_eq!(
            write("controls/test_sox.py", None),
            (Decision::Approve, Some("finance-controls"))
        );
        for command in [
            "sqlite3 books.db \"UPDATE journal_lines SET debit = 0\"",
            "psql -c 'delete from invoices where id = 4'",
            "psql -c \"INSERT INTO payments VALUES (1)\"",
        ] {
            assert_eq!(
                decide_with(&policy, command),
                (Decision::Approve, Some("finance-ledger-sql")),
                "{command}"
            );
        }
        // Reading financial tables is not controlled.
        assert_eq!(
            decide_with(&policy, "sqlite3 books.db 'select * from journal_entries'"),
            (Decision::Allow, None)
        );
        assert_eq!(
            policy
                .controlling_rules("ledgerly/money.py", &mut || true)
                .len(),
            1
        );
        assert!(
            policy
                .controlling_rules("README.md", &mut || true)
                .is_empty()
        );
        // The rule for applied migrations controls committed ones only.
        let ids = |committed: bool| -> Vec<String> {
            policy
                .controlling_rules("migrations/0001.sql", &mut || committed)
                .iter()
                .map(|r| r.id.clone())
                .collect()
        };
        assert!(ids(true).contains(&"finance-applied-migrations".to_owned()));
        assert!(!ids(false).contains(&"finance-applied-migrations".to_owned()));
    }

    #[test]
    fn project_rules_are_validated() {
        let mut policy = defaults();
        let rule = |id: &str, paths: &[&str], commands: &[&str]| CustomRule {
            id: id.to_owned(),
            description: "test".to_owned(),
            decision: Decision::Approve,
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            commands: commands.iter().map(|s| (*s).to_owned()).collect(),
            committed_only: false,
        };
        assert!(put_rule(&mut policy, rule("git-push", &["x"], &[])).is_err());
        assert!(put_rule(&mut policy, rule("Bad Id", &["x"], &[])).is_err());
        assert!(put_rule(&mut policy, rule("empty", &[], &[])).is_err());
        assert!(put_rule(&mut policy, rule("bad-regex", &[], &["("])).is_err());
        assert!(!put_rule(&mut policy, rule("billing", &["src/billing/**"], &[])).unwrap());
        assert!(put_rule(&mut policy, rule("billing", &["lib/billing/**"], &[])).unwrap());
        assert_eq!(policy.custom.len(), 1);
    }
}
