package engine

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

func TestFixtureDecisionsAndIsolation(t *testing.T) {
	ctx := context.Background()
	executions := 0
	c := &Client{Fixtures: true, User: "priya", Run: func(context.Context, []string) ([]byte, error) { executions++; return nil, nil }}
	s := c.Snapshot(ctx)
	if len(s.Approvals.Requests) != 3 || s.Errors["approvals"] != "" {
		t.Fatalf("fixture contract: %+v", s.Errors)
	}
	a, err := c.Approval(ctx, 1)
	if err != nil {
		t.Fatal(err)
	}
	r, err := c.Perform(ctx, Action{Kind: "approve", ID: 1, Digest: a.Digest, Note: "reviewed"})
	if err != nil || !r.Simulated {
		t.Fatal(r, err)
	}
	a, _ = c.Approval(ctx, 1)
	if a.Status != "approved" || Value(a.Approver) != "priya" {
		t.Fatal(a)
	}
	if _, err = c.Perform(ctx, Action{Kind: "approve", ID: 1, Digest: a.Digest}); err == nil {
		t.Fatal("duplicate decision accepted")
	}
	if executions != 0 {
		t.Fatal("fixture executed real command")
	}
	other := &Client{Fixtures: true, User: "other"}
	a, _ = other.Approval(ctx, 1)
	if a.Status != "pending" {
		t.Fatal("fixture state leaked")
	}
}
func TestStaleDigestNeverMutates(t *testing.T) {
	c := &Client{Fixtures: true, User: "reviewer"}
	_, err := c.Perform(context.Background(), Action{Kind: "approve", ID: 1, Digest: "stale"})
	if err == nil {
		t.Fatal("accepted stale digest")
	}
	a, _ := c.Approval(context.Background(), 1)
	if a.Status != "pending" {
		t.Fatal("changed stale request")
	}
}
func TestCommandsAreArgumentArrays(t *testing.T) {
	args, err := Command(Action{Kind: "approve", ID: 5, Digest: "digest", Note: "$(not-a-shell); & literal"}, "a b")
	if err != nil {
		t.Fatal(err)
	}
	expected := []string{"approve", "5", "--yes", "--as", "a b", "--note", "$(not-a-shell); & literal", "--json"}
	if !reflect.DeepEqual(args, expected) {
		t.Fatal(args)
	}
	if _, err = Command(Action{Kind: "approve", ID: 5, Digest: "digest", Note: "--reject"}, "person"); err == nil {
		t.Fatal("accepted option-like note")
	}
}
func TestMoneyUsesDeclarationsAndExactArithmetic(t *testing.T) {
	var input Object
	_ = Decode([]byte(`{"parameters":{"amount":12345,"currency":"usd"},"op":"refund"}`), &input)
	a := Approval{Target: "stripe/write", Input: input}
	cur := "parameters.currency"
	declarations := []Declaration{{Tool: a.Target, Amount: "parameters.amount", Currency: &cur, MinorUnits: true}}
	value, ok := ApprovalAmount(a, declarations, false)
	if !ok || value != "USD 123.45" {
		t.Fatal(value, ok)
	}
	if _, ok = ApprovalAmount(a, nil, false); ok {
		t.Fatal("guessed units")
	}
	if _, ok = amount(json.Number("1.1"), "USD", true); ok {
		t.Fatal("fractional minor units")
	}
	if _, ok = amount(json.Number("123"), "XYZ", true); ok {
		t.Fatal("unknown currency")
	}
}
func TestJSONAndRedaction(t *testing.T) {
	for _, data := range []string{`{} {}`, `{} trailing`} {
		if Decode([]byte(data), &Object{}) == nil {
			t.Fatal("accepted trailing input")
		}
	}
	raw := SafeJSON(Object{"api_key": "private", "authorization_code": "ledger-ref", "body": "sk_test_secret123", "text": "\x1b]52;c;clipboard\aVisible\x1b[31m red"})
	if strings.Contains(raw, "private") || strings.Contains(raw, "secret123") || strings.Contains(raw, "clipboard") {
		t.Fatal(raw)
	}
	if !strings.Contains(raw, "ledger-ref") || !strings.Contains(raw, "Visible") {
		t.Fatal(raw)
	}
}
func TestLiveSchemaFailureAndReceipt(t *testing.T) {
	ctx := context.Background()
	fixture := &Client{Fixtures: true, User: "priya"}
	s := fixture.Snapshot(ctx)
	a, _ := fixture.Approval(ctx, 1)
	executions := 0
	c := &Client{User: "priya", Run: func(_ context.Context, args []string) ([]byte, error) {
		switch args[0] {
		case "approvals":
			return json.Marshal(s.Approvals)
		case "payments":
			return []byte(`{"schema":"fabric.payments.list/v1","payments":[]}`), nil
		default:
			executions++
			return []byte(`{"schema":"fabric.approve/v2","requests":[]}`), nil
		}
	}}
	if _, err := c.Perform(ctx, Action{Kind: "approve", ID: 1, Digest: a.Digest}); err == nil {
		t.Fatal("approved unknown live units")
	}
	if executions != 0 {
		t.Fatal("executed unknown payment")
	}
	if _, err := c.Perform(ctx, Action{Kind: "approve", ID: 1, Digest: a.Digest, Reject: true}); err == nil {
		t.Fatal("accepted mismatched receipt")
	}
	if executions != 1 {
		t.Fatal(executions)
	}
}
