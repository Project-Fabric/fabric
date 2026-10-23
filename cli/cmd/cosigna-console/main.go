package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	osuser "os/user"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/Project-Fabric/fabric/cli/internal/animation"
	"github.com/Project-Fabric/fabric/cli/internal/engine"
	"github.com/Project-Fabric/fabric/cli/internal/tui"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/spf13/cobra"
)

var version = "0.1.0"

func main() {
	if err := command().Execute(); err != nil {
		fmt.Fprintln(os.Stderr, "cosigna-console:", engine.Clean(err.Error()))
		os.Exit(1)
	}
}
func command() *cobra.Command {
	var fixture, noAnimation, noColor, printView, jsonOutput bool
	var watch bool
	var project, binary, user string
	var width, height int
	client := func() (*engine.Client, error) {
		dir, err := filepath.Abs(project)
		if err != nil {
			return nil, err
		}
		stat, err := os.Stat(dir)
		if err != nil || !stat.IsDir() {
			return nil, errors.New("--project must be an existing directory")
		}
		identity := user
		if fixture && identity == "" {
			identity = "priya@northwind.test"
		}
		if !fixture && identity == "" {
			if account, err := osuser.Current(); err == nil {
				identity = account.Username
			}
		}
		return &engine.Client{Fixtures: fixture, Project: dir, Binary: binary, User: identity}, nil
	}
	show := func(page int) func(*cobra.Command, []string) error {
		return func(cmd *cobra.Command, args []string) error {
			c, err := client()
			if err != nil {
				return err
			}
			if jsonOutput || printView {
				s := c.Snapshot(cmd.Context())
				if jsonOutput {
					var output any = s
					switch page {
					case 1:
						output = s.Approvals
					case 2:
						output = s.Calls
					case 3:
						output = struct {
							Mandates engine.Mandates
							Consents engine.Consents
							Errors   map[string]string
						}{s.Mandates, s.Consents, s.Errors}
					case 4:
						output = s.Agents
					case 5:
						output = s.Verification
					case 7:
						output = s.History
					case 8:
						output = s.Rules
					case 10:
						output = s.Changes
					}
					key := map[int]string{1: "approvals", 2: "calls", 4: "agents", 5: "verification", 7: "history", 8: "rules", 10: "changes"}[page]
					if message := s.Errors[key]; message != "" {
						return errors.New(message)
					}
					fmt.Fprintln(cmd.OutOrStdout(), engine.SafeJSON(output))
				} else {
					fmt.Fprintln(cmd.OutOrStdout(), tui.Preview(c, s, page, width, height))
				}
				return nil
			}
			info, err := os.Stdin.Stat()
			if err != nil || info.Mode()&os.ModeCharDevice == 0 {
				return errors.New("interactive mode needs a terminal; use --print or --json for piped output")
			}
			model := tui.New(c, noAnimation, noColor || os.Getenv("NO_COLOR") != "", page)
			model.Watch = watch && !fixture
			_, err = tea.NewProgram(model, tea.WithAltScreen(), tea.WithContext(cmd.Context())).Run()
			return err
		}
	}
	root := &cobra.Command{Use: "cosigna-console", Short: "Cosigna developer workspace", Long: "Cosigna: checkpoints, agent activity, and approvals in your terminal.\nType /help inside a session for commands. Run --fixtures to explore the Northwind demo.", Version: version, Args: cobra.NoArgs, SilenceUsage: true, SilenceErrors: true, RunE: show(0)}
	flags := root.PersistentFlags()
	flags.BoolVar(&fixture, "fixtures", false, "Use embedded fixtures; mutations last for this process only")
	flags.StringVar(&project, "project", ".", "Cosigna project directory")
	flags.StringVar(&binary, "engine", "cosigna", "Cosigna engine executable name or absolute path")
	flags.StringVar(&user, "user", os.Getenv("CONSOLE_USER"), "Approver identity recorded by the engine")
	flags.BoolVar(&noAnimation, "no-animation", false, "Disable animation")
	flags.BoolVar(&noColor, "no-color", false, "Disable color (also respects NO_COLOR)")
	flags.BoolVar(&watch, "watch", true, "Follow live calls, approvals and checkpoints every five seconds")
	flags.BoolVar(&printView, "print", false, "Print one static view and exit")
	flags.BoolVar(&jsonOutput, "json", false, "Print sanitized JSON instead of opening the interface")
	flags.IntVar(&width, "width", 104, "Width for --print")
	flags.IntVar(&height, "height", 38, "Height for --print")
	names := []string{"approvals", "activity", "authority", "agents", "evidence", "overview"}
	for i, name := range names {
		root.AddCommand(&cobra.Command{Use: name, Short: "Open " + name, Args: cobra.NoArgs, RunE: show(i + 1)})
	}
	for _, view := range []struct {
		name string
		page int
	}{{"session", 0}, {"status", 6}, {"checkpoints", 7}, {"rules", 8}, {"connections", 9}, {"changes", 10}} {
		root.AddCommand(&cobra.Command{Use: view.name, Short: "Open " + view.name, Args: cobra.NoArgs, RunE: show(view.page)})
	}
	animationCommand := &cobra.Command{Use: "animation", Short: "Print a computed ASCII torus frame", Args: cobra.NoArgs, RunE: func(cmd *cobra.Command, args []string) error {
		fmt.Fprintln(cmd.OutOrStdout(), animation.Frame(min(max(width, 1), 100), min(max(height, 1), 40), .9))
		return nil
	}}
	root.AddCommand(animationCommand)
	// Explicit commands are useful over SSH or without the TUI. They retain the
	// same typed confirmation and stale-request checks. There is intentionally no --yes.
	for _, reject := range []bool{false, true} {
		reject := reject
		verb := "approve"
		if reject {
			verb = "reject"
		}
		var note string
		decision := &cobra.Command{Use: verb + " ID", Short: strings.Title(verb) + " a reviewed request with typed confirmation", Args: cobra.ExactArgs(1), RunE: func(cmd *cobra.Command, args []string) error {
			if jsonOutput || printView {
				return errors.New("decision commands require an interactive terminal")
			}
			id, err := strconv.ParseUint(args[0], 10, 64)
			if err != nil || id == 0 {
				return errors.New("ID must be a positive integer")
			}
			c, err := client()
			if err != nil {
				return err
			}
			a, err := c.Approval(cmd.Context(), id)
			if err != nil {
				return err
			}
			if a.Status != "pending" {
				return errors.New("request is no longer pending")
			}
			s := c.Snapshot(cmd.Context())
			money, known := engine.ApprovalSummary(a, s.Declarations.Payments, c.Fixtures)
			if !reject && !known {
				return errors.New("payment units are unknown; approval disabled")
			}
			info, err := os.Stdin.Stat()
			if err != nil || info.Mode()&os.ModeCharDevice == 0 {
				return errors.New("a terminal is required for a decision")
			}
			action := engine.Action{Kind: "approve", ID: id, Digest: a.Digest, Reject: reject, Note: note}
			if _, err = engine.Command(action, c.User); err != nil {
				return err
			}
			// A small Bubble Tea confirmation model avoids scanner read-ahead and keeps
			// Windows input handling consistent with the full terminal interface.
			m := confirmation{client: c, action: action, description: fmt.Sprintf("Request #%d · %s · %s\nDigest: %s\n\n%s", id, money, a.Target, a.Digest, engine.SafeJSON(a.Input)), phrase: fmt.Sprintf("%s %d", verb, id)}
			result, err := tea.NewProgram(m, tea.WithContext(cmd.Context())).Run()
			if err != nil {
				return err
			}
			return result.(confirmation).err
		}}
		decision.Flags().StringVar(&note, "note", "", "Optional decision note")
		root.AddCommand(decision)
	}
	return root
}

