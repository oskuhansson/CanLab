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
| W2.8 nonlinear systems | `docs/w2.8-nonlinear.md`, and the code in `stash@{0}` | **Half-built and stashed, not merged.** `SystemAnswer { None, All, Points, Family }` and `solve_nonlinear` exist and type-check; the search finds the right resultants for circle ∩ line and circle ∩ circle, and five of its eleven tests still fail. Do not un-stash expecting green. |
| W3 calculus | plan row | The two failing corpus rows (`2024-kevat-N-1-2`, `2025-kevat-M-1-5`) are definite integrals waiting for this. They fail honestly. |
| W2.1 leftover | `json.rs` | The union JSON decoder is deliberately absent; encoding without decoding is documented. |
| Snapshot portability | `docs/lessons.md` lesson 10 | A test that no rendered glyph is tofu. The gate in `snapshot.rs` is the evidence that it is needed; the test itself is still unwritten. |
| **W14 CanSat light transport** | plan section W14, and below | `canlab-optics` is the crate: rays, intervals, Cherenkov, the tent outline. The Monte Carlo that drives them is not written. |

## Test counts, last run

`cargo test --offline -j 2` on the committed tree: **920 passed, 0 failed, 5
ignored.** The five ignored are the snapshot frames, and they are ignored
because this machine is not the one they were rendered on (see below).

| Suite | Count |
|---|---|
| `canlab-solver` | 145 |
| `canlab-gui` | 120 + 5 ignored |
| `canlab-cli` | 5 + 13 + 10 |
| `canlab-optics` (new) | 22 + 14 + 9 |
| `canlab-poly` | 16 + 10 |
| Corpus | 10 of 10 rows the runner selects |

## What changed this session, and why

**Six failing tests on a clean checkout.** Two corpus tests asserted that
`x^3 = 2` declines; `0bbe5bc` ("Every numeric root") had made the native backend
answer it numerically, so they failed for a reason unrelated to the rule they
tested. They now drive a stub backend for the rule, and a new test pins the real
one: a numeric answer to an `exact` row must fail *with the number shown*.

**A quartic kept four roots of five.** `higher_degree_roots` returned the
rational roots and *claimed success* whether or not it had solved the leftover
factor, so `(x−1)(x+1)(x−2)(x³−2)` answered `{±1, ±2}` for a five-root equation.
It now reports whether the leftover was finished, and both `solve` and
`solve_exactly` decline instead of returning a partial list. Two tests: one on
the count (four roots from the exact path), one on the refusal (a list missing a
real root is worse than a decline, because a decline can be retried).

**The CLI's `≈`.** `canlab solve-all 'x^4 - x - 1'` printed
`-0.7244… \cup 1.2207…`, which reads exactly like an exact answer.
`Solutions::Approximation` carries a residual precisely so a caller can say
otherwise; the CLI now prints one `x ≈ value (f(x) = …)` line per approximation,
which is the difference between a correct number and an honest one.

**Five snapshot tests skipped off Windows, with the measurement that justifies
it.** The committed reference frames were rendered on Windows; on this machine
the frames are pixel-identical *except* in the glyphs Windows has and DejaVu does
not (`θ`, `∅`, `≠`, `≅`, `∥`, `⋯`). `snapshot.rs` records that, the tests carry
`#[cfg_attr(not(windows), ignore = ...)]`, and `scripts/remote.ps1 snapshots`
passes `--include-ignored` so the Linux build server still renders them. The
test that would let them run everywhere is story 10 of `docs/lessons.md`.

**`canlab-optics`, the start of W14.** Four modules, 44 tests:

- `vec3` — reflection, plane intersection, unit directions, and the
  starting-plane tolerance that stops a bouncing photon hitting its own wall
  forever. No trigonometry in the inner loop.
- `interval` — outward-rounded bounds on the bit pattern, so "the index is
  1.84 ± 0.01" yields a guarantee rather than a difference of two floats. Checked
  against a million random interior points, not against its own implementation.
