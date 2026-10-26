use std::sync::{Arc, Barrier};

use cosigna_agent::{
    Actor, BudgetInput, Error, Plan, Role, Service, Stage, Task, TaskInput, api,
    planner::{FixturePlanner, Planner},
    provider::{Outcome, Scenario},
};
use serde_json::json;

const AT: i64 = 1_800_000_000;

fn actor(role: Role) -> Actor {
    Actor {
        owner: "alice".into(),
        role,
    }
}

struct Setup {
    directory: tempfile::TempDir,
    service: Service,
    budget: String,
}

impl Setup {
    fn new(limit: i64) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let budget = service
            .create_budget(
                &actor(Role::Owner),
                BudgetInput {
                    currency: "USD".into(),
                    limit_minor: limit,
                    per_purchase_minor: limit,
                    merchants: vec!["fixture-cafe".into()],
                    expires_at: AT + 3600,
                    second_approval_above_minor: None,
                },
                AT,
            )
            .unwrap()
            .id;
        Self {
            directory,
            service,
            budget,
        }
    }
    fn input(&self, key: &str) -> TaskInput {
        TaskInput {
            budget_id: self.budget.clone(),
            request: "Order one oat latte".into(),
            idempotency_key: key.into(),
            required_features: vec!["oat_milk".into()],
        }
    }
    fn quote(&self, key: &str) -> Task {
        let task = self
            .service
            .create_task(&actor(Role::Planner), self.input(key), AT)
            .unwrap();
        self.service
            .plan(&actor(Role::Planner), &task.id, &FixturePlanner, AT)
            .unwrap()
    }
    fn approve(&self, key: &str) -> Task {
        let task = self.quote(key);
        self.service
            .approve(
                &actor(Role::Owner),
                &task.id,
                &task.quote.as_ref().unwrap().digest().unwrap(),
                AT,
            )
            .unwrap()
    }
}

#[test]
fn exact_quote_approval_and_replays_preserve_one_purchase_across_restart() {
    let s = Setup::new(1000);
    let task = s.quote("once");
    assert!(matches!(
        s.service
            .approve(&actor(Role::Planner), &task.id, "fake", AT),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        s.service.approve(&actor(Role::Owner), &task.id, "fake", AT),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        s.service
            .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT),
        Err(Error::Conflict(_))
    ));
    let digest = task.quote.as_ref().unwrap().digest().unwrap();
    let approved = s
        .service
        .approve(&actor(Role::Owner), &task.id, &digest, AT)
        .unwrap();
    let replay = s
        .service
        .approve(&actor(Role::Owner), &task.id, &digest, AT)
        .unwrap();
    assert_eq!(approved.version, replay.version);
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    let receipt = s
        .service
        .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(receipt.stage, Stage::Confirmed);
    assert!(receipt.receipt.as_ref().unwrap().simulated);
    let reopened = Service::open(s.directory.path()).unwrap();
    let duplicate = reopened
        .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(
        receipt.receipt.unwrap().order_id,
        duplicate.receipt.unwrap().order_id
    );
    assert_eq!(reopened.provider.order_count().unwrap(), 1);
    let balance = reopened.balance(&actor(Role::Owner), &s.budget).unwrap();
    assert_eq!(
        (
            balance.reserved_minor,
            balance.spent_minor,
            balance.available_minor
        ),
        (0, 700, 300)
    );
    let events = reopened.events(&actor(Role::Owner), &task.id).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == "submission_started")
            .count(),
        1
    );
}

#[test]
fn same_key_same_input_returns_state_and_different_input_conflicts() {
    let s = Setup::new(1000);
    let task = s.approve("replay");
    let replay = s
        .service
        .create_task(&actor(Role::Planner), s.input("replay"), AT)
        .unwrap();
    assert_eq!((task.id, task.version), (replay.id, replay.version));
    let mut changed = s.input("replay");
    changed.request = "Order ten oat lattes".into();
    assert!(matches!(
        s.service.create_task(&actor(Role::Planner), changed, AT),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn concurrent_approvals_reserve_only_available_money() {
    let s = Setup::new(1000);
    let tasks = [s.quote("one"), s.quote("two")];
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let service = Service::open(s.directory.path()).unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.approve(
                    &actor(Role::Owner),
                    &task.id,
                    &task.quote.unwrap().digest().unwrap(),
                    AT,
                )
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::BudgetExceeded)))
            .count(),
        1
    );
    let balance = s.service.balance(&actor(Role::Owner), &s.budget).unwrap();
    assert_eq!(
        (balance.reserved_minor, balance.available_minor),
        (700, 300)
    );
}

