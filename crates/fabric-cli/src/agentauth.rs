//! The link to an AgentAuth API: money moves only when the user's consent allows it.
//!
//! The gateway (`fabric mcp-proxy`) and AgentAuth are one product. AgentAuth holds what people
//! granted (consents with spending limits); the gateway sits in front of the tools that move
//! money. Before a payment tool call goes through, the gateway asks AgentAuth:
//!
//! ```text
//! agent ── create_refund(amount=4000, currency=usd) ──▶ gateway ── POST /v1/authorize ──▶ AgentAuth
//!                                                          │        ALLOW / DENY / STEP_UP
//!                                                          ▼
//!                                         ALLOW: forwarded to Stripe   DENY: refused
//!                                         STEP_UP: held for a person   unreachable: refused
//! ```
//!
//! Setup, per project:
//!
//! ```text
//! fabric secret set agentauth-api < api-key.txt          # the AgentAuth API key
//! fabric secret set consent-acme < delegation-token.txt  # the consent the user granted
//! fabric connect agentauth --url https://api.agentauth.in --api-key-secret agentauth-api
//! fabric payments add stripe/create_refund --amount amount --currency currency --minor-units
//! fabric mcp-proxy --name stripe --consent consent-acme --secret-env STRIPE_SECRET_KEY=stripe-test -- <server>
//! ```
//!
//! The gateway holds the consent's delegation token, so the agent cannot spend it anywhere
//! else. Local `deny` rules still win. Every decision is recorded in the evidence with the
//! authorization code, so a payment can be traced from the agent's call to the consent.

use std::fs;
use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CliError, exit};
use crate::evidence;
use crate::home::{Home, Project, display_path, write_durably};
use crate::identity;

const FILE: &str = "agentauth.json";
const FORMAT: &str = "fabric-agentauth";
const VERSION: u32 = 1;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Where a payment tool keeps its amount, currency and counterparty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentTool {
    /// `server/tool`, exactly.
    pub tool: String,
    /// The argument holding the amount; dotted for nested fields (`transfer.amount`).
    pub amount: String,
    /// Amounts are integers in the currency's smallest unit (cents), as Stripe sends them.
    #[serde(default)]
    pub minor_units: bool,
    #[serde(default)]
    pub currency: Option<String>,
    /// Used when the call has no currency argument.
    #[serde(default)]
    pub default_currency: Option<String>,
    #[serde(default)]
    pub merchant: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key_secret: Option<String>,
    #[serde(default)]
    pub payments: Vec<PaymentTool>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    config: Config,
}

pub fn load(project: &Project) -> Result<Config, CliError> {
    let path = project.dir.join(FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let file: ConfigFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_agentauth",
                    format!("{}: {e}", display_path(&path)),
                )
            })?;
            if file.format != FORMAT || file.version != VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_agentauth",
                    format!("{}: unsupported file", display_path(&path)),
                ));
            }
            Ok(file.config)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(CliError::io(&path, e)),
    }
}

fn save(project: &Project, config: &Config) -> Result<(), CliError> {
    let file = ConfigFile {
        format: FORMAT.to_owned(),
        version: VERSION,
        config: config.clone(),
    };
    write_durably(
        &project.dir.join(FILE),
        &serde_json::to_vec_pretty(&file).expect("config serializes"),
    )
}

/// Digits after the decimal point in a currency's major unit (ISO 4217).
fn exponent(currency: &str) -> u32 {
    match currency.to_ascii_uppercase().as_str() {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX"
        | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

fn field<'a>(arguments: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(arguments, |v, key| v.get(key))
}

/// A payment as AgentAuth sees it: a decimal amount in major units, and its currency.
#[derive(Debug, Clone, PartialEq)]
pub struct Payment {
    pub amount: String,
    pub currency: String,
    pub merchant: Option<String>,
}

impl PaymentTool {
    /// Reads the payment from a call's arguments. `Err` when it cannot be read: a payment
    /// whose amount is unknown is not authorized.
    pub fn payment(&self, arguments: &Value) -> Result<Payment, String> {
        let currency = self
            .currency
            .as_deref()
            .and_then(|f| field(arguments, f))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.default_currency.clone())
            .ok_or("the call has no currency")?
            .to_ascii_uppercase();
        let raw = field(arguments, &self.amount).ok_or("the call has no amount")?;
        let text = match raw {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => return Err("the amount is not a number".to_owned()),
        };
        let amount = if self.minor_units {
            let minor: u64 = text
                .parse()
                .map_err(|_| format!("{text} is not a whole number of minor units"))?;
            let places = exponent(&currency);
            if places == 0 {
                minor.to_string()
            } else {
                let unit = 10u64.pow(places);
                format!(
                    "{}.{:0width$}",
                    minor / unit,
                    minor % unit,
                    width = places as usize
                )
            }
        } else {
            text
        };
        let merchant = self
            .merchant
            .as_deref()
            .and_then(|f| field(arguments, f))
            .and_then(|v| v.as_str().map(str::to_owned));
        Ok(Payment {
            amount,
            currency,
            merchant,
        })
    }
}

