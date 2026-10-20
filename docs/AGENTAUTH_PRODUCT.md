# AgentAuth: the product, layer by layer

**Status:** plan, 20 October 2026. Supersedes the build order in `PIVOT_FINANCE.md` from
item 12 on. Fabric's engine is AgentAuth's core.

## What AgentAuth is

Spending authority and audit for the AI agents that move a company's money. A company's
agents (refunds, payouts, bills, collections, month-end close, and the code behind them) act
through AgentAuth: within delegated limits they act at once; outside them a person decides;
everything is recorded as evidence an auditor can verify; mistakes can be reversed.

Not a payment network and not a checkout protocol: AgentAuth governs an organization's own
agents across all its systems, with the controls auditors already test (delegated authority,
approval of exceptions, segregation of duties, evidence).

## Principles

1. **The agent never holds the keys.** Credentials to Stripe, the ERP, the bank stay in the
   gateway; the agent gets capabilities, not secrets.
2. **Fail closed.** If the policy cannot be evaluated, the action does not happen.
3. **Every decision is evidence.** Allowed, held, approved, rejected, covered by a mandate:
   recorded, hash-chained, signed, verifiable offline by someone who does not trust us.
4. **People decide; models assist.** AI explains, summarizes, drafts mandates and scores risk.
   It never approves.
5. **Local-first core, optional cloud.** The gateway works on its own and offline; the cloud
   adds teams, inboxes and retention. The core is inspectable.
6. **Claims match tests.** No guarantee in docs or sales that the code and tests do not establish.

## The layers

### 1. Core engine (built: Fabric)
Policy evaluation (rules over files, commands, tool calls), mandates, approvals, change
requests, signed sign-off, hash-chained evidence, signed evidence packs, checkpoints and undo.
Rust, single binary, no network.

### 2. Gateway: where agents meet systems
- **Built:** MCP proxy (any MCP agent), Claude Code hook.
- **Credential broker:** the gateway holds each system's API key (OS keychain, never in the
  project or logs) and injects it into upstream calls; the agent's environment has none.
- **Agent identity:** each agent gets its own token, issued by a person, scoped to a set of
  tools; the gateway authenticates it instead of trusting the name the agent gives. Mandates
  and evidence bind to the authenticated agent.
- **Authorize API:** a local HTTP endpoint for agents that are not MCP clients
  (`POST /v1/authorize` → allow / held / deny with a request id), with Python and TypeScript
  SDKs and LangChain, OpenAI Agents SDK and CrewAI adapters.
- **Connectors:** Stripe first (in front of Stripe's MCP server or API: refunds, payouts,
  credit notes), then QuickBooks, Xero, NetSuite, bank payouts. A connector knows each
  action's amount, currency and counterparty, so limits read them exactly.

### 3. Consent model: how authority is granted
- **Principals:** people and roles (controller, CFO, support lead) with their own keys.
- **Agents:** registered identities with an owner and scopes.
- **Mandates** (built, v1): delegated, bounded, expiring authority; signed when required.
  Next: counterparty and currency constraints, per-agent totals across mandates, approval
  chains (above $X, two approvers).
- **Approvals:** for everything outside a mandate (built), signed on another device (built);
  next: passkey-signed approvals on a phone.
- **Consent receipts:** a signed, portable statement that a principal authorized an agent's
  action, verifiable by a third party (a payments provider, an auditor) without our servers.

### 4. Security
Threat model per layer (`THREAT_MODEL.md` extended); keys in the OS keychain, hardware-backed
where available; authenticated agents; fail-closed gateway; signed releases and SBOM;
dependency scanning in CI; external penetration test before the first paying customer; SOC 2
readiness for the cloud.

### 5. Control plane (cloud, optional, self-hostable)
Organizations, users, roles, SSO. **Approval inbox** (web, Slack, mobile; passkey signing).
**Mandate builder.** **Agent registry.** **Evidence vault:** gateways sync their signed evidence;
append-only with retention; auditor access by link. **Dashboards:** spend by agent, holds,
approval times. Gateways keep working offline with cached mandates.

### 6. Design and experience
One design system across the web app, docs and site (from the existing agentauth-site).
Five-minute quickstart: install, connect Stripe, grant a mandate, watch an agent work.
Approver experience first: an approval must be understandable in ten seconds on a phone.

### 7. AI inside the product
- Natural-language mandates: "let the support bot refund up to $200 per ticket" drafted into
  terms a person confirms.
- Approver briefs: what the agent is doing, why it was held, what is unusual.
- Risk scoring of actions (amount, counterparty, time, history) feeding the decision to hold.
- All optional, all advisory, never in the approval path.

## Build order

Each milestone is usable on its own and demoable.

| # | Milestone | Layers |
|---|---|---|
| M1 | **Gateway hardening:** credential broker, agent identity, fail-closed by default | 2, 4 |
| M2 | **Stripe connector** and the 90-second demo (a refunds agent under a mandate) | 2, 3 |
| M3 | **Authorize API + Python SDK** (+ LangChain adapter) | 2 |
| M4 | **Control plane MVP:** approval inbox (web, passkeys), mandate builder, evidence sync | 5, 6 |
| M5 | **Consent receipts** and approval chains | 3 |
| M6 | **AI assist:** NL mandates, approver briefs, risk scores | 7 |
| M7 | **Security hardening:** signed releases, SBOM, external pen test | 4 |
| M8 | **More connectors:** QuickBooks/Xero/NetSuite, bank payouts | 2 |

## Decisions still open

- Open-source the core (recommended: gateway, engine, evidence open; cloud paid).
- Repository: move the engine into `agentauth-io` and rename the command `agentauth`
  (recommended, after M2).
- Cloud stack (recommended: Rust server sharing the engine's crates; TypeScript web app).
