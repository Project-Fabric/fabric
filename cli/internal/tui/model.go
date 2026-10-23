package tui

import (
	"context"
	"fmt"
	"strings"
	"time"
	"unicode"

	"github.com/Project-Fabric/fabric/cli/internal/engine"
	tea "github.com/charmbracelet/bubbletea"
)

type tickMsg time.Time
type snapshotMsg engine.Snapshot
type followMsg engine.Snapshot
type pollMsg time.Time
type actionMsg struct {
	receipt  engine.Receipt
	err      error
	snapshot engine.Snapshot
}
type row struct {
	title, sub string
	index      int
}

var pages = []string{"Session", "Approvals", "Activity", "Authority", "Agents", "Evidence", "Project status", "Checkpoints", "Rules", "Connections", "Changes"}

type Model struct {
	Watch, polling                       bool
	commandMode                          bool
	draft                                string
	cursor, commandSelection, historyAt  int
	historyDraft                         string
	history                              []string
	entries                              []sessionEntry
	Client                               *engine.Client
	Data                                 engine.Snapshot
	Page                                 int
	Width, Height                        int
	NoAnimation, NoColor                 bool
	selected, scroll                     int
	detail, all, payments, busy, ticking bool
	phase                                float64
	overlay, entry, note, status         string
	action                               engine.Action
	receipt                              *engine.Receipt
}

