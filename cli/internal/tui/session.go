package tui

import (
	"context"
	"fmt"
	"strconv"
	"strings"
	"unicode"

	"github.com/Project-Fabric/fabric/cli/internal/engine"
	tea "github.com/charmbracelet/bubbletea"
)

type sessionEntry struct{ title, text string }
type diffMsg struct {
	diff engine.Diff
	err  error
}
type commandSpec struct {
	name, args, description string
	page                    int
}

var commands = []commandSpec{
	{"/help", "", "Show commands and keyboard shortcuts", -1},
	{"/status", "", "Inspect project recording and data sources", 6},
	{"/activity", "", "Inspect recent agent tool calls", 2},
	{"/approvals", "", "Review requests waiting for a person", 1},
	{"/checkpoints", "", "Browse recorded project checkpoints", 7},
	{"/checkpoint", "message", "Record a checkpoint after confirmation", -1},
	{"/changes", "", "Browse authorized changes", 10},
	{"/diff", "", "Compare the last checkpoint to the working tree", -1},
	{"/rules", "", "Inspect configured guard rules", 8},
	{"/connect", "[claude-code|codex|cursor]", "Set up a coding-agent integration", 9},
	{"/agents", "", "Inspect registered tool identities", 4},
	{"/authority", "", "Inspect mandates and consents", 3},
	{"/evidence", "", "Inspect evidence verification and export", 5},
	{"/approve", "ID", "Open a request and prepare approval", -1},
	{"/reject", "ID", "Open a request and prepare rejection", -1},
	{"/init", "", "Start recording this project after confirmation", -1},
	{"/refresh", "", "Reload local engine data", -1},
	{"/animation", "on|off", "Set welcome animation for this session", -1},
	{"/clear", "", "Clear session messages and input history", -1},
	{"/home", "", "Return to the developer session", 0},
	{"/quit", "", "Exit Cosigna", -1},
}

func (m Model) commandFocused() bool { return (m.Page == 0 || m.commandMode) && m.overlay == "" }
func (m Model) suggestions() []commandSpec {
	if !m.commandFocused() || !strings.HasPrefix(m.draft, "/") || strings.ContainsAny(m.draft, " \t") {
		return nil
	}
	var result []commandSpec
	for _, command := range commands {
		if strings.HasPrefix(command.name, m.draft) {
			result = append(result, command)
		}
	}
	return result
}
func (m *Model) appendEntry(title, text string) {
	m.entries = append(m.entries, sessionEntry{engine.Clean(title), engine.Clean(text)})
	if len(m.entries) > 50 {
		m.entries = append([]sessionEntry(nil), m.entries[len(m.entries)-50:]...)
	}
	m.scroll = 0
}

func (m Model) commandKey(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	key := msg.String()
	input := []rune(m.draft)
	switch key {
	case "ctrl+r":
		return m.executeCommand("/refresh")
	case "esc":
		m.draft = ""
		m.cursor = 0
		m.commandMode = false
		m.commandSelection = 0
		return m.startTick()
	case "ctrl+u":
		m.draft = ""
		m.cursor = 0
		m.commandSelection = 0
		return m, nil
	case "ctrl+a", "home":
		m.cursor = 0
		return m, nil
	case "ctrl+e", "end":
		m.cursor = len(input)
		return m, nil
	case "left":
		m.cursor = max(0, m.cursor-1)
		return m, nil
	case "right":
		m.cursor = min(len(input), m.cursor+1)
		return m, nil
	case "backspace", "ctrl+h":
		if m.cursor > 0 {
			input = append(input[:m.cursor-1], input[m.cursor:]...)
			m.cursor--
			m.draft = string(input)
		}
		m.commandSelection = 0
		return m, nil
	case "delete":
		if m.cursor < len(input) {
			m.draft = string(append(input[:m.cursor], input[m.cursor+1:]...))
		}
		m.commandSelection = 0
		return m, nil
	case "up", "down":
		suggestions := m.suggestions()
		if len(suggestions) > 0 && m.historyAt == len(m.history) {
			step := 1
			if key == "up" {
				step = -1
			}
			m.commandSelection = (m.commandSelection + step + len(suggestions)) % len(suggestions)
			return m, nil
		}
		if len(m.history) > 0 {
			if key == "up" {
				if m.historyAt == len(m.history) {
					m.historyDraft = m.draft
				}
				m.historyAt = max(0, m.historyAt-1)
			} else {
				m.historyAt = min(len(m.history), m.historyAt+1)
			}
			m.draft = m.historyDraft
			if m.historyAt < len(m.history) {
				m.draft = m.history[m.historyAt]
			}
			m.cursor = len([]rune(m.draft))
		}
		return m, nil
	case "pgup":
		m.scroll += max(1, m.Height/2)
		return m, nil
	case "pgdown":
		m.scroll = max(0, m.scroll-max(1, m.Height/2))
		return m, nil
	case "tab":
		suggestions := m.suggestions()
		if len(suggestions) > 0 {
			choice := suggestions[min(m.commandSelection, len(suggestions)-1)]
			m.draft = choice.name
			if choice.args != "" {
				m.draft += " "
			}
			m.cursor = len([]rune(m.draft))
			m.commandSelection = 0
		}
		return m, nil
	case "enter":
		if strings.TrimSpace(m.draft) == "" {
			return m, nil
		}
		suggestions := m.suggestions()
		if len(suggestions) > 0 {
			choice := suggestions[min(m.commandSelection, len(suggestions)-1)]
			if m.draft != choice.name {
				m.draft = choice.name
				if choice.args != "" {
					m.draft += " "
					m.cursor = len([]rune(m.draft))
					m.commandSelection = 0
					return m, nil
				}
			}
		}
		text := strings.TrimSpace(m.draft)
		m.draft = ""
		m.cursor = 0
		m.commandMode = false
		m.commandSelection = 0
		if len(m.history) == 0 || m.history[len(m.history)-1] != text {
			m.history = append(m.history, engine.Clean(text))
			if len(m.history) > 100 {
				m.history = append([]string(nil), m.history[len(m.history)-100:]...)
			}
		}
		m.historyAt = len(m.history)
		return m.executeCommand(text)
	}
	if msg.Type == tea.KeyRunes || msg.Type == tea.KeySpace {
		text := string(msg.Runes)
		if msg.Type == tea.KeySpace {
			text = " "
		}
		for _, r := range text {
			if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) || len(input) >= 1000 {
				continue
			}
			input = append(input, 0)
			copy(input[m.cursor+1:], input[m.cursor:])
			input[m.cursor] = r
			m.cursor++
		}
		m.draft = string(input)
		m.commandSelection = 0
		m.historyAt = len(m.history)
	}
	return m, nil
}

