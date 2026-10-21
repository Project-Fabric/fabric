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
fabric guard pack finance [--scope GLOB]...      # SOX change-control rules
fabric guard pack finance-ops      # rules for agents operating the books through tools
fabric guard add-rule ID --decision D [--path GLOB]... [--command REGEX]... [--tool GLOB]... [--committed-only]
fabric mcp-proxy --name NAME [--secret-env VAR=NAME]... -- <server command>
fabric agent add NAME [--tool GLOB]... | revoke NAME | list | require on|off
fabric secret set NAME < file | remove NAME | list
fabric connect agentauth --url URL --api-key-secret NAME
fabric payments add SERVER/TOOL --amount FIELD [--minor-units] [--currency FIELD]
                    [--default-currency CUR] [--merchant FIELD] [--when FIELD=VALUE]... | list
fabric mandate grant TITLE --tool GLOB... [--agent A] [--max-per-call N] [--max-per-day N]
                     [--max-calls-per-day N] --expires YYYY-MM-DD [--out FILE]
fabric mandate grant --signed FILE | revoke N | list
fabric guard remove-rule ID
fabric approvals [--all]           # changes waiting for a person's approval
fabric approve N [N...] [--reject] [--as NAME] [--note TEXT] [--yes]
fabric approve N [N...] --change TITLE [--path GLOB]...   # also authorize the whole task
fabric changes [--all]             # authorized changes waiting for review
fabric review N [--reject] [--as NAME] [--note TEXT] [--yes]
fabric approvals export N [N...] [--out FILE]      # for an approver elsewhere
fabric review N --export FILE                      # likewise, a change's result
fabric approver key|list|add NAME --key KEY|remove NAME|require on|off
fabric approver sign FILE [--reject] [--change TITLE] [--path GLOB]... [--note T] [--yes]
fabric approve --signed FILE       # apply a signed approval
fabric review N --signed FILE      # apply a signed review
fabric revert --change N [--yes]   # undo a whole change, keeping other work
fabric evidence verify [--deep]    # is the record intact? any unapproved controlled changes?
fabric evidence report N           # a reviewer's page for one checkpoint
fabric evidence report --change N  # an auditor's page for one authorized change
fabric evidence export --out DIR   # signed evidence pack
fabric evidence verify-pack DIR [--key FINGERPRINT]
fabric evidence key                # this installation's signing key fingerprint
fabric evidence calls [-n N]       # tool calls agents made, with decisions and results
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

### Project rules, approvals and the finance pack

Projects can add their own rules over file paths (globs) or commands (patterns), with a fourth
decision, `approve`: the action is held until a person approves that exact action with
`fabric approve N` in their own terminal. The agent is told the request number; when it
retries the same action, the approval is used up and the action proceeds. A different action
needs its own approval, rejections are final for that action, and unused approvals expire
after 4 hours.

```sh
fabric guard pack finance --scope "src/billing/**" --scope "migrations/**"
fabric approvals                      # #1 pending  Edit src/billing/tax.py (finance-in-scope-code)
fabric approve 1 3 4                  # shows each proposed diff, asks once, records each
fabric approve 2 --reject --note "post a reversing entry instead"
```

**Changes: authorize once, review once.** Approving every held edit separately is how one
feature ends up with half a dozen approvals. For a task that needs many edits, approve the
first held request together with the whole task:

```sh
fabric approve 1 --change "CHG-1042: late fees" --path "src/billing/**"
# ... the agent finishes the task; its further edits under src/billing/ proceed, recorded ...
fabric changes                        # #1 open  CHG-1042: late fees  [src/billing/**]
fabric review 1                       # the task's full diff; accept or --reject
```

