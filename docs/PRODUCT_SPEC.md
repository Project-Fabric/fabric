# Product spec

Version 0.2 · October 2026. Supersedes the v0.1 "integrity and recovery layer" spec, which
remains the basis for the storage and recovery engineering standards.

## Thesis

Git records changes humans chose to commit. Agents make hundreds of small changes nobody
chose, and each agent vendor keeps its own siloed, partial history. Fabric is the
**vendor-neutral record of what every coding agent actually did** — queryable by any agent,
reversible by any human, at the granularity of a single agent action.

Vendors have no incentive to make their agents interoperate. A neutral layer does.

## Core capabilities, in build order

1. **Recorder.** A checkpoint per agent action (tool call), captured via agent hooks
   (Claude Code first) with a file-watcher fallback. Each step records changed paths, content
   identities, the triggering command or prompt, and the agent. Target: snapshot of an
   unchanged 10k-file repo in under 100 ms (requires a stat cache).
2. **Undo.** Restore any checkpoint, or revert a single step while keeping later steps.
   Overlapping later changes are reported as conflicts, never guessed. Every restore first
   snapshots the current state, so every restore is itself undoable. Restores are journaled
   and the result is verified.
3. **Bisect.** Tag checkpoints with configured test results; find the step that turned tests
   red and offer to revert just that step.
4. **Shared memory over MCP.** Any agent can ask "what has been tried on `auth.py`, by which
   agent, and did tests pass?" Answers come from recorded steps and verification results, not
   chat transcripts.
5. **Team audit and policy** (paid). Org-wide history of agent actions, retention, policy.

## Explicitly not claimed

- That agent output is correct (tests passing is evidence, not proof).
- Isolation: a snapshot is not a sandbox. Agents with shell access can change things outside
  the project; Fabric can at most report what it observes.
- Protection against a deliberately adversarial agent with the user's privileges. Fabric
  protects against accidents and mistakes.

## The demo (target: week 4–5)

1. Claude Code refactors a module and breaks three tests.
2. Fabric bisects the recorded steps, finds the breaking step, reverts only that step;
   tests pass.
3. Switch to Codex. It queries Fabric over MCP, learns what was tried and why it failed, and
   fixes it correctly first time.

## Roadmap

Since the finance pivot (20 October 2026), the current build order and status are in
`PIVOT_FINANCE.md` ("What we build"). The plan below is the original one; weeks 1 to 5 were
built.

| Week | Engineering |
|------|-------------|
| 1 | Workspace, object store, manifests, snapshots, diff, undoable restore |
| 2 | Stat cache, CLI with versioned JSON output, crash and corruption tests |
| 3 | Claude Code hook integration, per-step action log |
| 4 | Single-step revert, test bisect |
| 5 | MCP server for cross-agent queries |
| 6–8 | Beta feedback, hardening, public launch |

## Success metrics (week 8)

Weekly active developers, week-2 retention, teams asking for a paid tier, unprompted pain
stories from user interviews. Download counts are not a success metric.