func (m Model) executeCommand(text string) (tea.Model, tea.Cmd) {
	parts := strings.Fields(text)
	if len(parts) == 0 {
		return m, nil
	}
	verb := parts[0]
	argument := strings.TrimSpace(strings.TrimPrefix(text, verb))
	fail := func(message string) (tea.Model, tea.Cmd) {
		m.Page = 0
		m.appendEntry("Command needs attention", message)
		return m, nil
	}
	var spec *commandSpec
	for i := range commands {
		if commands[i].name == verb {
			spec = &commands[i]
			break
		}
	}
	if spec == nil {
		return fail("Choose a Cosigna command with /help. Try /status for project context, /activity for agent events, or /approvals for requests.")
	}
	if spec.args == "" && argument != "" {
		return fail("Usage: " + verb)
	}
	m.status = ""
	m.scroll = 0
	if spec.page >= 0 && !(verb == "/connect" && argument != "") {
		m.Page = spec.page
		m.selected = 0
		m.detail = false
		if m.Page == 0 {
			return m.startTick()
		}
		return m, nil
	}
	switch verb {
	case "/quit":
		return m, tea.Quit
	case "/clear":
		m.Page = 0
		m.entries = nil
		m.history = nil
		m.historyAt = 0
		m.historyDraft = ""
		return m.startTick()
	case "/help":
		var lines []string
		for _, c := range commands {
			lines = append(lines, fmt.Sprintf("%-29s %s", c.name+" "+c.args, c.description))
		}
		lines = append(lines, "", "Tab completes · ↑↓ select / recall · Ctrl+K opens commands", "PgUp/PgDn scroll the session · Esc returns · Ctrl+C exits")
		m.Page = 0
		m.appendEntry("Cosigna commands", strings.Join(lines, "\n"))
		return m, nil
	case "/refresh":
		m.busy = true
		next, tick := m.startTick()
		return next, tea.Batch(m.load(), tick)
	case "/animation":
		if argument != "on" && argument != "off" {
			return fail("Usage: /animation on|off")
		}
		m.NoAnimation = argument == "off"
		m.status = "Welcome animation " + argument
		return m.startTick()
	case "/diff":
		m.Page = 0
		m.busy = true
		client := m.Client
		return m, func() tea.Msg { diff, err := client.Diff(context.Background()); return diffMsg{diff, err} }
	case "/approve", "/reject":
		id, err := strconv.ParseUint(argument, 10, 64)
		if err != nil || id == 0 {
			return fail("Usage: " + verb + " ID")
		}
		m.Page = 1
		m.all = true
		m.detail = true
		m.selected = -1
		for index, r := range m.rows() {
			if m.Data.Approvals.Requests[r.index].ID == id {
				m.selected = index
				break
			}
		}
		if m.selected < 0 {
			m.selected = 0
			m.detail = false
			return fail("Request not loaded. Use /refresh, then open /approvals.")
		}
		// Always land on the full request first. A/R opens the separate decision
		// flow, preventing the command itself from skipping review.
		m.status = "Review the complete input. Press A to prepare approval."
		if verb == "/reject" {
			m.status = "Review the complete input. Press R to prepare rejection."
		}
		return m, nil
	case "/connect":
		if argument == "cursor" {
			m.Page = 9
			m.selected = 2
			m.detail = true
			return m, nil
		}
		if argument != "claude-code" && argument != "codex" {
			return fail("Usage: /connect claude-code|codex|cursor")
		}
		m.action = engine.Action{Kind: "integrate", Name: argument}
	case "/checkpoint":
		if argument == "" {
			return fail("Usage: /checkpoint message")
		}
		m.action = engine.Action{Kind: "checkpoint", Note: argument}
	case "/init":
		m.action = engine.Action{Kind: "project-init"}
	}
	if _, err := engine.Command(m.action, m.Client.User); err != nil {
		return fail(err.Error())
	}
	m.overlay = "confirm"
	m.entry = ""
	m.note = m.action.Note
	return m, nil
}
