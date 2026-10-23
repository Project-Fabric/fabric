package engine

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/Project-Fabric/fabric/cli/fixtures"
)

type Runner func(context.Context, []string) ([]byte, error)
type Client struct {
	Fixtures              bool
	Project, Binary, User string
	Run                   Runner
	mu                    sync.Mutex
	memory                *Snapshot
}

// Commands receive an argument array, never a shell. Agent-session environment
// markers remain inherited so that the engine's human-only checks still apply.
func (c *Client) execute(ctx context.Context, args []string) ([]byte, error) {
	if c.Run != nil {
		return c.Run(ctx, args)
	}
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, c.Binary, args...)
	cmd.Dir = c.Project
	var output limitedBuffer
	cmd.Stdout = &output
	err := cmd.Run()
	if output.overflow {
		return nil, errors.New("engine response exceeded 4 MiB")
	}
	data := output.data
	if ctx.Err() != nil {
		return nil, errors.New("engine command timed out or was cancelled")
	}
	if err != nil {
		var header struct {
			Schema string `json:"schema"`
			Error  struct {
				Message string `json:"message"`
			} `json:"error"`
		}
		if Decode(data, &header) == nil && header.Schema == "fabric.error/v1" {
			return nil, errors.New(Clean(header.Error.Message))
		}
		var exit *exec.ExitError
		if !(errors.As(err, &exit) && exit.ExitCode() == 5 && len(args) > 1 && args[0] == "evidence" && args[1] == "verify") {
			return nil, errors.New("Cosigna could not complete the command; check installation, project and permissions")
		}
	}
	return data, nil
}

type limitedBuffer struct {
	data     []byte
	overflow bool
}

func (b *limitedBuffer) Write(p []byte) (int, error) {
	if len(b.data)+len(p) > 4*1024*1024 {
		b.overflow = true
		return 0, errors.New("output limit")
	}
	b.data = append(b.data, p...)
	return len(p), nil
}

func (c *Client) load(ctx context.Context, file, schema string, args []string, out any) error {
	var data []byte
	var err error
	if c.Fixtures {
		data, err = fixtures.Files.ReadFile(file)
	} else {
		data, err = c.execute(ctx, append(args, "--json"))
	}
	if err != nil {
		return err
	}
	return CheckSchema(data, schema, out)
}
func (c *Client) snapshot(ctx context.Context) Snapshot {
	if c.memory != nil {
		return clone(*c.memory)
	}
	s := Snapshot{Errors: map[string]string{}}
	tasks := []struct {
		key, file, schema string
		args              []string
		out               any
	}{
		{"history", "workspace_log.json", "fabric.log/v1", []string{"log", "-n", "30"}, &s.History},
		{"rules", "guard_rules.json", "fabric.guard.rules/v2", []string{"guard", "rules"}, &s.Rules},
		{"changes", "workspace_changes.json", "fabric.changes/v1", []string{"changes", "--all"}, &s.Changes},
		{"approvals", "inbox_approvals.json", "fabric.approvals/v1", []string{"approvals", "--all"}, &s.Approvals},
		{"calls", "evidence_calls.json", "fabric.evidence.calls/v1", []string{"evidence", "calls", "-n", "200"}, &s.Calls},
		{"agents", "agent_list.json", "fabric.agent.list/v1", []string{"agent", "list"}, &s.Agents},
		{"mandates", "inbox_mandates.json", "fabric.mandate.list/v1", []string{"mandate", "list"}, &s.Mandates},
		{"verification", "evidence_verify.json", "fabric.evidence.verify/v1", []string{"evidence", "verify"}, &s.Verification},
		{"declarations", "payments_list.json", "fabric.payments.list/v1", []string{"payments", "list"}, &s.Declarations},
	}
	var wg sync.WaitGroup
	var em sync.Mutex
	for _, task := range tasks {
		wg.Add(1)
		go func(t struct {
			key, file, schema string
			args              []string
			out               any
		}) {
			defer wg.Done()
			if err := c.load(ctx, t.file, t.schema, t.args, t.out); err != nil {
				em.Lock()
				s.Errors[t.key] = Clean(err.Error())
				em.Unlock()
			}
		}(task)
	}
	wg.Wait()
	if c.Fixtures {
		data, err := fixtures.Files.ReadFile("agentauth_consents.json")
		if err == nil {
			err = Decode(data, &s.Consents)
		}
		if err != nil {
			s.Errors["consents"] = Clean(err.Error())
		}
		c.memory = &s
	} else {
		s.Errors["consents"] = "Consents are available in the web Console; this CLI reads local Cosigna authority."
	}
	return clone(s)
}
func (c *Client) Snapshot(ctx context.Context) Snapshot {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.snapshot(ctx)
}
func clone(s Snapshot) Snapshot { // Keep fixture data private to this client, including nested inputs.
	raw, _ := json.Marshal(s)
	var out Snapshot
	_ = Decode(raw, &out)
	return out
}

