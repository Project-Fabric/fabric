# Cosigna Console: build brief (v1)

> **Priority reset, 23 October 2026:** Cosigna is two products: the transaction gateway and
> the voice task assistant. Gateway hardening comes first. Pause additional UI/voice expansion;
> preserve existing screens and their contracts. See [the current rebuild brief](../docs/COSIGNA_REBUILD.md)
> for the agreed boundaries and acceptance gates. The dashboard will expose gateway state;
> it must not implement a separate payment-authority path.

> **Personal assistant preview, 22 October 2026:** `/tasks` extends the interface with
> synthetic shopping and flight scenarios, following the user's broader assistant vision.
> This is a local UI prototype only: page-memory state, explicit demo approvals and simulated
> outcomes. It must not invoke engine mutations or claim live search, identity verification,
> provider access or booking. Preserve the finance screens and their existing contracts.
> See [the assistant research proposal](../docs/COSIGNA_VOICE_COMMERCE_RESEARCH.md).

> **Web priority update, 21 October 2026:** the CLI is under development. Current work focuses
> on the landing page, dashboard, integrations and Cosigna-managed agents. Add an integrations
> directory and managed-agent configuration drafts without presenting them as live connections
> or running agents. Existing session, CSRF and engine-boundary requirements below still apply.
> See [the current web plan](../docs/WEB_PRODUCT_PLAN.md).

**For:** the coding agent building the console (Codex). **Owner of the engine and data contract:**
the Cosigna engine in this repository (`crates/fabric-cli`; its command is still `fabric`).

**Naming.** The product is **Cosigna** (formerly AgentAuth): use "Cosigna" in every user-facing
string, page title and component name. Some code and commands still say `fabric` or
`agentauth` (for example `fabric connect agentauth`, `AGENTAUTH_URL`); keep calling them by
their current names, since they are being renamed separately, but never show those names in
the UI. **Date:** 21 October 2026.

Read this whole file, then `../docs/CLI.md` (sections: payments authorized by AgentAuth,
mandates, the gateway, signed approvals, evidence) and `../docs/EXPERIMENT_NORTHWIND.md` before
writing code. Build only what is described here; ask (write a question in `console/QUESTIONS.md`)
rather than guess when something is unclear.

## 1. What the console is

Cosigna controls the AI agents that move a company's money. The engine already enforces
everything from the command line: agents act within consents and mandates, exceptions wait for
a person, every decision is recorded in a tamper-evident evidence chain. **The console is the
screen for the people who decide and the people who check.** It does not enforce anything
itself: every action it takes is a call to the engine, so the console can never be the reason
money moves.

Users, in order of importance:

1. **Controller / approver** (a finance person, often on a phone): sees what is waiting, decides
   in seconds, with enough context to be right.
2. **Finance lead**: grants and revokes spending authority (consents, mandates) and agents.
3. **Auditor**: verifies the record and sees why every payment happened or did not.
4. **Engineer setting it up**: payment tools, rules, agent identities.

## 2. Screens (v1)

Build in this order; each must work end to end before the next.

### 2.1 Approval inbox (`/approvals`) — the most important screen
- List of **pending** requests first, newest first; tabs for approved / rejected / all.
- Each row: amount and currency (formatted, e.g. `$450.00`), what it is (tool and operation,
  e.g. "Refund"), counterparty (`input.payment_intent` or `merchant`), agent, time waiting,
  rule that held it (`rule`, e.g. `finance-ops-postings`).
- Detail view (`/approvals/[id]`): everything above plus the agent's stated reason
  (`input.reason`), the full tool input as formatted JSON, the session, who requested and when.
- **Approve / Reject** with an optional note. A confirmation step shows exactly what will be
  recorded ("Approve refund of $450.00 to pi_… as priya@… "). Never a one-tap approve.
- Must be usable on a 375 px wide phone; the decision must be understandable in ten seconds.
- Data: `fabric approvals --all --json` (fixture `inbox_approvals.json`). Action: `fabric approve
  <id> --yes --as <console user> [--note TEXT]` or with `--reject`.

### 2.2 Activity (`/activity`) — what agents did with money
- Timeline of tool calls: time, agent, tool, decision (`allow` / `approve` / `deny`), outcome
  (`ok`, `error`, `held`, `blocked`), and for payments the Cosigna decision
  (`payment.decision`: `ALLOW`, `DENY`, `STEP_UP`, `UNAVAILABLE`), amount, authorization code
  or reason (`payment.reason`, e.g. `budget_exhausted: The consent has 30.0 USD left of 50.0`).
- Filters: agent, outcome, payments only. Blocked payments must stand out.
- Data: `fabric evidence calls -n 200 --json` (fixture `evidence_calls.json`).

### 2.3 Authority (`/authority`) — consents and mandates
- **Consents** (from the Cosigna API, still served by the AgentAuth service, fixture `agentauth_consents.json`): intent, limit,
  currency, single/multi-use, expiry, active or not. Spent so far: sum of `ALLOW` payments in
  activity for that consent (`payment.consent`).
