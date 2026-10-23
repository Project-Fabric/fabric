use std::sync::{Arc, Mutex};

use cosigna_agent::{
    Budget, BudgetInput, Plan, TaskInput,
    planner::{HostedPlanner, HostedProvider, Planner, ResponsesTransport, parse_response},
};
use serde_json::{Value, json};

fn response(text: &str) -> Value {
    json!({"status":"completed","output":[{"type":"reasoning"},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]})
}

struct FakeTransport {
    body: Arc<Mutex<Option<Value>>>,
}
struct FakeGroqTransport(FakeTransport);
impl ResponsesTransport for FakeGroqTransport {
    fn provider(&self) -> HostedProvider {
        HostedProvider::Groq
    }
    fn send(&self, body: &Value) -> cosigna_agent::Result<Value> {
        self.0.send(body)
    }
}
impl ResponsesTransport for FakeTransport {
    fn send(&self, body: &Value) -> cosigna_agent::Result<Value> {
        *self.body.lock().unwrap() = Some(body.clone());
        Ok(response(
            r#"{"offer_id":"oat-latte","quantity":1,"clarification":null}"#,
        ))
    }
}

#[test]
fn hosted_request_is_structured_bounded_and_has_no_execution_authority() {
    let captured = Arc::new(Mutex::new(None));
    let planner = HostedPlanner::new(
        "gpt-6.1-sol".into(),
        FakeTransport {
            body: captured.clone(),
        },
    )
    .unwrap();
    let budget = Budget {
        id: "budget".into(),
        owner: "alice".into(),
        revoked: false,
        terms: BudgetInput {
            currency: "USD".into(),
            limit_minor: 10000,
            per_purchase_minor: 1000,
            merchants: vec!["fixture-cafe".into()],
            expires_at: 1000,
        },
    };
    let task = TaskInput {
        budget_id: "budget".into(),
        request: "one oat latte please".into(),
        idempotency_key: "private-idempotency-key".into(),
        required_features: vec!["oat_milk".into()],
    };
    assert_eq!(
        planner
            .plan(&task, &budget, &[])
            .unwrap()
            .offer_id
            .as_deref(),
        Some("oat-latte")
    );
    let body = captured.lock().unwrap().take().unwrap();
    assert_eq!(body["model"], "gpt-6.1-sol");
    assert_eq!(body["store"], false);
    assert_eq!(body["reasoning"]["effort"], "low");
    assert_eq!(body["text"]["format"]["strict"], true);
    assert_eq!(
        body["text"]["format"]["schema"]["additionalProperties"],
        false
    );
    assert!(body.get("tools").is_none());
    assert!(!body.to_string().contains("private-idempotency-key"));
    assert!(!body.to_string().contains("alice"));
}

#[test]
fn malformed_incomplete_refused_tool_and_authority_outputs_fail_closed() {
    let valid = r#"{"offer_id":"oat-latte","quantity":1,"clarification":null}"#;
    let mut incomplete = response(valid);
    incomplete["status"] = json!("incomplete");
    let mut tool = response(valid);
    tool["output"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"function_call","name":"pay"}));
    let mut refused = response(valid);
    refused["output"][1]["content"][0] = json!({"type":"refusal","refusal":"no"});
    let mut duplicate = response(valid);
    duplicate["output"]
        .as_array_mut()
        .unwrap()
        .push(response(valid)["output"][1].clone());
    for value in [
        incomplete,
        tool,
        refused,
        duplicate,
        response("not json"),
        response(r#"{"offer_id":"oat-latte","quantity":1,"clarification":null,"approved":true}"#),
        response(r#"{"offer_id":"oat-latte","quantity":0,"clarification":null}"#),
        response(r#"{"offer_id":null,"quantity":1,"clarification":"Which size?"}"#),
        response(r#"{"offer_id":"oat-latte","quantity":1,"clarification":"Also pay now"}"#),
    ] {
        assert!(parse_response(&value).is_err(), "{value}");
    }
    let question = parse_response(&response(
        r#"{"offer_id":null,"quantity":0,"clarification":"Which size?"}"#,
    ))
    .unwrap();
    assert!(question.offer_id.is_none());
    assert_eq!(question.clarification.as_deref(), Some("Which size?"));
    assert!(
        serde_json::from_str::<Plan>(
            r#"{"offer_id":"oat-latte","quantity":1,"clarification":null,"price":1}"#
        )
        .is_err()
    );
}

#[test]
fn groq_uses_the_same_constrained_plan_and_rejects_models_from_other_providers() {
    assert_eq!(
        HostedProvider::Groq.endpoint(),
        "https://api.groq.com/openai/v1/responses"
    );
    assert_eq!(
        HostedProvider::Openai.endpoint(),
        "https://api.openai.com/v1/responses"
    );
    let captured = Arc::new(Mutex::new(None));
    assert!(
        HostedPlanner::new(
            "gpt-6.1-sol".into(),
            FakeGroqTransport(FakeTransport {
                body: captured.clone()
            })
        )
        .is_err()
    );
    assert!(
        HostedPlanner::new(
            "openai/gpt-oss-120b".into(),
            FakeTransport {
                body: captured.clone()
            }
        )
        .is_err()
    );
    assert!(
        HostedPlanner::new(
            "https://example.test/model".into(),
            FakeGroqTransport(FakeTransport {
                body: captured.clone()
            })
        )
        .is_err()
    );
    assert!(captured.lock().unwrap().is_none());
    for model in ["openai/gpt-oss-120b", "openai/gpt-oss-20b"] {
        let planner = HostedPlanner::new(
            model.into(),
            FakeGroqTransport(FakeTransport {
                body: captured.clone(),
            }),
        )
        .unwrap();
        let budget = Budget {
            id: "budget".into(),
            owner: "private-owner".into(),
            revoked: false,
            terms: BudgetInput {
                currency: "USD".into(),
                limit_minor: 2000,
                per_purchase_minor: 1000,
                merchants: vec!["fixture-cafe".into()],
                expires_at: 1000,
            },
        };
        let task = TaskInput {
            budget_id: budget.id.clone(),
            request: "one oat latte please".into(),
            idempotency_key: "private-task-key".into(),
            required_features: vec!["oat_milk".into()],
        };
        assert_eq!(
            planner
                .plan(&task, &budget, &[])
                .unwrap()
                .offer_id
                .as_deref(),
            Some("oat-latte")
        );
        let body = captured.lock().unwrap().take().unwrap();
        assert_eq!(body["model"], model);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["max_output_tokens"], 1600);
        assert_eq!(body["text"]["format"]["strict"], true);
        assert!(body.get("tools").is_none());
        assert!(!body.to_string().contains("private-owner"));
        assert!(!body.to_string().contains("private-task-key"));
    }
}
