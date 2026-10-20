# Pivot: controls for AI agents in finance

**Status:** decided 20 October 2026, to be validated by interviews · **Owner:** Ashok

## Why we are pivoting

- **Developers do not pay for undo.** At least seven open-source "undo for agents" tools have
  almost no users (`MARKET_RESEARCH.md`).
- **Models rarely make generic mistakes.** The lessons calibration showed the current model
  follows best practice on its own; it fails on rules it cannot infer
  (`EXPERIMENT_LESSONS.md`).
- **Finance turns both around.** Companies under SOX must prove that every change to a
  financially significant system was authorised, tested, approved and traceable, and that
  whoever wrote a change did not approve it. That is a legal requirement with a budget, and
  finance is full of rules a model cannot infer.

## Positioning

> **Fabric: SOX-grade change control for AI agents.** Every agent action on a financially
> significant system is recorded with its full before and after, checked against finance
> rules, approved by a human where required, and reversible, with evidence an auditor
> accepts.

Fabric is the **control layer**, not the agent. We do not build an AI accountant. We sit under
coding agents (Claude Code, Codex, Cursor) and, later, finance-operations agents, and produce
the evidence their users' auditors ask for.

## Order

1. **Finance engineering teams using coding agents** at SOX-subject fintechs, payments companies
   and pre-IPO startups. Closest to what exists today.
2. **Finance-operations agents** (journal entries, reconciliations, close workbooks): the same
   controls applied to data instead of code; a partner channel for AI accounting vendors.
3. **Banks** (DORA in the EU, SR 11-7 model risk in the US): after references exist.

## What auditors ask (practitioner sources, not regulator text)

There is no AI-specific PCAOB standard; existing ITGC and ICFR standards apply. Practitioner
guides list recurring auditor questions; the ones Fabric can answer directly:

| Auditor question | Fabric today | Gap |
|---|---|---|
| How is a change version-controlled, and who requested and approved it? | Per-step record with prompt, agent and files | **Approval record** (who approved, when) |
| Show the audit trail for a specific change, inputs to result | Step log, full before/after content, diff | **Tamper-evident, signed export** |
| Can a change be reconstructed? | Content-addressed snapshots, verified restore | Retention policy (practitioners cite 7 years) |
| Where is the boundary between AI and human judgment, enforced in the system? | Guard: allow / ask / deny | **Finance rule packs**; approval tied to identity |
| What is the remediation path when the AI is wrong? | `bisect`, single-step `revert`, verified restore | Remediation recorded in the evidence |
| Who can change the AI's configuration? | Guard overrides kept outside the project, audit log | Config changes in the evidence |

## Competitors and neighbours

| Kind | Examples | What it means for us |
|---|---|---|
| Audit gateways for coding agents | Proofpane (hash-chained audit, signed evidence packs), Agentic Governance Gateway, AgentGate, ProofFlow, hoop.dev | **A signed audit log alone is a commodity.** Our difference must be full before/after content, single-step reversal and finance rules |
| Agent security platforms | AIR ($50M seed; customers mostly financial services and pharma), Hush Security | Well funded, broad; identity and access, not change content |
| Finance-ops AI with controls | Kognitos, FloQast | Own their agents; potential partners for the record layer |
| AI accountants | Basis ($100M Series B at $1.15B, Feb 2026), Rillet | Do not compete; their customers' auditors are our reason to exist |
| Agent vendors | Anthropic, OpenAI, Cursor, GitHub | Could add audit logs; unlikely to add cross-vendor, auditor-facing evidence |

Most of these figures come from secondary sources and need checking before a pitch.

## What we build (in order, smallest first)

Status as of 20 October 2026. Results with real agents: `EXPERIMENT_FINANCE.md`.

