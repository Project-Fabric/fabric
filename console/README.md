# Cosigna Console

Local, single-user Next.js 14 console. The approval inbox is the primary flow; activity,
authority, agents, evidence and overview use the captured engine contracts in `fixtures/`.
The engine is the authority. The console neither executes payments nor weakens refusals.

## Fixtures first

```powershell
cd C:\Users\Harsha\Pictures\project\console
npm install
npm run dev
```

Open the **one-time URL printed in that terminal**. It expires after ten minutes and is
consumed on first use. The server binds only to `127.0.0.1:3090`, exchanges the URL token
for an httpOnly, SameSite=Strict signed cookie, and redirects to `/approvals` without the token.
The session lasts twelve hours; restarting the process invalidates every session. Every
mutation requires the session, matching Origin and per-session CSRF token. Assets and pages
also require authentication. Do not start this app using bare `next dev` / `next start`:
the provided server is the session boundary.

`npm run dev` explicitly selects fixtures and uses the fixture approver
`priya@northwind.test` unless `CONSOLE_USER` is set. Decisions, mandate changes and agent
changes update temporary memory for that session. They never invoke Cosigna, edit fixtures,
create evidence packs or move money. Restart to restore the fixture state. Fixture token
registration displays an explicitly fake, one-time token. The light/dark preference is the
only browser-persisted application setting.

## Checks

```powershell
npm run typecheck
npm run lint
npm test
node --test tests/security.test.mjs
npm run build
npm run test:integration
```

The production build scans all `.next/static` JavaScript for credential/JWT patterns.
Component tests use captured fixtures. Command tests assert argument arrays and `shell:false`;
notes are never interpolated into shell commands. Unknown engine schema versions fail visibly.
Vitest is launched through its public Node API so configuration loading does not require
native bundling or enumeration of directories outside the workspace. The assertions and
component transforms remain the normal Vitest suite. Production integration checks start
temporary fixture servers on separate loopback ports; they leave the preview session alone.
The CSS includes a 375 px layout, keyboard focus and reduced motion. The palette has AA
text-contrast regression tests; rendered browser and screen-reader checks are still required. Inter is bundled locally;
no font service or external resource is needed at runtime.

## Live mode

Run from a person's terminal. Configure server-only environment variables (or copy
`.env.example` to `.env.local` and edit it):

```powershell
$env:CONSOLE_FIXTURES = '0'
$env:CONSOLE_USER = 'your-approver-identity'
$env:CONSOLE_PROJECT = 'C:\Users\Harsha\Pictures\agentauth-stripe-demo'
# Optional absolute binary path; otherwise cosigna is resolved on PATH.
$env:CONSOLE_FABRIC = 'C:\Users\Harsha\.cargo\bin\cosigna.exe'
# Optional: required only for listing API consents.
$env:AGENTAUTH_URL = 'http://127.0.0.1:18090'
# Set AGENTAUTH_API_KEY privately in this terminal. Never use a NEXT_PUBLIC_ variable.
# Optional: existing parent directory for newly exported evidence packs.
$env:CONSOLE_EXPORT_DIR = 'C:\Users\Harsha\Pictures\agentauth-evidence'
npm run build
npm start
```

The API lists `/v1/consents?limit=100&offset=0` using a server-side key. Engine commands use
`execFile` with argument arrays, `--json`, a configured cwd, a timeout and no shell. Engine
agent markers are never removed. If Cosigna refuses a decision or requires a separate signing
key, that message is shown; the console does not retry around it. Identity comes only from
`CONSOLE_USER`, not from the browser. Export destinations are server-selected children of
`CONSOLE_EXPORT_DIR`; no browser-supplied path is accepted.

The live activity page reads Northwind's actual recorded refunds and three blocked requests.
Live approvals show normalized money only where a payment declaration supplies its units;
unknown units leave approval disabled. Read `QUESTIONS.md` for the contract questions and
limitations, including currency-free mandates and bounded activity totals. Consents are
read-only in this brief. Approving an exact request records permission; the agent must retry
that action separately. Approval does not itself execute a refund.

Use `npm run start:fixtures` to preview a production build with simulated actions.

## Current web development

The public landing preview is a separate buildless app in [site/](../site/README.md).
Its typography, editorial layout and restrained motion take direction from Glasswing;
Cosigna's text and canvas artwork are original. No public route was added to this Console's
session boundary to serve the marketing page.