/// A project's live connection, ready for the proxy.
pub struct Connection {
    url: String,
    api_key: String,
    consent: Option<String>,
    payments: Vec<PaymentTool>,
}

pub enum Decision {
    Allow {
        code: String,
        consent: Option<String>,
    },
    Deny {
        reason: String,
        message: String,
    },
    StepUp {
        message: String,
    },
}

impl Connection {
    /// The project's connection, if it has one. `consent_secret` names the stored delegation
    /// token the proxy authorizes with.
    pub fn open(
        home: &Home,
        project: &Project,
        consent_secret: Option<&str>,
    ) -> Result<Option<Self>, CliError> {
        let config = load(project)?;
        let (Some(url), Some(key_name)) = (config.url, config.api_key_secret) else {
            if consent_secret.is_some() {
                return Err(CliError::new(
                    exit::ERROR,
                    "not_connected",
                    "--consent needs a connection: `fabric connect agentauth --url ... --api-key-secret ...`",
                ));
            }
            return Ok(None);
        };
        let api_key = identity::secret_value(home, &key_name)?;
        let consent = consent_secret
            .map(|name| identity::secret_value(home, name))
            .transpose()?;
        Ok(Some(Self {
            url: url.trim_end_matches('/').to_owned(),
            api_key,
            consent,
            payments: config.payments,
        }))
    }

    /// The payment tool `tool` is, if it is one.
    pub fn payment_tool(&self, tool: &str) -> Option<&PaymentTool> {
        self.payments.iter().find(|p| p.tool == tool)
    }

    /// Asks AgentAuth whether the consent allows this payment. `Err` when there is no
    /// answer (unreachable, refused, malformed): the caller refuses the payment.
    pub fn authorize(&self, payment: &Payment, tool: &str) -> Result<Decision, String> {
        let consent = self
            .consent
            .as_deref()
            .ok_or("this gateway has no consent to pay with (start it with --consent NAME)")?;
        let body = json!({
            "delegation_token": consent,
            "action": "payment",
            "transaction": {
                "amount": payment.amount,
                "currency": payment.currency,
                "merchant_id": payment.merchant.clone().unwrap_or_else(|| tool.to_owned()),
                "description": format!("agent tool call {tool}"),
            },
        });
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = agent
            .post(&format!("{}/v1/authorize", self.url))
            .header("X-API-Key", &self.api_key)
            .send_json(&body)
            .map_err(|e| format!("AgentAuth could not be reached: {e}"))?;
        let status = response.status().as_u16();
        let reply: Value = response
            .body_mut()
            .read_json()
            .map_err(|e| format!("AgentAuth answered {status} with an unreadable body: {e}"))?;
        if status != 200 {
            return Err(format!(
                "AgentAuth refused the request ({status}): {}",
                reply["detail"].as_str().unwrap_or("no detail")
            ));
        }
        let text = |k: &str| reply[k].as_str().unwrap_or_default().to_owned();
        match reply["decision"].as_str() {
            Some("ALLOW") => Ok(Decision::Allow {
                code: text("authorization_code"),
                consent: reply["consent_id"].as_str().map(str::to_owned),
            }),
            Some("DENY") => Ok(Decision::Deny {
                reason: text("reason"),
                message: text("message"),
            }),
            Some("STEP_UP") => Ok(Decision::StepUp {
                message: text("message"),
            }),
            other => Err(format!("AgentAuth gave no decision ({other:?})")),
        }
    }
}

// ---- Commands ------------------------------------------------------------------------------

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
    match crate::approvals::in_agent_environment() {
        Some(marker) => Err(CliError::new(
            exit::ERROR,
            "from_agent",
            format!(
                "connecting AgentAuth is done by a person, not from inside an agent session ({marker} is set)"
            ),
        )),
        None => Ok(()),
    }
}

