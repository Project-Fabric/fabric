//! Sign-off by another person: approvals and reviews signed with the approver's own key.
//!
//! A local `fabric approve` shows only that someone at the developer's machine approved.
//! Segregation of duties needs more: the approver is a different person, with their own key,
//! possibly on another machine. The flow, without any server:
//!
//! 1. The project registers the approvers it trusts: `fabric approver add NAME --key PUBKEY`.
//!    Each approver's key lives in their own Fabric installation (`fabric approver key`).
//! 2. The developer exports what needs a decision: `fabric approvals export N` (held requests)
//!    or `fabric review N --export` (a change's result). The file holds the exact actions or
//!    the content ids under review.
//! 3. The approver signs a decision on their machine: `fabric approver sign FILE`. Their
//!    Fabric recomputes each action's digest from the exact input before showing it, so what
//!    they sign is what they saw.
//! 4. The developer applies it: `fabric approve --signed FILE` or `fabric review N --signed
//!    FILE`. Fabric checks the signature, that the key is registered for this project, and that
//!    the requests or the reviewed content still match exactly.
//!
//! With `fabric approver require on`, unsigned approvals and reviews are refused, and so are
//! decisions signed with this installation's own approver key: the approver must hold a key
//! from another installation. Every signed decision is kept in the evidence with its payload
//! and signature, so `fabric evidence verify` and `verify-pack` re-check them offline.
//!
//! What a signature shows: the holder of a registered key made this decision about exactly
//! this content. Who holds a key is a matter of how keys are handed out; registering one is
//! recorded in the evidence with who did it.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write as _};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::approvals::{digest, in_agent_environment};
use crate::error::{CliError, exit};
use crate::evidence::{self, Record};
use crate::home::{Home, Project, display_path, write_durably};
use crate::pack::{fingerprint, hex, installation_key, unhex};
use crate::timefmt;

const FILE: &str = "approvers.json";
const FORMAT: &str = "fabric-approvers";
const VERSION: u32 = 1;
const KEY_FILE: &str = "approver-ed25519.key";
pub const REQUEST_FORMAT: &str = "fabric-approval-request";
pub const REVIEW_REQUEST_FORMAT: &str = "fabric-review-request";
const DECISION_FORMAT: &str = "fabric-signed-decision";
const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approver {
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub added_by: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    /// Approvals and reviews must be signed by a registered approver from another installation.
    pub required: bool,
    pub approvers: Vec<Approver>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    format: String,
    version: u32,
    required: bool,
    approvers: Vec<Approver>,
}

pub fn registry(project: &Project) -> Result<Registry, CliError> {
    let path = project.dir.join(FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let file: RegistryFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_approvers",
                    format!("{}: {e}", display_path(&path)),
                )
            })?;
            if file.format != FORMAT || file.version != VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_approvers",
                    format!("{}: unsupported approvers file", display_path(&path)),
                ));
            }
            Ok(Registry {
                required: file.required,
                approvers: file.approvers,
            })
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(CliError::io(&path, e)),
    }
}

fn save(project: &Project, registry: &Registry) -> Result<(), CliError> {
    let file = RegistryFile {
        format: FORMAT.to_owned(),
        version: VERSION,
        required: registry.required,
        approvers: registry.approvers.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&file).expect("approvers always serialize");
    write_durably(&project.dir.join(FILE), &bytes)
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

fn parse_key(text: &str) -> Result<VerifyingKey, CliError> {
    unhex::<32>(text)
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "invalid_key",
                "not an Ed25519 public key (64 hex characters, as `fabric approver key` prints)",
            )
        })
}

/// Unsigned approvals and reviews are refused when the project requires signed ones.
pub fn refuse_unsigned(project: &Project, what: &str) -> Result<(), CliError> {
    if registry(project)?.required {
        return Err(CliError::new(
            exit::ERROR,
            "signature_required",
            format!(
                "this project requires {what} signed by a registered approver: export the request, have the approver run `fabric approver sign` on their own machine, then apply it with --signed"
            ),
        ));
    }
    Ok(())
}

// ---- `fabric approver ...` ----------------------------------------------------------------

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

pub fn key_cmd(home: &Home, json_output: bool) -> Result<u8, CliError> {
    let key = installation_key(home, KEY_FILE, true)?
        .ok_or_else(|| CliError::new(exit::ERROR, "no_key", "the key could not be created"))?
        .verifying_key();
    print(
        json_output,
        json!({
            "schema": "fabric.approver.key/v1",
            "algorithm": "ed25519",
            "public_key": hex(key.as_bytes()),
            "fingerprint": fingerprint(&key),
        }),
        &format!(
            "Approver key fingerprint: {}\nPublic key: {}\nGive the public key to the project owner: fabric approver add <your name> --key {}",
            fingerprint(&key),
            hex(key.as_bytes()),
            hex(key.as_bytes())
        ),
    );
    Ok(exit::OK)
}

