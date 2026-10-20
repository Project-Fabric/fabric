//! Evidence commands: verify the chain, write per-change reports, and export a signed,
//! self-verifying evidence pack an auditor can check without Fabric's data directory.
//!
//! A pack is a directory:
//!
//! ```text
//! summary.json     what the pack covers, the chain head, and the BLAKE3 hash of every file
//! summary.sig      Ed25519 signature of summary.json's exact bytes, in hex
//! evidence.jsonl   the evidence chain
//! changes/<n>.md   one report per checkpoint
//! README.md        how to verify it
//! ```
//!
//! The signing key belongs to this Fabric installation (`<home>/keys/`). A valid signature
//! shows the pack was produced by the holder of that key and not changed since; a reviewer
//! should compare the key's fingerprint with one obtained separately.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Value, json};

use crate::checkpoint::read_log;
use crate::commands::Ctx;
use crate::error::{CliError, exit};
use crate::evidence::{self, ChainReport};
use crate::guard::Policy;
use crate::home::{Home, display_path, write_durably};
use crate::report;
use crate::signoff;
use crate::timefmt;

const PACK_FORMAT: &str = "fabric-evidence-pack";
const PACK_VERSION: u32 = 1;
const KEY_FILE: &str = "evidence-ed25519.key";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

pub fn fingerprint(key: &VerifyingKey) -> String {
    evidence::hash_hex(key.as_bytes())[..16].to_owned()
}

/// This installation's evidence signing key, created on first use.
fn signing_key(home: &Home) -> Result<SigningKey, CliError> {
    installation_key(home, KEY_FILE, true)?.ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "no_key",
            "the signing key could not be created",
        )
    })
}

/// One of this installation's keys in `<home>/keys/`. Without `create`, a missing key is
/// `None`.
pub fn installation_key(
    home: &Home,
    file: &str,
    create: bool,
) -> Result<Option<SigningKey>, CliError> {
    let dir = home.root.join("keys");
    let path = dir.join(file);
    match fs::read_to_string(&path) {
        Ok(text) => unhex::<32>(&text)
            .map(|seed| Some(SigningKey::from_bytes(&seed)))
            .ok_or_else(|| {
                CliError::new(
                    exit::INTEGRITY,
                    "invalid_key",
                    format!("{} is not a valid signing key", display_path(&path)),
                )
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "no_randomness",
                    format!("cannot generate a key: {e}"),
                )
            })?;
            fs::create_dir_all(&dir).map_err(|e| CliError::io(&dir, e))?;
            write_durably(&path, hex(&seed).as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                    .map_err(|e| CliError::io(&path, e))?;
            }
            Ok(Some(SigningKey::from_bytes(&seed)))
        }
        Err(e) => Err(CliError::io(&path, e)),
    }
}

fn chain_json(report: &ChainReport) -> Value {
    json!({
        "ok": report.ok(),
        "records": report.records,
        "head": report.head,
        "problems": report.problems,
        "incomplete_tail": report.incomplete_tail,
    })
}

pub fn verify_cmd(ctx: &Ctx, deep: bool) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let (report, records) = evidence::verify_project(&store, &project, deep)?;
    let policy = Policy::load(&project)?;
    let log = read_log(&project)?;
    let mut exceptions = Vec::new();
    for checkpoint in &log {
        exceptions.extend(report::exceptions_for(
            &store, &project, &policy, &log, &records, checkpoint,
        )?);
    }
    let (signed_checked, signed_problems) = signoff::check_evidence(&records);
    ctx.emit(
        "fabric.evidence.verify/v1",
        json!({
            "chain": chain_json(&report),
            "deep": deep,
            "exceptions": exceptions,
            "signed_decisions": { "checked": signed_checked, "problems": signed_problems },
        }),
        || {
            let mut out = String::new();
            if report.records == 0 {
                out.push_str("No evidence recorded yet.\n");
            } else if report.ok() {
                let _ = writeln!(
                    out,
                    "Evidence chain intact: {} records, head {}",
                    report.records,
                    report.head.as_deref().unwrap_or_default()
                );
            } else {
                let _ = writeln!(
                    out,
                    "EVIDENCE CHAIN BROKEN ({} problems):",
                    report.problems.len()
                );
                for p in &report.problems {
                    let _ = writeln!(out, "  {p}");
                }
            }
            if report.incomplete_tail {
                out.push_str("Note: the last record was interrupted while being written.\n");
            }
            if signed_problems.is_empty() {
                if signed_checked > 0 {
                    let _ = writeln!(out, "{signed_checked} signed approver decisions verified.");
                }
            } else {
                let _ = writeln!(
                    out,
                    "SIGNED DECISIONS FAILED VERIFICATION ({} problems):",
                    signed_problems.len()
                );
                for p in &signed_problems {
                    let _ = writeln!(out, "  {p}");
                }
            }
            if exceptions.is_empty() {
                out.push_str("No exceptions: every controlled change had an approval.\n");
            } else {
                let _ = writeln!(
                    out,
                    "{} exception(s): controlled files changed without approval:",
                    exceptions.len()
                );
                for e in &exceptions {
                    let _ = writeln!(
                        out,
                        "  #{} {} ({})",
                        e.checkpoint,
                        e.path,
                        e.rules.join(", ")
                    );
                }
            }
            out
        },
    );
    Ok(if report.ok() && signed_problems.is_empty() {
        exit::OK
    } else {
        exit::INTEGRITY
    })
}

