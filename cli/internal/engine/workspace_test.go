package engine

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

func TestLocalCodeApprovalDoesNotRequirePaymentConfiguration(t *testing.T) {
	a := Approval{ID: 7, Digest: "reviewed-file-input", Status: "pending", Tool: "Edit", Target: "src/billing/tax.go", Input: Object{"file_path": "src/billing/tax.go", "old_string": "old", "new_string": "new"}}
	c := &Client{User: "reviewer", Run: func(_ context.Context, args []string) ([]byte, error) {
		switch args[0] {
		case "approvals":
			return json.Marshal(Approvals{Schema: "fabric.approvals/v1", Requests: []Approval{a}})
		case "approve":
			a.Status = "approved"
			u := "reviewer"
			a.Approver = &u
			return json.Marshal(Approvals{Schema: "fabric.approve/v2", Requests: []Approval{a}})
		default:
			return nil, errors.New("payment configuration should not be read for a file edit")
		}
	}}
	if _, err := c.Perform(context.Background(), Action{Kind: "approve", ID: 7, Digest: a.Digest}); err != nil {
		t.Fatal(err)
	}
	for _, tool := range []string{"stripe/issue_refund", "mcp__stripe__Bash", "unknown"} {
		if _, ok := ApprovalSummary(Approval{Tool: tool}, nil, false); ok {
			t.Fatal("unrecognized tool accepted without declared units", tool)
		}
	}
}

func TestDeveloperActionsAreBoundedAndSimulated(t *testing.T) {
	calls := 0
	c := &Client{Fixtures: true, User: "developer", Run: func(context.Context, []string) ([]byte, error) { calls++; return nil, nil }}
	before := c.Snapshot(context.Background()).History.Total
	for _, a := range []Action{{Kind: "checkpoint", Note: "before refactor; $(literal)"}, {Kind: "integrate", Name: "codex"}, {Kind: "project-init"}} {
		r, err := c.Perform(context.Background(), a)
		if err != nil || !r.Simulated {
			t.Fatal(r, err)
		}
	}
	if calls != 0 {
		t.Fatal("fixture setup ran an engine command")
	}
	if c.Snapshot(context.Background()).History.Total != before+1 {
		t.Fatal("fixture checkpoint not reflected")
	}
	if _, err := Command(Action{Kind: "integrate", Name: "codex --unsafe"}, "developer"); err == nil {
		t.Fatal("unbounded setup target")
	}
	args, err := Command(Action{Kind: "checkpoint", Note: "before; $(literal)"}, "developer")
	if err != nil || args[2] != "before; $(literal)" {
		t.Fatal(args, err)
	}
}

func TestFollowReadsOnlyChangingSources(t *testing.T) {
	fixture := &Client{Fixtures: true, User: "test"}
	s := fixture.Snapshot(context.Background())
	var read []string
	c := &Client{Run: func(_ context.Context, args []string) ([]byte, error) {
		read = append(read, strings.Join(args, " "))
		switch args[0] {
		case "approvals":
			return json.Marshal(s.Approvals)
		case "log":
			return json.Marshal(s.History)
		case "evidence":
			if args[1] != "calls" {
				t.Fatal("background verification")
			}
			return json.Marshal(s.Calls)
		}
		return nil, errors.New("unexpected background command")
	}}
	follow := c.Follow(context.Background())
	if len(read) != 3 || len(follow.Errors) != 0 {
		t.Fatal(read, follow.Errors)
	}
}