While the change is open, file edits by *that agent session* to the change's paths proceed
without separate approvals; each is recorded in the evidence (`change_edit`), and shell
commands that change those files are not rolled back. Without `--path`, the change covers
exactly the requested files. A change starts where the task did: at the session's latest
prompt before it was authorized (the state before the agent acted on that prompt), so edits
the agent made before its first held change belong to it. `fabric review` shows the agent
steps and the complete diff of the change's paths since it started, then records the
decision bound to exactly that state: the reviewed state becomes a checkpoint and each file's content id goes into the
evidence; if the project changes while the reviewer reads, nothing is recorded. Review closes
the change; later edits need a new approval. A rejected change stays in the project until it
is reverted: `fabric revert --change N` puts every file the change's session touched between
its start and its review (or now, while it is open) back as it was when the change started,
including files outside the change's paths such as the agent's tests, and keeps all other
work. A file changed again after the change is a conflict and nothing is changed, as with
`fabric revert` of one step. The result is recorded as a checkpoint labelled
`reverted change #N: <title>`, and the change's report shows it.

A change never covers shell commands a rule holds (such as `finance-ledger-sql`),
other sessions, or files under a `deny` rule. Agents are denied `fabric review`, and it
refuses to run inside an agent session, as `fabric approve` does.

The finance pack adds `finance-in-scope-code` (approve; the given scope),
`finance-applied-migrations` (deny; committed files under `migrations/`), `finance-ledger-sql`
(approve; SQL that writes to journal, ledger, invoice, payment, refund, account, period,
revenue or balance tables), `finance-controls` (approve; `controls/`, `CODEOWNERS`) and
`finance-production-config` (deny). Change any of them with `fabric guard set`.

**Changes made around the hold are rolled back.** A shell command can change a file without
naming it (`python fix.py`, `sed -i`), so the guard cannot hold it in advance. After every agent
step, Fabric checks the files it changed against `approve` and `deny` project rules and the
approvals used in that step; any controlled file changed without one is put back as it was
before the step (a file the step created is removed), the rollback is recorded as a
checkpoint and in the evidence, and the agent is told to make the change with Edit or Write
so it can be approved. Nothing is lost: the step's checkpoint holds what the agent wrote, and
`fabric revert` of the rollback brings it back. A person's own edit to a controlled file made
during the same agent step is rolled back too, and recoverable the same way.

Segregation of duties, and its limits: an agent running `fabric approve` is denied by the
guard, and the command refuses to run where a known agent marks its shell (`CLAUDECODE`,
`CODEX_SANDBOX`). These stop an agent approving its own change by mistake, not one set on
evading them, and the developer who prompted the agent can approve its work. The evidence
records the approver, their OS account, and whether it is the same account the agent ran
under. For approval by another person, see below.

### Sign-off by another person: signed approvals

A local `fabric approve` shows only that someone at the developer's machine approved. For
segregation of duties, approvers sign decisions with their own key, on their own machine,
without a server:

```sh
# The approver, once, on their machine: print their public key.
fabric approver key
# The project owner registers it, and can require signed approvals.
fabric approver add "Dana (controller)" --key 3f2a…
fabric approver require on

# Developer: export what needs a decision; send the file to the approver.
fabric approvals export 1 --out req-1.json
# Approver: review and sign (works without the project); send the result back.
fabric approver sign req-1.json --change "CHG-1042: late fees" --path "src/billing/**"
# Developer: apply it. The signature, the key and the exact actions are checked.
fabric approve --signed req-1.signed.json

# The same for the change's result.
fabric review 1 --export review-1.json
fabric approver sign review-1.json
fabric review 1 --signed review-1.signed.json
```

The approver's Fabric recomputes each action's digest from the exact input before showing it
and refuses a request edited after export, so what is signed is what was shown. Applying a
decision checks the Ed25519 signature, that the key is registered for this project, that the
decision was made for this project, and that the requests (by digest) or the change's files
(by content id) are exactly the ones signed for; anything else is refused and nothing is
recorded. The decision supplies the requests, the outcome, the change and the note.

With `fabric approver require on`, unsigned `fabric approve` and `fabric review` are refused,
and so are decisions signed with this installation's own approver key: the approver must hold
a key from another Fabric installation. Registering and removing approvers and changing the
requirement are recorded in the evidence, need a person's terminal, and are denied to agents.

Each signed decision is kept in the evidence with its payload, signature and key.
`fabric evidence verify` and `fabric evidence verify-pack` re-check them offline: the
signature, that the key was a registered approver when it was used, and that the decision is
about the recorded event. Reports name the approver and key fingerprint.