#[test]
fn concurrent_execution_claims_have_only_one_provider_order() {
    let s = Setup::new(1000);
    let task = s.approve("race");
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let service = Service::open(s.directory.path()).unwrap();
            let id = task.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.execute(&actor(Role::Executor), &id, Scenario::Success, AT)
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    assert_eq!(
        s.service.task(&actor(Role::Owner), &task.id).unwrap().stage,
        Stage::Confirmed
    );
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .spent_minor,
        700
    );
}

#[test]
fn lost_success_response_holds_money_until_lookup_after_restart() {
    let s = Setup::new(1000);
    let task = s.approve("timeout");
    assert_eq!(
        s.service
            .execute(
                &actor(Role::Executor),
                &task.id,
                Scenario::TimeoutAfterAccept,
                AT
            )
            .unwrap()
            .stage,
        Stage::Unknown
    );
    let service = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    assert!(service.cancel(&actor(Role::Owner), &task.id, AT).is_err());
    assert_eq!(
        service
            .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT)
            .unwrap()
            .stage,
        Stage::Unknown
    );
    assert_eq!(service.provider.order_count().unwrap(), 1);
    assert_eq!(
        service
            .reconcile(&actor(Role::Owner), &task.id, AT)
            .unwrap()
            .stage,
        Stage::Confirmed
    );
    assert_eq!(
        service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
}

#[test]
fn missing_lookup_is_not_proof_of_failure_and_does_not_retry() {
    let s = Setup::new(1000);
    let task = s.approve("before-timeout");
    s.service
        .execute(
            &actor(Role::Executor),
            &task.id,
            Scenario::TimeoutBeforeAccept,
            AT,
        )
        .unwrap();
    assert_eq!(
        s.service
            .reconcile(&actor(Role::Owner), &task.id, AT)
            .unwrap()
            .stage,
        Stage::Unknown
    );
    s.service
        .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
}

#[test]
fn crash_between_provider_acceptance_and_ledger_update_is_reconciled() {
    let s = Setup::new(1000);
    let task = s.approve("crash");
    let (claimed, fresh) = s
        .service
        .begin_submission(&actor(Role::Executor), &task.id, AT)
        .unwrap();
    assert!(fresh);
    let quote = claimed.quote.unwrap();
    let key = claimed.attempt_key.unwrap();
    assert!(matches!(
        s.service
            .provider
            .submit(&key, &quote, Scenario::Success, AT)
            .unwrap(),
        Outcome::Observed(_)
    ));
    let service = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        service.task(&actor(Role::Owner), &task.id).unwrap().stage,
        Stage::Submitting
    );
    assert_eq!(
        service
            .reconcile(&actor(Role::Owner), &task.id, AT)
            .unwrap()
            .stage,
        Stage::Confirmed
    );
    assert_eq!(service.provider.order_count().unwrap(), 1);
}

#[test]
fn decline_and_changed_price_release_but_never_charge_the_changed_quote() {
    for scenario in [Scenario::Decline, Scenario::Success] {
        let s = Setup::new(1000);
        let task = s.approve("decline");
        if matches!(scenario, Scenario::Success) {
            s.service.provider.change_price("oat-latte", 800).unwrap();
        }
        assert_eq!(
            s.service
                .execute(&actor(Role::Executor), &task.id, scenario, AT)
                .unwrap()
                .stage,
            Stage::Declined
        );
        let balance = s.service.balance(&actor(Role::Owner), &s.budget).unwrap();
        assert_eq!(
            (
                balance.reserved_minor,
                balance.spent_minor,
                balance.available_minor
            ),
            (0, 0, 1000)
        );
        if matches!(scenario, Scenario::Success) {
            assert_eq!(s.service.provider.order_count().unwrap(), 0);
        }
    }
}

