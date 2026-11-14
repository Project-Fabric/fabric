//! The `evm_test` rail against a real local chain (Anvil, in Docker): refunds go back to the
//! payer once; a held refund pays only after approval; a lost broadcast is completed with the
//! same signed bytes; an attempt whose nonce went to another transaction is void and moves no
//! money. These need Docker, so they are ignored by `cargo test` and run by `check-all`
//! (`cargo test --test evm_rail -- --ignored`). Test chain and test token only.
use std::{
    process::Command,
    time::{Duration, Instant},
};

use cosigna_agent::{
    Actor, Error, Role, Service, api::Credentials, evm_rail::EvmRail, gateway,
    operations::OperationStage,
};
use serde_json::{Value, json};

const IMAGE: &str = "ghcr.io/foundry-rs/foundry:stable";
const AT: i64 = 1_800_000_000;
/// Anvil's first two development accounts. Public keys for local chains, holding nothing real.
const GATEWAY_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const GATEWAY: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const CUSTOMER: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";
const SOMEONE_ELSE: &str = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc";
const BYTECODE: &str = include_str!("fixtures/evm/TestUSD.bytecode");
/// Token units per cent at 6 decimals.
const PER_CENT: u128 = 10_000;

struct Chain {
    container: String,
    url: String,
}

impl Drop for Chain {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .output();
    }
}

