package tui

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/Project-Fabric/fabric/cli/internal/animation"
	"github.com/Project-Fabric/fabric/cli/internal/engine"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
)

func (m Model) projectPath() string {
	if m.Client.Fixtures {
		return "Northwind · demo workspace"
	}
	if m.Data.History.ProjectRoot != "" {
		return engine.Clean(m.Data.History.ProjectRoot)
	}
	if m.Client.Project != "" {
		return engine.Clean(m.Client.Project)
	}
	return "Current directory"
}

func (m Model) developerView() string {
	if m.Width < 36 || m.Height < 16 {
		return "Cosigna\n\nResize to at least 36 columns × 16 rows.\nCtrl+C to quit."
	}
	width := min(m.Width-4, 118)
	mode := "LOCAL"
	if m.Client.Fixtures {
		mode = "FIXTURES"
	}
	badge := mode + " · " + m.pendingLabel()
	if m.Watch {
		badge += " · following"
	}
	header := m.accent("◈ ") + m.strong("cosigna") + m.muted("  v0.1.0  /  "+pages[m.Page])
	header += "  " + m.muted(badge)
	project := m.projectPath()
	if !m.Client.Fixtures {
		project = filepath.Clean(project)
	}
	context := m.muted(ansi.Truncate(project, width, "…"))
	rule := m.muted(strings.Repeat("─", width))
	suggestions := m.suggestions()
	suggestionHeight := 0
	slots := min(5, len(suggestions), max(1, m.Height-14))
	if len(suggestions) > 0 {
		suggestionHeight = slots + 1
	}
	bodyHeight := max(3, m.Height-11-suggestionHeight)
	var body string
	switch {
	case m.overlay != "":
		body = m.overlayView(width)
		if m.overlay == "receipt" {
			body = m.scrollText(body, width, bodyHeight)
		}
	case m.busy:
		body = m.muted("Reading the local project…")
	case m.Page == 0:
		body = m.sessionView(width, bodyHeight)
	case m.detail:
		body = m.details(width, bodyHeight)
	case m.Page == 6:
		body = m.details(width, bodyHeight)
	default:
		body = m.list(width, bodyHeight)
	}
	lines := strings.Split(body, "\n")
	if len(lines) > bodyHeight {
		lines = lines[:bodyHeight]
	}
	for len(lines) < bodyHeight {
		lines = append(lines, "")
	}
	for i := range lines {
		lines[i] = ansi.Truncate(lines[i], width, "…")
	}
	footer := ansi.Truncate(m.commandBar(width), width, "…")
	if len(suggestions) > 0 {
		start := max(0, m.commandSelection-slots+1)
		end := min(len(suggestions), start+slots)
		var choices []string
		for i := start; i < end; i++ {
			s := suggestions[i]
			prefix := "  "
			text := fmt.Sprintf("%-22s %s", s.name+" "+s.args, s.description)
			if i == m.commandSelection {
				prefix = "> "
				text = m.accent(text)
			}
			choices = append(choices, ansi.Truncate(prefix+text, width, "…"))
		}
		footer = m.muted(ansi.Truncate("COMMANDS  ·  ↑↓ select  ·  tab complete  ·  enter open", width, "…")) + "\n" + strings.Join(choices, "\n") + "\n" + footer
	}
	status := engine.Clean(m.status)
	if status == "" {
		status = "/ commands · ctrl+k command menu · esc back · ctrl+c exit"
	}
	if m.detail && m.Page == 1 {
		status = "a approve · r reject · ↑↓ scroll · / commands · esc back"
	}
	if m.Page == 6 && m.overlay == "" {
		status = "↑↓ scroll · / commands · esc back"
	}
	if m.detail && (m.Page == 3 || m.Page == 4) {
		status = "r revoke · ↑↓ scroll · / commands · esc back"
	}
	if m.detail && m.Page == 9 && m.selected < 2 {
		status = "c configure integration · / commands · esc back"
	}
	if m.overlay != "" {
		status = "enter continue · esc cancel · ctrl+u clear · ctrl+c exit"
	}
	screen := ansi.Truncate(header, width, "…") + "\n" + context + "\n\n" + strings.Join(lines, "\n") + "\n" + rule + "\n" + footer + "\n" + rule + "\n" + m.muted(ansi.Truncate(status, width, "…"))
	return lipgloss.NewStyle().Padding(1, 2).Render(screen)
}

func (m Model) pendingLabel() string {
	if m.busy {
		return "loading"
	}
	if m.Data.Errors["approvals"] != "" {
		return "approvals unavailable"
	}
	count := 0
	for _, a := range m.Data.Approvals.Requests {
		if a.Status == "pending" {
			count++
		}
	}
	return fmt.Sprintf("%d pending", count)
}

