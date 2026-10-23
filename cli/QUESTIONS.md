# Go terminal interface questions

The user requested a working Go CLI UI after the web Console brief. This is a separate Go
terminal client named `cosigna-console`, calling the existing `cosigna` engine (`fabric` remains compatible) through JSON commands.
The terminal client does not replace or rewrite engine enforcement.

- Approval requests do not consistently carry normalized payment units. The inbox fixture's
  `stripe/issue_refund` is explicitly USD cents in the brief. Live display requires matching
  `payments list` metadata; unknown payment units disable approval and show exact raw input.
  The engine's built-in Edit/Write/MultiEdit/NotebookEdit/Bash requests are reviewed as local
  file or shell actions, with their complete inputs and no invented currency.
- Mandate usage uses integer micro-units and no currency. Display units, never invented dollars.
- Some blocked activity records have no normalized `payment` object. Show the recorded gateway
  refusal; do not synthesize a Cosigna payment decision from prose.
- The Dribbble page identifies the Daytona text/ASCII launcher design, but its full-size CDN
  image was not retrievable in this session. Exact visual matching needs an accessible image.
  Build an original Cosigna ASCII identity and dark launcher from that reference; implement
  the torus renderer from the linked mathematical explanation, with source attribution.

Animation belongs to the idle welcome screen. It is paused while typing, reviewing or
confirming a decision, and `--no-animation` disables it. No animation represents fake payment
progress or approval completion.

The product is now Cosigna. The Go terminal binary is `cosigna-console.exe` so it does not
shadow the installed Rust `cosigna.exe` engine. The engine is called as `cosigna` by default;
`--engine` can still select the compatible `fabric` executable. Existing JSON schema names,
Go module paths, fixture filenames and API configuration remain unchanged in this stage.

Developer-session direction: use a command prompt, completion, in-memory history, project
context, lightweight event following, and checkpoint/rule/connection views. Existing finance
views remain reachable through slash commands. The four new workspace fixture files are
explicitly synthetic examples, alongside the original Northwind captures.

Open product decisions (not implemented by assumption):
- Should the terminal eventually accept natural-language prompts and delegate them to a
  coding agent, or remain an engine companion? No provider, chat model, arbitrary shell
  execution or multi-agent orchestration was added for the UI resemblance request.
- Should `cosigna ui` dispatch to the Go executable in a later engine packaging step?
  The separate binary avoids replacing the already installed Rust command.
- Should restore/revert and change review be exposed here? This increment keeps them as
  explicit engine commands; browsing checkpoints and authorized changes is read-only.

Connection limitations are shown in the interface: Claude Code uses project hooks and an
MCP registration attempt; Codex uses MCP and agent-driven checkpoints; Cursor has a manual
MCP guide, with no claim of automatic interception or completed integration.