pub fn add_cmd(
    project: &Project,
    name: &str,
    key: &str,
    json_output: bool,
) -> Result<u8, CliError> {
    person_only("registering an approver")?;
    let key = parse_key(key)?;
    if name.trim().is_empty() {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_name",
            "an approver needs a name",
        ));
    }
    let mut registry = registry(project)?;
    let public_key = hex(key.as_bytes());
    if registry
        .approvers
        .iter()
        .any(|a| a.name == name || a.public_key == public_key)
    {
        return Err(CliError::new(
            exit::ERROR,
            "approver_exists",
            "an approver with this name or key is already registered; remove it first",
        ));
    }
    let approver = Approver {
        name: name.to_owned(),
        public_key,
        fingerprint: fingerprint(&key),
        added_by: evidence::os_user(),
        added_at: timefmt::now_rfc3339(),
    };
    registry.approvers.push(approver.clone());
    save(project, &registry)?;
    evidence::append(
        project,
        json!({
            "kind": "approver_added",
            "name": approver.name,
            "public_key": approver.public_key,
            "fingerprint": approver.fingerprint,
            "by": approver.added_by,
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.approver.add/v1", "approver": approver }),
        &format!(
            "Registered approver {} (key {}).",
            approver.name, approver.fingerprint
        ),
    );
    Ok(exit::OK)
}

pub fn remove_cmd(project: &Project, name: &str, json_output: bool) -> Result<u8, CliError> {
    person_only("removing an approver")?;
    let mut registry = registry(project)?;
    let Some(index) = registry.approvers.iter().position(|a| a.name == name) else {
        return Err(CliError::new(
            exit::ERROR,
            "unknown_approver",
            format!("no approver named {name:?}; see `fabric approver list`"),
        ));
    };
    let removed = registry.approvers.remove(index);
    save(project, &registry)?;
    evidence::append(
        project,
        json!({
            "kind": "approver_removed",
            "name": removed.name,
            "public_key": removed.public_key,
            "fingerprint": removed.fingerprint,
            "by": evidence::os_user(),
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.approver.remove/v1", "approver": removed }),
        &format!("Removed approver {}.", removed.name),
    );
    Ok(exit::OK)
}

pub fn require_cmd(project: &Project, on: bool, json_output: bool) -> Result<u8, CliError> {
    person_only("changing the approval requirement")?;
    let mut registry = registry(project)?;
    if on && registry.approvers.is_empty() {
        return Err(CliError::new(
            exit::ERROR,
            "no_approvers",
            "register at least one approver first (`fabric approver add`)",
        ));
    }
    registry.required = on;
    save(project, &registry)?;
    evidence::append(
        project,
        json!({
            "kind": "policy_changed",
            "change": if on { "signed approvals required" } else { "signed approvals not required" },
            "by": evidence::os_user(),
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.approver.require/v1", "required": on }),
        if on {
            "Approvals and reviews must now be signed by a registered approver from another installation."
        } else {
            "Signed approvals are no longer required."
        },
    );
    Ok(exit::OK)
}

pub fn list_cmd(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let registry = registry(project)?;
    let mut human = format!(
        "Signed approvals {}required.\n",
        if registry.required { "" } else { "not " }
    );
    if registry.approvers.is_empty() {
        human.push_str("No approvers registered.");
    }
    for a in &registry.approvers {
        human.push_str(&format!(
            "{}  key {}  added {} by {}\n",
            a.name,
            a.fingerprint,
            a.added_at,
            a.added_by.as_deref().unwrap_or("-")
        ));
    }
    print(
        json_output,
        json!({ "schema": "fabric.approver.list/v1", "required": registry.required, "approvers": registry.approvers }),
        human.trim_end(),
    );
    Ok(exit::OK)
}

// ---- Signing, on the approver's machine ----------------------------------------------------

pub struct SignOptions<'a> {
    pub reject: bool,
    pub change: Option<&'a str>,
    pub paths: &'a [String],
    pub note: Option<&'a str>,
    pub yes: bool,
    pub out: Option<&'a Path>,
}

fn read_json(path: &Path) -> Result<Value, CliError> {
    let bytes = fs::read(path).map_err(|e| CliError::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        CliError::new(
            exit::ERROR,
            "invalid_file",
            format!("{}: {e}", display_path(path)),
        )
    })
}

fn invalid(message: impl Into<String>) -> CliError {
    CliError::new(exit::ERROR, "invalid_request", message)
}

pub fn sign_cmd(
    home: &Home,
    file: &Path,
    o: &SignOptions<'_>,
    json_output: bool,
) -> Result<u8, CliError> {
    person_only("signing a decision")?;
    let request = read_json(file)?;
    if request["version"] != FILE_VERSION {
        return Err(invalid("unsupported request file version"));
    }
    let project = request["project"]["id"]
        .as_str()
        .ok_or_else(|| invalid("the request names no project"))?
        .to_owned();
    let (payload, shown, what) = match request["format"].as_str() {
        Some(REQUEST_FORMAT) => {
            let mut shown = String::new();
            let mut requests = Vec::new();
            for r in request["requests"].as_array().into_iter().flatten() {
                let tool = r["tool"].as_str().unwrap_or_default();
                // What is signed is the digest of the exact input shown here, recomputed.
                let recomputed = digest(tool, &r["input"]);
                if r["digest"].as_str() != Some(recomputed.as_str()) {
                    return Err(invalid(format!(
                        "request #{} does not match its own digest; it was changed after export",
                        r["id"]
                    )));
                }
                shown.push_str(&format!(
                    "Approval request #{} ({}: {})\nAgent {} wants to run {} on {}\nRequested {}\n\n{}\n",
                    r["id"],
                    r["rule"].as_str().unwrap_or_default(),
                    r["reason"].as_str().unwrap_or_default(),
                    r["agent"].as_str().unwrap_or_default(),
                    tool,
                    r["target"].as_str().unwrap_or_default(),
                    r["requested_at"].as_str().unwrap_or_default(),
                    r["change_text"].as_str().unwrap_or_default(),
                ));
                requests.push(json!({ "id": r["id"], "digest": recomputed }));
            }
            if requests.is_empty() {
                return Err(invalid("the request file holds no requests"));
            }
            if o.change.is_some() && o.reject {
                return Err(invalid(
                    "--change authorizes work; it cannot be used with --reject",
                ));
            }
            if o.change.is_none() && !o.paths.is_empty() {
                return Err(invalid("--path is only used with --change"));
            }
            let change = o
                .change
                .map(|title| json!({ "title": title, "paths": o.paths }));
            if let Some(title) = o.change {
                shown.push_str(&format!(
                    "This also authorizes change \"{title}\" for {}.\n",
                    if o.paths.is_empty() {
                        "the requested files".to_owned()
                    } else {
                        o.paths.join(", ")
                    }
                ));
            }
            let what = if requests.len() == 1 {
                "this change".to_owned()
            } else {
                format!("these {} changes", requests.len())
            };
            (
                json!({
                    "kind": "approval",
                    "project": project,
                    "requests": requests,
                    "decision": if o.reject { "reject" } else { "approve" },
                    "change": change,
                    "note": o.note,
                    "signed_at": timefmt::now_rfc3339(),
                }),
                shown,
                what,
            )
        }
        Some(REVIEW_REQUEST_FORMAT) => {
            if o.change.is_some() || !o.paths.is_empty() {
                return Err(invalid(
                    "--change and --path apply to approval requests, not reviews",
                ));
            }
            let c = &request["change"];
            let shown = format!(
                "Change #{}: {}\nAuthorized by {} for {}\n\nAgent steps:\n  {}\n\n{}\n",
                c["id"],
                c["title"].as_str().unwrap_or_default(),
                c["authorized_by"].as_str().unwrap_or_default(),
                c["paths"]
                    .as_array()
                    .map(|p| p
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default(),
                request["steps"]
                    .as_array()
                    .map(|s| s
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n  "))
                    .unwrap_or_default(),
                request["diff"].as_str().unwrap_or_default(),
            );
            (
                json!({
                    "kind": "review",
                    "project": project,
                    "change": c["id"],
                    "snapshot": request["snapshot"],
                    "files": request["files"],
                    "decision": if o.reject { "reject" } else { "accept" },
                    "note": o.note,
                    "signed_at": timefmt::now_rfc3339(),
                }),
                shown,
                format!("change #{}", c["id"]),
            )
        }
        _ => return Err(invalid("not a Fabric approval or review request")),
    };

    let key = installation_key(home, KEY_FILE, true)?
        .ok_or_else(|| CliError::new(exit::ERROR, "no_key", "the key could not be created"))?;
    let verb = match payload["decision"].as_str() {
        Some("reject") => "Reject",
        Some("accept") => "Accept",
        _ => "Approve",
    };
    if !json_output || !o.yes {
        eprintln!("{shown}");
    }
    if !o.yes {
        if !io::stdin().is_terminal() {
            return Err(CliError::new(
                exit::NEEDS_CONFIRMATION,
                "needs_confirmation",
                format!("review {what} above, then rerun with --yes to sign"),
            ));
        }
        eprint!(
            "{verb} {what} and sign with key {}? [y/N] ",
            fingerprint(&key.verifying_key())
        );
        let _ = io::stderr().flush();
        let mut answer = String::new();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            eprintln!("Nothing signed.");
            return Ok(exit::NEEDS_CONFIRMATION);
        }
    }
    let bytes = serde_json::to_vec(&payload).expect("payloads always serialize");
    let signature = key.sign(&bytes);
    let decision = json!({
        "format": DECISION_FORMAT,
        "version": FILE_VERSION,
        "payload": payload,
        "public_key": hex(key.verifying_key().as_bytes()),
        "signature": hex(&signature.to_bytes()),
    });
    let out = o.out.map_or_else(
        || {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("decision");
            file.with_file_name(format!("{stem}.signed.json"))
        },
        PathBuf::from,
    );
    write_durably(
        &out,
        &serde_json::to_vec_pretty(&decision).expect("decisions always serialize"),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.approver.sign/v1", "out": display_path(&out), "decision": decision }),
        &format!(
            "Signed: {} {what}. Send {} back to the developer, who applies it with --signed.",
            verb.to_lowercase(),
            display_path(&out)
        ),
    );
    Ok(exit::OK)
}

// ---- Verifying, on the developer's machine -------------------------------------------------

/// A signed decision whose signature and signer have been checked.
pub struct Verified {
    pub approver: String,
    pub fingerprint: String,
    pub payload: Value,
    /// What the evidence keeps so the signature can be checked again offline.
    pub evidence: Value,
}

fn verify_signature(public_key: &str, signature: &str, payload: &Value) -> Option<VerifyingKey> {
    let key = unhex::<32>(public_key).and_then(|b| VerifyingKey::from_bytes(&b).ok())?;
    let signature = Signature::from_bytes(&unhex::<64>(signature)?);
    let bytes = serde_json::to_vec(payload).ok()?;
    key.verify(&bytes, &signature).ok().map(|()| key)
}

/// Reads and checks a signed decision of `kind` (`approval` or `review`) for this project.
pub fn verify_file(
    home: &Home,
    project: &Project,
    file: &Path,
    kind: &str,
) -> Result<Verified, CliError> {
    let decision = read_json(file)?;
    if decision["format"] != DECISION_FORMAT || decision["version"] != FILE_VERSION {
        return Err(invalid(
            "not a signed Fabric decision (made by `fabric approver sign`)",
        ));
    }
    let payload = decision["payload"].clone();
    let public_key = decision["public_key"].as_str().unwrap_or_default();
    let signature = decision["signature"].as_str().unwrap_or_default();
    let key = verify_signature(public_key, signature, &payload).ok_or_else(|| {
        CliError::new(
            exit::INTEGRITY,
            "bad_signature",
            "the signature does not match the decision; nothing recorded",
        )
    })?;
    if payload["kind"] != kind {
        return Err(invalid(format!("this file is not a signed {kind}")));
    }
    if payload["project"].as_str() != Some(project.id.as_str()) {
        return Err(invalid("this decision was signed for another project"));
    }
    let registry = registry(project)?;
    let approver = registry
        .approvers
        .iter()
        .find(|a| a.public_key == hex(key.as_bytes()))
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "unknown_approver",
                format!(
                    "key {} is not a registered approver for this project (`fabric approver add`)",
                    fingerprint(&key)
                ),
            )
        })?;
    let same_installation =
        installation_key(home, KEY_FILE, false)?.is_some_and(|own| own.verifying_key() == key);
    if registry.required && same_installation {
        return Err(CliError::new(
            exit::ERROR,
            "same_installation",
            "this decision was signed with this installation's own approver key; the project requires an approver from another installation",
        ));
    }
    Ok(Verified {
        approver: approver.name.clone(),
        fingerprint: approver.fingerprint.clone(),
        payload: payload.clone(),
        evidence: json!({
            "public_key": public_key,
            "signature": signature,
            "payload": payload,
            "same_installation": same_installation,
        }),
    })
}

/// Re-checks every signed decision in the evidence: the signature, that the key was a
/// registered approver when it was used, and that the decision is about the recorded event.
/// Returns how many were checked and the problems found.
pub fn check_evidence(records: &[Record]) -> (usize, Vec<String>) {
    let mut registered: Vec<String> = Vec::new();
    let mut checked = 0;
    let mut problems = Vec::new();
    for record in records {
        let e = &record.body.event;
        match e["kind"].as_str() {
            Some("approver_added") => {
                if let Some(key) = e["public_key"].as_str() {
                    registered.push(key.to_owned());
                }
                continue;
            }
            Some("approver_removed") => {
                registered.retain(|k| Some(k.as_str()) != e["public_key"].as_str());
                continue;
            }
            _ => {}
        }
        let signed = &e["signed"];
        if signed.is_null() {
            continue;
        }
        checked += 1;
        let at = format!("evidence record #{}", record.body.seq);
        let public_key = signed["public_key"].as_str().unwrap_or_default();
        let payload = &signed["payload"];
        if verify_signature(
            public_key,
            signed["signature"].as_str().unwrap_or_default(),
            payload,
        )
        .is_none()
        {
            problems.push(format!("{at}: the approver's signature does not verify"));
            continue;
        }
        if !registered.iter().any(|k| k == public_key) {
            problems.push(format!(
                "{at}: signed by a key that was not a registered approver at the time"
            ));
        }
        let about_event = match e["kind"].as_str() {
            Some("approval_granted" | "approval_rejected") => {
                payload["requests"].as_array().is_some_and(|rs| {
                    rs.iter()
                        .any(|r| r["id"] == e["request"] && r["digest"] == e["digest"])
                }) && (payload["decision"] == "reject") == (e["kind"] == "approval_rejected")
            }
            Some("change_opened") => {
                payload["change"]["title"] == e["title"] && payload["decision"] == "approve"
            }
            Some("change_reviewed") => {
                payload["change"] == e["change"]
                    && payload["files"] == e["files"]
                    && (payload["decision"] == "accept") == (e["outcome"] == "accepted")
            }
            _ => false,
        };
        if !about_event {
            problems.push(format!("{at}: the signed decision is not about this event"));
        }
    }
    (checked, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn signatures_bind_the_exact_payload() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let payload = json!({ "kind": "approval", "requests": [{ "id": 1, "digest": "ab" }] });
        let signature = hex(&key.sign(&serde_json::to_vec(&payload).unwrap()).to_bytes());
        let public = hex(key.verifying_key().as_bytes());
        assert!(verify_signature(&public, &signature, &payload).is_some());
        let changed = json!({ "kind": "approval", "requests": [{ "id": 2, "digest": "ab" }] });
        assert!(verify_signature(&public, &signature, &changed).is_none());
        assert!(verify_signature(&public, "00", &payload).is_none());
    }

    fn record(seq: u64, event: Value) -> Record {
        Record {
            hash: String::new(),
            body: evidence::Body {
                v: 1,
                seq,
                prev: String::new(),
                at: String::new(),
                event,
            },
        }
    }

    #[test]
    fn evidence_checks_signer_registration_and_subject() {
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let payload = json!({
            "kind": "approval",
            "requests": [{ "id": 1, "digest": "d1" }],
            "decision": "approve",
        });
        let signed = json!({
            "public_key": public,
            "signature": hex(&key.sign(&serde_json::to_vec(&payload).unwrap()).to_bytes()),
            "payload": payload,
        });
        let granted = |request: u64| json!({ "kind": "approval_granted", "request": request, "digest": "d1", "signed": signed });
        let added = json!({ "kind": "approver_added", "public_key": public });
        let removed = json!({ "kind": "approver_removed", "public_key": public });

        let ok = [record(1, added.clone()), record(2, granted(1))];
        assert_eq!(check_evidence(&ok), (1, Vec::new()));
        // Signed before the key was registered, or after it was removed.
        let early = [record(1, granted(1)), record(2, added.clone())];
        assert_eq!(check_evidence(&early).1.len(), 1);
        let late = [
            record(1, added.clone()),
            record(2, removed),
            record(3, granted(1)),
        ];
        assert_eq!(check_evidence(&late).1.len(), 1);
        // A valid signature reused for a different request.
        let reused = [record(1, added.clone()), record(2, granted(2))];
        assert!(check_evidence(&reused).1[0].contains("not about this event"));
        // A payload changed after signing.
        let mut forged = granted(1);
        forged["signed"]["payload"]["decision"] = json!("reject");
        let forged = [record(1, added), record(2, forged)];
        assert!(check_evidence(&forged).1[0].contains("does not verify"));
    }
}
