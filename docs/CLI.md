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
fabric resume                      # finish an interrupted restore
```

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
`fabric.resume/v1`, `fabric.integrate/v1`, and on failure `fabric.error/v1`:

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