func (c *Client) Approval(ctx context.Context, id uint64) (Approval, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	var list Approvals
	if c.Fixtures {
		list = c.snapshot(ctx).Approvals
	} else if err := c.load(ctx, "", "fabric.approvals/v1", []string{"approvals", "--all"}, &list); err != nil {
		return Approval{}, err
	}
	for _, a := range list.Requests {
		if a.ID == id {
			return a, nil
		}
	}
	return Approval{}, errors.New("approval request not found")
}

func (c *Client) Perform(ctx context.Context, a Action) (Receipt, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if strings.TrimSpace(c.User) == "" {
		return Receipt{}, errors.New("provide --user with the approver's identity")
	}
	args, err := Command(a, c.User)
	if err != nil {
		return Receipt{}, err
	}
	if a.Kind == "approve" {
		var list Approvals
		if c.Fixtures {
			list = c.snapshot(ctx).Approvals
		} else if err := c.load(ctx, "", "fabric.approvals/v1", []string{"approvals", "--all"}, &list); err != nil {
			return Receipt{}, err
		}
		found := false
		for _, current := range list.Requests {
			if current.ID == a.ID && current.Status == "pending" && current.Digest == a.Digest && a.Digest != "" {
				found = true
			}
		}
		if !found {
			return Receipt{}, errors.New("request changed or was already decided; refresh before deciding")
		}
		if !a.Reject {
			var declarations Declarations
			needsDeclaration := true
			for _, current := range list.Requests {
				if current.ID == a.ID && LocalRequest(current) {
					needsDeclaration = false
				}
			}
			if c.Fixtures {
				declarations = c.snapshot(ctx).Declarations
			} else if needsDeclaration {
				if err := c.load(ctx, "", "fabric.payments.list/v1", []string{"payments", "list"}, &declarations); err != nil {
					return Receipt{}, err
				}
			}
			for _, current := range list.Requests {
				if current.ID == a.ID {
					if _, known := ApprovalSummary(current, declarations.Payments, c.Fixtures); !known {
						return Receipt{}, errors.New("payment units are unknown; review the declarations before approving")
					}
				}
			}
		}
	}
	receipt := Receipt{Simulated: c.Fixtures, Args: args, Message: "Recorded by Cosigna. This authorizes a retry; it does not execute the tool call."}
	if c.Fixtures {
		s := c.snapshot(ctx)
		now := time.Now().UTC().Format(time.RFC3339)
		switch a.Kind {
		case "checkpoint":
			seq := s.History.Total + 1
			s.History.Checkpoints = append([]Checkpoint{{Seq: seq, CreatedAt: now, Kind: "manual", Label: a.Note}}, s.History.Checkpoints...)
			s.History.Total = seq
		case "approve":
			for i := range s.Approvals.Requests {
				r := &s.Approvals.Requests[i]
				if r.ID == a.ID {
					r.Status = "approved"
					if a.Reject {
						r.Status = "rejected"
					}
					r.Approver = &c.User
					r.DecidedAt = &now
					r.Note = &a.Note
				}
			}
		case "agent-revoke":
			found := false
			for i := range s.Agents.Agents {
				r := &s.Agents.Agents[i]
				if r.Name == a.Name && r.RevokedAt == nil {
					r.RevokedAt = &now
					found = true
				}
			}
			if !found {
				return Receipt{}, errors.New("no active agent has that name")
			}
		case "mandate-revoke":
			found := false
			for i := range s.Mandates.Mandates {
				r := &s.Mandates.Mandates[i]
				if r.ID == a.ID && r.RevokedAt == nil {
					r.RevokedAt = &now
					found = true
				}
			}
			if !found {
				return Receipt{}, errors.New("no active mandate has that ID")
			}
		}
		c.memory = &s
		receipt.Message = "Simulation recorded in this process only. No engine command ran; no money moved."
		if a.Kind == "evidence-export" {
			receipt.Message = "Export simulated only. No signed pack was created; no signing fingerprint is available."
		}
		if a.Kind == "integrate" {
			receipt.Message = "Connection simulated. No agent configuration was changed."
		}
		if a.Kind == "project-init" {
			receipt.Message = "Initialization simulated. No project was created or captured."
		}
		return receipt, nil
	}
	data, err := c.execute(ctx, args)
	if err != nil {
		return Receipt{}, err
	}
	schemas := map[string]string{"approve": "fabric.approve/v2", "agent-revoke": "fabric.agent.revoke/v1", "mandate-revoke": "fabric.mandate.revoke/v1", "evidence-export": "fabric.evidence.export/v1", "checkpoint": "fabric.checkpoint/v1", "integrate": "fabric.integrate/v1", "project-init": "fabric.init/v1"}
	var result struct {
		Requests    []Approval `json:"requests"`
		Out         string     `json:"out"`
		Fingerprint string     `json:"fingerprint"`
	}
	if err = CheckSchema(data, schemas[a.Kind], &result); err != nil {
		return Receipt{}, err
	}
	if a.Kind == "checkpoint" || a.Kind == "integrate" || a.Kind == "project-init" {
		var details Object
		if err = Decode(data, &details); err != nil {
			return Receipt{}, err
		}
		receipt.Detail = SafeJSON(details)
		switch a.Kind {
		case "checkpoint":
			recorded, ok := details["recorded"].(bool)
			if !ok {
				return Receipt{}, errors.New("checkpoint receipt is missing its recorded status")
			}
			receipt.Message = "No changes since the last checkpoint."
			if recorded {
				receipt.Message = "Checkpoint recorded. Open /checkpoints to inspect it."
			}
		case "integrate":
			if details["agent"] != a.Name {
				return Receipt{}, errors.New("integration receipt names a different agent")
			}
			receipt.Message = "Integration command completed. Inspect the setup result below, then restart your coding agent."
		case "project-init":
			receipt.Message = "Project initialization completed. Open /status to inspect recording."
		}
	}
	if a.Kind == "approve" {
		matched := false
		status := "approved"
		if a.Reject {
			status = "rejected"
		}
		for _, r := range result.Requests {
			if r.ID == a.ID && r.Digest == a.Digest && r.Status == status && Value(r.Approver) == c.User {
				matched = true
			}
		}
		if !matched {
			return Receipt{}, errors.New("receipt did not match this decision; refresh before retrying")
		}
	}
	if a.Kind == "evidence-export" {
		if result.Out == "" || result.Fingerprint == "" {
			return Receipt{}, errors.New("export receipt is missing its path or signing fingerprint")
		}
		receipt.Out = result.Out
		receipt.Fingerprint = result.Fingerprint
		receipt.Message = "Signed evidence pack exported. Verify the pack against the displayed fingerprint."
	}
	return receipt, nil
}
func Command(a Action, user string) ([]string, error) {
	validate := func(s string) bool {
		return strings.TrimSpace(s) != "" && !strings.HasPrefix(s, "-") && !strings.ContainsAny(s, "\x00\r\n")
	}
	if !validate(user) {
		return nil, errors.New("invalid approver identity")
	}
	switch a.Kind {
	case "checkpoint":
		if !validate(a.Note) || len(a.Note) > 500 {
			return nil, errors.New("checkpoint message must be 1–500 bytes, without a leading dash or newline")
		}
		return []string{"checkpoint", "-m", a.Note, "--if-changed", "--json"}, nil
	case "project-init":
		return []string{"init", "--json"}, nil
	case "integrate":
		if a.Name != "claude-code" && a.Name != "codex" {
			return nil, errors.New("supported setup targets are claude-code and codex")
		}
		return []string{"integrate", a.Name, "--json"}, nil
	case "approve":
		if a.ID == 0 || a.Digest == "" {
			return nil, errors.New("a reviewed approval ID and digest are required")
		}
		args := []string{"approve", strconv.FormatUint(a.ID, 10), "--yes", "--as", user}
		if a.Reject {
			args = append(args, "--reject")
		}
		if a.Note != "" {
			if !validate(a.Note) || len(a.Note) > 2000 {
				return nil, errors.New("invalid note (maximum 2000 bytes; no leading dash or newline)")
			}
			args = append(args, "--note", a.Note)
		}
		return append(args, "--json"), nil
	case "agent-revoke":
		if !validate(a.Name) {
			return nil, errors.New("invalid agent name")
		}
		return []string{"agent", "revoke", a.Name, "--json"}, nil
	case "mandate-revoke":
		if a.ID == 0 {
			return nil, errors.New("invalid mandate ID")
		}
		return []string{"mandate", "revoke", fmt.Sprint(a.ID), "--json"}, nil
	case "evidence-export":
		if !validate(a.Out) {
			return nil, errors.New("an output directory is required")
		}
		return []string{"evidence", "export", "--out", a.Out, "--json"}, nil
	default:
		return nil, errors.New("unsupported action")
	}
}
