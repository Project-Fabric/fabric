# Competitive benchmark

**Date:** 20 October 2026 · **Machine:** Windows 11, NTFS, Defender on · **Versions:** Fabric
(`main`, release build), Turnback 0.9.1 (npm), agent-safepoint 0.1.1 (built from `de9e63b`),
git 2.55 · **Runner:** [`bench/competitors/bench.py`](../bench/competitors/bench.py) ·
**Raw results:** [`bench/competitors/results-2026-10-20-windows.json`](../bench/competitors/results-2026-10-20-windows.json)

## Method

Each scenario runs on a fresh, identical Python project (a git repository with a test suite, a
`.gitignore`, and an ignored `.env`) in its own directory, with each tool's storage isolated.
Agent steps are simulated by running Python snippets; each tool records them its own documented
way: `fabric checkpoint`, `turnback run --label … -- <step>`, `safepoint checkpoint`, and for the
git baseline a commit after every step. No AI agent is involved, so runs are deterministic.

The git baseline is **git used with perfect discipline** — a commit after every single agent step.
Agents and people do not do that by default; it is the "80–160 commits per session" workaround
from the market research.

## Results

| # | Scenario | Fabric | Turnback | agent-safepoint | git (commit every step) |
|---|---|---|---|---|---|
| S1 | Undo the last step | ✅ | ✅ | ✅ | ✅ |
| S2 | Undo an `rm -rf` done by a shell command | ✅ | ✅ | ✅ | ✅ |
| S3 | Undo a middle step, keep later steps | ✅ | ✅ manual¹ | — not supported | ✅ `git revert` |
| S4 | A restore never destroys your unrecorded work | ✅ | ◐ partial² | ❌ new file lost | ✅ |
| S5 | Damaged storage is detected, not restored | ✅ | ✅ | ✅ | ✅ |
| S6 | History survives the agent deleting the project's hidden folders | ✅ | ✅ | ❌ history lost³ | ✅ |
| S7 | Recover from `git clean -fdx` | ✅ work file · ❌ `.env`⁴ | ✅ both | ❌ both lost³ | ✅ work file · ❌ `.env` |
| S8 | Find the step that broke the tests | ✅ automatic | — not supported | — not supported | ✅ `git bisect`⁵ |
| S9 | Stop `rm -rf` / `git clean` before they run | ✅ asks first | — not supported | — not supported | — not supported |
| | **Passed** | **9 of 9** (`.env` excepted) | 6 + 1 partial | 3 | 8 |

1. Turnback has no single-step undo. The closest is restoring a turn's start for paths you name
   (`restore <turn> --path …`); it worked here because no later step touched the same file, but it
   has no conflict check and would silently overwrite later work on the same file.
2. Turnback snapshots ignored files, so the restore overwrote the `.env` the person had edited
   after the agent's step. `turnback redo` brought it back, together with the person's new file.
3. agent-safepoint keeps its history in `.safepoint/` inside the project; deleting hidden folders
   or running `git clean -fdx` deletes the history with it.
4. Fabric deliberately never stores `.env`, keys or certificates (no secrets in its store), so it
   cannot bring them back. Turnback can, because it stores ignored files under 5 MB. A real
   trade-off: Fabric's guard asks before `git clean -fdx` runs at all (S9).
5. Works only because this baseline committed after every step.

## Speed (3,000 files of ~4 KB)

| | Fabric | Turnback | agent-safepoint | git |
|---|---|---|---|---|
| First checkpoint | 6.9 s | **2.3 s** | 14.4 s | not measured⁶ |
| Checkpoint, nothing changed | **0.18 s** | 0.29 s | 0.35 s | 0.10 s |
| Checkpoint after one file changed (each agent step) | **0.11 s** | 0.39 s | 0.37 s | 0.12 s |
| Storage after 12 checkpoints | 2.0 MB | **0.7 MB** | 22.2 MB | 0.7 MB |

6. The runner committed the files before timing, so git's first-commit cost was not measured.

Fabric's first checkpoint is slower than Turnback's because Fabric fsyncs every stored object for
power-loss safety (see [BENCHMARKS.md](BENCHMARKS.md)); git-based storage (Turnback, git) is also
smaller thanks to delta compression in packs.

## What this shows — and does not

**Shows:**
- Fabric is the only tool that passed every recovery scenario **and** prevents destructive
  commands, and the only one that finds the breaking step without the agent committing every step.
- Fabric is fastest per agent step among the dedicated tools.
- **Turnback is a strong, real competitor:** it matched Fabric on most recovery scenarios and beat
  it on first-checkpoint speed, storage size and recovering ignored files like `.env`.
- agent-safepoint's in-project storage is a structural weakness.
- **git with perfect discipline is nearly as capable** — the problem it leaves is that nobody
  commits after every agent step; Turnback and Fabric automate exactly that.

**Does not show:**
- Real-world use: the scenarios were designed by us, the steps are simulated, and there is one
  machine (Windows) and one small project.
- macOS/Linux speed, very large repositories, or long-running sessions.
- Anything about demand: being the most capable tool does not mean developers will install it.

## Where Fabric should improve, from this benchmark

1. **First-checkpoint speed:** 3× slower than Turnback. Options: batch durability, or fsync only
   at the end of a capture with a recovery check.
2. **Storage size:** 3× larger than git-based tools; delta compression for similar files.
3. **Ignored files:** consider opt-in backup of ignored non-secret files (Turnback-style, size
   capped) while still never storing secrets.