The dashboard now includes `/integrations` and `/automations`. Integration cards show
test-mode validation or planned status; connection plans collect no credentials and run no
OAuth flow. Cosigna agent templates create in-memory drafts and export a versioned
`cosigna.agent-draft/v1` configuration. Drafts never invoke engine mutations, models or
payment APIs and are lost on navigation unless exported. Every export has execution disabled
and human review before writes. The existing `/agents` route remains the identity registry.

The CLI is under development; web work has priority. See [WEB_PRODUCT_PLAN.md](../docs/WEB_PRODUCT_PLAN.md)
for the connector architecture and the prerequisites for a real managed-agent runtime.

## Personal task preview

Startup regression checks: `npm run test:dev` verifies the real development sign-in, fixture
mode and session/CSRF flow in a temporary checkout. Add `-- --production` to also build and
exercise the production HTTP flows there, without replacing a running dev server's cache.

Open **Tasks preview** in the signed-in console, or visit `/tasks`. Two fictional scenarios
cover shopping and flights: compare offers, apply a task budget, review and confirm a demo
approval, then simulate success, a changed price, or an unknown outcome followed by a status
lookup. A mismatched or over-budget offer cannot advance to approval.

All data and outcomes are synthetic, even when the rest of the console is in live mode.
The preview uses page memory only and resets on navigation/reload. It does not search the web,
call the engine, connect providers, verify identity, or issue purchases/tickets. It is a UX
prototype, not evidence of production authorization or budget enforcement.

## Agent payments from the Cosigna gateway

The approval inbox also shows the gateway's money operations that need a person: refunds or
payouts held above an agent's automatic limit, and payments whose result is unknown. Approve
(bound to the exact proposal digest), reject, or settle them through the payment system. Set:

```powershell
$env:CONSOLE_GATEWAY_URL = 'http://127.0.0.1:3092'   # the running `cosigna-agent serve`
$env:CONSOLE_GATEWAY_DIR = 'C:\path\to\gateway-profile'  # holds the gateway's credentials.json
```

The owner credential is read server-side for each call and never sent to the browser. The
gateway address must be loopback. With a gateway and no `CONSOLE_PROJECT`, the console runs in
gateway-only mode: the Fabric sections are left out instead of showing errors.

**Behind a TLS proxy** (the server deployment in `deploy/docker`), set `CONSOLE_PUBLIC_ORIGIN`
to the address people open, such as `https://cosigna.example.com` (with the port if it is not
443). The console then listens on all interfaces, accepts only requests for that host that
arrive with `X-Forwarded-Proto: https`, checks the `Origin` of every change against it, marks
cookies `Secure`, and prints its one-time link with that origin. Live check:
`npm run test:public`. In fixtures mode the section shows `fixtures/gateway_operations.json`,
which was captured from a real gateway. Decisions are recorded by the gateway as its owner role. Per-person
approver identity on the gateway is still planned (G3). Live check: `npm run test:gateway`.

**Named approvers sign in with their own key.** The owner registers approvers on the gateway
(`POST /gateway/v1/approvers`, each with a currency and a limit) and gives each person their
`approverkey_…` once. An approver opens `http://127.0.0.1:3090/signin` and pastes it. The console
checks it with the gateway (`/approvers/me`) and keeps it only in this process's memory for that
session: never on disk, never in the browser, and gone on sign-out or restart. An approver
session sees only the agent payments and their limit, and can approve or reject; the gateway
refuses approvals above their limit and records each decision under their name. Settling unknown
results, exposure, the local engine screens and every other action stay with the owner. Sign-in
needs the console's own origin and allows five failed attempts a minute for the whole console.
It needs a live gateway (not fixtures mode). The console binds to 127.0.0.1 only, so approvers
must use this machine until there is a hosted deployment.

Each agent payment has a **Timeline** (also under **Recent agent payments**): who asked, what
policy decided and why, who approved, what the payment system answered, where it stands now and
what can happen next, each step citing its evidence record, and whether the evidence chain
verifies. Timelines come from the live gateway (`/operations/{id}/timeline`); fixtures mode has
none.

Below it, **What agents can pay without you** shows the gateway's worst-case exposure
(`GET /gateway/v1/exposure`, `cosigna.exposure/v1`): the most agents can still move with nobody
signing, what limits each agent (its budget or what open cases owe), and what requiring a case
would cut a refunds agent to. It is an upper bound under the current grants, not a forecast. In
fixtures mode it shows `fixtures/gateway_exposure.json`, captured from a real gateway.