/// Tool calls from the evidence, one entry per call: a forwarded call is joined with its
/// result; one never answered shows as `no_response`.
pub fn tool_calls(records: &[evidence::Record]) -> Vec<Value> {
    let mut calls: Vec<Value> = Vec::new();
    for record in records {
        let e = &record.body.event;
        if e["kind"] != "tool_call" {
            continue;
        }
        let entry = json!({
            "record": record.body.seq,
            "at": record.body.at,
            "agent": e["agent"],
            "session": e["session"],
            "tool": e["tool"],
            "digest": e["digest"],
            "arguments": e["arguments"],
            "decision": e["decision"],
            "rule": e["rule"],
            "outcome": if e["outcome"] == "forwarded" { json!("no_response") } else { e["outcome"].clone() },
            "result": e["result"],
        });
        let answered = e["outcome"] != "forwarded"
            && e["arguments"].is_null()
            && calls
                .iter_mut()
                .rev()
                .find(|c| {
                    c["outcome"] == "no_response"
                        && c["digest"] == e["digest"]
                        && c["session"] == e["session"]
                })
                .map(|c| {
                    c["outcome"] = e["outcome"].clone();
                    c["result"] = e["result"].clone();
                })
                .is_some();
        if !answered {
            calls.push(entry);
        }
    }
    calls
}

pub fn calls_cmd(ctx: &Ctx, limit: usize) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let (_, records) = evidence::verify_chain(&evidence::read_chain(&project)?);
    let calls: Vec<Value> = tool_calls(&records).into_iter().rev().take(limit).collect();
    ctx.emit(
        "fabric.evidence.calls/v1",
        json!({ "calls": calls }),
        || {
            let mut out = String::new();
            if calls.is_empty() {
                out.push_str(
                    "No tool calls recorded (agents call tools through `fabric mcp-proxy`).\n",
                );
            }
            for c in &calls {
                let s = |key: &str| c[key].as_str().unwrap_or("-").to_owned();
                let _ = writeln!(
                    out,
                    "#{} {} {} {} {} -> {}{}\n    args: {}\n    result: {}",
                    c["record"],
                    s("at"),
                    s("agent"),
                    s("tool"),
                    s("decision"),
                    s("outcome"),
                    c["rule"]
                        .as_str()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default(),
                    s("arguments"),
                    s("result").lines().next().unwrap_or_default(),
                );
            }
            out
        },
    );
    Ok(exit::OK)
}

pub fn change_report_cmd(ctx: &Ctx, id: u64) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let (_, records) = evidence::verify_chain(&evidence::read_chain(&project)?);
    let (markdown, exceptions) = report::change_report(
        &store,
        &project,
        &Policy::load(&project)?,
        &read_log(&project)?,
        &records,
        id,
    )?;
    ctx.emit(
        "fabric.evidence.change-report/v1",
        json!({ "change": id, "markdown": markdown, "exceptions": exceptions }),
        || markdown.clone(),
    );
    Ok(exit::OK)
}

pub fn report_cmd(ctx: &Ctx, seq: u64) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let (markdown, exceptions) = report::checkpoint_report(&store, &project, seq)?;
    ctx.emit(
        "fabric.evidence.report/v1",
        json!({ "checkpoint": seq, "markdown": markdown, "exceptions": exceptions }),
        || markdown.clone(),
    );
    Ok(exit::OK)
}

