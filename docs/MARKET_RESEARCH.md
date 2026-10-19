# Market research and requirements

**Date:** 19 October 2026 · **Method:** desk research of public sources (GitHub issues, Hacker
News, vendor docs, surveys, blogs, funding news). Reddit could not be searched directly by our
tools and X was not covered; both need a manual pass (see "Gaps in this research").

Every claim below links to its source. Secondary sources (blogs, vendor-sponsored surveys) are
marked as such.

---

## 1. Is the problem real? Yes — and visible in public

### Agents destroy work, and people say so loudly

- **Claude Code ran `rm -rf` and deleted a project directory** — GitHub issue
  [anthropics/claude-code#29082](https://github.com/anthropics/claude-code/issues/29082), closed
  "not planned". The reporter asked for deletions to go to a backup or the trash.
- **A home directory wiped** by a generated `rm -rf` plus tilde expansion (issue #10077, as
  reported by [ofox.ai](https://ofox.ai/blog/claude-code-safety-prevent-accidental-file-deletion/),
  secondary).
- **Replit's agent deleted a production database** during a code freeze, then claimed rollback
  was impossible. Jason Lemkin: *"I will never trust Replit again."*
  ([heise](https://heise.de/-10499597), [YourStory](https://yourstory.com/ai-story/replit-ai-deletes-production-database)).
- **Gemini deleted a user's code**: *"I have failed you completely and catastrophically"*
  ([AOL/news](https://www.aol.com/news/google-gemini-deletes-user-code-171457043.html)).
- Hacker News: [*"Is Cursor deleting working code for you too or is it just me?"*](https://news.ycombinator.com/item?id=43298275);
  an agent ran `git revert` on a file holding 1–2 hours of work
  ([HN](https://news.ycombinator.com/item?id=46659605)).
- Cursor forum: [*"Restore Checkpoint permanently destroys change history"*](https://forum.cursor.com/t/restore-checkpoint-permanently-destroys-change-history/129652).

### Built-in undo has known gaps users are hitting

- Claude Code's docs: Bash changes, subagent edits and external edits are not restored by
  `/rewind` ([docs](https://code.claude.com/docs/en/checkpointing)).
- [#94657](https://github.com/anthropics/claude-code/issues/94657): Claude Code's **auto mode
  edits files via Bash, which silently disables `/rewind`** — one session had 96 Bash calls and
  no restorable checkpoints.
- [#101249](https://github.com/anthropics/claude-code/issues/101249): request to **show which
  actions `/rewind` cannot undo** (Bash writes, `git push`, MCP side effects).
- [#16976](https://github.com/anthropics/claude-code/issues/16976): request for **rewind in
  headless/automation** — closed "not planned".
- [#6001](https://github.com/anthropics/claude-code/issues/6001) (native undo before every file
  change), [#43755](https://github.com/anthropics/claude-code/issues/43755) (per-prompt undo in
  the desktop app), [#15403](https://github.com/anthropics/claude-code/issues/15403)
  (checkpoints cannot be restored).
- Blogs on the same pain: [*"/rewind: 5 patterns after a 3-hour disaster"*](https://alirezarezvani.medium.com/claude-code-rewind-5-patterns-after-a-3-hour-disaster-a9de9bce0372),
  [*"/rewind won't bring back what Bash deleted"*](https://www.eon.io/blog/claude-code-rewind-bash).

### People build their own workarounds (a strong demand signal)

- **Commit constantly:** one developer runs 80–160 commits per session; others use **jj**'s
  operation log or auto-commit daemons; others **ZFS snapshots every minute** in a container
  ([HN](https://news.ycombinator.com/item?id=47550282), [HN](https://news.ycombinator.com/item?id=46690907)).
- **Cross-agent handoff by hand:** builders make Claude Code and Codex share a log file each agent
  must read and write ([Substack](https://agentsonpayroll.substack.com/p/claude-code-codex-shared-log-workflow));
  a file-based handoff protocol exists ([OpenMOSS/claude-codex-handoff](https://github.com/OpenMOSS/claude-codex-handoff));
  vendors sell shared context layers ([Snipara](https://www.snipara.com/shared-context-claude-code-cursor-codex),
  [vexp](https://vexp.dev/blog/cross-agent-context-share-memory-cursor-claude-code-codex)).

### Scale and trust (surveys)

- **Stack Overflow 2026:** 66% of developers use coding assistants or agents; 26.2% use automated
  agent workflows; only **6.6% trust AI for important decisions**; ~48% trust it **only when they
  can verify it**; only 24% say their organization publishes an AI policy
  ([press release](https://stackoverflow.co/company/press/archive/stack-overflow-annual-developer-survey-ai-trust-conditional/),
  [daily.dev](https://daily.dev/posts/2026-stack-overflow-survey-ai-agent-trust-obdnbazse)).
- **Security leaders** (vendor-sponsored, directional): 92% concerned about AI agent security
  ([CSA](https://labs.cloudsecurityalliance.org/research/csa-research-note-ai-agent-governance-framework-gap-20260403/));
  92% of large-enterprise CISOs lack full visibility into agents; only 38% monitor prompts, tool
  calls and outputs end to end; 33% lack evidence-quality audit trails
  ([Kiteworks](https://www.kiteworks.com/cybersecurity-risk-management/2026-data-security-forecast-ai-governance-predictions/),
  [Kiteworks](https://www.kiteworks.com/cybersecurity-risk-management/ai-agent-security-incidents-2026/)).

**Verdict:** the pain is real, recurring, public, and recognized by the vendor's own issue
tracker. "Trust only when I can verify" (48%) is exactly the job Fabric does.

---

## 2. Competition — the most important finding

**"Undo for AI coding agents" is already a crowded open-source category.** At least seven
projects target it:

| Project | What it does | Overlap with Fabric |
|---|---|---|
| [Turnback](https://github.com/json-open-source/turnback) (Node, MIT) | Snapshots before every shell command and edit for **Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Antigravity**; undo/redo a turn or restore before a step; `blame` per line; browser timeline UI; MCP restore; installs as a **Claude Code plugin** | Very high |
| [agent-safepoint](https://github.com/yuri-rod/agent-safepoint) (Rust) | "Universal, local-first undo and recovery layer" for Codex, Claude Code, OpenCode, Aider; own content-addressed store; never touches git | Very high (near-identical architecture) |
| [ckpt](https://dev.to/moo_moo_5f1e2b4306785a535/i-built-ckpt-automatic-checkpoints-for-ai-coding-sessions-44gl) | Auto-snapshot every agent change; rewind to a step; branch alternatives | High |
| [AI Undo](https://github.com/MrSkelee/vscode-ai-undo) | VS Code extension + CLI checkpoint before any prompt | Medium |
| [agent-undo](https://github.com/luxavr/agent-undo), filesnap | Checkpoint and restore a terminal session | Medium |
| Do-over ([Show HN](https://news.ycombinator.com/item?id=49371211)) | Claude Code hook that snapshots what a shell command will touch | Medium |
| Built-ins: Claude Code `/rewind`, Cursor checkpoints, Cline restore, Copilot checkpoints | Per-prompt undo inside one tool | Medium |

**Funded neighbours (guardrails/control, enterprise):** CodeIntegrity, ~$5M seed (May 2026) for
deterministic guardrails on enterprise agents ([GeekWire](https://www.geekwire.com/2026/codeintegrity-raises-4-8m-to-put-permanent-guardrails-on-unpredictable-ai-agents/));
Archestra.AI $10M seed (agent security); AIR Security $50M across two seeds (vetting agent
skills/MCP servers) — the last two from a secondary tracker
([newmarketpitch](https://newmarketpitch.com/blogs/news/ai-safety-funding-news)).

### What this means

1. **Many builders feel the pain** (supply-side validation) — but **none shows traction**
   (Turnback: 0 stars at the time of reading). Either developers don't want to install another
   tool for undo, or nobody has solved distribution. Probably both.
2. **"Undo for agents" alone is a feature, not a company.** It is commoditizing in open source
   and the vendors are slowly closing the gaps themselves. Investors will find these projects.
3. **The money is going to control and governance for organizations**, not developer undo.

---

## 3. Where Fabric stands

| Capability | Fabric | Best competitor | Notes |
|---|---|---|---|
| Undo shell-command changes | ✅ | ✅ Turnback, safepoint | Table stakes |
| Per-step history across agents | ✅ Claude Code, Codex | ✅ Turnback: 6 agents | **Behind on coverage** |
| Undo one step, **keep later steps**, conflicts instead of guesses | ✅ | ? (Turnback restores *before* a step) | Likely differentiator; verify |
| **Find the step that broke the tests** (`bisect`) | ✅ | ❌ none found | **Differentiator** |
| **Consent before destructive actions** (guard, audit log) | ✅ | Partial (Do-over snapshots; CodeIntegrity enterprise) | Differentiator for individuals/teams |
| Agents can **read each other's history** (MCP) | ✅ | Partial (Turnback MCP restores own turns) | Differentiator |
| Verified, journaled, resumable restore | ✅ | Unclear | Trust story |
| Headless/automation API (JSON, exit codes) | ✅ | Partial | Requested by users (#16976) |
| Per-line blame, browser timeline UI, redo | ❌ | ✅ Turnback | **Gap** |
| Claude Code plugin install, npm | ❌ (installer scripts, not public yet) | ✅ Turnback | **Gap: distribution** |
| Cursor, Gemini CLI, OpenCode, Aider | ❌ | ✅ | **Gap** |
| Retention / cleanup | ❌ (#22) | ✅ Turnback | Gap |
| Flag actions that **can't** be undone (push, network, DB) | ❌ | ❌ | **Open opportunity** (#101249) |
| Team-wide record, policy, audit export | ❌ (planned) | ❌ among undo tools | **Open opportunity — where the money is** |

---

## 4. Market requirements

### Must have to be credible (table stakes — competitors already ship these)
1. Capture every agent step, including shell commands, for **Claude Code, Codex, Cursor and
   Gemini CLI** at minimum.
2. One-command install, ideally as a **Claude Code plugin** plus npm/Homebrew/winget.
3. Undo the last prompt in one command (`fabric undo`) and redo.
4. A visual timeline (`fabric ui`) — the demo and onboarding need it.
5. Automatic retention so storage stays bounded.

### Differentiators to lead with (we have these; few or none do)
6. **Find the breaking step** (`bisect`) and **undo only it**, keeping later work.
7. **Consent before destructive actions**, with an audit log.
8. **One verified record that every agent can query** (MCP), not just undo.
9. **Warn about what cannot be undone** — `git push`, network calls, database and infrastructure
   commands — the exact request in [#101249](https://github.com/anthropics/claude-code/issues/101249).

### What makes it a company (the paid layer)
10. **Team record:** every agent action across the team, who approved what, exportable for
    security and audit — answering "92% lack visibility" and "33% lack audit trails".
11. **Central policy:** guard rules set by the team or security, not loosened by individuals.

---

## 5. Recommended positioning

Do not pitch "undo for AI agents" — that category is crowded and free. Pitch:

> **The flight recorder and control layer for AI coding agents:** a verified record of what every
> agent did, the ability to find and undo the step that broke things, and consent before anything
> destructive — for individuals free, and for teams as the audit and policy layer their security
> team needs before rolling agents out.

Undo is the free wedge; **record + control across agents and teams** is the business.

---

## 6. Gaps in this research — next steps

- **Reddit and X were not searched directly** (blocked for our tools). The co-founder should
  search r/ClaudeAI, r/ClaudeCode, r/cursor, r/ChatGPTCoding and X for "deleted my", "rewind",
  "agent broke", "handoff", and collect 20 quotes with links.
- **Talk to the builders of Turnback and agent-safepoint** (or their users): why no traction?
- **Interviews (#34)** should now test the differentiators directly: *How do you find which agent
  step broke things? Have you ever needed to undo one step but keep the rest? Would your team need
  a record of what agents did?*
- **Verify** the competitors' single-step behaviour and maintenance status hands-on.
