//! Replay: run a proposed policy over a company's past refunds and payouts before switching it
//! on. Each past operation goes through the same `operations::decide` and `apply_case` the live
//! gateway uses, in time order, with the budget, refundable amounts and case commitments
//! tracked as it goes. Local only: the history is read from a file and nothing is sent anywhere.
//!
//! What it assumes, stated in every report: operations held for a person are approved by that
//! person (they happened), so they draw on the budget like automatic ones; denied operations
//! are not paid; a refund's refundable amount is checked only when the history gives the
//! payment's amount.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    Budget, BudgetInput, Error, MAX_MINOR, Result,
    operations::{
        Case, Decision, OperationGrant, OperationKind, OperationRequest, apply_case, decide,
    },
};

pub const API_VERSION: &str = "cosigna.replay/v1";
const RAIL: &str = "replay";

pub const NOTES: [&str; 3] = [
    "Uses the live gateway's own decision rules; the history never leaves this machine.",
    "Assumes operations held for a person were approved by them (they happened), so they draw on the budget; denied operations are not paid.",
    "A refund's remaining refundable amount is checked only where the history gives the payment's amount.",
];

/// The policy to try: one budget and the agent grant that would make these operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub budget: PolicyBudget,
    pub grant: PolicyGrant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyBudget {
    pub currency: String,
    pub limit_minor: i64,
    pub per_operation_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_approval_above_minor: Option<i64>,
    /// Models issuing a fresh budget every N days (from the first operation). Absent: one
    /// budget for the whole history, as the gateway's budgets are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renew_every_days: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyGrant {
    pub kinds: Vec<OperationKind>,
    pub auto_approve_minor: i64,
    #[serde(default)]
    pub require_case: bool,
}

/// One past operation, from a CSV row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Past {
    pub id: String,
    pub at: i64,
    pub kind: OperationKind,
    pub source: Option<String>,
    pub payee: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub customer: Option<String>,
    pub case_id: Option<String>,
    /// The original payment's amount, when known (refunds).
    pub payment_amount_minor: Option<i64>,
    /// The company marked this operation as a mistake after the fact.
    pub mistake: bool,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Tally {
    pub count: u64,
    pub amount_minor: i64,
}

#[derive(Debug, Serialize)]
pub struct RuleTally {
    pub outcome: &'static str,
    pub rule: String,
    pub count: u64,
    pub amount_minor: i64,
}

