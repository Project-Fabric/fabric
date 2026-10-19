# Live test: Fabric vs Claude Code `/rewind`

**Date:** 19 October 2026 · **Agents:** Claude Code 2.1.303 (headless `claude -p`), Codex CLI
0.163.0 (`codex exec`) · **Machine:** Windows 11 · **Fabric:** release build of `0946f5c`

An isolated Python project (cart totals with a unit test suite, in a git repository) was set
up, `fabric init` and `fabric integrate claude-code` were run, and real agents were given real
prompts. `/rewind` is interactive-only, so it is compared using its documented behaviour
([code.claude.com/docs/en/checkpointing](https://code.claude.com/docs/en/checkpointing)).

## Results

| # | Situation | `/rewind` (documented) | Fabric (observed) |
|---|---|---|---|
| 1 | One prompt, three edits (add `with_tax`, change discount semantics, write README); the discount change breaks a test | Checkpoints are per prompt: restoring code undoes **all three** edits | Hooks recorded the prompt and each edit as separate steps. `fabric bisect` found the breaking edit (#4) in 4 test runs; `fabric revert 4` undid only it. Tests pass; `with_tax` and README kept |
| 2 | Claude renames a test file with `git mv` and creates `CHANGELOG.md` with `python -c` (Bash only) | "Checkpointing does not track files modified by Bash commands … cannot be undone through rewind" | Recorded as one step, with the rename detected. `fabric revert 8` restored the original file and removed the new one; later work kept; tests pass |
| 3 | Claude is told to `rm -rf scratch` | No built-in protection (user-written permission rules only) | The guard returned `ask`; in headless mode the command was declined. Claude explained, stated it had not tried to work around it, and left the folder intact. Logged in `fabric guard log` |
| 4 | Codex is asked what Claude changed in `cart.py`, which change broke tests and how it was fixed | Not possible: checkpoints are private to the Claude session | Codex called `fabric_timeline`, `fabric_file_history` and `fabric_step` (6 calls) and correctly reported the tax step, the breaking step (#4), and its revert (#6), with no logs pasted |

## Bugs found and fixed during the test

- **Directories holding ignored files blocked restore** (`0946f5c`). `.claude/` (created by
  `fabric integrate`, holding only an ignored local settings file) made `bisect` stop with a
  conflict when going back to the first checkpoint. Such directories are now kept, with only
  their recorded contents removed.

## Limits found (now on the board)

- **Git's own state is not undone.** After reverting the `git mv` step the files were right,
  but git's staging area still showed the rename. Fabric restores files, not `.git`; it should
  at least warn when an undone step ran git commands. (`/rewind` does not undo this either.)
- **File-level undo does not understand meaning.** Codex noticed that after reverting the
  discount change, the README written in a later step still described dollar discounts.
  Fabric could point out other steps from the same prompt when reverting one.
- **No test run after a revert.** Codex observed there was no recorded test result after the
  revert; `fabric revert --test CMD` would confirm the fix.
- **Interactive approval prompt not yet seen.** The guard was verified in headless mode
  (declined); the approval dialog in an interactive session still needs to be checked.

## Conclusion

On the four situations tested, Fabric did what `/rewind` cannot: undo one step inside a
multi-edit prompt, undo changes made through shell commands, ask before a destructive
command, and let a second agent read what the first one did.