What a signature shows, plainly: the holder of a registered key made this decision about
exactly this content. It does not show who holds the key; that depends on how keys are handed
out, and who registered each key is in the evidence. Someone who can write to Fabric's data
directory on the developer's machine can register a key of their own.

Limits, stated plainly: the guard recognizes common destructive commands by pattern; it is not
a sandbox, and a determined agent could phrase a destructive action it does not recognize.
Patterns also match inside quoted text, so `grep 'rm -rf'` asks too; for consent, a false
alarm is the safer mistake. If the hook itself fails it stays silent rather than blocking the
agent, and Claude Code's own permission system still applies. Only agents with pre-action
hooks (Claude Code today) can be guarded; Codex and others are covered by checkpoints and
undo, but not by consent prompts.

## Evidence for reviewers and auditors

Every checkpoint, guard decision, approval and rule change is appended to a hash-chained
evidence log (format in `STORAGE_FORMAT.md`).

- `fabric evidence verify` checks every hash and link, that recorded snapshots are present
  (`--deep` re-hashes their content), and that the checkpoint log matches the evidence. It
  also lists **exceptions**: files under an `approve` or `deny` project rule that an agent step
  changed without an approval or an open change in that step and that were not rolled back (for example a
  directory or symlink, or changes recorded before rollback existed). Exit code 5
  if the chain is broken.
- `fabric evidence report N` shows one checkpoint as a reviewer reads it: agent, prompt,
  guard decisions, approvals and change authorizations, exceptions, the diff, and how to
  reverse it.
- `fabric evidence report --change N` shows one authorized change as an auditor samples a
  change ticket: who authorized it and when, with which note; the prompt and every step; the
  controls in effect; exceptions; who reviewed it and the outcome, flagging when the same
  person authorized and reviewed; a check that the content recorded at review matches the
  reviewed checkpoint's snapshot; the complete diff; and how to reverse it. Built from the
  evidence chain and snapshots alone. Exported packs include one per change in `tickets/`.
- `fabric evidence export --out DIR` writes a signed pack; `fabric evidence verify-pack DIR
  --key FINGERPRINT` checks it anywhere, without the project or Fabric's data directory.

The guard's JSON schemas are now `fabric.guard.rules/v2`, `fabric.guard.check/v2`,
`fabric.guard.set/v2` and `fabric.guard.log/v2` (v2: the `approve` decision, project rules,
and `rule` may name a project rule).

## Agents that act through tools: `mcp-proxy` and the `finance-ops` pack

Agents increasingly change systems through tools rather than files: a finance-operations
agent calls `post_journal_entry` or `record_payment` on an ERP's MCP server. Project rules can
match tool calls as `server/tool` (`--tool GLOB`), and the `finance-ops` pack holds calls
that post to the books or move money (`*/post_*`, `*/record_*`, `*/pay_*`, `*/refund*`,
`*/void_*`, `*/reverse_*`, `*/write_off*`, `*/create_*`, `*/update_*` and similar) for a
recorded approval, denies calls that delete or erase records (`*/delete_*`, `*/drop_*`,
`*/purge_*`, …), and leaves reads alone. Matching is by tool name only, not by arguments.

There are two ways the rules reach a tool call:

- **Claude Code**: its pre-action hook sees MCP calls (`mcp__<server>__<tool>`) and applies
  the rules like any other action.
- **Any MCP client** (Codex, Cursor, an in-house agent): point the client at
  `fabric mcp-proxy --name <server> -- <the server's command>` instead of the server. The proxy
  runs in the project, forwards everything, and for each `tools/call` applies the rules:
  allowed calls go through; held calls get an error result naming the approval request
  (`fabric approve N`), and the same call made again after approval goes through once;
  denied calls are refused. Tools under a rule are marked in `tools/list`. If the project's
  rules cannot be read, calls are not forwarded.

