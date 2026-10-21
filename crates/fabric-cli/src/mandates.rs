//! Mandates: bounded authority a person delegates to an agent ahead of time.
//!
//! Holding every money movement for a person is safe but slow: a refunds agent waits on a
//! human for every $40 refund. A mandate says what an agent may do on its own, within limits:
//!
//! ```text
//! fabric mandate grant "Small refunds" --tool "*/issue_refund" --max-per-call 500 \
//!     --max-per-day 2000 --max-calls-per-day 20 --agent close-bot --expires 2026-11-30
//! ```
//!
//! A tool call held by an `approve` rule goes through without waiting when an active mandate
//! covers it: the tool matches, the agent matches (if the mandate names one, the call must be
//! authenticated as that agent: see `identity`), the call's
//! amount is within the per-call limit, and the day's total and count stay within theirs. The
//! use is recorded in the evidence with the mandate, the amount and the running totals. Calls
//! outside every mandate are held for a person as before; `deny` rules are never covered.
//!
//! A call's amount is the largest `amount` argument, or the sum of `debit` arguments (a
//! journal entry's size), found anywhere in its arguments; values are decimal strings or
//! numbers. A call with a limit but no readable amount is not covered. Days are UTC.
//!
//! Granting is recorded with who granted it. When the project requires signed approvals, a
//! mandate must be signed by a registered approver from another installation: `fabric mandate
//! grant ... --out FILE` writes the proposal, the approver signs it with `fabric approver sign`,
//! and `fabric mandate grant --signed FILE` applies it. Mandates cover tool calls only, not
//! file edits or shell commands.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use globset::GlobSet;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::approvals::in_agent_environment;
use crate::changes::globs;
use crate::error::{CliError, exit};
use crate::evidence::{self, Record};
use crate::home::{Home, Project, display_path, write_durably};
use crate::signoff;
use crate::timefmt;

const FILE: &str = "mandates.json";
const LOCK_FILE: &str = "mandates.lock";
const FORMAT: &str = "fabric-mandates";
const VERSION: u32 = 1;
pub const PROPOSAL_FORMAT: &str = "fabric-mandate-proposal";
/// A call passing both the hook and the proxy counts once against a mandate.
const SAME_CALL_SECS: u64 = 120;
/// Amounts are kept as integers of this many decimal places.
const SCALE: u32 = 6;