type confirmation struct {
	client                     *engine.Client
	action                     engine.Action
	description, phrase, input string
	busy, done                 bool
	err                        error
}
type confirmationResult struct {
	receipt engine.Receipt
	err     error
}

func (m confirmation) Init() tea.Cmd { return nil }
func (m confirmation) View() string {
	if m.done {
		return engine.Clean(m.description) + "\n"
	}
	mode := "LIVE ENGINE ACTION"
	if m.client.Fixtures {
		mode = "FIXTURE SIMULATION"
	}
	if m.busy {
		return "Waiting for Cosigna...\n"
	}
	return mode + "\n\n" + engine.Clean(m.description) + "\n\nApprover: " + engine.Clean(m.client.User) + "\nNote: " + engine.Clean(m.action.Note) + "\n\nType '" + m.phrase + "' to confirm (esc cancels):\n> " + engine.Clean(m.input) + "\n"
}
func (m confirmation) Update(message tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := message.(type) {
	case confirmationResult:
		m.err = msg.err
		m.busy = false
		m.done = true
		m.description = msg.receipt.Message
		return m, tea.Quit
	case tea.KeyMsg:
		if msg.String() == "ctrl+c" || msg.String() == "esc" {
			m.err = errors.New("cancelled")
			return m, tea.Quit
		}
		if m.busy {
			return m, nil
		}
		switch msg.String() {
		case "enter":
			if m.input != m.phrase {
				m.input = ""
				return m, nil
			}
			m.busy = true
			return m, func() tea.Msg {
				r, e := m.client.Perform(context.Background(), m.action)
				return confirmationResult{r, e}
			}
		case "backspace":
			r := []rune(m.input)
			if len(r) > 0 {
				m.input = string(r[:len(r)-1])
			}
		case "ctrl+u":
			m.input = ""
		default:
			if msg.Type == tea.KeyRunes || msg.Type == tea.KeySpace {
				for _, r := range msg.Runes {
					if r >= 32 && r != 127 && len(m.input) < 256 {
						m.input += string(r)
					}
				}
			}
		}
	}
	return m, nil
}
