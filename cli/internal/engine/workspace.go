package engine

import "context"

type ChangeCounts struct {
	Added    uint64 `json:"added"`
	Modified uint64 `json:"modified"`
	Removed  uint64 `json:"removed"`
	Renamed  uint64 `json:"renamed"`
}

type Checkpoint struct {
	Seq       uint64       `json:"seq"`
	CreatedAt string       `json:"created_at"`
	Kind      string       `json:"kind"`
	Label     string       `json:"label"`
	Agent     *string      `json:"agent"`
	Snapshot  string       `json:"snapshot"`
	Changes   ChangeCounts `json:"changes"`
}

type History struct {
	Schema      string       `json:"schema"`
	ProjectRoot string       `json:"project_root"`
	Total       uint64       `json:"total"`
	Checkpoints []Checkpoint `json:"checkpoints"`
}

type Rule struct {
	ID          string `json:"rule"`
	Description string `json:"description"`
	Decision    string `json:"decision"`
	AppliesTo   string `json:"applies_to"`
	Source      string `json:"source"`
}
type Rules struct {
	Schema string `json:"schema"`
	Rules  []Rule `json:"rules"`
}

type Change struct {
	ID             uint64   `json:"id"`
	Title          string   `json:"title"`
	Status         string   `json:"status"`
	Paths          []string `json:"paths"`
	Session        *string  `json:"session"`
	BaseCheckpoint uint64   `json:"base_checkpoint"`
	AuthorizedBy   string   `json:"authorized_by"`
	AuthorizedAt   string   `json:"authorized_at"`
	ReviewedBy     *string  `json:"reviewed_by"`
	ReviewedAt     *string  `json:"reviewed_at"`
}
type Changes struct {
	Schema  string   `json:"schema"`
	Changes []Change `json:"changes"`
}

type Diff struct {
	Schema string `json:"schema"`
	From   string `json:"from"`
	To     string `json:"to"`
	Diff   Object `json:"diff"`
}

// Diff is loaded only on an explicit request: the engine captures the working
// tree to compute it, so it is not a background status poll.
func (c *Client) Diff(ctx context.Context) (Diff, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	var diff Diff
	err := c.load(ctx, "workspace_diff.json", "fabric.diff/v1", []string{"diff"}, &diff)
	return diff, err
}

// Local code requests carry complete file/command input rather than currency
// declarations. These are the exact built-in tool names recognized by the engine.
// MCP requests still require declared payment units before approval.
func ApprovalSummary(a Approval, declarations []Declaration, fixture bool) (string, bool) {
	switch a.Tool {
	case "Edit", "Write", "MultiEdit", "NotebookEdit":
		return "File change · " + a.Target, true
	case "Bash":
		return "Shell command · review exact input", true
	default:
		return ApprovalAmount(a, declarations, fixture)
	}
}

func LocalRequest(a Approval) bool {
	switch a.Tool {
	case "Edit", "Write", "MultiEdit", "NotebookEdit", "Bash":
		return true
	}
	return false
}

// Follow reads the small, changing parts of the workspace. Full evidence
// verification and authority queries remain explicit refreshes.
func (c *Client) Follow(ctx context.Context) Snapshot {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.Fixtures {
		return c.snapshot(ctx)
	}
	s := Snapshot{Errors: map[string]string{}}
	reads := []struct {
		key, schema string
		args        []string
		out         any
	}{
		{"calls", "fabric.evidence.calls/v1", []string{"evidence", "calls", "-n", "200"}, &s.Calls},
		{"approvals", "fabric.approvals/v1", []string{"approvals", "--all"}, &s.Approvals},
		{"history", "fabric.log/v1", []string{"log", "-n", "30"}, &s.History},
	}
	for _, r := range reads {
		if err := c.load(ctx, "", r.schema, r.args, r.out); err != nil {
			s.Errors[r.key] = Clean(err.Error())
		}
	}
	return s
}