#[derive(Debug, Serialize)]
pub struct Replayed {
    pub id: String,
    pub at: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub outcome: &'static str,
    pub rule: String,
    /// `checked`: it named a case found in the case records, so it was checked against what
    /// the business said was owed. `unknown`: no record to check, so whether it was owed is
    /// unknown, whatever the outcome.
    pub case_check: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Mistakes {
    pub labelled: u64,
    /// Held for a person or denied: not paid without someone looking.
    pub stopped: u64,
    pub stopped_amount_minor: i64,
    pub paid_automatically: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub policy: Policy,
    pub operations: u64,
    pub from: i64,
    pub to: i64,
    /// In the budget's currency; other currencies are denied and counted separately.
    pub automatic: Tally,
    pub held_for_a_person: Tally,
    pub denied: Tally,
    pub other_currency: u64,
    /// Paid automatically with no business record (case) to check it against: the policy
    /// allowed it, but nothing confirmed it was owed.
    pub paid_without_a_record: Tally,
    pub held_per_week: f64,
    pub by_rule: Vec<RuleTally>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mistakes: Option<Mistakes>,
    pub replayed: Vec<Replayed>,
    pub notes: [&'static str; 3],
}

impl Policy {
    fn validate(&self) -> Result<()> {
        let b = &self.budget;
        let mut kinds = self.grant.kinds.clone();
        kinds.sort_by_key(|k| *k == OperationKind::Refund);
        kinds.dedup();
        if b.currency.len() != 3
            || !b.currency.bytes().all(|c| c.is_ascii_uppercase())
            || !(1..=MAX_MINOR).contains(&b.limit_minor)
            || !(1..=b.limit_minor).contains(&b.per_operation_minor)
            || b.second_approval_above_minor
                .is_some_and(|t| !(0..=MAX_MINOR).contains(&t))
            || b.renew_every_days.is_some_and(|d| !(1..=3660).contains(&d))
            || self.grant.kinds.is_empty()
            || kinds.len() != self.grant.kinds.len()
            || !(0..=MAX_MINOR).contains(&self.grant.auto_approve_minor)
        {
            return Err(Error::Invalid("invalid replay policy".into()));
        }
        Ok(())
    }
}

/// Replays `history` (any order; sorted by time, then id) under `policy`.
pub fn replay(policy: Policy, mut history: Vec<Past>, cases: &[Case]) -> Result<Report> {
    policy.validate()?;
    history.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    let budget = Budget {
        id: "replay".into(),
        owner: "replay".into(),
        terms: BudgetInput {
            currency: policy.budget.currency.clone(),
            limit_minor: policy.budget.limit_minor,
            per_purchase_minor: policy.budget.per_operation_minor,
            merchants: vec![RAIL.into()],
            expires_at: i64::MAX,
            second_approval_above_minor: policy.budget.second_approval_above_minor,
        },
        revoked: false,
    };
    let grant = OperationGrant {
        kinds: policy.grant.kinds.clone(),
        rails: vec![RAIL.into()],
        auto_approve_minor: policy.grant.auto_approve_minor,
        require_case: policy.grant.require_case,
    };
    let start = history.first().map_or(0, |p| p.at);
    let period = |at: i64| {
        policy
            .budget
            .renew_every_days
            .map_or(0, |days| (at - start).max(0) / (days * 86_400))
    };
    let mut spent: BTreeMap<i64, i64> = BTreeMap::new();
    let mut refunded: BTreeMap<String, i64> = BTreeMap::new();
    let mut committed: BTreeMap<String, i64> = BTreeMap::new();
    let (mut automatic, mut held, mut denied) =
        (Tally::default(), Tally::default(), Tally::default());
    let mut other_currency = 0;
    let mut unchecked = Tally::default();
    let mut rules: BTreeMap<(&'static str, String), Tally> = BTreeMap::new();
    let mut replayed = Vec::with_capacity(history.len());
    let mut mistakes = Mistakes {
        labelled: 0,
        stopped: 0,
        stopped_amount_minor: 0,
        paid_automatically: Vec::new(),
    };
    for past in &history {
        let request = OperationRequest {
            budget_id: budget.id.clone(),
            kind: past.kind,
            rail: RAIL.into(),
            source: past.source.clone(),
            payee: past.payee.clone(),
            amount_minor: past.amount_minor,
            currency: past.currency.clone(),
            reason: "replay".into(),
            idempotency_key: past.id.clone(),
            case_id: past.case_id.clone(),
        };
        let used = spent.entry(period(past.at)).or_default();
        // The rail's own check comes first, as in the gateway.
        let already = past
            .source
            .as_ref()
            .map_or(0, |s| refunded.get(s).copied().unwrap_or(0));
        let decision = match past.payment_amount_minor {
            Some(paid)
                if past.kind == OperationKind::Refund && past.amount_minor > paid - already =>
            {
                Decision::Denied("exceeds_refundable".into())
            }
            _ => {
                let case = past
                    .case_id
                    .as_ref()
                    .and_then(|id| cases.iter().find(|c| &c.id == id));
                let case_committed = past
                    .case_id
                    .as_ref()
                    .map_or(0, |id| committed.get(id).copied().unwrap_or(0));
                apply_case(
                    decide(
                        &request,
                        &budget,
                        budget.terms.limit_minor - *used,
                        Some(Some(&grant)),
                    ),
                    &request,
                    Some(Some(&grant)),
                    case,
                    case_committed,
                    &json!({"customer": past.customer}),
                )
            }
        };
        let case_check = match &past.case_id {
            Some(id) if cases.iter().any(|c| &c.id == id) => "checked",
            _ => "unknown",
        };
        let (outcome, rule, paid) = match decision {
            Decision::AutoApproved(rule) => ("automatic", rule, true),
            Decision::Held(rule) => ("held_for_a_person", rule, true),
            Decision::Denied(rule) => ("denied", rule, false),
        };
        if paid {
            *used += past.amount_minor;
            if let Some(source) = &past.source {
                *refunded.entry(source.clone()).or_default() += past.amount_minor;
            }
            if let Some(case) = &past.case_id {
                *committed.entry(case.clone()).or_default() += past.amount_minor;
            }
        }
        let in_currency = past.currency == budget.terms.currency;
        if !in_currency {
            other_currency += 1;
        }
        let amount = if in_currency { past.amount_minor } else { 0 };
        if outcome == "automatic" && past.case_id.is_none() {
            unchecked.count += 1;
            unchecked.amount_minor += amount;
        }
        let tally = match outcome {
            "automatic" => &mut automatic,
            "held_for_a_person" => &mut held,
            _ => &mut denied,
        };
        tally.count += 1;
        tally.amount_minor += amount;
        let by_rule = rules.entry((outcome, rule.clone())).or_default();
        by_rule.count += 1;
        by_rule.amount_minor += amount;
        if past.mistake {
            mistakes.labelled += 1;
            if outcome == "automatic" {
                mistakes.paid_automatically.push(past.id.clone());
            } else {
                mistakes.stopped += 1;
                mistakes.stopped_amount_minor += amount;
            }
        }
        replayed.push(Replayed {
            id: past.id.clone(),
            at: past.at,
            amount_minor: past.amount_minor,
            currency: past.currency.clone(),
            outcome,
            rule,
            case_check,
        });
    }
    let (from, to) = (start, history.last().map_or(0, |p| p.at));
    let days = ((to - from) as f64 / 86_400.0).max(1.0);
    Ok(Report {
        operations: history.len() as u64,
        from,
        to,
        held_per_week: (held.count as f64 * 7.0 / days * 10.0).round() / 10.0,
        automatic,
        held_for_a_person: held,
        denied,
        other_currency,
        paid_without_a_record: unchecked,
        by_rule: rules
            .into_iter()
            .map(|((outcome, rule), t)| RuleTally {
                outcome,
                rule,
                count: t.count,
                amount_minor: t.amount_minor,
            })
            .collect(),
        mistakes: (mistakes.labelled > 0).then_some(mistakes),
        replayed,
        policy,
        notes: NOTES,
    })
}

// ---------- CSV input ----------

/// Minimal RFC 4180 reader: comma-separated, double-quoted fields may contain commas, quotes
/// (doubled) and newlines. Returns rows of fields.
fn csv_rows(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let (mut row, mut field) = (Vec::new(), String::new());
    let (mut quoted, mut chars) = (
        false,
        text.trim_start_matches('\u{feff}').chars().peekable(),
    );
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            (false, c) => field.push(c),
        }
    }
    if quoted {
        return Err(Error::Invalid("CSV ends inside a quoted field".into()));
    }
    row.push(field);
    if row.iter().any(|f| !f.is_empty()) {
        rows.push(row);
    }
    Ok(rows)
}

fn columns(rows: &[Vec<String>], required: &[&str]) -> Result<BTreeMap<String, usize>> {
    let header = rows
        .first()
        .ok_or_else(|| Error::Invalid("CSV has no header row".into()))?;
    let map: BTreeMap<String, usize> = header
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
        .collect();
    for name in required {
        if !map.contains_key(*name) {
            return Err(Error::Invalid(format!(
                "CSV is missing the `{name}` column"
            )));
        }
    }
    Ok(map)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
}

/// Reads a history CSV. Columns: `id`, `at` (Unix seconds or RFC 3339 UTC like
/// `2026-09-01T10:00:00Z`), `kind` (`refund`/`payout`), `amount_minor`, `currency`, and
/// optionally `source`, `payee`, `customer`, `case_id`, `payment_amount_minor`, `mistake`
/// (`yes`/`true`/`1`).
pub fn read_history(text: &str) -> Result<Vec<Past>> {
    let rows = csv_rows(text)?;
    let col = columns(&rows, &["id", "at", "kind", "amount_minor", "currency"])?;
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate().skip(1) {
        let line = index + 1;
        let get = |name: &str| {
            col.get(name)
                .and_then(|i| row.get(*i))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let bad = |what: &str| Error::Invalid(format!("history row {line}: {what}"));
        let id = get("id")
            .filter(|v| identifier(v))
            .ok_or_else(|| bad("invalid id"))?;
        let at = get("at")
            .and_then(|v| parse_time(&v))
            .ok_or_else(|| bad("invalid at"))?;
        let kind = match get("kind").as_deref() {
            Some("refund") => OperationKind::Refund,
            Some("payout") => OperationKind::Payout,
            _ => return Err(bad("kind must be refund or payout")),
        };
        let amount = |name: &str| -> Result<Option<i64>> {
            get(name)
                .map(|v| {
                    v.parse::<i64>()
                        .ok()
                        .filter(|a| (1..=MAX_MINOR).contains(a))
                        .ok_or_else(|| bad(&format!("invalid {name}")))
                })
                .transpose()
        };
        let amount_minor = amount("amount_minor")?.ok_or_else(|| bad("missing amount_minor"))?;
        let currency = get("currency")
            .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or_else(|| bad("currency must be three uppercase letters"))?;
        let optional_id = |name: &str| -> Result<Option<String>> {
            match get(name) {
                Some(v) if identifier(&v) => Ok(Some(v)),
                Some(_) => Err(bad(&format!("invalid {name}"))),
                None => Ok(None),
            }
        };
        let source = optional_id("source")?;
        let payee = optional_id("payee")?;
        if (kind == OperationKind::Refund && source.is_none())
            || (kind == OperationKind::Payout && payee.is_none())
        {
            return Err(bad("a refund needs `source`, a payout needs `payee`"));
        }
        out.push(Past {
            id,
            at,
            kind,
            source,
            payee,
            amount_minor,
            currency,
            customer: optional_id("customer")?,
            case_id: optional_id("case_id")?,
            payment_amount_minor: amount("payment_amount_minor")?,
            mistake: matches!(
                get("mistake").map(|v| v.to_ascii_lowercase()).as_deref(),
                Some("yes" | "true" | "1")
            ),
        });
    }
    Ok(out)
}

/// Reads a cases CSV: `id`, `customer`, `source`, `currency`, `owed_minor`.
pub fn read_cases(text: &str) -> Result<Vec<Case>> {
    let rows = csv_rows(text)?;
    let col = columns(
        &rows,
        &["id", "customer", "source", "currency", "owed_minor"],
    )?;
    rows.iter()
        .enumerate()
        .skip(1)
        .map(|(index, row)| {
            let get = |name: &str| {
                col.get(name)
                    .and_then(|i| row.get(*i))
                    .map(|v| v.trim().to_string())
            };
            let bad = || Error::Invalid(format!("cases row {}: invalid value", index + 1));
            let id = get("id").filter(|v| identifier(v)).ok_or_else(bad)?;
            let owed = get("owed_minor")
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|a| (1..=MAX_MINOR).contains(a))
                .ok_or_else(bad)?;
            Ok(Case {
                id,
                customer: get("customer").filter(|v| identifier(v)).ok_or_else(bad)?,
                source: get("source").filter(|v| identifier(v)).ok_or_else(bad)?,
                currency: get("currency").filter(|c| c.len() == 3).ok_or_else(bad)?,
                owed_minor: owed,
            })
        })
        .collect()
}

/// Unix seconds, or RFC 3339 in UTC (`YYYY-MM-DDTHH:MM:SSZ`, optional fraction).
fn parse_time(value: &str) -> Option<i64> {
    if let Ok(seconds) = value.parse::<i64>() {
        return (0..=253_402_300_799).contains(&seconds).then_some(seconds);
    }
    let v = value.strip_suffix('Z')?;
    let (date, time) = v.split_once('T')?;
    let time = time.split('.').next()?;
    let d: Vec<i64> = date
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let t: Vec<i64> = time
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if d.len() != 3
        || t.len() != 3
        || !(1..=12).contains(&d[1])
        || !(1..=31).contains(&d[2])
        || !(0..24).contains(&t[0])
        || !(0..60).contains(&t[1])
        || !(0..61).contains(&t[2])
    {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if d[1] <= 2 {
        (d[0] - 1, d[1] + 9)
    } else {
        (d[0], d[1] - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d[2] - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + t[0] * 3600 + t[1] * 60 + t[2])
}

/// A short, plain-language summary for a meeting.
pub fn summary(report: &Report) -> String {
    let c = &report.policy.budget.currency;
    let money = |minor: i64| format!("{} {}.{:02}", c, minor / 100, minor % 100);
    let mut out = format!(
        "Replayed {} operations under the proposed policy.\n\
         Paid automatically:   {:>5}  {}\n\
         Held for a person:    {:>5}  {}  (about {} a week)\n\
         Stopped:              {:>5}  {}\n",
        report.operations,
        report.automatic.count,
        money(report.automatic.amount_minor),
        report.held_for_a_person.count,
        money(report.held_for_a_person.amount_minor),
        report.held_per_week,
        report.denied.count,
        money(report.denied.amount_minor),
    );
    if report.paid_without_a_record.count > 0 {
        out += &format!(
            "Paid automatically with no business record to check against: {} ({}); whether these were owed is unknown.
",
            report.paid_without_a_record.count,
            money(report.paid_without_a_record.amount_minor)
        );
    }
    if report.other_currency > 0 {
        out += &format!(
            "({} operations in other currencies were stopped and not summed.)\n",
            report.other_currency
        );
    }
    if let Some(m) = &report.mistakes {
        out += &format!(
            "Known mistakes: {} of {} would not have been paid without a person ({}).\n",
            m.stopped,
            m.labelled,
            money(m.stopped_amount_minor)
        );
        if !m.paid_automatically.is_empty() {
            out += &format!(
                "  Still paid automatically: {}\n",
                m.paid_automatically.join(", ")
            );
        }
    }
    out += "Why operations were held or stopped:\n";
    for r in report.by_rule.iter().filter(|r| r.outcome != "automatic") {
        out += &format!(
            "  {:<18} {:<28} {:>5}  {}\n",
            r.outcome,
            r.rule,
            r.count,
            money(r.amount_minor)
        );
    }
    for note in report.notes {
        out += &format!("Note: {note}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_handles_quotes_commas_newlines_and_a_bom() {
        let rows = csv_rows("\u{feff}a,b\r\n\"x, \"\"y\"\"\nz\",2\n\n").unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["x, \"y\"\nz", "2"]]);
        assert!(csv_rows("a\n\"open").is_err());
    }

    #[test]
    fn times_parse_as_utc() {
        assert_eq!(parse_time("1800000000"), Some(1_800_000_000));
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-26T12:30:15.250Z"), Some(1_793_017_815));
        assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        for bad in [
            "2026-10-26T12:30:15",
            "2026-13-01T00:00:00Z",
            "yesterday",
            "-5",
        ] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }
}
