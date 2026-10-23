package tui

import (
	"context"
	"github.com/Project-Fabric/fabric/cli/internal/engine"
	tea "github.com/charmbracelet/bubbletea"
	"strings"
	"testing"
)

func key(m Model, key string) Model {
	var msg tea.KeyMsg
	switch key {
	case "enter":
		msg.Type = tea.KeyEnter
	case "esc":
		msg.Type = tea.KeyEsc
	case "tab":
		msg.Type = tea.KeyTab
	default:
		msg = tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune(key)}
	}
	model, _ := m.Update(msg)
	return model.(Model)
}
func ready() Model {
	c := &engine.Client{Fixtures: true, User: "reviewer"}
	m := New(c, false, true, 0)
	m.Data = c.Snapshot(context.Background())
	m.busy = false
	return m
}
func TestKeyboardApprovalRequiresExplicitConfirmation(t *testing.T) {
	m := ready()
	m = key(m, "1")
	if len(m.rows()) != 1 {
		t.Fatal("pending filter")
	}
	m = key(m, "enter")
	m = key(m, "a")
	if m.overlay != "note" {
		t.Fatal(m.overlay)
	}
	m = key(m, "enter")
	if m.overlay != "confirm" {
		t.Fatal(m.overlay)
	}
	m = key(m, "enter")
	if m.busy {
		t.Fatal("empty confirmation dispatched")
	}
	m = key(m, "approve 1")
	model, cmd := m.Update(tea.KeyMsg{Type: tea.KeyEnter})
	m = model.(Model)
	if !m.busy || cmd == nil {
		t.Fatal("exact confirmation did not dispatch")
	}
	result := cmd()
	model, _ = m.Update(result)
	m = model.(Model)
	if m.overlay != "receipt" || !strings.Contains(m.status, "Simulation") {
		t.Fatal(m.status)
	}
	a, _ := m.Client.Approval(context.Background(), 1)
	if a.Status != "approved" {
		t.Fatal(a.Status)
	}
}
func TestCancelNeverDecides(t *testing.T) {
	m := ready()
	m = key(m, "1")
	m = key(m, "enter")
	m = key(m, "r")
	m = key(m, "enter")
	m = key(m, "esc")
	a, _ := m.Client.Approval(context.Background(), 1)
	if a.Status != "pending" || m.overlay != "" {
		t.Fatal("cancel mutated")
	}
}
func TestAnimationPausesDuringReview(t *testing.T) {
	m := ready()
	model, cmd := m.Update(tickMsg{})
	m = model.(Model)
	if m.phase == 0 || cmd == nil {
		t.Fatal("launcher did not animate")
	}
	m = key(m, "1")
	m = key(m, "enter")
	phase := m.phase
	model, cmd = m.Update(tickMsg{})
	m = model.(Model)
	if cmd != nil || phase != m.phase {
		t.Fatal("review screen animated")
	}
	m.NoAnimation = true
	if m.tick() != nil {
		t.Fatal("motion preference ignored")
	}
}
func TestRepeatedBackDoesNotMultiplyTimers(t *testing.T) {
	m := ready()
	for i := 0; i < 5; i++ {
		model, cmd := m.Update(tea.KeyMsg{Type: tea.KeyEsc})
		m = model.(Model)
		if cmd != nil {
			t.Fatal("scheduled a duplicate animation timer")
		}
	}
}
func TestViewsResizeAndSanitize(t *testing.T) {
	m := ready()
	for page := 0; page <= 6; page++ {
		m.Page = page
		for _, size := range [][2]int{{20, 10}, {36, 16}, {80, 24}, {120, 40}} {
			m.Width = size[0]
			m.Height = size[1]
			if m.View() == "" {
				t.Fatal("empty view")
			}
		}
	}
	m.Page = 1
	m.Width = 100
	m.Height = 36
	m = key(m, "enter")
	if !strings.Contains(m.View(), "USD 450.00") {
		t.Fatal(m.View())
	}
}