pub fn export_cmd(ctx: &Ctx, out: &Path) -> Result<u8, CliError> {
    let project = ctx.project()?;
    let store = ctx.home.open_store()?;
    let (chain, records) = evidence::verify_project(&store, &project, false)?;
    if !chain.ok() {
        return Err(CliError::new(
            exit::INTEGRITY,
            "corrupt_evidence",
            "the evidence chain is broken; run `fabric evidence verify` (nothing exported)",
        ));
    }
    if chain.records == 0 {
        return Err(CliError::new(
            exit::ERROR,
            "no_evidence",
            "no evidence recorded yet for this project",
        ));
    }
    if out.exists() && fs::read_dir(out).map_or(true, |mut d| d.next().is_some()) {
        return Err(CliError::new(
            exit::ERROR,
            "output_exists",
            format!("{} already exists and is not empty", display_path(out)),
        ));
    }
    for dir in ["changes", "tickets"] {
        fs::create_dir_all(out.join(dir)).map_err(|e| CliError::io(out, e))?;
    }

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut put = |name: &str, bytes: &[u8]| -> Result<(), CliError> {
        write_durably(&out.join(name), bytes)?;
        files.insert(name.to_owned(), evidence::hash_hex(bytes));
        Ok(())
    };
    let text = evidence::read_chain(&project)?;
    let complete = text.rfind('\n').map_or("", |end| &text[..=end]);
    put("evidence.jsonl", complete.as_bytes())?;

    let policy = Policy::load(&project)?;
    let log = read_log(&project)?;
    let mut exceptions = Vec::new();
    let evidenced: Vec<u64> = records
        .iter()
        .filter(|r| r.body.event["kind"] == "checkpoint")
        .filter_map(|r| r.body.event["checkpoint"].as_u64())
        .collect();
    for seq in &evidenced {
        let (markdown, found) = report::render(&store, &project, &policy, &log, &records, *seq)?;
        exceptions.extend(found);
        put(&format!("changes/{seq:06}.md"), markdown.as_bytes())?;
    }
    let tickets = report::change_ids(&records);
    for id in &tickets {
        let (markdown, _) = report::change_report(&store, &project, &policy, &log, &records, *id)?;
        put(&format!("tickets/change-{id:04}.md"), markdown.as_bytes())?;
    }
    put("README.md", pack_readme().as_bytes())?;

    let key = signing_key(&ctx.home)?;
    let verifying = key.verifying_key();
    let summary = json!({
        "format": PACK_FORMAT,
        "version": PACK_VERSION,
        "created_at": timefmt::now_rfc3339(),
        "project": { "id": project.id, "root": display_path(&project.root) },
        "chain": { "records": chain.records, "head": chain.head },
        "checkpoints": evidenced,
        "changes": tickets,
        "exceptions": exceptions,
        "files": files,
        "signing": {
            "algorithm": "ed25519",
            "public_key": hex(verifying.as_bytes()),
            "fingerprint": fingerprint(&verifying),
        },
    });
    let summary_bytes = serde_json::to_vec_pretty(&summary).expect("summaries always serialize");
    write_durably(&out.join("summary.json"), &summary_bytes)?;
    let signature = key.sign(&summary_bytes);
    write_durably(
        &out.join("summary.sig"),
        hex(&signature.to_bytes()).as_bytes(),
    )?;

    let shown = display_path(out);
    ctx.emit(
        "fabric.evidence.export/v1",
        json!({
            "out": shown,
            "records": chain.records,
            "head": chain.head,
            "checkpoints": evidenced.len(),
            "exceptions": exceptions.len(),
            "fingerprint": fingerprint(&verifying),
        }),
        || {
            format!(
                "Exported {} records and {} change reports to {shown}\nChain head: {}\nSigning key fingerprint: {}\n{}Check it anywhere with `fabric evidence verify-pack {shown}`.\n",
                chain.records,
                evidenced.len(),
                chain.head.as_deref().unwrap_or_default(),
                fingerprint(&verifying),
                if exceptions.is_empty() {
                    String::new()
                } else {
                    format!("{} exception(s) are listed in summary.json.\n", exceptions.len())
                }
            )
        },
    );
    Ok(exit::OK)
}

fn pack_readme() -> String {
    "# Fabric evidence pack\n\n\
     This directory records what AI coding agents did to a project: every checkpoint, guard \
     decision, approval and rule change, in a hash-chained log (`evidence.jsonl`), with one \
     report per checkpoint in `changes/` and one per authorized change (change ticket) in \
     `tickets/`.\n\n\
     ## Verify\n\n\
     ```sh\nfabric evidence verify-pack <this directory> --key <fingerprint>\n```\n\n\
     Verification checks the Ed25519 signature over `summary.json`, the BLAKE3 hash of every \
     file it lists, and every hash and link in the evidence chain. Get the expected key \
     fingerprint from the team that produced the pack through a separate channel.\n\n\
     ## What it shows\n\n\
     A valid pack was produced by the holder of the signing key and has not changed since. \
     The chain shows records were not edited, removed or reordered after later records were \
     written. It does not show that the producer recorded everything: an agent action Fabric \
     was not integrated with is not in it.\n"
        .to_owned()
}

