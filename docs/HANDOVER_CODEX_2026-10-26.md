# Handover to Codex, 26 October 2026

Claude Code is stepping back; Codex takes the next work. This note says where things stand,
what to build next and the rules the founder has set. Repository facts (code, tests, formats)
are in the code and `docs/STORAGE_FORMAT.md`; this note covers only what is not.

## First: sync the main worktree

Claude worked in a second worktree (`project-casecheck`) and pushed to `origin/main`. The main
worktree (`C:\Users\Harsha\Pictures\project`) was 21+ commits behind, with uncommitted Codex
edits to `docs/ROADMAP.md`, `docs/STORAGE_FORMAT.md`, `docs/YC_READINESS.md` and
`.github/workflows/product-checks.yml`, plus untracked research docs. `STORAGE_FORMAT.md`
changed upstream too (ledger v9), so pull with care and merge, do not overwrite.

## Where things stand (all on `origin/main`, CI green)

Codex's build order from 26 October:

| Step | State |
|---|---|
| 1 Approver sign-in | Done |
| 2 Replay of past refunds (rows with no case record are `unknown`) | Done (`e2b387f`) |
| 3 Case connector (CSV or help desk over HTTPS) | Done (`3a82d66`); passed on a real Zendesk trial (`92c216c`) |
| 4 Stripe test mode | Passed on a real Stripe test account (`5578ba4`, `scripts/stripe-test-check.mjs`) |
| 5 Refund recovery worker + operator timeline | Done (`59f35c4`) |
| 6 Before/after comparison with a real company | Waiting on a company (founders) |

Board (`Cosigna roadmap`, project 2): E12-E15 done, E16 (Zendesk OAuth refresh) todo, E6
(#95) in progress with a progress comment.

## Next task: the recorded demo with real agents

Goal: real AI agents work support tickets through Cosigna, with **Stripe test-mode refunds**
as the rail and **Zendesk tickets** as the case records, then the run is recorded. Today's
runs were scripted refunds, not agents. The earlier Northwind experiment
(`docs/EXPERIMENT_NORTHWIND.md`, `experiments/northwind/`) used real agents but through the
old AgentAuth gateway, not the Cosigna gateway.

Pieces that exist:
- `cosigna-agent serve --stripe-test --cases <zendesk config>`: both can be on at once.
- `cosigna-agent mcp --gateway-url ...`: MCP tools for one agent (`cosigna_request_refund`,
  `cosigna_operation_status`, `cosigna_budget`); the agent sees its own token only.
- Agent grant with `"require_case": true` so every refund must name a ticket (`zd_<number>`).

Things to know before building it:
- **Tickets must hold real Stripe ids.** The three trial tickets hold fixture ids
  (`pay_fixture_*`, `cus_fixture_*`). For the Stripe rail, tickets need the PaymentIntent id
  (`pi_...`) and the Stripe customer id (`cus_...`).
- **Attach a Stripe customer to each test payment.** The case check compares the customer only
  when the payment has one (`operations.rs`, `customer.is_some_and(...)`). Payments made with
  `pm_card_visa` and no customer skip that comparison, so the demo would show a weaker check
  than the product has. Create customers and pass `customer` when creating the PaymentIntents.
- **The OAuth token expires** and Cosigna cannot refresh it (E16). Get a fresh token right
  before recording; an expired token fails closed with `case_records_unavailable`.
- Ticket creation in Zendesk can be done by the founder by hand (it took ~10 minutes) or via
  the API, which would need a token with write scope: ask first, and keep the demo's own
  token `tickets:read`.

## Accounts (test only; no secrets here)

- **Zendesk trial** `fintech-93589.zendesk.com`, ends about 9 November 2026. Custom ticket
  fields: Customer id `31329385749148`, Payment id `31329450259996`, Amount owed
  `31329423031068` (Decimal). Tickets 2, 3, 5 hold the fixture cases A, B, C. Zendesk retires
  API tokens on 30 April 2027; use the OAuth client `cosigna_test` (Confidential) with the
  `client_credentials` grant and scope `tickets:read`, sent as `bearer`
  (`examples/cases/zendesk-oauth.json`). The client secret is with the founder only.
- **Stripe** test account of the founder. Test keys only (`sk_test_`); the gateway refuses
  live keys.

## Rules the founder has set

- **Credentials:** never ask for or handle keys in chat. The founder types them into their own
  PowerShell window with hidden input, and pastes back only results:
  ```powershell
  $s = Read-Host "Secret" -AsSecureString
  $env:COSIGNA_STRIPE_TEST_SECRET_KEY = [System.Net.NetworkCredential]::new("", $s).Password
  ```
  Plain `Read-Host` echoes the secret; one Zendesk secret was exposed that way.
  Scripts must never print a secret and should check that it is in no gateway file.
- **Git identity:** commit only as `Ashok Kumar <110275819+Ashok-kumar290@users.noreply.github.com>`;
  never `vallip823@gmail.com`. Check before each commit.
- **AgentAuth repo:** do not push its `main` (it auto-deploys) or change its logic.
- Keep vulnerability details off public repos. Do not send the founder's email to unrelated
  services.
- After shipping, update `docs/PIVOT_FINANCE.md` status and the GitHub board.
- `AGENTS.md` applies: fmt, clippy `-D warnings`, full tests; never weaken tests; bump formats
  and update `STORAGE_FORMAT.md`; claim only what tests show.

## Open decisions waiting on the founders

- Design-partner shortlist and outreach (YC W27 deadline 2 November). Their help desk decides
  what comes after Zendesk; their refund history is what makes replay convincing.
- Where the console is hosted.
- The AgentAuth `.pem` file removal.
