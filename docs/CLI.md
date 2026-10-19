# `fabric` command reference

```sh
fabric init [PATH]                 # start recording a project (default: current directory)
fabric integrate claude-code       # checkpoint every Claude Code prompt and change
fabric checkpoint [-m MSG] [--agent NAME] [--if-changed]
fabric log [-n N]                  # newest first
fabric diff [FROM] [TO]            # default: latest checkpoint -> the project now
fabric restore CHECKPOINT [--yes]  # shows the plan; applies only with --yes
fabric revert CHECKPOINT [--yes]   # undo one checkpoint's changes, keep everything after it
fabric bisect --test CMD [--good N] [--bad N]   # find where CMD started failing
fabric show CHECKPOINT             # what one checkpoint changed, with diffs
fabric history PATH [-n N]         # every recorded change to a file, and who made it
fabric resume                      # finish an interrupted restore
fabric integrate claude-code|codex # connect an agent
fabric mcp                         # MCP server (started by agents, not people)
fabric guard rules|check|set|log   # consent rules for agent actions
```

## Installing

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.ps1 | iex
```

Both download the latest release for your platform, verify its SHA-256 checksum, install
`fabric`, and tell you if it is not on your `PATH`. From source: `cargo install --path
crates/fabric-cli`. Releases are built by `.github/workflows/release.yml` when a `v*` tag is
pushed. (While the repository is private, the install scripts need it to be public, or a
download through `gh release download`.)

## Consent before destructive actions: `guard`

`fabric integrate claude-code` also installs a `PreToolUse` hook, so Claude Code asks Fabric
before every shell command and file write. Each rule decides **allow** (Fabric stays silent
and Claude Code's own permissions apply), **ask** (Claude Code pauses for your approval,
showing the reason) or **deny** (blocked; the agent is told why and not to work around it).
The strictest matching rule wins.

| Rule | Default | Covers |
|------|---------|--------|
| `system-destroy` | deny | `mkfs`, `dd of=/dev/…`, `rm -rf /` or `~`, `format C:`, deleting a whole drive |
| `fabric-tamper` | deny | an agent running `fabric restore/revert/resume/gc/integrate/guard set` |
| `git-internals` | deny | writing inside `.git/` |
| `fabric-data` | deny | writing inside Fabric's data directory |
| `recursive-delete` | ask | `rm -r`, `Remove-Item -Recurse`, `rmdir /s`, `shutil.rmtree`, `find -delete` |
| `file-delete` | ask | `rm`, `del`, `Remove-Item`, `git rm`, `os.remove` |
| `git-history-rewrite` | ask | `push --force`, `reset --hard`, `clean -f`, `checkout -- .`, `branch -D`, `rebase` |
| `git-push` | ask | `git push` |
| `publish` | ask | `npm/cargo publish`, `twine upload`, `docker push`, `gh release create` |
| `database-destructive` | ask | `DROP TABLE`, `TRUNCATE`, `DELETE FROM`, `FLUSHALL` |
| `infrastructure-destructive` | ask | `terraform destroy/apply`, `kubectl delete`, cloud `delete/terminate`, `docker rm/prune` |
| `remote-script` | ask | `curl … \| sh`, `irm … \| iex` |
| `outside-project` | ask | writing a file outside the project |
| `secrets` | ask | writing `.env*`, `*.pem`, `*.key`, SSH keys |
| `agent-config` | ask | writing `.claude/`, `.mcp.json`, `.codex/`, `.cursor/`, `.github/workflows/` |

```sh
fabric guard rules                    # every rule and its decision for this project
fabric guard check "rm -rf build"     # what would happen?  → ask (recursive-delete)
fabric guard check --file .env
fabric guard set git-push allow       # change a rule for this project
fabric guard log                      # every action that needed approval or was blocked
```

Overrides are stored in Fabric's data directory, not the project, and an agent running
`fabric guard set` is itself denied, so an agent cannot loosen its own rules. Every `ask` and
`deny` is appended to an audit log with the time, session, tool, target and rule.

Limits, stated plainly: the guard recognizes common destructive commands by pattern; it is not
a sandbox, and a determined agent could phrase a destructive action it does not recognize.
Patterns also match inside quoted text, so `grep 'rm -rf'` asks too; for consent, a false
alarm is the safer mistake. If the hook itself fails it stays silent rather than blocking the
agent, and Claude Code's own permission system still applies. Only agents with pre-action
hooks (Claude Code today) can be guarded; Codex and others are covered by checkpoints and
undo, but not by consent prompts.

## Sharing history between agents: MCP

`fabric mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio.
Any agent connected to it can ask what other agents did in the project:

| Tool | What it answers |
|------|-----------------|
| `fabric_timeline` | Recent checkpoints across all agents, with test results |
| `fabric_file_history` | Every change to one file: which agent, which step, did tests pass |
| `fabric_step` | Exactly what one checkpoint changed, with unified diffs |
| `fabric_test_results` | Recorded test runs and the checkpoint each ran against |
| `fabric_checkpoint` | Record the current state with a description (agents without hooks, like Codex, use this) |