/// The terms of a mandate: what is granted, and the limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terms {
    pub title: String,
    /// Globs over tool calls as `server/tool`.
    pub tools: Vec<String>,
    /// The agent's name (an MCP client's name, or `claude-code`); any agent if absent.
    #[serde(default)]
    pub agent: Option<String>,
    /// Decimal strings, e.g. "500.00".
    #[serde(default)]
    pub max_per_call: Option<String>,
    #[serde(default)]
    pub max_per_day: Option<String>,
    #[serde(default)]
    pub max_calls_per_day: Option<u64>,
    /// `YYYY-MM-DD`: the mandate ends at the start of this day (UTC). Required.
    pub expires: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Use {
    pub at_unix: u64,
    pub day: String,
    pub digest: String,
    pub tool: String,
    /// Scaled by 10^6; `None` if the call had no amount.
    pub amount: Option<i128>,
    pub via: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mandate {
    pub id: u64,
    pub terms: Terms,
    pub granted_by: String,
    pub granted_at: String,
    #[serde(default)]
    pub granter_key: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
    #[serde(default)]
    pub uses: Vec<Use>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MandatesFile {
    format: String,
    version: u32,
    mandates: Vec<Mandate>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The UTC date of a Unix time, `YYYY-MM-DD`.
fn day_of(unix: u64) -> String {
    timefmt::rfc3339(unix)[..10].to_owned()
}

/// Parses a decimal amount ("1,250.50" is not accepted; "1250.50" is) into an integer scaled
/// by 10^6. Negative and malformed amounts are `None`.
pub fn parse_amount(text: &str) -> Option<i128> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || (text.contains('.') && fraction.is_empty())
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > SCALE as usize
        || whole.len() > 24
    {
        return None;
    }
    let whole: i128 = whole.parse().ok()?;
    let mut scaled = whole.checked_mul(10i128.pow(SCALE))?;
    if !fraction.is_empty() {
        let digits: i128 = fraction.parse().ok()?;
        scaled += digits * 10i128.pow(SCALE - fraction.len() as u32);
    }
    Some(scaled)
}

fn format_amount(scaled: i128) -> String {
    let unit = 10i128.pow(SCALE);
    let cents = (scaled % unit) / 10i128.pow(SCALE - 2);
    format!("{}.{cents:02}", scaled / unit)
}

fn amount_value(value: &Value) -> Option<i128> {
    match value {
        Value::String(s) => parse_amount(s),
        Value::Number(n) => parse_amount(&n.to_string()),
        _ => None,
    }
}

/// The amount of a call: the largest `amount` argument, or the sum of `debit` arguments,
/// whichever is larger. `Err` if an amount-like argument cannot be read.
pub fn call_amount(arguments: &Value) -> Result<Option<i128>, ()> {
    fn walk(value: &Value, amounts: &mut Vec<i128>, debits: &mut i128) -> Result<(), ()> {
        match value {
            Value::Object(map) => {
                for (key, v) in map {
                    match key.as_str() {
                        "amount" => amounts.push(amount_value(v).ok_or(())?),
                        "debit" if !v.is_null() => *debits += amount_value(v).ok_or(())?,
                        _ => walk(v, amounts, debits)?,
                    }
                }
            }
            Value::Array(items) => {
                for v in items {
                    walk(v, amounts, debits)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut amounts = Vec::new();
    let mut debits = 0;
    walk(arguments, &mut amounts, &mut debits)?;
    let largest = amounts.into_iter().max();
    Ok(match (largest, debits) {
        (None, 0) => None,
        (a, d) => Some(a.unwrap_or(0).max(d)),
    })
}

fn invalid(message: impl Into<String>) -> CliError {
    CliError::new(exit::ERROR, "invalid_mandate", message)
}

impl Terms {
    pub fn validate(&self) -> Result<GlobSet, CliError> {
        if self.title.trim().is_empty() {
            return Err(invalid("a mandate needs a title"));
        }
        if self.tools.is_empty() {
            return Err(invalid("a mandate needs at least one --tool pattern"));
        }
        for (name, limit) in [
            ("--max-per-call", &self.max_per_call),
            ("--max-per-day", &self.max_per_day),
        ] {
            if let Some(text) = limit
                && parse_amount(text).is_none()
            {
                return Err(invalid(format!(
                    "{name} is a decimal amount such as 500 or 500.00"
                )));
            }
        }
        if self.max_per_call.is_none()
            && self.max_per_day.is_none()
            && self.max_calls_per_day.is_none()
        {
            return Err(invalid(
                "a mandate needs at least one limit: --max-per-call, --max-per-day or --max-calls-per-day",
            ));
        }
        let valid_date = self.expires.len() == 10 && timefmt::parse_date(&self.expires).is_some();
        if !valid_date {
            return Err(invalid("--expires is a date, YYYY-MM-DD"));
        }
        globs(&self.tools).map_err(invalid)
    }

    fn expired(&self, now: u64) -> bool {
        timefmt::parse_date(&self.expires).is_none_or(|end| now >= end)
    }
}

fn with_mandates<T>(
    project: &Project,
    f: impl FnOnce(&mut Vec<Mandate>) -> Result<(T, bool), CliError>,
) -> Result<T, CliError> {
    let lock_path = project.dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| CliError::io(&lock_path, e))?;
    lock.lock().map_err(|e| CliError::io(&lock_path, e))?;
    let path = project.dir.join(FILE);
    let mut mandates = match fs::read(&path) {
        Ok(bytes) => {
            let file: MandatesFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_mandates",
                    format!("{}: {e}", display_path(&path)),
                )
            })?;
            if file.format != FORMAT || file.version != VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_mandates",
                    format!("{}: unsupported mandates file", display_path(&path)),
                ));
            }
            file.mandates
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    let (result, changed) = f(&mut mandates)?;
    if changed {
        let file = MandatesFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            mandates,
        };
        let bytes = serde_json::to_vec_pretty(&file).expect("mandates always serialize");
        write_durably(&path, &bytes)?;
    }
    drop(lock);
    Ok(result)
}

