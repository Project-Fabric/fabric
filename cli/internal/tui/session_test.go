package tui

import (
	"context"
	"strings"
	"testing"

	"github.com/Project-Fabric/fabric/cli/internal/engine"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/x/ansi"
)

func press(m Model, key tea.KeyType) Model {
	model, _ := m.Update(tea.KeyMsg{Type: key})
	return model.(Model)
}
func run(m Model, command string) Model { m = key(m, command); return key(m, "enter") }

func TestCommandCompletionAndNavigation(t *testing.T) {
	m := ready()
	m = key(m, "/checkp")
	m = press(m, tea.KeyTab)
	if m.draft != "/checkpoints" {
		t.Fatal(m.draft)
	}
	m = key(m, "enter")
	if m.Page != 7 || len(m.rows()) != 2 {
		t.Fatal("checkpoint view did not open")
	}
	m = press(m, tea.KeyCtrlK)
	m = key(m, "connect")
	m = key(m, "enter")
	if m.Page != 9 || len(m.rows()) != 3 {
		t.Fatal("connection view did not open")
	}
	m = run(m, "/activity")
	if m.Page != 2 {
		t.Fatal("pasted slash command did not open from a list")
	}
}

func TestInputEditingAndHistory(t *testing.T) {
	m := ready()
	m = key(m, "/statu")
	m = press(m, tea.KeyLeft)
	m = press(m, tea.KeyRight)
	m = key(m, "s")
	m = key(m, "enter")
	if m.Page != 6 {
		t.Fatal("edited command did not run")
	}
	m = key(m, "esc")
	m = press(m, tea.KeyUp)
	if m.draft != "/status" {
		t.Fatal("history was not recalled", m.draft)
	}
	m = press(m, tea.KeyDown)
	if m.draft != "" {
		t.Fatal("history draft not restored")
	}
	m = run(m, "/clear")
	if len(m.history) != 0 || len(m.entries) != 0 {
		t.Fatal("clear left input history")
	}
}

func TestSlashApprovalLandsOnReviewWithoutMutation(t *testing.T) {
	m := run(ready(), "/approve 1")
	if m.Page != 1 || !m.detail || m.overlay != "" {
		t.Fatal("approval bypassed full review")
	}
	a, _ := m.Client.Approval(context.Background(), 1)
	if a.Status != "pending" {
		t.Fatal("command decided without confirmation")
	}
}

func TestSetupAndCheckpointRequireConfirmation(t *testing.T) {
	for _, command := range []string{"/connect claude-code", "/connect codex", "/checkpoint before refactor", "/init"} {
		m := run(ready(), command)
		if m.overlay != "confirm" || m.busy {
			t.Fatal(command, "did not stop for confirmation")
		}
		m = key(m, "enter")
		if m.busy {
			t.Fatal(command, "accepted empty confirmation")
		}
		m = key(m, "esc")
		if m.overlay != "" {
			t.Fatal("cancel failed")
		}
	}
	m := run(ready(), "/connect cursor")
	if m.Page != 9 || !m.detail || m.overlay != "" {
		t.Fatal("Cursor manual setup was not shown")
	}
	m = run(ready(), "/connect unsupported")
	if len(m.entries) != 1 || m.overlay != "" {
		t.Fatal("accepted unknown integration")
	}
}

func TestConfirmationRefusesUnreadableTerminal(t *testing.T) {
	m := run(ready(), "/checkpoint before refactor")
	m.Width = 40
	m.Height = 16
	m = key(m, m.phrase())
	m = key(m, "enter")
	if m.busy || !strings.Contains(m.status, "Resize") {
		t.Fatal("dispatched from an unreadable confirmation")
	}
}

func TestFollowNeverChangesReviewInProgress(t *testing.T) {
	m := ready()
	m.Watch = true
	m = key(m, "1")
	m = key(m, "enter")
	before := m.Data.Approvals.Requests[0].Digest
	other := engine.Snapshot{Errors: map[string]string{}}
	model, _ := m.Update(followMsg(other))
	m = model.(Model)
	if len(m.Data.Approvals.Requests) == 0 || m.Data.Approvals.Requests[0].Digest != before {
		t.Fatal("background refresh changed the active review")
	}
	model, cmd := m.Update(pollMsg{})
	m = model.(Model)
	if m.polling || cmd == nil {
		t.Fatal("polling was not deferred during review")
	}
}

func TestNoShellOrLanguageModelExecution(t *testing.T) {
	m := run(ready(), "npm install; echo injected")
	if m.busy || m.overlay != "" || len(m.entries) != 1 {
		t.Fatal("plain text dispatched an action")
	}
	if !strings.Contains(m.entries[0].text, "/help") {
		t.Fatal("no actionable guidance")
	}
}

func TestDeveloperLayoutsFitEveryScreen(t *testing.T) {
	for _, size := range [][2]int{{36, 16}, {60, 24}, {80, 24}, {110, 34}} {
		for page := 0; page < len(pages); page++ {
			m := ready()
			m.Page = page
			m.Width = size[0]
			m.Height = size[1]
			for _, menu := range []bool{false, true} {
				if menu {
					m = press(m, tea.KeyCtrlK)
				}
				view := m.View()
				lines := strings.Split(view, "\n")
				if len(lines) > m.Height {
					t.Fatalf("page %d menu %t: %d lines exceed %d", page, menu, len(lines), m.Height)
				}
				for _, line := range lines {
					if ansi.StringWidth(line) > m.Width {
						t.Fatalf("page %d: line %q exceeds width %d", page, line, m.Width)
					}
				}
				if strings.Contains(view, "AgentAuth") {
					t.Fatal("old product label")
				}
			}
		}
	}
}
