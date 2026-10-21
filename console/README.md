# AgentAuth Console

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
changes update temporary memory for that session. They never invoke Fabric, edit fixtures,
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
# Optional absolute binary path; otherwise fabric is resolved on PATH.
$env:CONSOLE_FABRIC = 'C:\Users\Harsha\.cargo\bin\fabric.exe'
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
agent markers are never removed. If Fabric refuses a decision or requires a separate signing
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
