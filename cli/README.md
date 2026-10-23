# Cosigna terminal

**Status: under development.** Active product work now prioritizes the landing page,
dashboard, payment integrations and Cosigna-managed agents. The CLI remains available for
development and testing; it is not a production release. See [the web plan](../docs/WEB_PRODUCT_PLAN.md).

A developer command session for the existing Rust `cosigna` engine (`fabric` remains a
compatible alias). It opens with project context, a compact ASCII identity and a small torus.
A persistent command prompt, completion menu and session history drive the interface.
Checkpoints, rules, coding-agent connections and activity are primary; finance approvals,
mandates and evidence remain available through the same command menu.

The interaction references are [Claude Code's interactive mode](https://code.claude.com/docs/en/interactive-mode),
[Codex's command interface](https://developers.openai.com/codex/cli/slash-commands) and
[Cursor's slash commands](https://cursor.com/docs/cli/reference/slash-commands).
The original ASCII direction comes from [Daytona's launcher](https://dribbble.com/shots/24164275-Daytona-CLI-launcher).
The Go torus renderer implements projection, lighting and a depth buffer from
[Andy Sloane's mathematical explanation](https://www.a1k0n.net/2011/07/20/donut-math.html).

## Build and try

Install **Go 1.24 or newer** from [go.dev](https://go.dev/doc/install), then run in PowerShell:

```powershell
cd C:\Users\Harsha\Pictures\project\cli
powershell -ExecutionPolicy Bypass -File .\build.ps1
.\bin\cosigna-console.exe --fixtures
```

The script fetches the pinned Go dependencies, formats the source, runs `go vet` and tests,
then builds `bin\cosigna-console.exe`. Build/module caches stay under `cli/.cache`. The resulting
executable embeds the sanitized fixtures and does not require Node or the web Console.

Go formatting, vet, tests and the Windows build now run successfully in this environment.
The compiled terminal was exercised in a Windows pseudoterminal. Source tests also verify
small-terminal layouts, input editing, confirmation and cancellation. Visual review in your
preferred terminal/font is still useful.

## Use it

The default screen is a developer session. Type `/` or press Ctrl+K from any view to
open the command menu. Up/down chooses a suggestion, Tab completes it, Enter opens it.
The prompt supports left/right, Home/End, Backspace/Delete, Ctrl+A/E/U, and in-memory command
recall with up/down. PgUp/PgDn scroll session output. Esc cancels input or returns to the
session. Ctrl+C exits; Q also exits when browsing a view.

| Command | Behavior |
| --- | --- |
| `/status` | Project path, checkpoint/rule counts and unavailable data sources. |
| `/activity` | Recent tool calls; Enter opens input, decision, outcome and result. |
| `/checkpoints` | Recorded snapshots, agents and change counts. |
| `/checkpoint message` | Review and confirm a new checkpoint; captures only when changed. |
| `/diff` | Explicit comparison of the latest checkpoint and current working tree. |
| `/changes` | Authorized changes and their recorded state; read-only here. |
| `/rules` | Configured rules and decisions; read-only. |
| `/connect` | Setup guide for Claude Code, Codex and Cursor. |
| `/connect claude-code` | Confirm project hooks and an MCP registration attempt. |
| `/connect codex` | Confirm MCP configuration, shared across Codex projects. |
| `/connect cursor` | Show manual MCP configuration; changes no files. |
| `/init` | Confirm project registration and its initial checkpoint. |
| `/approvals` | Tab switches pending/all. Enter reviews input; A/R starts a decision. |
| `/approve ID`, `/reject ID` | Open the full request first; retain the separate A/R and confirmation steps. |
| `/agents`, `/authority` | Inspect identities/mandates; R revokes from their detail view. |
| `/evidence` | Verification details; E exports after confirmation from the list. |
| `/refresh` | Full reload, including evidence verification and authority. |
| `/animation on\|off` | Set welcome motion for this session. |
| `/clear` | Clear session messages and command history. |
| `/home`, `/help`, `/quit` | Session, command reference, exit. |

Live mode follows calls, approvals and checkpoint history every five seconds. It avoids
background verification and defers updates while reviewing or confirming. Disable following
with `--watch=false`. Errors show unavailable counts rather than treating them as zero.
A configured rule is not evidence that an agent is using hooks or the gateway.

Details and receipts scroll with up/down. The torus runs at 24 ticks per second while the
welcome screen is idle, pauses while typing/reviewing, and leaves the transcript clear once
commands produce output. Small layouts omit it. `--no-animation` disables animation ticks;
`--no-color` and `NO_COLOR` disable color. The terminal background is preserved; use a dark
terminal profile for the reference appearance. Confirmations require at least 60?24 so their
scope and confirmation input are readable.

Fixture decisions and revocations update only this process's in-memory dataset. Quitting
resets them. Fixture export creates no pack. No engine commands run in fixture mode.

```powershell
# Open the inbox directly
.\bin\cosigna-console.exe --fixtures approvals

# Print one noninteractive frame or sanitized JSON
.\bin\cosigna-console.exe --fixtures --print
.\bin\cosigna-console.exe --fixtures approvals --json
.\bin\cosigna-console.exe animation --width 60 --height 20

# Review a particular request with a typed confirmation
.\bin\cosigna-console.exe --fixtures approve 1 --note "Reviewed the cancellation"
.\bin\cosigna-console.exe --fixtures reject 1 --note "Needs support review"

# Connect to the real engine, from your own terminal
.\bin\cosigna-console.exe --project C:\path\to\cosigna-project --user controller@example.com
# If Cosigna is not on PATH, also pass --engine C:\path\to\cosigna.exe
```

Live decisions call `cosigna approve ID --yes --as USER ... --json` only after confirmation
and a fresh pending-status/digest check. Approval authorizes a retry of the tool call; the
terminal does not execute refunds. Cosigna enforces expiry, signatures and its own checks.
There remains a race between the client's refresh and Cosigna's decision command; Cosigna's
own request validation is authoritative. The frontend passes the ID, not a new digest flag.

MCP payment amounts require a matching declaration from `cosigna payments list`. Unknown
units/currencies disable payment approval and display raw input; rejection remains available. No
amount is converted with floating-point arithmetic. Mandate usage has no recorded currency
and is shown as engine units, not dollars. Live consent API integration, agent creation and
mandate grant forms remain outside this first inbox-focused increment; use the existing
engine or web Console for those actions.

## Implementation

```text
cmd/cosigna-console   Cobra entry point and explicit decision commands
internal/tui         Command session, completion, history, views and confirmation
internal/animation   Go torus renderer, perspective, shading, depth buffer
internal/engine      Versioned JSON contracts, fixture state, exec.CommandContext
fixtures             Northwind captures plus explicitly synthetic workspace samples
```

Commands use argument arrays with no shell, inherit agent-session markers, and bound
engine response size/time. Engine-derived terminal text removes escape sequences and
redacts recognizable credentials. Credentials are not saved or included in logs. There is
no telemetry or runtime cloud dependency in this frontend.

Tests cover computed animation frames, fixture isolation, stale/duplicate decisions,
typed confirmation, cancellation, exact amounts, argument construction, redaction and
resize handling. Run them with `go test ./...` or `build.ps1`. Open contract/design questions
are recorded in [QUESTIONS.md](QUESTIONS.md) and [the Console questions](../console/QUESTIONS.md).

Local file-edit and shell requests use the engine's exact built-in tool identities and show
their complete input instead of pretending they carry currency. Unknown MCP requests still
need declared payment units. Decisions always recheck status/digest and engine receipts.

Live mode uses `--user` / `CONSOLE_USER`, falling back to the local OS account name. The
confirmation displays the identity. Setup, init, checkpoints and revocations all require
typed confirmation. Claude Code and Codex integration have different recording guarantees;
the connection guide describes them without claiming a connection from configuration alone.
Cursor setup supplies MCP tools, not automatic tool interception. No real integration was
changed during development testing.

Session input/history stays in memory (up to 100 commands and 50 messages); `/clear` and exit
remove it. This is a deterministic engine companion, with no language-model provider or
arbitrary shell execution behind the prompt. Natural-language agent orchestration and
additional destructive operations are recorded as product questions, not simulated features.
