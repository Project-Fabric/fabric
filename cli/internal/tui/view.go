package tui

import (
	"fmt"
	"sort"
	"strings"

	"github.com/Project-Fabric/fabric/cli/internal/engine"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
)

const wordmark = `  ___ ___  ___ ___ ___ _  _   _
 / __/ _ \/ __|_ _/ __| \| | /_\
| (_| (_) \__ \| | (_ | .` + "`" + ` |/ _ \
 \___\___/|___/___\___|_|\_/_/ \_\`

func (m Model) color(code, text string) string {
	if m.NoColor {
		return text
	}
	return lipgloss.NewStyle().Foreground(lipgloss.Color(code)).Render(text)
}
func (m Model) accent(text string) string { return m.color("#8CE99A", text) }
func (m Model) muted(text string) string  { return m.color("#A1A1AA", text) }
func (m Model) strong(text string) string {
	if m.NoColor {
		return text
	}
	return lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("#FAFAFA")).Render(text)
}
func (m Model) View() string { return m.developerView() }

func (m Model) list(width, height int) string {
	var out []string
	subtitle := map[int]string{1: "PENDING REQUESTS", 2: "RECENT TOOL CALLS · up to 200", 3: "BOUNDED AUTHORITY", 4: "REGISTERED AGENTS", 5: "EVIDENCE", 6: "PROJECT STATUS", 7: "CHECKPOINT HISTORY · up to 30", 8: "CONFIGURED RULES", 9: "CONNECT YOUR CODING AGENT", 10: "AUTHORIZED CHANGES"}[m.Page]
	if m.Page == 1 && m.all {
		subtitle = "ALL REQUESTS"
	}
	if m.Page == 2 && m.payments {
		subtitle = "WRITE / PAYMENT CALLS · up to 200"
	}
	out = append(out, m.muted(subtitle), "")
	key := map[int]string{1: "approvals", 2: "calls", 3: "mandates", 4: "agents", 5: "verification", 7: "history", 8: "rules", 10: "changes"}[m.Page]
	if err := m.Data.Errors[key]; err != "" {
		out = append(out, m.color("#FF9B9B", "Unavailable: "+engine.Clean(err)), "")
	}
	rows := m.rows()
	if len(rows) == 0 {
		return strings.Join(append(out, "No records in this view."), "\n")
	}
	capacity := max(1, (height-len(out)-1)/3)
	start := max(0, m.selected-capacity+1)
	end := min(len(rows), start+capacity)
	for i := start; i < end; i++ {
		r := rows[i]
		prefix := "  "
		title := engine.Clean(r.title)
		if i == m.selected {
			prefix = "> "
			title = m.accent(title)
		}
		out = append(out, prefix+title, "  "+m.muted(engine.Clean(r.sub)), "")
	}
	out = append(out, m.muted(fmt.Sprintf("%d / %d", m.selected+1, len(rows))))
	return strings.Join(out, "\n")
}
func (m Model) detailText() string {
	rows := m.rows()
	if m.selected >= len(rows) {
		return "No record selected."
	}
	i := rows[m.selected].index
	switch m.Page {
	case 7:
		return "RECORDED CHECKPOINT\n\n" + engine.SafeJSON(m.Data.History.Checkpoints[i]) + "\n\nThis is a recorded snapshot. Restore and revert stay explicit engine commands."
	case 8:
		return "CONFIGURED RULE\n\n" + engine.SafeJSON(m.Data.Rules.Rules[i]) + "\n\nA configured rule does not establish that an agent is using the gateway or hooks."
	case 9:
		return m.connectionDetails()
	case 10:
		return "AUTHORIZED CHANGE\n\n" + engine.SafeJSON(m.Data.Changes.Changes[i])
	case 1:
		a := m.Data.Approvals.Requests[i]
		money, known := engine.ApprovalSummary(a, m.Data.Declarations.Payments, m.Client.Fixtures)
		warning := ""
		if !known {
			warning = "\nAPPROVAL DISABLED: units are not declared. Rejection remains available.\n"
		}
		return fmt.Sprintf("REQUEST #%d  /  %s\n\n%s\n%s\nTool       %s\nAgent      %s\nSession    %s\nRequested  %s\nRule       %s\nReason     %s\nApprover   %s\nNote       %s\nDigest     %s\n\nEXACT INPUT (credentials redacted)\n%s", a.ID, a.Status, money, warning, a.Target, a.Agent, engine.Value(a.Session), a.RequestedAt, a.Rule, a.Reason, engine.Value(a.Approver), engine.Value(a.Note), a.Digest, engine.SafeJSON(a.Input))
	case 2:
		c := m.Data.Calls.Calls[i]
		return fmt.Sprintf("TOOL CALL #%d / %s\n\nTool       %s\nAgent      %s\nWhen       %s\nDecision   %s\nRule       %s\nSession    %s\n\nINPUT\n%s\n\nRECORDED RESULT\n%s\n\nPAYMENT AUTHORIZATION\n%s", c.Record, c.Outcome, c.Tool, c.Agent, c.At, c.Decision, engine.Value(c.Rule), engine.Value(c.Session), engine.SafeJSON(engine.Input(c)), engine.Clean(engine.Value(c.Result)), engine.SafeJSON(c.Payment))
	case 3:
		if i < len(m.Data.Mandates.Mandates) {
			a := m.Data.Mandates.Mandates[i]
			return "MANDATE\n\n" + engine.SafeJSON(a) + "\n\nAmount limits are engine units. Usage amounts are integer micro-units (1,000,000 = 1 unit).\nThe mandate schema does not specify a currency. Expiry is the start of its UTC day."
		}
		return "CONSENT (fixture)\n\n" + engine.SafeJSON(m.Data.Consents.Consents[i-len(m.Data.Mandates.Mandates)])
	case 4:
		return "AGENT IDENTITY\n\n" + engine.SafeJSON(m.Data.Agents.Agents[i]) + fmt.Sprintf("\n\nAuthenticated agent identity required: %t", m.Data.Agents.Required)
	case 5:
		if err := m.Data.Errors["verification"]; err != "" {
			return "Verification unavailable: " + err
		}
		v := m.Data.Verification
		status := "Problems reported"
		if v.OK() {
			status = "Checks passed"
		}
		return fmt.Sprintf("%s\n\nChain records: %d\nDeep content check: %t\nSigned decisions checked: %d\nMandate uses checked: %d\n\n%s\n\nThese checks cover the evidence record and the checks reported above.\nThey do not verify external Stripe balances or business correctness.", status, v.Chain.Records, v.Deep, v.Signed.Checked, v.MandateUses.Checked, engine.SafeJSON(v))
	case 6:
		count := func(key string, value any) string {
			if m.Data.Errors[key] != "" {
				return "unavailable"
			}
			return fmt.Sprint(value)
		}
		pending, blocked, active := 0, 0, 0
		for _, a := range m.Data.Approvals.Requests {
			if a.Status == "pending" {
				pending++
			}
		}
		for _, c := range m.Data.Calls.Calls {
			if c.Outcome == "blocked" {
				blocked++
			}
		}
		for _, a := range m.Data.Agents.Agents {
			if a.RevokedAt == nil {
				active++
			}
		}
		text := fmt.Sprintf("PROJECT STATUS\n\n%s\n\nCheckpoints         %s (up to 30 loaded)\nConfigured rules    %s\nPending requests    %s\nRecent tool calls   %s (up to 200 loaded)\nBlocked outcomes    %s in this window\nActive identities   %s\n\nUse /connect to inspect agent setup; rule counts do not establish integration.\n\n", m.projectPath(), count("history", m.Data.History.Total), count("rules", len(m.Data.Rules.Rules)), count("approvals", pending), count("calls", len(m.Data.Calls.Calls)), count("calls", blocked), count("agents", active))
		keys := make([]string, 0, len(m.Data.Errors))
		for key := range m.Data.Errors {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			text += key + ": " + m.Data.Errors[key] + "\n"
		}
		return text
	}
	return ""
}
func (m Model) details(width, height int) string {
	return m.scrollText(engine.Clean(m.detailText()), width, height)
}
func (m Model) scrollText(text string, width, height int) string {
	lines := strings.Split(text, "\n")
	var wrapped []string
	for _, line := range lines {
		wrapped = append(wrapped, strings.Split(ansi.Hardwrap(line, max(1, width), true), "\n")...)
	}
	start := min(m.scroll, max(0, len(wrapped)-height+1))
	end := min(len(wrapped), start+height-1)
	return strings.Join(wrapped[start:end], "\n") + "\n" + m.muted(fmt.Sprintf("lines %d–%d of %d", start+1, end, len(wrapped)))
}
func (m Model) overlayView(width int) string {
	switch m.overlay {
	case "note":
		return m.strong("RECORD A DECISION") + "\n\nOptional note (enter to skip):\n\n" + m.accent(engine.Clean(m.entry)+"▌") + "\n\nA separate confirmation follows."
	case "export":
		return m.strong("EXPORT SIGNED EVIDENCE") + "\n\nOutput directory (must not already exist):\n\n" + m.accent(engine.Clean(m.entry)+"▌") + "\n\nFixture mode simulates export; it creates no files."
	case "confirm":
		mode := "This records a live engine action."
		if m.Client.Fixtures {
			mode = "Simulation only. No real engine mutation."
		}
		context := ""
		if m.action.Kind == "approve" {
			for _, a := range m.Data.Approvals.Requests {
				if a.ID == m.action.ID {
					money, _ := engine.ApprovalSummary(a, m.Data.Declarations.Payments, m.Client.Fixtures)
					context = fmt.Sprintf("#%d · %s · %s\n%s\n", a.ID, money, a.Target, a.Digest)
				}
			}
		}
		if m.action.Out != "" {
			context = engine.Clean(m.action.Out) + "\n"
		}
		if m.action.Kind == "checkpoint" || m.action.Kind == "project-init" || m.action.Kind == "integrate" {
			context = "Project: " + m.projectPath() + "\n"
			switch m.action.Kind {
			case "checkpoint":
				context += "Capture the working tree if it changed. No restore or deletion."
			case "project-init":
				context += "Register this project and capture its initial checkpoint."
			case "integrate":
				if m.action.Name == "codex" {
					context += "Scope: global Codex configuration. Registers MCP; recording is agent-driven."
				} else {
					context += "Scope: project Claude Code hooks and an MCP registration attempt."
				}
			}
		}
		return m.strong("CONFIRM / "+strings.ToUpper(m.phrase())) + "\n" + mode + "\n\nType " + m.accent(m.phrase()) + " and press enter:\n" + m.accent(engine.Clean(m.entry)+"▌") + "\n\nApprover: " + engine.Clean(m.Client.User) + "\nNote: " + engine.Clean(m.note) + "\n" + engine.Clean(context)
	case "working":
		return "Waiting for Cosigna...\n\nThe engine enforces its own human approval and policy checks."
	case "receipt":
		text := m.strong("DECISION RECEIPT") + "\n\n" + engine.Clean(m.status)
		if m.receipt != nil {
			if m.receipt.Detail != "" {
				text += "\n\nEngine result:\n" + m.receipt.Detail
			}
			text += "\n\nArgument array:\n" + engine.SafeJSON(m.receipt.Args)
			if m.receipt.Out != "" {
				text += "\n\nPack: " + engine.Clean(m.receipt.Out) + "\nKey: " + engine.Clean(m.receipt.Fingerprint)
			}
		}
		return text
	}
	return ""
}

// Preview is a plain-text render for CI, piped output and terminals without ANSI.
func Preview(client *engine.Client, data engine.Snapshot, page, width, height int) string {
	m := New(client, true, true, page)
	m.Data = data
	m.busy = false
	m.Width = width
	m.Height = height
	return m.View()
}