#[test]
fn expired_quotes_and_revocation_block_submission_and_owner_can_cancel() {
    let s = Setup::new(1000);
    let task = s.approve("expired");
    assert!(
        s.service
            .execute(
                &actor(Role::Executor),
                &task.id,
                Scenario::Success,
                AT + 301
            )
            .is_err()
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    s.service
        .cancel(&actor(Role::Owner), &task.id, AT + 301)
        .unwrap();
    let task = s.approve("revoked");
    s.service
        .revoke_budget(&actor(Role::Owner), &s.budget)
        .unwrap();
    assert!(
        s.service
            .execute(&actor(Role::Executor), &task.id, Scenario::Success, AT)
            .is_err()
    );
    s.service.cancel(&actor(Role::Owner), &task.id, AT).unwrap();
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
}

#[test]
fn an_expired_quote_says_so_and_planning_again_gives_a_fresh_one_to_approve() {
    // Room for two purchases: one approved on time, one re-quoted.
    let s = Setup::new(5000);
    let quoted = s.quote("slow-owner");
    let old_digest = quoted.quote.as_ref().unwrap().digest().unwrap();
    let expires = quoted.quote.as_ref().unwrap().expires_at;
    // At the boundary second the quote is still approvable; one second later it is not, and
    // the refusal names the reason instead of a generic conflict.
    let late = s
        .service
        .approve(&actor(Role::Owner), &quoted.id, &old_digest, expires);
    assert!(matches!(late, Err(Error::Conflict(ref m)) if m.starts_with("quote_expired")));
    let still_valid = s.quote("on-time");
    s.service
        .approve(
            &actor(Role::Owner),
            &still_valid.id,
            &still_valid.quote.as_ref().unwrap().digest().unwrap(),
            expires - 1,
        )
        .unwrap();
    // Planning again issues a fresh quote with a new digest; the old digest stays refused.
    let fresh = s
        .service
        .plan(
            &actor(Role::Planner),
            &quoted.id,
            &FixturePlanner,
            expires + 60,
        )
        .unwrap();
    assert_eq!(fresh.stage, Stage::Quoted);
    let fresh_digest = fresh.quote.as_ref().unwrap().digest().unwrap();
    assert_ne!(fresh_digest, old_digest);
    assert!(
        s.service
            .approve(&actor(Role::Owner), &quoted.id, &old_digest, expires + 60)
            .is_err()
    );
    s.service
        .approve(&actor(Role::Owner), &quoted.id, &fresh_digest, expires + 60)
        .unwrap();
    let done = s
        .service
        .execute(
            &actor(Role::Executor),
            &quoted.id,
            Scenario::Success,
            expires + 61,
        )
        .unwrap();
    assert_eq!(done.stage, Stage::Confirmed);
}

#[test]
fn an_approved_quote_that_expires_before_payment_is_held_back_and_can_be_requoted() {
    let s = Setup::new(1000);
    let approved = s.approve("approved-late");
    let expires = approved.quote.as_ref().unwrap().expires_at;
    let late = s.service.execute(
        &actor(Role::Executor),
        &approved.id,
        Scenario::Success,
        expires,
    );
    assert!(matches!(late, Err(Error::Conflict(ref m)) if m.starts_with("quote_expired")));
    assert_eq!(
        s.service.provider.order_count().unwrap(),
        0,
        "nothing was paid"
    );
    // A plain replay of plan before expiry does not re-quote an approved task...
    let replay = s
        .service
        .plan(&actor(Role::Planner), &approved.id, &FixturePlanner, AT)
        .unwrap();
    assert_eq!(replay.stage, Stage::Approved);
    // ...after expiry it does, and releases the hold until the owner approves again.
    let fresh = s
        .service
        .plan(
            &actor(Role::Planner),
            &approved.id,
            &FixturePlanner,
            expires,
        )
        .unwrap();
    assert_eq!(fresh.stage, Stage::Quoted);
    assert!(fresh.approved_digest.is_none());
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
}

#[test]
fn a_repeated_exact_approval_after_payment_started_is_not_a_conflict() {
    // Two owner clicks race: the second approval arrives after the first one's payment began.
    let s = Setup::new(1000);
    let approved = s.approve("double-click");
    let digest = approved.quote.as_ref().unwrap().digest().unwrap();
    let paid = s
        .service
        .execute(&actor(Role::Executor), &approved.id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(paid.stage, Stage::Confirmed);
    let again = s
        .service
        .approve(&actor(Role::Owner), &approved.id, &digest, AT)
        .unwrap();
    assert_eq!(again.stage, Stage::Confirmed);
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    assert!(matches!(
        s.service
            .approve(&actor(Role::Owner), &approved.id, &"0".repeat(64), AT),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn replanning_releases_old_reservation_and_requires_fresh_owner_approval() {
    let s = Setup::new(1000);
    let old = s.approve("requote");
    let old_digest = old.quote.as_ref().unwrap().digest().unwrap();
    let plan = Plan {
        offer_id: Some("oat-latte".into()),
        quantity: 1,
        clarification: None,
    };
    let new = s
        .service
        .apply_plan(
            &actor(Role::Planner),
            &old.id,
            old.version,
            plan.clone(),
            AT + 1,
        )
        .unwrap();
    assert_eq!(new.stage, Stage::Quoted);
    assert!(new.approved_digest.is_none());
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
    assert!(
        s.service
            .approve(&actor(Role::Owner), &old.id, &old_digest, AT + 1)
            .is_err()
    );
    assert!(
        s.service
            .execute(&actor(Role::Executor), &old.id, Scenario::Success, AT + 1)
            .is_err()
    );
    assert!(matches!(
        s.service
            .apply_plan(&actor(Role::Planner), &old.id, old.version, plan, AT + 1),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn ownership_and_roles_are_checked_before_planning_or_execution() {
    struct MustNotRun;
    impl Planner for MustNotRun {
        fn plan(
            &self,
            _: &TaskInput,
            _: &cosigna_agent::Budget,
            _: &[cosigna_agent::Offer],
        ) -> cosigna_agent::Result<Plan> {
            panic!("unauthorized model call");
        }
        fn name(&self) -> &str {
            "must-not-run"
        }
    }
    let s = Setup::new(1000);
    let task = s
        .service
        .create_task(&actor(Role::Planner), s.input("private"), AT)
        .unwrap();
    let stranger = Actor {
        owner: "bob".into(),
        role: Role::Owner,
    };
    assert!(matches!(
        s.service.plan(&stranger, &task.id, &MustNotRun, AT),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.service.events(&stranger, &task.id),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.service
            .execute(&actor(Role::Planner), &task.id, Scenario::Success, AT),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        s.service
            .plan(&actor(Role::Executor), &task.id, &MustNotRun, AT),
        Err(Error::Forbidden)
    ));
}

#[test]
fn currency_seller_features_ceiling_and_catalog_are_enforced_outside_model() {
    for constraint in [
        "currency",
        "seller",
        "feature",
        "ceiling",
        "invented_offer",
        "quantity",
    ] {
        let s = Setup::new(1000);
        let mut terms = BudgetInput {
            currency: "USD".into(),
            limit_minor: 1000,
            per_purchase_minor: 1000,
            merchants: vec!["fixture-cafe".into()],
            expires_at: AT + 3600,
            second_approval_above_minor: None,
        };
        if constraint == "currency" {
            terms.currency = "INR".into();
        }
        if constraint == "seller" {
            terms.merchants = vec!["other-cafe".into()];
        }
        if constraint == "ceiling" {
            terms.per_purchase_minor = 500;
        }
        let budget = s
            .service
            .create_budget(&actor(Role::Owner), terms, AT)
            .unwrap();
        let mut input = s.input("constraint");
        input.budget_id = budget.id;
        if constraint == "feature" {
            input.required_features.push("decaf".into());
        }
        let task = s
            .service
            .create_task(&actor(Role::Planner), input, AT)
            .unwrap();
        let plan = Plan {
            offer_id: Some(
                if constraint == "invented_offer" {
                    "external-url"
                } else {
                    "oat-latte"
                }
                .into(),
            ),
            quantity: if constraint == "quantity" {
                u32::MAX
            } else {
                1
            },
            clarification: None,
        };
        assert!(
            s.service
                .apply_plan(&actor(Role::Planner), &task.id, task.version, plan, AT)
                .is_err(),
            "{constraint}"
        );
        assert_eq!(
            s.service.task(&actor(Role::Owner), &task.id).unwrap().stage,
            Stage::Draft
        );
    }
}

#[test]
fn clarification_never_reserves_money_and_expired_budget_cannot_be_used() {
    let s = Setup::new(1000);
    let mut input = s.input("clarify");
    input.request = "something nice".into();
    let task = s
        .service
        .create_task(&actor(Role::Planner), input, AT)
        .unwrap();
    let task = s
        .service
        .plan(&actor(Role::Planner), &task.id, &FixturePlanner, AT)
        .unwrap();
    assert_eq!(task.stage, Stage::NeedsInput);
    assert!(task.quote.is_none());
    assert!(
        s.service
            .approve(&actor(Role::Owner), &task.id, "invented", AT)
            .is_err()
    );
    assert!(
        s.service
            .create_task(&actor(Role::Planner), s.input("late"), AT + 3600)
            .is_err()
    );
    assert_eq!(
        s.service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .available_minor,
        1000
    );
}

#[test]
fn provider_idempotency_rejects_changed_payload_and_ledger_rejects_future_schema() {
    let s = Setup::new(1000);
    let task = s.approve("provider-key");
    let mut quote = task.quote.unwrap();
    s.service
        .provider
        .submit("fixed-key", &quote, Scenario::Success, AT)
        .unwrap();
    quote.quantity = 2;
    quote.total_minor = 1400;
    assert!(matches!(
        s.service
            .provider
            .submit("fixed-key", &quote, Scenario::Success, AT),
        Err(Error::Conflict(_))
    ));
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    let db = rusqlite::Connection::open(s.directory.path().join("transactions.sqlite")).unwrap();
    db.pragma_update(None, "user_version", 10).unwrap();
    drop(db);
    assert!(matches!(
        Service::open(s.directory.path()),
        Err(Error::StoreFormat)
    ));
}

#[test]
fn foreign_databases_are_never_taken_over() {
    let directory = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(directory.path().join("transactions.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE unrelated (value TEXT); INSERT INTO unrelated VALUES ('keep');")
        .unwrap();
    drop(db);
    assert!(matches!(
        Service::open(directory.path()),
        Err(Error::StoreFormat)
    ));
}

#[test]
fn api_flow_exposes_digest_and_keeps_approval_out_of_planner_role() {
    let s = Setup::new(1000);
    let call = |role, method, path: &str, body| {
        api::dispatch(
            &s.service,
            &FixturePlanner,
            &actor(role),
            method,
            path,
            &serde_json::to_vec(&body).unwrap(),
            AT,
        )
    };
    let task = call(
        Role::Planner,
        "POST",
        "/v1/tasks",
        serde_json::to_value(s.input("api")).unwrap(),
    )
    .unwrap();
    let id = task["data"]["id"].as_str().unwrap();
    let path = format!("/v1/tasks/{id}");
    let quoted = call(Role::Planner, "POST", &format!("{path}/plan"), json!({})).unwrap();
    let body = json!({"quote_digest":quoted["approval_digest"]});
    assert!(matches!(
        call(
            Role::Planner,
            "POST",
            &format!("{path}/approve"),
            body.clone()
        ),
        Err(Error::Forbidden)
    ));
    call(Role::Owner, "POST", &format!("{path}/approve"), body).unwrap();
    assert!(matches!(
        call(Role::Owner, "POST", &format!("{path}/execute"), json!({})),
        Err(Error::Forbidden)
    ));
    let result = call(
        Role::Executor,
        "POST",
        &format!("{path}/execute"),
        json!({}),
    )
    .unwrap();
    assert_eq!(result["data"]["stage"], "confirmed");
    assert_eq!(result["simulated_provider"], true);
    assert!(matches!(
        call(
            Role::Planner,
            "POST",
            &format!("{path}/plan"),
            json!({"approve":true})
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn demo_binary_completes_without_model_credentials_or_real_merchants() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .arg("demo")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["simulated"], true);
    assert_eq!(value["synthetic_orders"], 2);
    assert_eq!(value["balance"]["spent_minor"], 1400);
    assert_eq!(value["outcomes"][2]["blocked"], "budget_exceeded");
}