- `cherenkov` — the emission law as both a float and the parsed LaTeX the CAS
  evaluates, with a test that the two agree at five `(n, β)`; Sellmeier
  dispersion; the `1/λ²` weighted mean wavelength; photons per millimetre.
  The first coefficient set was 7 % high at 400 nm and its own validation test
  found it.
- `outline` — the block as a prism over a tent: an exact signed distance for the
  convex polygon, an outward normal for the sloping faces, and the tropical
  polynomial `min(y + |x|, 3) − 1.5` whose facets are the roof and the shoulders.
  A test ties the drawing's six corners to the polynomial's switches.

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

On Linux, which is where this session ran:

```sh
cargo test --offline -j 2                       # whole workspace, ~80 s
cargo test -p canlab-optics --offline           # the new crate, <1 s
cargo run -p canlab-cli --offline -- solve-all 'x^4 - x - 1'
```

On Windows, the project's original loop still applies:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; . .\scripts\env.ps1
cargo test -p canlab-solver -p canlab-gui --offline -j 1
.\scripts\remote.ps1 test -Package canlab-solver
.\scripts\remote.ps1 build
```

## What this session learned, in one place

Four bugs shared one shape: **structure carried meaning and a reader fell through
to its default** — `5x²` read as `(5x)²`, `|x−1|` read as `x−1`, `x²+y²=a` read as
`a = x²+y²`, and equations from different systems paired as if they were
simultaneous.

This session added five more of one *different* shape: **a function returned
something that looked like its answer when it had only part of one.** The quartic
kept four roots and said "solved". The reduction's Sylvester rows were mirrored,
so `30y² − 191` became `30 − 191y²` — a wrong polynomial, correctly solved. The
signed distance had its normal pointing inward, so the centre of the block was a
positive distance outside. `biquadratic_roots` returned "no real roots" for a
discriminant that meant "roots on the real axis are real". And the Sellmeier
coefficients were three numbers that happened to be typed.

The rule that came out of it is the one to carry forward: **a partial answer must
be distinguishable from a complete one at the type level, or a test must count.**
Every one of those five was found by a test that asked for a *number of answers*,
a *sign*, or a *value from outside the crate* — never by one that asked "did it
return something".

And a rule about this session's own habits: three of those fixes were first
attempted as a stub with a `// fill in` comment, compiled, and left in place.
A stub that returns `None` is indistinguishable from a correct refusal to every
caller, so it hides until the tests that matter are written. Write the refusal
test first.

## The one thing waiting on a person

Two stashes, both deliberately left:

- `stash@{0}` — W2.8 nonlinear systems, mid-flight: the types, the search, the
  resultant construction and the summarised failure modes (see below). It is not
  green and should not be merged until it is.
- A second stash, from the previous session, holds an LLM-agent panel built
  without being asked: 512 lines, a 14-command protocol, a sixth panel and a
  layout restructure. The protocol is worth keeping; the layout change should be
  a decision, not a side effect.

## W2.8's failure modes, for whoever picks it up

The search (`nonlinear.rs` in the stash) is architecturally sound — reducer,
branch, resultant pair, then a numeric check of every point against every
original equation — and the remaining failures are in the primitives:

1. **A substituted equation with one unknown and one *negative* discriminant is
   decided by a constant check before the numeric verifier sees it.** `x² + c`
   with `c` a float is in fact a quadratic with roots; the initial
   `constant_value` filter in `run` must not answer for it.
2. **The resultant's roots are acquired in the two coordinates independently**,
   so pairing them is `O(n²)` and needs a filter. Circle ∩ circle gives four
   candidate pairs of which two are real; the check should keep two.
3. `biquadratic_roots` and `complete_the_square` in the stash are correct and
   worth keeping; the `nested_root_roots` path is only correct for `Δ > 0`.