func New(client *engine.Client, noAnimation, noColor bool, page int) Model {
	return Model{Client: client, NoAnimation: noAnimation, NoColor: noColor, Page: page, Width: 100, Height: 34, busy: true, ticking: !noAnimation}
}
func (m Model) Init() tea.Cmd { return tea.Batch(m.load(), m.tick(), m.poll()) }
func (m Model) poll() tea.Cmd {
	if !m.Watch {
		return nil
	}
	return tea.Tick(5*time.Second, func(t time.Time) tea.Msg { return pollMsg(t) })
}
func (m Model) load() tea.Cmd {
	return func() tea.Msg { return snapshotMsg(m.Client.Snapshot(context.Background())) }
}
func (m Model) tick() tea.Cmd {
	if m.NoAnimation {
		return nil
	}
	return tea.Tick(time.Second/24, func(t time.Time) tea.Msg { return tickMsg(t) })
}
func (m Model) startTick() (Model, tea.Cmd) {
	if m.NoAnimation || m.ticking || !m.animate() {
		return m, nil
	}
	m.ticking = true
	return m, m.tick()
}
func (m Model) animate() bool {
	return ((m.Page == 0 && len(m.entries) == 0 && m.draft == "") || m.busy) && m.overlay == "" && !m.detail
}
func (m Model) Update(message tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := message.(type) {
	case pollMsg:
		if !m.Watch {
			return m, nil
		}
		if m.busy || m.polling || m.overlay != "" || m.detail || m.draft != "" {
			return m, m.poll()
		}
		m.polling = true
		client := m.Client
		return m, tea.Batch(m.poll(), func() tea.Msg { return followMsg(client.Follow(context.Background())) })
	case followMsg:
		m.polling = false
		if m.overlay != "" || m.detail || m.busy {
			return m, nil
		}
		s := engine.Snapshot(msg)
		if m.Data.Errors == nil {
			m.Data.Errors = map[string]string{}
		}
		for _, key := range []string{"calls", "approvals", "history"} {
			if err := s.Errors[key]; err != "" {
				m.Data.Errors[key] = err
			} else {
				delete(m.Data.Errors, key)
				switch key {
				case "calls":
					m.Data.Calls = s.Calls
				case "approvals":
					m.Data.Approvals = s.Approvals
				case "history":
					m.Data.History = s.History
				}
			}
		}
		m.selected = min(m.selected, max(0, len(m.rows())-1))
		return m, nil
	case diffMsg:
		m.busy = false
		if msg.err != nil {
			m.appendEntry("Diff unavailable", msg.err.Error())
		} else {
			m.appendEntry("Changes since the last checkpoint", engine.SafeJSON(msg.diff))
		}
		return m, nil
	case tea.WindowSizeMsg:
		m.Width = max(1, msg.Width)
		m.Height = max(1, msg.Height)
		return m, nil
	case tickMsg:
		m.ticking = false
		if m.animate() && !m.NoAnimation {
			m.phase += .055
			return m.startTick()
		}
		return m, nil
	case snapshotMsg:
		m.Data = engine.Snapshot(msg)
		m.busy = false
		m.selected = min(m.selected, max(0, len(m.rows())-1))
		return m, nil
	case actionMsg:
		m.busy = false
		m.overlay = "receipt"
		m.scroll = 0
		m.receipt = &msg.receipt
		if msg.err != nil {
			m.status = "Could not complete: " + engine.Clean(msg.err.Error())
			m.receipt = nil
		} else {
			m.status = msg.receipt.Message
			m.Data = msg.snapshot
			m.detail = false
			m.selected = min(m.selected, max(0, len(m.rows())-1))
			m.appendEntry("Recorded action", msg.receipt.Message)
		}
		return m, nil
	case tea.KeyMsg:
		key := msg.String()
		if key == "ctrl+c" {
			return m, tea.Quit
		}
		if m.busy {
			return m, nil
		}
		if m.overlay != "" {
			return m.overlayKey(msg)
		}
		if key == "ctrl+k" {
			m.commandMode = true
			m.draft = "/"
			m.cursor = 1
			m.commandSelection = 0
			return m, nil
		}
		if m.commandFocused() && !(m.Page == 0 && m.draft == "" && len(key) == 1 && key[0] >= '1' && key[0] <= '6') {
			return m.commandKey(msg)
		}
		if msg.Type == tea.KeyRunes && !msg.Alt && strings.HasPrefix(string(msg.Runes), "/") {
			m.commandMode = true
			m.draft = ""
			m.cursor = 0
			m.commandSelection = 0
			return m.commandKey(msg)
		}
		if key == "q" {
			return m, tea.Quit
		}
		if key == "ctrl+r" {
			m.detail = false
			m.scroll = 0
			m.busy = true
			m.status = "Refreshing local records..."
			next, tick := m.startTick()
			return next, tea.Batch(m.load(), tick)
		}
		if key == "esc" {
			if m.detail {
				m.detail = false
				m.scroll = 0
				return m, nil
			}
			m.Page = 0
			m.selected = 0
			return m.startTick()
		}
		if len(key) == 1 && key[0] >= '1' && key[0] <= '6' {
			m.Page = int(key[0] - '0')
			m.selected = 0
			m.detail = false
			m.scroll = 0
			return m, nil
		}
		if m.detail || m.Page == 6 {
			switch key {
			case "down", "j":
				m.scroll++
			case "up", "k":
				m.scroll = max(0, m.scroll-1)
			case "pgdown":
				m.scroll += max(1, m.Height-12)
			case "pgup":
				m.scroll = max(0, m.scroll-max(1, m.Height-12))
			case "home":
				m.scroll = 0
			case "a", "r":
				return m.beginDecision(key)
			case "c":
				if m.Page == 9 && m.selected < 2 {
					name := []string{"claude-code", "codex"}[m.selected]
					return m.executeCommand("/connect " + name)
				}
			case "enter":
				m.detail = false
				m.scroll = 0
			}
			return m, nil
		}
		switch key {
		case "up", "k":
			m.selected = max(0, m.selected-1)
		case "down", "j":
			m.selected = min(max(0, len(m.rows())-1), m.selected+1)
		case "home":
			m.selected = 0
		case "end":
			m.selected = max(0, len(m.rows())-1)
		case "pgdown":
			m.selected = min(max(0, len(m.rows())-1), m.selected+max(1, (m.Height-13)/3))
		case "pgup":
			m.selected = max(0, m.selected-max(1, (m.Height-13)/3))
		case "enter":
			if len(m.rows()) > 0 {
				m.detail = true
				m.scroll = 0
			}
		case "tab":
			if m.Page == 1 {
				m.all = !m.all
				m.selected = 0
			}
		case "f":
			if m.Page == 2 {
				m.payments = !m.payments
				m.selected = 0
			}
		case "e":
			if m.Page == 5 {
				m.overlay = "export"
				m.entry = ""
			}
		}
	}
	return m, nil
}
func (m Model) beginDecision(key string) (tea.Model, tea.Cmd) {
	rows := m.rows()
	if m.selected >= len(rows) {
		return m, nil
	}
	index := rows[m.selected].index
	switch m.Page {
	case 1:
		a := m.Data.Approvals.Requests[index]
		if a.Status != "pending" {
			m.status = "Only pending requests can be decided."
			return m, nil
		}
		if key == "a" {
			if _, known := engine.ApprovalSummary(a, m.Data.Declarations.Payments, m.Client.Fixtures); !known {
				m.status = "Approval disabled: payment units are unknown. Review the raw input and declarations."
				return m, nil
			}
		}
		m.action = engine.Action{Kind: "approve", ID: a.ID, Digest: a.Digest, Reject: key == "r"}
		m.overlay = "note"
	case 3:
		if key != "r" || index >= len(m.Data.Mandates.Mandates) {
			return m, nil
		}
		a := m.Data.Mandates.Mandates[index]
		if a.RevokedAt != nil {
			return m, nil
		}
		m.action = engine.Action{Kind: "mandate-revoke", ID: a.ID}
		m.overlay = "confirm"
	case 4:
		if key != "r" {
			return m, nil
		}
		a := m.Data.Agents.Agents[index]
		if a.RevokedAt != nil {
			return m, nil
		}
		m.action = engine.Action{Kind: "agent-revoke", Name: a.Name}
		m.overlay = "confirm"
	default:
		return m, nil
	}
	m.entry = ""
	m.note = ""
	m.status = ""
	return m, nil
}
func (m Model) phrase() string {
	switch m.action.Kind {
	case "checkpoint":
		return "create checkpoint"
	case "project-init":
		return "initialize project"
	case "integrate":
		return "connect " + m.action.Name
	case "approve":
		verb := "approve"
		if m.action.Reject {
			verb = "reject"
		}
		return fmt.Sprintf("%s %d", verb, m.action.ID)
	case "agent-revoke":
		return "revoke " + m.action.Name
	case "mandate-revoke":
		return fmt.Sprintf("revoke mandate %d", m.action.ID)
	case "evidence-export":
		return "export evidence"
	}
	return ""
}
func (m Model) overlayKey(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	key := msg.String()
	if key == "esc" {
		m.overlay = ""
		m.entry = ""
		m.receipt = nil
		m.status = "Cancelled."
		return m.startTick()
	}
	if m.overlay == "receipt" {
		switch key {
		case "enter":
			m.overlay = ""
			m.receipt = nil
			m.entry = ""
			m.scroll = 0
			return m.startTick()
		case "down", "j":
			m.scroll++
		case "up", "k":
			m.scroll = max(0, m.scroll-1)
		}
		return m, nil
	}
	if key == "enter" {
		switch m.overlay {
		case "note":
			m.note = m.entry
			m.action.Note = m.entry
			m.entry = ""
			m.overlay = "confirm"
		case "export":
			if strings.TrimSpace(m.entry) == "" {
				m.status = "Enter an output directory."
				return m, nil
			}
			m.action = engine.Action{Kind: "evidence-export", Out: m.entry}
			m.entry = ""
			m.overlay = "confirm"
		case "confirm":
			if m.Width < 60 || m.Height < 24 {
				m.status = "Resize to at least 60 columns × 24 rows to review and confirm."
				return m, nil
			}
			if m.entry != m.phrase() {
				m.status = "Type the exact phrase shown to confirm."
				return m, nil
			}
			if _, err := engine.Command(m.action, m.Client.User); err != nil {
				m.status = engine.Clean(err.Error())
				return m, nil
			}
			m.busy = true
			m.overlay = "working"
			m.status = "Waiting for Cosigna..."
			action := m.action
			client := m.Client
			return m, func() tea.Msg {
				receipt, err := client.Perform(context.Background(), action)
				var data engine.Snapshot
				if err == nil {
					data = client.Snapshot(context.Background())
				}
				return actionMsg{receipt: receipt, err: err, snapshot: data}
			}
		}
		return m, nil
	}
	if key == "backspace" || key == "ctrl+h" {
		r := []rune(m.entry)
		if len(r) > 0 {
			m.entry = string(r[:len(r)-1])
		}
		return m, nil
	}
	if key == "ctrl+u" {
		m.entry = ""
		return m, nil
	}
	if msg.Type == tea.KeyRunes || msg.Type == tea.KeySpace {
		text := string(msg.Runes)
		if msg.Type == tea.KeySpace {
			text = " "
		}
		for _, r := range text {
			if !unicode.IsControl(r) && len(m.entry) < 2000 {
				m.entry += string(r)
			}
		}
	}
	return m, nil
}