Every call through the proxy is recorded in the evidence: the agent (from the client's
`initialize`), the tool, its arguments, the decision and rule, and the outcome with the
server's result (`ok`, `error`, `held`, `blocked`). `fabric evidence calls` lists them. With
Claude Code and the proxy together, one approval covers one call at both checkpoints: the
proxy accepts an approval the hook used for the same action within the previous two minutes,
once. Give the server the same name in both places so the actions match.

### The gateway holds the keys and knows the agent

**Credentials.** The agent should never hold the keys to the systems it acts on.
`fabric secret set stripe-test < key.txt` stores a credential (read from standard input, never
from the command line) in this installation's data directory, readable only by the user's
account; `fabric mcp-proxy --secret-env STRIPE_SECRET_KEY=stripe-test -- <server>` gives it to
that server's environment only. Values are never printed, logged or recorded, and no command
reads one back. Servers echo credentials by accident (an error such as `Invalid API key:
sk_...`), so every injected value of 8 characters or more is replaced by `[redacted credential
NAME]` in whatever the server sends back, before the agent or the evidence sees it. A missing
credential stops the proxy from starting. Agents are denied `fabric secret`, and any read,
search or shell command that names the credential store.

**Agent identity.** An MCP client says who it is, and anything can say anything. `fabric agent
add close-bot --tool "stripe/refunds_*"` registers an agent and prints its token once (only a
BLAKE3 hash is kept); put it in the agent's gateway configuration as `FABRIC_AGENT_TOKEN`. The
proxy authenticates the token before starting (a bad or revoked token stops it), refuses calls
outside the agent's `--tool` scopes, never passes the token to the upstream server, and records
the authenticated name in the evidence (`agent_authenticated: true`); otherwise the client's
own name is recorded as unauthenticated. A mandate naming an agent covers only calls
authenticated as it, and only registered agents can be named. With `fabric agent require on`,
calls without a valid token are refused.

### Payments authorized by AgentAuth

The gateway and the AgentAuth API are one product: AgentAuth holds the consents people grant
(spending limits, merchants, expiry), and the gateway sits in front of the tools that move
money. Before a declared payment tool runs, the gateway asks AgentAuth's `/v1/authorize`
with the consent's delegation token.

```sh
fabric secret set agentauth-api < api-key.txt
fabric secret set consent-acme < delegation-token.txt
fabric connect agentauth --url https://api.agentauth.in --api-key-secret agentauth-api
fabric payments add stripe/create_refund --amount amount --minor-units --currency currency
fabric mcp-proxy --name stripe --consent consent-acme \
    --secret-env STRIPE_SECRET_KEY=stripe-test -- <stripe MCP server>
```

- **ALLOW**: the call goes through; the authorization code and consent are recorded.
- **DENY** (for example `budget_exhausted`): refused with AgentAuth's reason; the payment
  server never sees it.
- **STEP_UP**: held for a person's recorded approval (`fabric approve N`).
- **No answer** (unreachable, an error, a malformed reply), no consent configured, or an
  amount that cannot be read: refused. The gateway fails closed.

**Generic tools.** Some servers expose one tool for every operation: Stripe's MCP server has
`stripe_api_write`, whose `stripe_api_operation_id` argument says what it does (`PostRefunds`,
`PostPayouts`, `DeleteCustomersCustomer`, ...). A declaration can require argument values with
`--when` (all must hold), so only the money-moving operations are payments:

```sh
fabric payments add stripe/stripe_api_write --when stripe_api_operation_id=PostRefunds     --amount parameters.amount --minor-units --currency parameters.currency --default-currency usd
fabric guard add-rule stripe-writes --decision approve --tool "stripe/stripe_api_write"
```

The rule makes every other write wait for a person, so an operation nobody classified never
goes through unexamined; declared payments are decided by AgentAuth instead. A Stripe refund
without an `amount` (a full refund) cannot be authorized from its arguments and is refused.

Local `deny` rules are applied first and still win. `--minor-units` converts integer
smallest-unit amounts (as Stripe sends them) to exact decimals by the currency's ISO 4217
exponent (0 for JPY, 3 for KWD, 2 otherwise). The gateway holds the delegation token, so the
agent cannot spend the consent anywhere else. The URL must be https, except for localhost.
Every decision is recorded as a `payment_authorization` event. Only calls through the proxy
are authorized this way; the Claude Code hook does not call AgentAuth.

