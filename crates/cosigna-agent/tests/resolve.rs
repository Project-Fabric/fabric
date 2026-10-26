use cosigna_agent::{
    Actor, BudgetInput, Error, Role, Service, Stage, Task, TaskInput, api,
    planner::FixturePlanner,
    provider::{Outcome, Scenario},
};
use serde_json::Value;

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
    fn new() -> Self {
        Self::with_limit(1000)
    }

    fn with_limit(limit: i64) -> Self {
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

    /// An approved purchase submitted under `scenario`.
    fn submitted(&self, key: &str, scenario: Scenario) -> Task {
        let owner = actor(Role::Owner);
        let task = self
            .service
            .create_task(
                &owner,
                TaskInput {
                    budget_id: self.budget.clone(),
                    request: "Order one oat latte".into(),
                    idempotency_key: key.into(),
                    required_features: vec![],
                },
                AT,
            )
            .unwrap();
        let quoted = self
            .service
            .plan(&owner, &task.id, &FixturePlanner, AT)
            .unwrap();
        let digest = quoted.quote.unwrap().digest().unwrap();
        self.service.approve(&owner, &task.id, &digest, AT).unwrap();
        self.service
            .execute(&actor(Role::Executor), &task.id, scenario, AT)
            .unwrap()
    }

    fn available(&self) -> i64 {
        self.service
            .balance(&actor(Role::Owner), &self.budget)
            .unwrap()
            .available_minor
    }

    fn last_event(&self) -> Value {
        let db =
            rusqlite::Connection::open(self.directory.path().join("transactions.sqlite")).unwrap();
        let body: String = db
            .query_row(
                "SELECT body FROM evidence ORDER BY seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str::<Value>(&body).unwrap()["event"].clone()
    }
}

#[test]
fn an_attempt_the_merchant_never_accepted_is_voided_and_its_hold_released() {
    let s = Setup::new();
    let task = s.submitted("lost", Scenario::TimeoutBeforeAccept);
    assert_eq!(task.stage, Stage::Unknown);
    assert_eq!(s.available(), 300);
    // Lookup alone can never settle this: absence is not proof.
    let owner = actor(Role::Owner);
    assert_eq!(
        s.service.reconcile(&owner, &task.id, AT).unwrap().stage,
        Stage::Unknown
    );
    assert!(matches!(
        s.service.cancel(&owner, &task.id, AT),
        Err(Error::Conflict(_))
    ));

    let resolved = s.service.resolve(&owner, &task.id, AT + 60).unwrap();
    assert_eq!(resolved.stage, Stage::Declined);
    assert!(resolved.receipt.is_none());
    assert_eq!(s.available(), 1000);
    assert_eq!(s.last_event()["kind"], "gateway.task.provider_voided");
    assert_eq!(s.last_event()["by"]["role"], "owner");
    assert_eq!(s.last_event()["reservation"]["state"], "released");

    // The original request arriving late is refused by the merchant.
    let quote = task.quote.as_ref().unwrap();
    let late = s
        .service
        .provider
        .submit(
            task.attempt_key.as_deref().unwrap(),
            quote,
            Scenario::Success,
            AT + 61,
        )
        .unwrap();
    assert!(matches!(late, Outcome::Voided));
    assert_eq!(s.service.provider.order_count().unwrap(), 0);

    // Resolving again changes nothing.
    let again = s.service.resolve(&owner, &task.id, AT + 120).unwrap();
    assert_eq!(again.version, resolved.version);
    assert!(s.service.verify_evidence().unwrap().ok());
}

#[test]
fn an_attempt_the_merchant_accepted_is_confirmed_not_released() {
    let s = Setup::new();
    let task = s.submitted("accepted-then-timeout", Scenario::TimeoutAfterAccept);
    assert_eq!(task.stage, Stage::Unknown);
    let resolved = s
        .service
        .resolve(&actor(Role::Owner), &task.id, AT + 60)
        .unwrap();
    assert_eq!(resolved.stage, Stage::Confirmed);
    assert!(resolved.receipt.as_ref().unwrap().accepted);
    assert_eq!(s.available(), 300);
    assert_eq!(s.service.provider.void_count().unwrap(), 0);
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    assert_eq!(s.last_event()["kind"], "gateway.task.provider_result");
}

#[test]
fn only_the_owner_can_resolve_and_settled_tasks_are_left_alone() {
    let s = Setup::with_limit(2000);
    let task = s.submitted("lost", Scenario::TimeoutBeforeAccept);
    for role in [Role::Planner, Role::Executor] {
        assert!(matches!(
            s.service.resolve(&actor(role), &task.id, AT),
            Err(Error::Forbidden)
        ));
    }
    // Legacy HTTP router: the planner/executor development roles cannot resolve either.
    for role in [Role::Planner, Role::Executor] {
        assert!(
            api::dispatch(
                &s.service,
                &FixturePlanner,
                &actor(role),
                "POST",
                &format!("/v1/tasks/{}/resolve", task.id),
                b"{}",
                AT
            )
            .is_err()
        );
    }
    let other = Actor {
        owner: "mallory".into(),
        role: Role::Owner,
    };
    assert!(matches!(
        s.service.resolve(&other, &task.id, AT),
        Err(Error::NotFound)
    ));
    assert_eq!(s.service.provider.void_count().unwrap(), 0);

    let confirmed = s.submitted("fine", Scenario::Success);
    let unchanged = s
        .service
        .resolve(&actor(Role::Owner), &confirmed.id, AT)
        .unwrap();
    assert_eq!(unchanged.version, confirmed.version);
    assert_eq!(unchanged.stage, Stage::Confirmed);
    assert_eq!(s.service.provider.void_count().unwrap(), 0);
}

#[test]
fn provider_store_upgrades_from_version_1_and_refuses_later_versions() {
    let s = Setup::new();
    let task = s.submitted("before-upgrade", Scenario::TimeoutBeforeAccept);
    let path = s.directory.path().join("fixture-provider.sqlite");
    // Reproduce a version 1 provider store in this synthetic profile only.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE voids; DROP TABLE fixture_cases; DROP TABLE fixture_payments; DROP TABLE fixture_payees; DROP TABLE money_attempts; DROP TABLE money_voids; PRAGMA user_version=1;")
        .unwrap();
    drop(db);
    let reopened = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        reopened
            .resolve(&actor(Role::Owner), &task.id, AT)
            .unwrap()
            .stage,
        Stage::Declined
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    db.pragma_update(None, "user_version", 5).unwrap();
    drop(db);
    assert!(matches!(
        Service::open(s.directory.path()),
        Err(Error::StoreFormat)
    ));
}