- **Mandates** (fixture `inbox_mandates.json`): title, tools, agent, per-call / per-day / calls
  per day limits, expiry, today's use from `uses`, revoked or not. Actions: grant (form mapping
  1:1 to `fabric mandate grant`), revoke (`fabric mandate revoke <id>`), both with confirmation.

### 2.4 Agents (`/agents`)
- Name, scopes (`tools`), created, revoked; whether tokens are required (`required`).
- Register an agent (`fabric agent add NAME --tool GLOB...`): the token is shown **once**, with
  a copy button and a warning; never stored by the console, never shown again.
- Revoke (`fabric agent revoke NAME`) with confirmation.
- Data: `fabric agent list --json` (fixture `agent_list.json`).

### 2.5 Evidence (`/evidence`) — for the auditor
- Chain status from `fabric evidence verify --json` (fixture `evidence_verify.json`): records,
  head hash, ok or problems; signed decisions checked; mandate uses replayed; exceptions.
  A large clear verdict ("Evidence intact: 236 records") or the problems listed.
- "Export signed evidence pack" (`fabric evidence export --out <dir>`), then show the
  fingerprint and the `verify-pack` command to give an auditor.
- Never claim more than the verifier checked: show exactly what `evidence verify` reports.

### 2.6 Overview (`/`) — last
- Waiting for approval (count, oldest), money moved by agents today and this week (from activity
  `ALLOW` payments), blocked attempts, evidence status. Each tile links to its screen.

## 3. Architecture and security (non-negotiable)

- **Next.js 14 (App Router), TypeScript strict, Tailwind, Radix UI, lucide-react**, matching the
  existing site (`agentauth-io/agentauth/agentauth-site`): black background `#000000`, white
  text, muted `#86868b`, borders `#1d1d1f`, one accent `#0071e3`, Inter. Add semantic colors
  for allowed (green), held/waiting (amber), blocked (red); support light mode as well.
- **All engine access is server-side.** Route handlers / server actions call the `fabric`
  binary with `execFile` (argument arrays, never a shell string, never user input interpolated
  into a command), always with `--json`, with `cwd` set to the configured project directory.
  Parse and validate JSON with zod against the fixture shapes; unknown `schema` versions are
  shown as an error, not guessed.
- **The Cosigna (AgentAuth) API is called server-side only**; its URL and key come from server env
  (`AGENTAUTH_URL`, `AGENTAUTH_API_KEY`). Nothing secret reaches the browser bundle.
- **Local and single-user in v1.** Bind to `127.0.0.1` only. On start, print a one-time URL with
  a random token (like Jupyter); it sets an httpOnly, SameSite=Strict session cookie. Every
  mutating request requires the session and a CSRF token. No other way in.
- **The console user's identity** (used for `--as`) comes from config (`CONSOLE_USER`), shown in
  the header. Every decision the console records is attributable.
- **Never display or log secrets:** API keys, Stripe keys, agent tokens (except the one-time
  display after registration), delegation tokens. The `fabric secret` commands are not exposed.
- The engine refuses approvals from inside an agent session; the console runs as a person's
  process. Do not try to work around any refusal from the engine: show its message.
- **Fixtures mode:** `CONSOLE_FIXTURES=1` serves the JSON files in `console/fixtures/` instead of
  calling `fabric`, and turns actions into no-ops that show what would have run. All UI
  development and tests use fixtures; live mode is for the demo.

## 4. Acceptance criteria

- `npm run typecheck`, `npm run lint`, `npm test` pass (component tests for each screen against
  fixtures; tests for the server's command builder proving no shell and no interpolation).
- A test that the production client bundle contains no fixture secret patterns
  (`sk_`, `rk_`, `fat_`, `aa_live_`, `eyJ` JWTs).
- Approval flow: pending → detail → approve with note → list updates; reject likewise; the
  executed command is exactly `fabric approve <id> --yes --as <user> --note <note>`.
- Phone width 375 px: inbox and approval detail fully usable without horizontal scroll.
- Keyboard and screen reader: every action reachable, labelled; WCAG AA contrast in both modes.
- Live mode demo: against `C:\Users\Harsha\Pictures\agentauth-stripe-demo` (the Northwind world)
  the activity screen shows the Northwind refunds, including the three blocked ones.

## 5. Out of scope for v1

Multiple users and roles, passkey-signed approvals in the browser (milestone M4), hosted
deployment, the Rust API, editing guard rules, Stripe dashboards. Do not build them; leave clean
seams (the session layer and the approval action) where they will go.

## 6. Where things are

- `console/` — this app (create it here). `console/fixtures/` — real JSON captured from the
  engine (Northwind run and a synthetic approvals world), no secrets.
- `../docs/CLI.md` — every command and JSON schema the console uses.
- `../crates/fabric-cli/src/` — the engine, if a field's meaning is unclear (do not change it;
  request changes in `console/QUESTIONS.md`).