pub fn connect_cmd(
    home: &Home,
    project: &Project,
    url: &str,
    api_key_secret: &str,
    json_output: bool,
) -> Result<u8, CliError> {
    person_only()?;
    if !(url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost"))
    {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_url",
            "the AgentAuth URL must be https (plain http only for localhost)",
        ));
    }
    // The key must exist now, not at the first payment.
    identity::secret_value(home, api_key_secret)?;
    let mut config = load(project)?;
    config.url = Some(url.trim_end_matches('/').to_owned());
    config.api_key_secret = Some(api_key_secret.to_owned());
    save(project, &config)?;
    evidence::append(
        project,
        json!({ "kind": "policy_changed", "change": "connected to AgentAuth", "url": config.url, "by": evidence::os_user() }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.connect/v1", "url": config.url, "api_key_secret": api_key_secret }),
        &format!(
            "Connected to AgentAuth at {}. Declare payment tools with `fabric payments add`.",
            config.url.as_deref().unwrap_or_default()
        ),
    );
    Ok(exit::OK)
}

pub fn payments_add_cmd(
    project: &Project,
    tool: PaymentTool,
    json_output: bool,
) -> Result<u8, CliError> {
    person_only()?;
    if !tool.tool.contains('/') || tool.tool.contains('*') {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "name the payment tool exactly, as server/tool",
        ));
    }
    if tool.currency.is_none() && tool.default_currency.is_none() {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_arguments",
            "give --currency (the argument holding it) or --default-currency",
        ));
    }
    let mut config = load(project)?;
    config.payments.retain(|p| p.tool != tool.tool);
    config.payments.push(tool.clone());
    save(project, &config)?;
    evidence::append(
        project,
        json!({ "kind": "policy_changed", "change": "payment tool declared", "tool": tool, "by": evidence::os_user() }),
    )?;
    print(
        json_output,
        json!({ "schema": "fabric.payments.add/v1", "payment": tool }),
        &format!(
            "Calls to {} are now authorized by AgentAuth before they run.",
            tool.tool
        ),
    );
    Ok(exit::OK)
}

pub fn payments_list_cmd(project: &Project, json_output: bool) -> Result<u8, CliError> {
    let config = load(project)?;
    let human = if config.payments.is_empty() {
        "No payment tools declared.".to_owned()
    } else {
        config
            .payments
            .iter()
            .map(|p| {
                format!(
                    "{}  amount={}{}  currency={}",
                    p.tool,
                    p.amount,
                    if p.minor_units { " (minor units)" } else { "" },
                    p.currency
                        .clone()
                        .or_else(|| p.default_currency.clone())
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    print(
        json_output,
        json!({ "schema": "fabric.payments.list/v1", "url": config.url, "payments": config.payments }),
        &human,
    );
    Ok(exit::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(minor: bool) -> PaymentTool {
        PaymentTool {
            tool: "stripe/create_refund".to_owned(),
            amount: "amount".to_owned(),
            minor_units: minor,
            currency: Some("currency".to_owned()),
            default_currency: Some("USD".to_owned()),
            merchant: Some("customer.id".to_owned()),
        }
    }

    #[test]
    fn minor_units_become_exact_decimals_per_currency() {
        let t = tool(true);
        let p = t
            .payment(&json!({ "amount": 4005, "currency": "usd" }))
            .unwrap();
        assert_eq!((p.amount.as_str(), p.currency.as_str()), ("40.05", "USD"));
        assert_eq!(
            t.payment(&json!({ "amount": 7, "currency": "usd" }))
                .unwrap()
                .amount,
            "0.07"
        );
        assert_eq!(
            t.payment(&json!({ "amount": 1500, "currency": "jpy" }))
                .unwrap()
                .amount,
            "1500"
        );
        assert_eq!(
            t.payment(&json!({ "amount": 1234, "currency": "kwd" }))
                .unwrap()
                .amount,
            "1.234"
        );
        assert_eq!(
            t.payment(&json!({ "amount": 500 })).unwrap().currency,
            "USD"
        );
        assert!(
            t.payment(&json!({ "amount": 12.5, "currency": "usd" }))
                .is_err()
        );
        assert!(t.payment(&json!({ "currency": "usd" })).is_err());
    }

    #[test]
    fn major_units_and_nested_fields() {
        let t = tool(false);
        let p = t
            .payment(
                &json!({ "amount": "19.99", "currency": "eur", "customer": { "id": "cus_1" } }),
            )
            .unwrap();
        assert_eq!(p.amount, "19.99");
        assert_eq!(p.merchant.as_deref(), Some("cus_1"));
    }
}