### Mandates: bounded authority, granted ahead of time

Holding every money movement for a person is safe but slow. A mandate says what an agent may
do on its own, within limits:

```sh
fabric mandate grant "Small refunds" --tool "*/issue_refund" \
    --max-per-call 500 --max-per-day 2000 --max-calls-per-day 20 \
    --agent close-bot --expires 2026-11-30
```

A tool call held by an `approve` rule goes through without waiting when an active mandate
covers it: the tool matches, the agent matches (if one is named), the call's amount is within
the per-call limit, and the day's total and number of calls stay within theirs. Anything else
is held for a person as before, and `deny` rules are never covered. An exact approval waiting
for the call is used before any mandate.

- **Amount of a call:** the largest `amount` argument, or the sum of `debit` arguments (a
  journal entry's size), whichever is larger, found anywhere in its arguments; decimal strings
  or numbers. A mandate with an amount limit does not cover a call without a readable amount.
  Days are UTC. A mandate ends at the start of its `--expires` day.
- **Granting** needs a person's terminal (agents are denied `fabric mandate`). When the project
  requires signed approvals, a mandate must be signed by a registered approver from another
  installation: `fabric mandate grant ... --out mandate.json`, the approver runs `fabric
  approver sign mandate.json`, then `fabric mandate grant --signed mandate.signed.json`.
- **Revoking** (`fabric mandate revoke N`) takes effect on the next call.
- **Evidence:** grants (with the terms and any signature), every use (tool, agent, amount, the
  day's running total and count) and revocations are recorded. `fabric evidence verify` and
  `verify-pack` replay every use against the mandate's recorded terms and report any use
  outside them: before the grant, after revocation or expiry, another tool or agent, or over a
  limit.
- A call passing both the Claude Code hook and the proxy counts once.

Mandates cover tool calls only, not file edits or shell commands. Limits are per mandate, not
per agent across mandates: two mandates covering the same tool each allow their own limits.

What the proxy cannot do: undo a call. A posting is reversed by the system's own means (a
reversing entry); Fabric records what was called and what came back, so the reversal can be
targeted and checked.

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

`fabric revert --change N` does the same for a whole authorized change at once (see
"Changes: authorize once, review once"). A change spread over several steps often cannot be
undone one step at a time, because its later steps change the same files again.

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
`fabric.show/v1`, `fabric.history/v1`, `fabric.resume/v1`, `fabric.integrate/v1`,
`fabric.approvals/v1`, `fabric.approve/v2` (with `change` when `--change` opened one),
`fabric.changes/v1`, `fabric.review/v1` (the change and the reviewed files' content ids),
`fabric.evidence.change-report/v1`, `fabric.approvals.export/v1`, `fabric.review.export/v1`,
`fabric.approver.key/v1`, `fabric.approver.add/v1`, `fabric.approver.remove/v1`,
`fabric.approver.list/v1`, `fabric.approver.require/v1`, `fabric.approver.sign/v1`,
`fabric.mandate.grant/v1`, `fabric.mandate.proposal/v1`, `fabric.mandate.revoke/v1`,
`fabric.mandate.list/v1`, `fabric.evidence.calls/v1`, `fabric.agent.add/v1` (includes the
token, once), `fabric.agent.revoke/v1`, `fabric.agent.list/v1`, `fabric.agent.require/v1`,
`fabric.secret.set/v1`, `fabric.secret.remove/v1`, `fabric.secret.list/v1` (names only),
`fabric.connect/v1`, `fabric.payments.add/v1`, `fabric.payments.list/v1`
(`fabric.evidence.verify/v1` adds `signed_decisions`, `fabric.evidence.verify_pack/v1` adds
its count), and
on failure `fabric.error/v1`:

```json
{ "schema": "fabric.error/v1", "error": { "kind": "not_a_project", "message": "…" } }
```

`fabric restore --json` and `fabric revert --json` report `"status"`
(`fabric revert --change` adds `"change"`, with `"checkpoint": null`):
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