func (m Model) commandBar(width int) string {
	if m.overlay != "" {
		return m.muted("  Complete the review above to continue.")
	}
	if !m.commandFocused() {
		return m.muted("❯ / commands   ·   enter inspect   ·   ↑↓ navigate")
	}
	input := []rune(engine.Clean(m.draft))
	cursor := min(m.cursor, len(input))
	available := max(1, width-3)
	prefix := string(input[:cursor])
	prefix = ansi.TruncateLeft(prefix, max(0, ansi.StringWidth(prefix)-available+1), "")
	tail := string(input[cursor:])
	if m.draft == "" {
		return m.accent("❯ ") + m.muted("Type / for commands")
	}
	return m.accent("❯ ") + ansi.Truncate(prefix+m.accent("▌")+tail, available, "…")
}

func (m Model) sessionView(width, height int) string {
	if len(m.entries) > 0 {
		var lines []string
		for _, entry := range m.entries {
			lines = append(lines, m.accent("● ")+m.strong(entry.title))
			for _, line := range strings.Split(entry.text, "\n") {
				lines = append(lines, strings.Split(ansi.Hardwrap(line, max(1, width-2), true), "\n")...)
			}
			lines = append(lines, "")
		}
		start := max(0, len(lines)-height-min(m.scroll, max(0, len(lines)-height)))
		return strings.Join(lines[start:min(len(lines), start+height)], "\n")
	}
	intro := m.accent(wordmark) + "\n\n" + m.strong("Your developer workspace.") + "\n" + m.muted("Checkpoints, approvals, and agent activity.")
	if height < 16 {
		intro = m.strong("Welcome to Cosigna") + "\n" + m.muted("Checkpoints, approvals, and agent activity.")
	}
	if width >= 76 && height >= 12 {
		phase := m.phase
		if m.NoAnimation {
			phase = .9
		}
		intro = lipgloss.JoinHorizontal(lipgloss.Top, lipgloss.NewStyle().Width(width-28).Render(intro), m.accent(animation.Frame(26, 7, phase)))
	}
	var lines []string
	lines = append(lines, intro, "", m.strong("Start here"), m.accent("/status")+m.muted("  project  ")+m.accent("/connect")+m.muted("  agent setup  ")+m.accent("/help")+m.muted("  all commands"))
	if m.Data.Errors["history"] != "" {
		lines = append(lines, "", m.color("#FFCA80", "Recording unavailable · /status shows the reason; /init starts a new project."))
	}
	if height >= 16 && len(m.Data.History.Checkpoints) > 0 {
		c := m.Data.History.Checkpoints[0]
		lines = append(lines, "", m.muted(fmt.Sprintf("LATEST CHECKPOINT  #%d  %s", c.Seq, engine.Clean(c.Label))))
	}
	if height >= 19 && len(m.Data.Calls.Calls) > 0 {
		lines = append(lines, "", m.muted("RECENT ACTIVITY  ·  /activity to inspect"))
		for _, c := range m.Data.Calls.Calls[:min(3, len(m.Data.Calls.Calls))] {
			symbol := "✓"
			color := "#8CE99A"
			if c.Outcome == "blocked" || c.Outcome == "error" {
				symbol = "!"
				color = "#FF9B9B"
			}
			lines = append(lines, m.color(color, symbol)+" "+engine.Clean(c.Agent)+m.muted("  "+engine.Clean(c.Tool)+" / "+engine.Clean(c.Outcome)))
		}
	}
	return strings.Join(lines, "\n")
}

func (m Model) connectionDetails() string {
	switch m.selected {
	case 0:
		return "CLAUDE CODE\n\nInstall project hooks for prompt and change checkpoints, plus MCP registration.\n\nPress C or use /connect claude-code to review setup.\n\nThe engine updates .claude/settings.local.json and attempts MCP registration.\nExisting settings are preserved by the engine. Inspect the receipt for the MCP result.\nRestart Claude Code after setup. No connection is inferred from this screen."
	case 1:
		return "CODEX\n\nRegister the local MCP server in the Codex configuration.\n\nPress C or use /connect codex to review setup.\n\nThis changes the global Codex configuration, not just this project.\nRestart Codex after setup. Steps are recorded when the agent calls the checkpoint tool.\nThis integration does not add Claude-style automatic per-tool hooks."
	default:
		return "CURSOR\n\nConfigure the local MCP server in Cursor's project MCP settings:\n\n" + engine.SafeJSON(engine.Object{"mcpServers": engine.Object{"cosigna": engine.Object{"command": m.Client.Binary, "args": []string{"mcp"}}}}) + "\n\nUse an absolute engine executable path if it is not on Cursor's PATH.\nNo configuration is changed by this screen. This provides MCP tools; it does not\ninstall automatic hooks or put all Cursor tool calls through the gateway."
	}
}
