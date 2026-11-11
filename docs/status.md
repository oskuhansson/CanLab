# Where things stand

A handover note, written at the end of a long session so the next one starts from
evidence rather than from memory. Every claim here was verified by running
something; where something is not verified, it says so.

## The plan

Ticked in `docs/roadmap-v2-plan.md`: **W0** (Bevy stack, headless snapshots,
evaluator perf, bench, corpus runner), **W2.1** (the answer type, including
`Union` and `Approximation`), **W2.1b** (every kind renders in the panel),
**W2.2–W2.7**.

Open, with their design decisions already written down:

| Item | Note | Next step |
|---|---|---|
| W2.8 nonlinear systems | `docs/w2.8-nonlinear.md` | Implement `SystemAnswer { None, All, Points }`, `solve_nonlinear` defaulting to `NotImplemented`; circle ∩ line and circle ∩ circle first, each point substituted back into **all** equations. |
| W3 calculus | plan row | The two failing corpus rows (`2024-kevat-N-1-2`, `2025-kevat-M-1-5`) are definite integrals waiting for this. They fail honestly. |
| W2.1 leftover | `json.rs` | The union JSON decoder is deliberately absent; encoding without decoding is documented. |
| Snapshot portability | `docs/lessons.md` lesson 10 | A test that no rendered glyph is tofu. Needs egui 0.36's font API read first: the check is the loaded fonts, not the string. |

## Test counts, last run

| Suite | Count |
|---|---|
| `canlab-solver` | 145 |
| `canlab-gui` | 125 |
| `canlab-cli` | 27 (5 unit + 12 cli + 10 corpus) |
| `canlab-core` / `input` / `num` | 66 / 156 / 31 |
| Corpus | 13 / 15 — the two failures are the integrals above |

`scripts/check-text.ps1`: 150 files, no control characters, every `.ps1` ASCII.

## The build server

`ssh ai` works (tailnet `100.88.227.20`, port 2220, key `~/.ssh/id_canlab`, comment
`canlab-agent`; revoke with `sed -i.bak '/canlab-agent/d' ~/.ssh/authorized_keys`).
`~/.ssh/config` keeps the old public hostname in a comment for the record.

| Capability | Measured |
|---|---|
| Test suites at `-j 8` | 136 solver + 31 num, matching local |
| Windows cross-build | **3m 23s** cold, **5.7 s** incremental; exe to `target\remote\` |
| Headless snapshots | renders, but **no symbol font** — frames are full of tofu |

`docs/remote.md` records the six bugs that running the script exposed, and the
two that remain: a `rust-version` preflight, and printing a failed command's log
before throwing. The PATH is fixed explicitly (absolute, from `id -un`) because
the server's `$HOME` arrives wrong through the SSH command.

## The loop to work in

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; . .\scripts\env.ps1
cargo test -p canlab-solver -p canlab-gui --offline -j 1      # edits, local
.\scripts\remote.ps1 test -Package canlab-solver              # -j 8, server
.\scripts\remote.ps1 build                                    # Windows exe, ~6 s warm
```

The window: `target\remote\canlab-gui.exe` is built from the last commit that
touched code; later commits have been `docs/` and `scripts/` only.

## What this session learned, in one place

Four bugs shared one shape: **structure carried meaning and a reader fell through
to its default** — `5x²` read as `(5x)²`, `|x−1|` read as `x−1`, `x²+y²=a` read as
`a = x²+y²`, and equations from different systems paired as if they were
simultaneous. Each was fixed with a test on **both** sides of the rule, and each
is written up in `docs/lessons.md` with the reason it survived.

Two more were about tests proving the implementation rather than searching for the
failure: a GUI test that asserted `app.solve` while the screen echoed the question,
and a snapshot suite that passes on a machine with no symbol font.

## The one thing waiting on a person

`git stash list` holds `stash@{0}`, an LLM-agent panel built without being asked:
512 lines, a 14-command protocol that maps onto real app features, a sixth panel,
and a layout restructure. The protocol is worth keeping; the layout change should
be a decision, not a side effect. It is stashed, not deleted.
