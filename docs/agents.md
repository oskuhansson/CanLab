# Who owns what

This project uses a small, **standing** set of specialist agents, not a new agent
per task. Each one owns a crate, keeps its own context, and is handed the next
job in that crate by id rather than being re-briefed from scratch. The point is
that the brief below — the invariants, the build commands, the tests — is loaded
once, and the agent that wrote a crate is the agent that changes it next.

An agent id is per session. The roster is durable; the ids are not. At the start
of a session, spawn one agent per owner from the briefs below and then reuse it.

## The roster

| Owner | Crates | Standing brief |
|---|---|---|
| **Core and input** | `canlab-core`, `canlab-input`, `canlab-parse`, `canlab-tex` | The expression tree stays canonical; raw constructors never normalize. The field's caret is a path into the tree, and every refusal has a reason. Definitions are document-wide and resolved in dependency order. |
| **Solver** | `canlab-solver`, `canlab-cli` | `SolverError::NotImplemented` is a normal answer, never an error. Exact rationals; a system with three answers (point, contradiction, family) keeps all three. The SymPy worker is optional and its absence is not a failure. |
| **Plot** | `canlab-plot`, `canlab-mathview` | A curve is a list of segments, and a break means "not connected here". Nothing emits a non-finite coordinate. Layout is pure and testable with a fake font. |
| **GUI** | `canlab-gui` | Panels are not boxes; controls live on the panel whose answer they produce. Palette colours come from the theme, never from a literal in a widget. A frame must never block on a solver or a file. |
| **Reviewer** | read-only, all crates | Reads the diff, builds it, runs it, and reports findings by severity with a triggering input. Never edits. |
| **Docs** | `docs/`, `README.md` | Plain English, specific, honest about cost and about what has not run. No marketing adjectives. `docs/acceptance.md` is generated, not written. |

## Conventions every owner follows

- Build with `-j 1`: `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"; cargo test -p <crate> --offline -j 1`.
- If the shell runs inside the file sandbox, load the env script first —
  `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; . .\scripts\env.ps1` —
  so `TMP`, `TEMPORARY` and the session file stay inside `target/`. Without it a
  test that writes to `std::env::temp_dir()` fails with "Access is denied" for a
  reason that has nothing to do with the code. See
  [lessons.md](lessons.md).
- `cargo clean -p windows -p pxfm -p icu_normalizer` if a build runs out of memory.
- Kill `canlab-gui.exe` before rebuilding it; Windows holds the executable.
- Tests are the deliverable, not an extra: a behaviour change without one is not
  finished.
- Do not touch a crate you do not own. Ask its owner, or the GUI owner for a
  cross-crate wiring change.
- Comments say why, in the project's voice, and must not claim more than the code
  does — a stale comment is a bug that the reviewer will file.
- **Every guard gets a refusal test.** If the code can decide not to draw, not to
  solve, or not to evaluate, a test asserts the refusal *and* the reason it gives.
  A feature that supplies a value the user did not type must justify it
  structurally — never positionally. See [lessons.md](lessons.md) for the bug that
  put this rule here.