Agents can read history and add checkpoints, but there is no tool to restore, revert or delete:
destructive operations stay with people. The server finds the project from its working
directory (or a `project` argument), opens the store only for the duration of each call, and
tells connecting agents in its instructions to check a file's history before changing it.

- `fabric integrate claude-code` registers it for all your projects with
  `claude mcp add --scope user`, alongside the hooks. If the `claude` command is not found it
  prints the command to run instead.
- `fabric integrate codex` adds `[mcp_servers.fabric]` to `$CODEX_HOME/config.toml` (default
  `~/.codex/config.toml`), keeping the rest of the file exactly as it was.

## Undoing one step: `revert`

`fabric revert 4` undoes what checkpoint #4 changed relative to #3, and keeps every change
recorded after it. It works path by path: a path #4 changed is put back only if it is still
exactly as #4 left it. If a later step changed the same path, the revert stops with a
conflict (exit code 4) and changes nothing — Fabric never merges file contents or guesses.
Like `restore`, it shows the plan first, needs `--yes`, records the current state so it can be
undone, and verifies the result.

## Finding the breaking step: `bisect`

`fabric bisect --test "pytest -x"` binary-searches the checkpoints between `--good` (default:
the oldest) and `--bad` (default: the project as it is now) for the first one where the test
command fails (non-zero exit). Checkpoints with identical state are tested once.

- Each candidate is restored **in place**, so ignored files such as `node_modules` or a
  virtualenv stay available to the tests. The current state is checkpointed first ("before
  bisect") and the project is always returned to it, even when bisecting fails.
- The command runs through the platform shell (`sh -c` or `cmd /C`) in the project root.
  Its output goes to `last-test-output.log` in the project's Fabric data directory, and every
  result is appended to `test-results.jsonl` there.
- It refuses to search if the tests already pass at `--bad` or already fail at `--good`.
- The output names the first failing checkpoint, its agent and its changes, and suggests
  `fabric revert N --yes` to undo just that step.

The test command is yours and runs with your permissions; bisect runs it once per tested
state.

`CHECKPOINT` is a number from `fabric log` (`12` or `#12`), `latest`, or a snapshot id prefix
of at least 6 hex characters. `TO` may also be `current`.

All commands work from any directory inside a recorded project.

## Data location

Everything is stored outside your projects, in `FABRIC_HOME` (default: the platform's local
data directory + `fabric`, e.g. `%LOCALAPPDATA%\fabric` on Windows,
`~/.local/share/fabric` on Linux, `~/Library/Application Support/fabric` on macOS). A project
is identified by its absolute path; moving a project starts a new history.

## Claude Code integration

`fabric integrate claude-code` adds two hooks to `.claude/settings.local.json` (per-user, not
committed), leaving any existing settings and hooks untouched:

- **`UserPromptSubmit`** — a `prompt` checkpoint before Claude acts, labelled with the first
  line of the prompt. `fabric restore <that checkpoint>` undoes everything Claude did for it.
- **`PostToolUse`** on `Write|Edit|MultiEdit|NotebookEdit|Bash` — an `agent_step` checkpoint
  after each tool call that changed the project (unchanged states are not recorded).

The hook prints nothing, always exits 0 and logs its own failures to
`FABRIC_HOME/hook-errors.log`, so it can never block or confuse the agent. Prompt text is
stored only in the local checkpoint log.

Each hook call currently re-reads the whole project. That is fine for small and medium
projects; a stat cache to skip unchanged files is the next performance task.

## Machine-readable output

Pass `--json` to any command. Output is one JSON object with a `schema` field naming its
versioned format: `fabric.init/v1`, `fabric.checkpoint/v1`, `fabric.log/v1`,
`fabric.diff/v1`, `fabric.restore/v1`, `fabric.revert/v1`, `fabric.bisect/v1`,
`fabric.show/v1`, `fabric.history/v1`, `fabric.resume/v1`, `fabric.integrate/v1`, and on
failure `fabric.error/v1`:

```json
{ "schema": "fabric.error/v1", "error": { "kind": "not_a_project", "message": "…" } }
```

`fabric restore --json` and `fabric revert --json` report `"status"`:
`requires_confirmation`, `conflicts`, `already_at_target` or `applied`, with the full plan;
once applied, also `report`, `undo_checkpoint` and the `recorded` checkpoint:

```json
{
  "schema": "fabric.restore/v1",
  "status": "requires_confirmation",
  "target": "#1",
  "plan": {
    "summary": { "create": 0, "modify": 1, "delete": 1 },
    "destructive": true,
    "conflicts": [],
    "operations": [ { "op": "remove", "path": "src/helper.py" }, … ]
  }
}
```

Fields may be added within a schema version; none are removed or change meaning.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error (see the message or `error.kind`) |
| 2 | Invalid command-line usage |
| 3 | Restore or revert needs confirmation: rerun with `--yes` |
| 4 | Restore or revert blocked by conflicts; nothing was changed |
| 5 | Integrity failure: corrupt or missing stored data, or a restore that did not verify |
| 6 | State changed: the project changed since planning, or an interrupted restore is pending |

## Checkpoint kinds

`init`, `manual`, `prompt`, `agent_step`, `pre_restore` (the undo point recorded before a
restore or revert, when the project had unrecorded changes), `restore`, `revert`.
