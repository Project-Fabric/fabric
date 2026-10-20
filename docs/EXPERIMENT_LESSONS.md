# Experiment: do outcome-verified lessons stop agents repeating mistakes?

**Status:** design, pre-registered before any run · **Owner:** Ashok · **Date:** 20 October 2026

## Question

When a coding agent breaks something in a repository and the break is caught, does giving future
agents a **lesson learned from that verified outcome** stop them making the same kind of mistake —
better than no memory, and better than Claude Code's own auto memory?

## Hypotheses (decided in advance)

- **H1.** Fabric's lessons reduce the **repeat-mistake rate** by at least **50% relative** to no
  memory.
- **H2.** Fabric's lessons beat **Claude Code auto memory** on repeat-mistake rate by at least
  **25% relative**, given the same prior experience.
- **H3.** Lessons do **not** hurt unrelated work: first-try pass rate on *control* tasks drops by
  no more than 5 percentage points, and the learned guard's **false alarms** stay under 1 in 10
  warnings.

If H1 fails, the direction is wrong. If H1 holds but H2 fails, Claude's built-in memory is good
enough and our edge is only cross-agent. If H3 fails, lessons cause harm and must be fixed first.

## Design

### Trap repositories

Small, realistic projects, each with a **trap**: a rule that is true in this codebase, enforced by
its tests, but **not obvious from reading the code** — so an agent can only reliably avoid it by
having learned it. Examples:

| Trap | Rule the tests enforce | Natural mistake |
|---|---|---|
| Discount semantics | `discount` is a fraction (0.1 = 10%) | Treating it as dollars |
| Money in cents | Amounts are integer cents | Using floats / dollars |
| Frozen public API | `get_user()` must keep its name; external callers depend on it | Renaming it "for clarity" |
| Immutable migrations | Existing migration files must never change | Editing an old migration |
| Timezones | Timestamps are stored in UTC | Using local time |
| Soft delete | Records are deactivated, never deleted | Writing a hard `DELETE` |
| Config precedence | Env vars override the config file | Reversing the order |
| Pagination | Pages are 1-indexed | Assuming 0-indexed |

Target: **8 traps**. Each has **two different tasks** that both touch the trap:

1. **Phase 1 — the first encounter.** A task where the agent is likely to fall into the trap. The
   tests catch it. The scripted "developer" reverts the step with Fabric and replies with the same
   short correction in every condition (e.g. *"test_discount failed and I reverted that change:
   discounts are fractions"*), so every memory system gets the same chance to learn.
2. **Phase 2 — a new task, fresh session.** A *different* task touching the same rule (e.g. "add a
   loyalty discount" after the first task was "add tax"). Did the agent repeat the mistake?

Plus **control tasks** with no prior failure, to measure harm (H3).

### Conditions

| | Memory available in phase 2 |
|---|---|
| **A. None** | Nothing. Fresh session, memory features off |
| **B. Claude auto memory** | Claude Code's built-in auto memory, which saw the phase-1 session and correction |
| **C. Fabric lessons** | Lessons from Fabric's memory graph, offered to the agent over MCP |
| **D. Fabric lessons + learned guard** | As C, and the guard warns the agent before an edit that resembles a past failure |

Same model, same prompts, same repositories in every condition. Each condition runs in its own
isolated copy of the repository and its own Fabric/Claude data, so nothing leaks between
conditions.

### Metrics

- **Repeat-mistake rate** (primary): phase-2 runs whose result fails the trap's test.
- **First-try pass rate**: all tests pass when the agent says it is done.
- **Harm**: pass rate on control tasks; learned-guard warnings on correct actions (false alarms).
- **Cost**: wall-clock time, and tokens where reported.

### Sample size

Agents are not deterministic, so each cell runs repeatedly.

- **Pilot:** 3 traps × 3 runs × 4 conditions = 36 phase-2 runs (plus phase 1 and controls).
  Purpose: check the harness and that traps actually trap. Not used for conclusions.
- **Main run:** 8 traps × 5 runs × 4 conditions = 160 phase-2 runs, plus 8 control tasks × 5 runs
  × 4 conditions. Roughly 10–20 hours of agent time; it runs unattended.

### Guarding against fooling ourselves

- **We design the traps, which biases toward Fabric.** Mitigations: control tasks measure harm;
  every trap and task is published; at least two traps are proposed by someone else (co-founder or
  a beta user); a later round uses real failures mined from open-source repository histories.
- **A trap that phase 1 does not trigger** (the agent gets it right first time) is dropped and
  replaced, not counted.
- **Conditions are not tuned after seeing results.** Prompts and lesson formats are frozen before
  the main run; changes require a new run.
- **Everything is reported**, including conditions where Fabric loses.

## What gets built

All reuses Fabric's existing step log, `bisect`, `revert`, guard hook and MCP server.

1. **Memory graph.** Nodes: sessions, prompts, steps, files, tests, lessons. Edges: *changed*,
   *broke*, *reverted by*, *learned from*, *prevented*. Stored per project in Fabric's data
   directory.
2. **Lesson extraction.** When a step is reverted or identified by `bisect`, record a lesson from
   the evidence: the step's diff, the failing test and its output, and the developer's correction.
   A short natural-language rule is written by a model from that evidence, linked to the files,
   symbols and tests involved.
3. **Consolidation ("sleep").** Periodically merge duplicate lessons, strengthen lessons that
   preceded successful work in related files, weaken lessons that were never relevant, and retire
   lessons associated with failures.
4. **Recall.** MCP tool `fabric_lessons` returns the lessons relevant to the files an agent is
   about to touch.
5. **Learned guard.** On `PreToolUse`, if an edit touches a file and symbol linked to a past failure
   and resembles the failing change, Fabric blocks it once with the lesson as the reason; the agent
   can proceed after reconsidering. Every warning is logged so false alarms can be counted.
6. **Harness.** A script that sets up each isolated repository, runs Claude Code headless through
   phase 1, the scripted correction and phase 2 in each condition, runs the tests, and records
   every metric to a results file.

## Timeline

| Day | Work |
|---|---|
| 1 | Memory graph, lesson extraction, `fabric_lessons`, learned guard (behind a flag) |
| 2 | 3 trap repositories, the harness, the pilot run; fix what the pilot breaks |
| 3 | Remaining traps, main run (unattended), write-up with every result |

## Changes after the smoke test (20 October 2026, before any counted run)

Recorded openly because they depart from the design above. No result has been counted yet.

1. **Rules moved to hidden CI tests.** In the first smoke test Claude read the visible test
   enforcing the rule and refused the phase-1 change, so the trap never fired. Real hidden
   constraints live in CI, other services or people's heads, not in tests the agent reads. Each
   trap's rule is now enforced only by a CI test run after the agent finishes (`ci.py`); the
   repository keeps ordinary tests that do not reveal it.
2. **Fair correction for Claude's memory.** In the second smoke test Claude edited code while
   acknowledging the correction (so setup B started phase 2 from a different project) and saved
   no memory. The correction now ends: *"Please remember this for future work in this project. No
   code changes are needed right now."*, and B is restored to the starting point afterwards.
3. **Calibration before comparison.** In the second smoke test even setup A (no memory) avoided
   the mistake in phase 2, so the trap could not show any difference. A calibration pass (5 traps
   × 1 run) measures which traps make the memory-less agent repeat the mistake; only those go into
   the main comparison. Two traps were added where the natural default breaks a hidden rule:
   **cents** (amounts are integer cents; the natural `amount * 1.075` returns a float) and
   **utc** (timestamps must be timezone-aware UTC; the natural `datetime.now()` is local and naive).
   Calibration results are reported alongside the main results.

## Decision after the experiment

- **H1 and H2 hold, H3 holds:** pivot the product and pitch to verified lessons; the number is the
  headline.
- **H1 holds, H2 fails:** lead with cross-agent and the learned guard, not memory alone.
- **H1 fails:** stay with the safety-and-record product; publish the negative result.