pub fn verify_pack_cmd(ctx: &Ctx, dir: &Path, expected_key: Option<&str>) -> Result<u8, CliError> {
    let mut problems: Vec<String> = Vec::new();
    let read = |name: &str| -> Result<Vec<u8>, CliError> {
        let path: PathBuf = dir.join(name);
        fs::read(&path).map_err(|e| CliError::io(&path, e))
    };
    let summary_bytes = read("summary.json")?;
    let summary: Value = serde_json::from_slice(&summary_bytes).map_err(|e| {
        CliError::new(
            exit::INTEGRITY,
            "invalid_pack",
            format!("summary.json: {e}"),
        )
    })?;
    if summary["format"] != PACK_FORMAT || summary["version"] != PACK_VERSION {
        return Err(CliError::new(
            exit::INTEGRITY,
            "invalid_pack",
            "not a supported Fabric evidence pack",
        ));
    }

    let key = summary["signing"]["public_key"]
        .as_str()
        .and_then(unhex::<32>)
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok());
    let signature = String::from_utf8(read("summary.sig")?)
        .ok()
        .and_then(|text| unhex::<64>(&text))
        .map(|bytes| Signature::from_bytes(&bytes));
    let key_fingerprint = key.as_ref().map(fingerprint);
    match (&key, signature) {
        (Some(key), Some(signature)) => {
            if key.verify(&summary_bytes, &signature).is_err() {
                problems.push("the signature does not match summary.json".to_owned());
            }
        }
        _ => problems.push("the public key or signature is missing or malformed".to_owned()),
    }
    if let Some(expected) = expected_key
        && key_fingerprint.as_deref() != Some(expected.trim())
    {
        problems.push(format!(
            "signed with key {}, not the expected {expected}",
            key_fingerprint.as_deref().unwrap_or("(none)")
        ));
    }

    let listed = summary["files"].as_object().cloned().unwrap_or_default();
    for (name, hash) in &listed {
        let safe = !name.contains("..") && !Path::new(name).is_absolute();
        match (safe, read(name)) {
            (true, Ok(bytes)) if Some(evidence::hash_hex(&bytes).as_str()) == hash.as_str() => {}
            (true, Ok(_)) => problems.push(format!("{name} was changed after the pack was signed")),
            (true, Err(_)) => problems.push(format!("{name} is missing")),
            (false, _) => problems.push(format!("{name} is not a path inside the pack")),
        }
    }
    if !listed.contains_key("evidence.jsonl") {
        problems.push("the pack does not list its evidence chain".to_owned());
    }

    let chain_text =
        String::from_utf8(read("evidence.jsonl").unwrap_or_default()).unwrap_or_default();
    let (chain, records) = evidence::verify_chain(&chain_text);
    problems.extend(
        chain
            .problems
            .iter()
            .map(|p| format!("evidence chain: {p}")),
    );
    let (signed_checked, signed_problems) = signoff::check_evidence(&records);
    problems.extend(signed_problems);
    if chain.head.as_deref() != summary["chain"]["head"].as_str() {
        problems.push("the chain head does not match the summary".to_owned());
    }

    let ok = problems.is_empty();
    ctx.emit(
        "fabric.evidence.verify_pack/v1",
        json!({
            "ok": ok,
            "problems": problems,
            "records": chain.records,
            "head": chain.head,
            "fingerprint": key_fingerprint,
            "exceptions": summary["exceptions"],
            "signed_decisions": signed_checked,
        }),
        || {
            let mut out = String::new();
            if ok {
                let _ = writeln!(
                    out,
                    "Pack verified: signature valid (key {}), {} files intact, {} chain records, head {}",
                    key_fingerprint.as_deref().unwrap_or_default(),
                    listed.len(),
                    chain.records,
                    chain.head.as_deref().unwrap_or_default()
                );
                if expected_key.is_none() {
                    out.push_str("Compare the key fingerprint with one obtained separately, or pass --key.\n");
                }
            } else {
                let _ = writeln!(out, "PACK FAILED VERIFICATION ({} problems):", problems.len());
                for p in &problems {
                    let _ = writeln!(out, "  {p}");
                }
            }
            out
        },
    );
    Ok(if ok { exit::OK } else { exit::INTEGRITY })
}

pub fn key_cmd(ctx: &Ctx) -> Result<u8, CliError> {
    let key = signing_key(&ctx.home)?.verifying_key();
    ctx.emit(
        "fabric.evidence.key/v1",
        json!({ "algorithm": "ed25519", "public_key": hex(key.as_bytes()), "fingerprint": fingerprint(&key) }),
        || format!("Evidence signing key fingerprint: {}\nPublic key: {}\n", fingerprint(&key), hex(key.as_bytes())),
    );
    Ok(exit::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&bytes), "0001abff");
        assert_eq!(unhex::<4>("0001abff"), Some(bytes));
        assert_eq!(unhex::<4>("0001abf"), None);
        assert_eq!(unhex::<2>("zz00"), None);
    }
}