/// Why a mandate does not cover a call, or how it does.
enum Fit {
    Covers {
        amount: Option<i128>,
        day_total: i128,
        day_calls: u64,
    },
    No,
}

fn fits(
    m: &Mandate,
    tool: &str,
    agent: &str,
    authenticated: bool,
    amount: Result<Option<i128>, ()>,
    now: u64,
) -> Fit {
    let t = &m.terms;
    if m.revoked_at.is_some() || t.expired(now) {
        return Fit::No;
    }
    if !globs(&t.tools).is_ok_and(|g| g.is_match(tool)) {
        return Fit::No;
    }
    // A mandate for a named agent covers only calls authenticated as it: a client's own
    // claim to be that agent is not enough.
    if t.agent
        .as_deref()
        .is_some_and(|a| a != agent || !authenticated)
    {
        return Fit::No;
    }
    let Ok(amount) = amount else { return Fit::No };
    let has_amount_limit = t.max_per_call.is_some() || t.max_per_day.is_some();
    if has_amount_limit && amount.is_none() {
        return Fit::No;
    }
    let value = amount.unwrap_or(0);
    if t.max_per_call
        .as_deref()
        .and_then(parse_amount)
        .is_some_and(|max| value > max)
    {
        return Fit::No;
    }
    let today = day_of(now);
    let (spent, calls) = m
        .uses
        .iter()
        .filter(|u| u.day == today)
        .fold((0i128, 0u64), |(s, c), u| {
            (s + u.amount.unwrap_or(0), c + 1)
        });
    if t.max_per_day
        .as_deref()
        .and_then(parse_amount)
        .is_some_and(|max| spent + value > max)
    {
        return Fit::No;
    }
    if t.max_calls_per_day.is_some_and(|max| calls + 1 > max) {
        return Fit::No;
    }
    Fit::Covers {
        amount,
        day_total: spent + value,
        day_calls: calls + 1,
    }
}

/// Called before a tool call under an `approve` rule is held. If an active mandate covers it,
/// the use is recorded and `true` returned: the call proceeds.
pub fn cover(
    project: &Project,
    tool: &str,
    agent: &str,
    authenticated: bool,
    digest: &str,
    arguments: &Value,
    via: &str,
) -> Result<bool, CliError> {
    let now = now_unix();
    let amount = call_amount(arguments);
    enum Outcome {
        Repeat,
        New(u64, Option<i128>, i128, u64),
        None,
    }
    let outcome = with_mandates(project, |mandates| {
        // The same call reaching a second checkpoint right after the first: already counted.
        for m in mandates.iter_mut() {
            if let Some(u) = m.uses.iter_mut().rev().find(|u| {
                u.digest == digest
                    && !u.via.iter().any(|v| v == via)
                    && now.saturating_sub(u.at_unix) <= SAME_CALL_SECS
            }) {
                u.via.push(via.to_owned());
                return Ok((Outcome::Repeat, true));
            }
        }
        for m in mandates.iter_mut() {
            if let Fit::Covers {
                amount,
                day_total,
                day_calls,
            } = fits(m, tool, agent, authenticated, amount, now)
            {
                m.uses.push(Use {
                    at_unix: now,
                    day: day_of(now),
                    digest: digest.to_owned(),
                    tool: tool.to_owned(),
                    amount,
                    via: vec![via.to_owned()],
                });
                return Ok((Outcome::New(m.id, amount, day_total, day_calls), true));
            }
        }
        Ok((Outcome::None, false))
    })?;
    match outcome {
        Outcome::None => Ok(false),
        Outcome::Repeat => Ok(true),
        Outcome::New(id, amount, day_total, day_calls) => {
            evidence::append(
                project,
                json!({
                    "kind": "mandate_used",
                    "mandate": id,
                    "tool": tool,
                    "agent": agent,
                    "digest": digest,
                    "amount": amount.map(format_amount),
                    "day": day_of(now),
                    "day_total": format_amount(day_total),
                    "day_calls": day_calls,
                    "via": via,
                }),
            )?;
            Ok(true)
        }
    }
}

// ---- `fabric mandate ...` ------------------------------------------------------------------

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

fn person_only() -> Result<(), CliError> {
    match in_agent_environment() {
        Some(marker) => Err(CliError::new(
            exit::ERROR,
            "from_agent",
            format!(
                "mandates are granted by a person in their own terminal, not from inside an agent session ({marker} is set)"
            ),
        )),
        None => Ok(()),
    }
}