impl Chain {
    fn start() -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let output = Command::new("docker")
            .args(["run", "-d", "--rm", "-p"])
            .arg(format!("127.0.0.1:{port}:8545"))
            .args(["--entrypoint", "anvil", IMAGE, "--host", "0.0.0.0"])
            .args(["--chain-id", "31337"])
            .output()
            .expect("docker");
        assert!(
            output.status.success(),
            "anvil did not start: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let chain = Self {
            container: String::from_utf8(output.stdout).unwrap().trim().to_string(),
            url: format!("http://127.0.0.1:{port}"),
        };
        let started = Instant::now();
        while chain.try_rpc("eth_chainId", json!([])).is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "anvil not ready"
            );
            std::thread::sleep(Duration::from_millis(250));
        }
        chain
    }

    fn try_rpc(&self, method: &str, params: Value) -> Option<Value> {
        let mut response = ureq::post(&self.url)
            .send_json(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .ok()?;
        response.body_mut().read_json::<Value>().ok()
    }

    fn rpc(&self, method: &str, params: Value) -> Value {
        let body = self.try_rpc(method, params).expect("rpc");
        assert!(body.get("error").is_none(), "{method}: {body}");
        body["result"].clone()
    }

    /// Sends from an unlocked development account and mines it; returns the receipt.
    fn send(&self, tx: Value) -> Value {
        let hash = self.rpc("eth_sendTransaction", json!([tx]));
        self.mine();
        self.rpc("eth_getTransactionReceipt", json!([hash]))
    }

    fn mine(&self) {
        self.rpc("evm_mine", json!([]));
    }

    fn automine(&self, on: bool) {
        self.rpc("evm_setAutomine", json!([on]));
    }

    fn call_data(selector: &str, address: &str, amount: u128) -> String {
        format!(
            "{selector}{:0>64}{amount:064x}",
            address.trim_start_matches("0x")
        )
    }

    fn balance(&self, token: &str, holder: &str) -> u128 {
        let data = format!("0x70a08231{:0>64}", holder.trim_start_matches("0x"));
        let result = self.rpc("eth_call", json!([{"to": token, "data": data}, "latest"]));
        u128::from_str_radix(result.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
    }

    fn pending_count(&self) -> u64 {
        let n = self.rpc("eth_getTransactionCount", json!([GATEWAY, "pending"]));
        u64::from_str_radix(n.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
    }

    fn pending_hashes(&self) -> Vec<String> {
        let block = self.rpc("eth_getBlockByNumber", json!(["pending", false]));
        block["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap().to_string())
            .collect()
    }
}

struct Lab {
    chain: Chain,
    token: String,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    bot: String,
    _directory: tempfile::TempDir,
}

fn rail_config(chain_id: u64, token: &str, confirmations: u64) -> String {
    json!({"schema":"cosigna.evm-rail/v1","chain_id":chain_id,"rpc_url_env":"RPC",
        "token":token,"decimals":6,"confirmations":confirmations,"private_key_env":"KEY"})
    .to_string()
}

impl Lab {
    fn new(confirmations: u64) -> Self {
        let chain = Chain::start();
        let deployed = chain.send(json!({"from": GATEWAY, "data": BYTECODE.trim()}));
        let token = deployed["contractAddress"].as_str().unwrap().to_string();
        // The customer holds USD 100.00; the gateway's account holds a float of USD 1,000.00.
        for (holder, cents) in [(CUSTOMER, 10_000u128), (GATEWAY, 100_000)] {
            chain.send(json!({"from": GATEWAY, "to": token,
                "data": Chain::call_data("0x40c10f19", holder, cents * PER_CENT)}));
        }
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::open(directory.path()).unwrap();
        let url = chain.url.clone();
        let rail = EvmRail::from_json(
            &rail_config(31_337, &token, confirmations),
            &directory.path().join("evm-test.sqlite"),
            |name| match name {
                "RPC" => Some(url.clone()),
                "KEY" => Some(GATEWAY_KEY.into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(rail.address(), GATEWAY);
        rail.verify_chain().unwrap();
        service.rails = service.rails.clone().with_evm_test(rail);
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let private: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let mut lab = Self {
            chain,
            token,
            service,
            credentials,
            owner: format!("Bearer {}", private["owner"].as_str().unwrap()),
            budget: String::new(),
            bot: String::new(),
            _directory: directory,
        };
        lab.budget = lab
            .call(
                &lab.owner.clone(),
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,"per_purchase_minor":50000,
                    "merchants":["fixture-cafe"],"expires_at":AT+86400}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        lab.bot =
            format!(
            "Bearer {}",
            lab.call(&lab.owner.clone(), "POST", "/agents",
                json!({"name":"stablecoin-refunds","budget_id":lab.budget,"permissions":["execute"],
                    "expires_at":AT+86400,"operations":{"kinds":["refund"],"rails":["evm_test"],
                    "auto_approve_minor":1000}}))
            .unwrap()["data"]["token"]
                .as_str()
                .unwrap()
        );
        lab
    }

    fn call(&self, auth: &str, method: &str, path: &str, body: Value) -> Result<Value, Error> {
        let bytes = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        gateway::dispatch(
            &self.service,
            &cosigna_agent::planner::FixturePlanner,
            &self.credentials,
            auth,
            method,
            &format!("/gateway/v1{path}"),
            &bytes,
            AT,
        )
    }

    /// The customer pays the gateway's account; returns the payment's transaction hash.
    fn customer_pays(&self, cents: u128) -> String {
        let receipt = self.chain.send(json!({"from": CUSTOMER, "to": self.token,
            "data": Chain::call_data("0xa9059cbb", GATEWAY, cents * PER_CENT)}));
        assert_eq!(receipt["status"], "0x1");
        receipt["transactionHash"].as_str().unwrap().to_string()
    }

    fn refund(&self, source: &str, cents: i64, key: &str) -> Result<Value, Error> {
        self.call(
            &self.bot,
            "POST",
            "/operations",
            json!({"budget_id":self.budget,"kind":"refund","rail":"evm_test","source":source,
                "amount_minor":cents,"currency":"USD","reason":"Order cancelled",
                "idempotency_key":key}),
        )
    }

    fn owner_actor() -> Actor {
        Actor {
            owner: "local-user".into(),
            role: Role::Owner,
        }
    }

    fn customer_balance(&self) -> u128 {
        self.chain.balance(&self.token, CUSTOMER)
    }
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn a_refund_goes_back_to_the_payer_once_and_a_held_one_only_after_approval() {
    let lab = Lab::new(1);
    let payment = lab.customer_pays(5000);
    let start = lab.customer_balance();

    // Within the agent's limit: paid at once, to the address that paid.
    let small = lab.refund(&payment, 800, "r1").unwrap();
    assert_eq!(small["data"]["stage"], "succeeded", "{small}");
    assert_eq!(small["data"]["proposal"]["facts"]["customer"], CUSTOMER);
    assert_eq!(small["data"]["proposal"]["facts"]["refundable_minor"], 5000);
    assert_eq!(small["data"]["receipt"]["simulated"], true);
    let tx = small["data"]["receipt"]["rail_reference"].as_str().unwrap();
    let mined = lab.chain.rpc("eth_getTransactionReceipt", json!([tx]));
    assert_eq!(mined["status"], "0x1");
    assert_eq!(mined["from"], GATEWAY);
    assert_eq!(lab.customer_balance(), start + 800 * PER_CENT);

    // The same request again is the same operation: no second transfer.
    let replay = lab.refund(&payment, 800, "r1").unwrap();
    assert_eq!(replay["data"]["id"], small["data"]["id"]);
    assert_eq!(lab.customer_balance(), start + 800 * PER_CENT);

    // Above the limit: held, and nothing moves until the owner approves this exact refund.
    let big = lab.refund(&payment, 2500, "r2").unwrap();
    assert_eq!(big["data"]["stage"], "held");
    assert_eq!(lab.customer_balance(), start + 800 * PER_CENT);
    let approved = lab
        .call(
            &lab.owner,
            "POST",
            &format!(
                "/operations/{}/approve",
                big["data"]["id"].as_str().unwrap()
            ),
            json!({"proposal_digest": big["proposal_digest"]}),
        )
        .unwrap();
    assert_eq!(approved["data"]["stage"], "succeeded", "{approved}");
    assert_eq!(lab.customer_balance(), start + 3300 * PER_CENT);

    // What is left is USD 17.00: more is refused before any decision, as is anything that is
    // not a confirmed payment into this account.
    let unrelated = lab
        .chain
        .send(json!({"from": CUSTOMER, "to": CUSTOMER, "value": "0x1"}));
    let unrelated = unrelated["transactionHash"].as_str().unwrap().to_string();
    let missing = format!("0x{}", "ab".repeat(32));
    // A token payment from the customer to a different address is not a payment to us.
    let elsewhere = lab.chain.send(json!({"from": CUSTOMER, "to": lab.token,
        "data": Chain::call_data("0xa9059cbb", SOMEONE_ELSE, 1000 * PER_CENT)}));
    let elsewhere = elsewhere["transactionHash"].as_str().unwrap().to_string();
    for (source, cents, rule) in [
        (payment.as_str(), 1701, "exceeds_refundable"),
        (unrelated.as_str(), 100, "source_not_found"),
        (missing.as_str(), 100, "source_not_found"),
        (elsewhere.as_str(), 100, "source_not_found"),
        ("not_a_transaction", 100, "source_not_found"),
    ] {
        let denied = lab
            .refund(source, cents, &format!("{source}-{cents}"))
            .unwrap();
        assert_eq!(
            denied["data"]["decision"],
            json!({"outcome":"denied","rule":rule}),
            "{source}: {denied}"
        );
    }
    // The refunds are unchanged; only the customer's own payment elsewhere left their balance.
    assert_eq!(
        lab.customer_balance(),
        start + 3300 * PER_CENT - 1000 * PER_CENT
    );

    let report = lab.service.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn a_refund_is_not_final_until_it_has_its_confirmations() {
    let lab = Lab::new(3);
    let payment = lab.customer_pays(5000);
    // The payment itself needs three blocks before it can be refunded.
    let early = lab.refund(&payment, 500, "early").unwrap();
    assert_eq!(
        early["data"]["decision"],
        json!({"outcome":"denied","rule":"payment_not_settled"})
    );
    lab.chain.mine();
    lab.chain.mine();
    let sent = lab.refund(&payment, 500, "r1").unwrap();
    assert_eq!(sent["data"]["stage"], "unknown", "{sent}");
    let id = sent["data"]["id"].as_str().unwrap().to_string();
    let owner = Lab::owner_actor();
    lab.chain.mine();
    let op = lab.service.reconcile_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Unknown);
    lab.chain.mine();
    let op = lab.service.reconcile_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Succeeded);
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn a_lost_broadcast_is_completed_with_the_same_signed_transaction_exactly_once() {
    let lab = Lab::new(1);
    let payment = lab.customer_pays(5000);
    let start = lab.customer_balance();
    lab.chain.automine(false);
    let sent = lab.refund(&payment, 900, "r1").unwrap();
    assert_eq!(sent["data"]["stage"], "unknown");
    let id = sent["data"]["id"].as_str().unwrap().to_string();
    // The node loses it before mining.
    let pending = lab.chain.pending_hashes();
    assert_eq!(pending.len(), 1);
    lab.chain.rpc("anvil_dropTransaction", json!([pending[0]]));
    lab.chain.mine();
    let owner = Lab::owner_actor();
    let op = lab.service.reconcile_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Unknown);
    assert_eq!(lab.customer_balance(), start);

    // Resolving sends the identical bytes again: same hash, so it can only land once.
    let op = lab.service.resolve_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Unknown);
    assert_eq!(lab.chain.pending_hashes(), pending);
    lab.service.resolve_operation(&owner, &id, AT).unwrap();
    lab.chain.mine();
    let op = lab.service.reconcile_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Succeeded);
    assert_eq!(op.receipt.unwrap().rail_reference, pending[0]);
    assert_eq!(lab.customer_balance(), start + 900 * PER_CENT);
    // Resolving a settled operation sends nothing more.
    lab.service.resolve_operation(&owner, &id, AT).unwrap();
    lab.chain.mine();
    assert_eq!(lab.customer_balance(), start + 900 * PER_CENT);
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn an_attempt_whose_nonce_went_to_another_transaction_is_void_and_moves_no_money() {
    let lab = Lab::new(1);
    let payment = lab.customer_pays(5000);
    let start = lab.customer_balance();
    lab.chain.automine(false);
    let nonce = lab.chain.pending_count();
    let sent = lab.refund(&payment, 1000, "r1").unwrap();
    assert_eq!(sent["data"]["stage"], "unknown");
    let id = sent["data"]["id"].as_str().unwrap().to_string();
    let lost = lab.chain.pending_hashes();
    lab.chain.rpc("anvil_dropTransaction", json!([lost[0]]));
    // Something else spends the account's nonce first (here, a transfer of nothing).
    lab.chain.rpc(
        "eth_sendTransaction",
        json!([{"from": GATEWAY, "to": GATEWAY, "value": "0x0", "nonce": format!("0x{nonce:x}")}]),
    );
    lab.chain.mine();

    let owner = Lab::owner_actor();
    let op = lab.service.resolve_operation(&owner, &id, AT).unwrap();
    assert_eq!(op.stage, OperationStage::Failed, "{op:?}");
    lab.chain.mine();
    assert!(
        lab.chain
            .rpc("eth_getTransactionReceipt", json!([lost[0]]))
            .is_null()
    );
    assert_eq!(lab.customer_balance(), start);

    // The released amount can be refunded again, under a new nonce.
    lab.chain.automine(true);
    let again = lab.refund(&payment, 1000, "r2").unwrap();
    assert_eq!(again["data"]["stage"], "succeeded", "{again}");
    assert_eq!(lab.customer_balance(), start + 1000 * PER_CENT);
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn a_node_on_another_chain_is_refused_before_anything_is_signed() {
    let chain = Chain::start();
    let directory = tempfile::tempdir().unwrap();
    let url = chain.url.clone();
    let token = format!("0x{}", "11".repeat(20));
    // Configured for Sepolia, but the node is a local chain.
    let rail = EvmRail::from_json(
        &rail_config(11_155_111, &token, 1),
        &directory.path().join("evm-test.sqlite"),
        |name| match name {
            "RPC" => Some(url.clone()),
            "KEY" => Some(GATEWAY_KEY.into()),
            _ => None,
        },
    )
    .unwrap();
    assert!(rail.verify_chain().is_err());
    // Connected anyway, it refuses every request before reading or signing anything.
    let mut service = Service::open(directory.path()).unwrap();
    service.rails = service.rails.clone().with_evm_test(rail);
    let owner = Lab::owner_actor();
    let budget = service
        .create_budget(
            &owner,
            serde_json::from_value(json!({"currency":"USD","limit_minor":100000,
            "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400}))
            .unwrap(),
            AT,
        )
        .unwrap();
    let request = serde_json::from_value(json!({"budget_id":budget.id,"kind":"refund",
        "rail":"evm_test","source":format!("0x{}", "ab".repeat(32)),"amount_minor":100,
        "currency":"USD","reason":"r","idempotency_key":"k"}))
    .unwrap();
    assert!(service.request_operation(&owner, request, AT).is_err());
    assert!(
        !directory.path().join("evm-test.sqlite").exists()
            || rusqlite::Connection::open(directory.path().join("evm-test.sqlite"))
                .unwrap()
                .query_row("SELECT COUNT(*) FROM evm_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap()
                == 0
    );
}

#[test]
#[ignore = "needs Docker: runs a local Anvil chain"]
fn a_dropped_attempt_keeps_its_nonce_and_the_next_refund_takes_a_new_one() {
    let lab = Lab::new(1);
    let payment = lab.customer_pays(5000);
    let start = lab.customer_balance();
    lab.chain.automine(false);
    let first = lab.refund(&payment, 700, "r1").unwrap();
    let dropped = lab.chain.pending_hashes();
    lab.chain.rpc("anvil_dropTransaction", json!([dropped[0]]));
    // The next refund must not take the dropped attempt's nonce, which it still owns.
    let second = lab.refund(&payment, 600, "r2").unwrap();
    assert_eq!(second["data"]["stage"], "unknown");
    let owner = Lab::owner_actor();
    for op in [&first, &second] {
        lab.service
            .resolve_operation(&owner, op["data"]["id"].as_str().unwrap(), AT)
            .unwrap();
    }
    lab.chain.mine();
    for op in [&first, &second] {
        let id = op["data"]["id"].as_str().unwrap();
        let settled = lab.service.reconcile_operation(&owner, id, AT).unwrap();
        assert_eq!(settled.stage, OperationStage::Succeeded, "{settled:?}");
    }
    assert_eq!(lab.customer_balance(), start + 1300 * PER_CENT);
}

#[test]
fn main_networks_and_unsafe_settings_are_refused_without_echoing_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("evm-test.sqlite");
    let token = format!("0x{}", "11".repeat(20));
    let secret = |name: &str| match name {
        "RPC" => Some("https://rpc.example.test/v2/very-secret-api-key".to_string()),
        "PLAIN" => Some("http://rpc.example.test".to_string()),
        "KEY" => Some(GATEWAY_KEY.to_string()),
        "BADKEY" => Some("0x1234".to_string()),
        _ => None,
    };
    let ok = rail_config(11_155_111, &token, 3);
    assert!(EvmRail::from_json(&ok, &store, secret).is_ok());
    let base = json!({"schema":"cosigna.evm-rail/v1","chain_id":11155111,"rpc_url_env":"RPC",
        "token":token,"private_key_env":"KEY"});
    let with = |field: &str, value: Value| {
        let mut config = base.clone();
        config[field] = value;
        config.to_string()
    };
    for config in [
        with("chain_id", json!(1)),    // Ethereum
        with("chain_id", json!(8453)), // Base
        with("chain_id", json!(137)),  // Polygon
        with("schema", json!("cosigna.evm-rail/v2")),
        with("rpc_url_env", json!("PLAIN")),
        with("rpc_url_env", json!("MISSING")),
        with("private_key_env", json!("BADKEY")),
        with("private_key_env", json!("MISSING")),
        with("token", json!("0x1234")),
        with("decimals", json!(1)),
        with("confirmations", json!(0)),
        with("extra", json!(1)),
    ] {
        let error = EvmRail::from_json(&config, &store, secret).err();
        let message = format!("{error:?}");
        assert!(error.is_some(), "accepted: {config}");
        assert!(
            !message.contains("very-secret-api-key") && !message.contains(&GATEWAY_KEY[2..]),
            "{message}"
        );
    }
}