1. **Signed, tamper-evident evidence export.** Hash-chain the step log; `fabric evidence
   export` writes a self-verifying pack (steps, approvals, diffs, guard decisions) with a
   verifier that runs offline. *Done* (#53).
2. **Approvals.** The guard's `ask` becomes an approval recorded with the approver's identity;
   the agent's run identity is never the approver (segregation of duties). *Done* (#54);
   approval by a second person on another machine is not built.
3. **Finance rule packs.** Paths and commands that need approval or are denied: ledger and
   billing code, applied migrations on financial tables, production credentials. *Done*
   (#55), with rollback of controlled files changed around the hold (e.g. by a script).
4. **Evidence report** a reviewer can read: one page per change. *Built* (per checkpoint and
   per change ticket); open until an auditor from the interviews has reviewed it (#56).
5. **Change requests.** Authorize a task once, review its result once, revert it whole, one
   report per change. *Done* (#57): cut a feature's controller work from 6 to 8 approvals to
   2 actions in the experiment.

Next, in order:

6. **A real controller in the loop.** Put `fabric review` and the change report in front of a
   controller or IT auditor from the interviews; measure what they catch and how long review
   takes (the scripted reviewer accepted a rule-breaking change) (#58).
7. **Live interactive sessions.** Change requests and rollback in a person's own Claude Code
   session, not only headless runs (#47).
8. **Approval from another person or machine.** Segregation of duties beyond "not the agent".
   *Done* (#59): approvers sign approvals and reviews with their own key on their own
   machine; a project can require it; signatures are re-verified from the evidence.
9. **Other agents.** Codex and others get checkpoints, undo and evidence but no hold or
   rollback (#46).
10. **Agents that act through tools.** *Done* (#60): rules over MCP tool calls, the
   `finance-ops` pack and `fabric mcp-proxy` for any agent; a material refund an agent made
   unapproved 3/3 alone was held and rejected 3/3 through Fabric (`EXPERIMENT_FINANCE_OPS.md`).
11. **Mandates.** *Done* (#61): bounded authority granted ahead of time (tools, agent,
   limits per call and per day, expiry), optionally signed by an approver; calls within it
   need no approval each, and every use is replayed against its terms from the evidence. The
   first piece of AgentAuth rebuilt as a product on this engine.

Everything stays local-first; no cloud dependency in the core (`AGENTS.md`).

## What would make us stop

Decided before the interviews (`INTERVIEWS_FINANCE.md`):

- **Continue:** at least 5 of the ~15 interviewees describe agent change control as a problem
  they have now or expect this audit cycle, and at least 2 agree to try a pilot.
- **Narrow:** only auditors care, not engineering or finance. Then the buyer is compliance,
  and the product leads with the evidence export.
- **Stop:** SOX-subject teams ban agents and do not plan to change, and auditors do not ask
  about them.

## Sources

- [Kognitos: what your SOX auditor will ask about AI automation](https://www.kognitos.com/blog/sox-auditor-questions-ai-automation/)
- [FloQast: what AI audit controls actually look like](https://www.floqast.com/blog/what-ai-audit-controls-actually-look-like)
- [Sourcegraph: proving agent provenance for regulated engineering teams](https://sourcegraph.com/blog/compliance-first-ai-proving-agent-provenance)
- [Augment: attributability and reversibility for enterprise audit](https://www.augmentcode.com/guides/multi-agent-outputs-n-pass-enterprise-audit)
- [Proofpane listing (Glama)](https://glama.ai/mcp/servers/mjy2sj20ji)
- [AIR $50M seed (PYMNTS)](https://www.pymnts.com/news/investment-tracker/2026/ai-agent-security-startup-air-raises-50-million-to-guard-enterprise-supply-chains/)
- [Basis $100M Series B (CPA Practice Advisor)](https://www.cpapracticeadvisor.com/2026/02/24/basis-raises-100-million-to-deploy-ai-agents-for-accounting-firms/178759/)
- [Rillet $25M (Yahoo Finance)](https://finance.yahoo.com/news/accounting-technology-startup-rillet-raises-141421968.html)