fn describe(t: &Terms) -> String {
    let mut limits = Vec::new();
    if let Some(v) = &t.max_per_call {
        limits.push(format!("up to {v} per call"));
    }
    if let Some(v) = &t.max_per_day {
        limits.push(format!("{v} per day"));
    }
    if let Some(v) = t.max_calls_per_day {
        limits.push(format!("{v} calls per day"));
    }
    format!(
        "{} may call {} {}, until {}",
        t.agent.as_deref().unwrap_or("any agent"),
        t.tools.join(", "),
        limits.join(", "),
        t.expires
    )
}

/// Grants a mandate: from `terms` (a local grant, or with `out` a proposal for an approver to
/// sign), or from a signed file.
pub fn grant_cmd(
    home: &Home,
    project: &Project,
    terms: Option<Terms>,
    out: Option<&Path>,
    signed: Option<&Path>,
    json_output: bool,
) -> Result<u8, CliError> {
    person_only()?;
    let (terms, granted_by, granter_key, signature) = match (terms, signed) {
        (Some(terms), None) => {
            terms.validate()?;
            if let Some(agent) = &terms.agent
                && !crate::identity::is_registered(project, agent)?
            {
                return Err(invalid(format!(
                    "--agent {agent} is not a registered agent; register it with `fabric agent add {agent}`"
                )));
            }
            if let Some(out) = out {
                let proposal = json!({
                    "format": PROPOSAL_FORMAT,
                    "version": 1,
                    "project": { "id": project.id, "root": display_path(&project.root) },
                    "exported_at": timefmt::now_rfc3339(),
                    "terms": terms,
                });
                write_durably(
                    out,
                    &serde_json::to_vec_pretty(&proposal).expect("proposals always serialize"),
                )?;
                print(
                    json_output,
                    json!({ "schema": "fabric.mandate.proposal/v1", "out": display_path(out), "terms": terms }),
                    &format!(
                        "Wrote {}. The approver signs it with `fabric approver sign {}`; apply it with `fabric mandate grant --signed <file>`.",
                        display_path(out),
                        display_path(out)
                    ),
                );
                return Ok(exit::OK);
            }
            signoff::refuse_unsigned(project, "mandates")?;
            let by = crate::approvals::git_identity(&project.root)
                .or_else(evidence::os_user)
                .ok_or_else(|| invalid("cannot tell who is granting"))?;
            (terms, by, None, None)
        }
        (None, Some(file)) => {
            let v = signoff::verify_file(home, project, file, "mandate")?;
            let terms: Terms = serde_json::from_value(v.payload["terms"].clone())
                .map_err(|e| invalid(format!("the signed mandate's terms: {e}")))?;
            terms.validate()?;
            if v.payload["decision"] != "grant" {
                return Err(invalid(
                    "the approver declined this mandate; nothing granted",
                ));
            }
            (terms, v.approver, Some(v.fingerprint), Some(v.evidence))
        }
        _ => return Err(invalid("give the mandate's terms, or --signed FILE")),
    };
    let mandate = with_mandates(project, |mandates| {
        let mandate = Mandate {
            id: mandates.iter().map(|m| m.id).max().unwrap_or(0) + 1,
            terms: terms.clone(),
            granted_by: granted_by.clone(),
            granted_at: timefmt::now_rfc3339(),
            granter_key: granter_key.clone(),
            revoked_at: None,
            uses: Vec::new(),
        };
        mandates.push(mandate.clone());
        Ok((mandate, true))
    })?;
    evidence::append(
        project,
        json!({
            "kind": "mandate_granted",
            "mandate": mandate.id,
            "terms": mandate.terms,
            "granted_by": mandate.granted_by,
            "granter_os_user": if signature.is_some() { None } else { evidence::os_user() },
            "granter_key": mandate.granter_key,
            "signed": signature,
        }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.mandate.grant/v1", "mandate": mandate }),
        &format!(
            "Granted mandate #{} \"{}\": {}.",
            mandate.id,
            mandate.terms.title,
            describe(&mandate.terms)
        ),
    );
    Ok(exit::OK)
}

pub fn revoke_cmd(project: &Project, id: u64, json_output: bool) -> Result<u8, CliError> {
    person_only()?;
    let mandate = with_mandates(project, |mandates| {
        let m = mandates
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| invalid(format!("no mandate #{id}; see `fabric mandate list`")))?;
        if m.revoked_at.is_some() {
            return Err(invalid(format!("mandate #{id} is already revoked")));
        }
        m.revoked_at = Some(timefmt::now_rfc3339());
        Ok((m.clone(), true))
    })?;
    evidence::append(
        project,
        json!({ "kind": "mandate_revoked", "mandate": id, "by": evidence::os_user() }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.mandate.revoke/v1", "mandate": mandate }),
        &format!("Revoked mandate #{id}; calls it covered are held for a person again."),
    );
    Ok(exit::OK)
}

pub fn list_cmd(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let mandates = with_mandates(project, |m| Ok((m.clone(), false)))?;
    let now = now_unix();
    let today = day_of(now);
    let mut human = String::new();
    if mandates.is_empty() {
        human.push_str("No mandates. Grant one with `fabric mandate grant`.");
    }
    for m in &mandates {
        let state = if m.revoked_at.is_some() {
            "revoked"
        } else if m.terms.expired(now) {
            "expired"
        } else {
            "active"
        };
        let (spent, calls) = m
            .uses
            .iter()
            .filter(|u| u.day == today)
            .fold((0i128, 0u64), |(s, c), u| {
                (s + u.amount.unwrap_or(0), c + 1)
            });
        human.push_str(&format!(
            "#{} {state} \"{}\": {}. Granted by {}. Today: {} calls, {}.\n",
            m.id,
            m.terms.title,
            describe(&m.terms),
            m.granted_by,
            calls,
            format_amount(spent)
        ));
    }
    print(
        json_output,
        json!({ "schema": "fabric.mandate.list/v1", "mandates": mandates }),
        human.trim_end(),
    );
    Ok(exit::OK)
}

/// Replays every mandate use in the evidence against the mandate's recorded terms: each use
/// must fall under a mandate granted before it, not revoked or expired, for a matching tool and
/// agent, and within every limit. Returns the uses checked and the problems found.
pub fn check_evidence(records: &[Record]) -> (usize, Vec<String>) {
    let mut granted: BTreeMap<u64, (Terms, bool)> = BTreeMap::new();
    let mut totals: BTreeMap<(u64, String), (i128, u64)> = BTreeMap::new();
    let mut checked = 0;
    let mut problems = Vec::new();
    for record in records {
        let e = &record.body.event;
        let Some(id) = e["mandate"].as_u64() else {
            continue;
        };
        match e["kind"].as_str() {
            Some("mandate_granted") => {
                if let Ok(terms) = serde_json::from_value::<Terms>(e["terms"].clone()) {
                    granted.insert(id, (terms, false));
                }
            }
            Some("mandate_revoked") => {
                if let Some(entry) = granted.get_mut(&id) {
                    entry.1 = true;
                }
            }
            Some("mandate_used") => {
                checked += 1;
                let at = format!("evidence record #{}", record.body.seq);
                let Some((terms, revoked)) = granted.get(&id) else {
                    problems.push(format!(
                        "{at}: mandate #{id} was used before it was granted"
                    ));
                    continue;
                };
                let day = e["day"].as_str().unwrap_or_default().to_owned();
                let used_at = timefmt::parse_rfc3339(&record.body.at).unwrap_or(u64::MAX);
                let amount = e["amount"].as_str().and_then(parse_amount);
                let total = totals.entry((id, day)).or_insert((0, 0));
                total.0 += amount.unwrap_or(0);
                total.1 += 1;
                let mut wrong = Vec::new();
                if *revoked {
                    wrong.push("after it was revoked");
                }
                if terms.expired(used_at) {
                    wrong.push("after it expired");
                }
                if !globs(&terms.tools)
                    .is_ok_and(|g| g.is_match(e["tool"].as_str().unwrap_or_default()))
                {
                    wrong.push("for a tool it does not cover");
                }
                if terms
                    .agent
                    .as_deref()
                    .is_some_and(|a| Some(a) != e["agent"].as_str())
                {
                    wrong.push("by another agent");
                }
                let over = |limit: &Option<String>, value: i128| {
                    limit
                        .as_deref()
                        .and_then(parse_amount)
                        .is_some_and(|max| value > max)
                };
                if over(&terms.max_per_call, amount.unwrap_or(0)) {
                    wrong.push("over the per-call limit");
                }
                if over(&terms.max_per_day, total.0) {
                    wrong.push("over the daily limit");
                }
                if terms.max_calls_per_day.is_some_and(|max| total.1 > max) {
                    wrong.push("over the daily call count");
                }
                if !wrong.is_empty() {
                    problems.push(format!("{at}: mandate #{id} used {}", wrong.join(", ")));
                }
            }
            _ => {}
        }
    }
    (checked, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_exact_decimals() {
        assert_eq!(parse_amount("500"), Some(500_000_000));
        assert_eq!(parse_amount("12.5"), Some(12_500_000));
        assert_eq!(parse_amount("0.000001"), Some(1));
        for bad in ["", "-5", "1,250.00", "1e3", "12.", "abc", "1.0000001"] {
            assert_eq!(parse_amount(bad), None, "{bad}");
        }
        assert_eq!(format_amount(12_500_000), "12.50");
    }

    #[test]
    fn a_calls_amount_is_its_largest_amount_or_its_debits() {
        let refund = json!({ "payment_id": 1, "amount": "40.00", "reason": "x" });
        assert_eq!(call_amount(&refund), Ok(Some(40_000_000)));
        let entry = json!({ "lines": [
            { "account": "6000", "debit": "12.50" },
            { "account": "6100", "debit": "7.50" },
            { "account": "1000", "credit": "20.00" },
        ] });
        assert_eq!(call_amount(&entry), Ok(Some(20_000_000)));
        assert_eq!(call_amount(&json!({ "limit": 5 })), Ok(None));
        assert_eq!(call_amount(&json!({ "amount": "lots" })), Err(()));
        assert_eq!(
            call_amount(&json!({ "amount": 99.5 })),
            Ok(Some(99_500_000))
        );
    }

    fn record(seq: u64, at: &str, event: Value) -> Record {
        Record {
            hash: String::new(),
            body: evidence::Body {
                v: 1,
                seq,
                prev: String::new(),
                at: at.to_owned(),
                event,
            },
        }
    }

    #[test]
    fn replaying_the_evidence_finds_uses_outside_the_terms() {
        let at = "2026-10-20T10:00:00Z";
        let granted = json!({ "kind": "mandate_granted", "mandate": 1, "terms": {
            "title": "t", "tools": ["*/issue_refund"], "agent": "bot",
            "max_per_call": "500", "max_per_day": "800", "expires": "2027-01-01" } });
        let used = |amount: &str, agent: &str| {
            json!({ "kind": "mandate_used", "mandate": 1, "tool": "erp/issue_refund",
                    "agent": agent, "amount": amount, "day": "2026-10-20" })
        };
        let ok = [
            record(1, at, granted.clone()),
            record(2, at, used("300.00", "bot")),
            record(3, at, used("400.00", "bot")),
        ];
        assert_eq!(check_evidence(&ok), (2, Vec::new()));

        let over_day = [
            record(1, at, granted.clone()),
            record(2, at, used("500.00", "bot")),
            record(3, at, used("400.00", "bot")),
        ];
        assert!(check_evidence(&over_day).1[0].contains("over the daily limit"));
        let wrong_agent = [
            record(1, at, granted.clone()),
            record(2, at, used("1.00", "other")),
        ];
        assert!(check_evidence(&wrong_agent).1[0].contains("by another agent"));
        let revoked = [
            record(1, at, granted.clone()),
            record(2, at, json!({ "kind": "mandate_revoked", "mandate": 1 })),
            record(3, at, used("1.00", "bot")),
        ];
        assert!(check_evidence(&revoked).1[0].contains("after it was revoked"));
        let expired = [
            record(1, at, granted),
            record(2, "2027-01-02T00:00:00Z", used("1.00", "bot")),
        ];
        assert!(check_evidence(&expired).1[0].contains("after it expired"));
        let ungranted = [record(1, at, used("1.00", "bot"))];
        assert!(check_evidence(&ungranted).1[0].contains("before it was granted"));
    }
}