func (m Model) rows() []row {
	var rows []row
	switch m.Page {
	case 7:
		for i, c := range m.Data.History.Checkpoints {
			rows = append(rows, row{fmt.Sprintf("#%d  %s", c.Seq, c.Label), c.CreatedAt + " · " + c.Kind + " · " + engine.Value(c.Agent), i})
		}
	case 8:
		for i, r := range m.Data.Rules.Rules {
			rows = append(rows, row{r.ID + " / " + r.Decision, r.Description, i})
		}
	case 9:
		rows = []row{{"Claude Code", "Project hooks + MCP setup", 0}, {"Codex", "MCP setup; checkpoint recording is agent-driven", 1}, {"Cursor", "Manual MCP setup instructions", 2}}
	case 10:
		for i, c := range m.Data.Changes.Changes {
			rows = append(rows, row{fmt.Sprintf("#%d  %s / %s", c.ID, c.Title, c.Status), strings.Join(c.Paths, ", ") + " · " + c.AuthorizedBy, i})
		}
	case 1:
		for i, a := range m.Data.Approvals.Requests {
			if !m.all && a.Status != "pending" {
				continue
			}
			money, _ := engine.ApprovalSummary(a, m.Data.Declarations.Payments, m.Client.Fixtures)
			rows = append(rows, row{fmt.Sprintf("#%d  %s  /  %s", a.ID, money, a.Status), a.Target + "  ·  " + a.Agent, i})
		}
	case 2:
		for i, c := range m.Data.Calls.Calls {
			if m.payments && c.Payment == nil && !strings.Contains(c.Tool, "write") {
				continue
			}
			money := ""
			if c.Payment != nil {
				money = "  " + strings.ToUpper(c.Payment.Currency) + " " + c.Payment.Amount
			}
			rows = append(rows, row{fmt.Sprintf("#%d  %s  /  %s%s", c.Record, c.Tool, c.Outcome, money), c.Agent + "  ·  " + c.At + "  ·  " + c.Decision, i})
		}
	case 3:
		for i, a := range m.Data.Mandates.Mandates {
			status := "granted"
			if a.RevokedAt != nil {
				status = "revoked"
			}
			rows = append(rows, row{fmt.Sprintf("#%d  %s  /  %s", a.ID, a.Terms.Title, status), strings.Join(a.Terms.Tools, ", ") + "  ·  expires " + engine.Value(a.Terms.Expires), i})
		}
		for i, c := range m.Data.Consents.Consents {
			rows = append(rows, row{"Consent  " + c.Description, c.Principal + "  ·  " + c.ID, len(m.Data.Mandates.Mandates) + i})
		}
	case 4:
		for i, a := range m.Data.Agents.Agents {
			status := "active"
			if a.RevokedAt != nil {
				status = "revoked"
			}
			rows = append(rows, row{a.Name + "  /  " + status, strings.Join(a.Tools, ", "), i})
		}
	case 5:
		rows = append(rows, row{"Evidence verification", "Hash chain, signatures and recorded mandate uses", 0})
	case 6:
		rows = append(rows, row{"Local control summary", "Counts from this loaded snapshot", 0})
	}
	return rows
}
