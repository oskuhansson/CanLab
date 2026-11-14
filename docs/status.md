# Where things stand

A handover note, written at the end of a long session so the next one starts from
evidence rather than from memory. Every claim here was verified by running
something; where something is not verified, it says so.

## The plan

Everything in `docs/roadmap-v2-plan.md` through **W3** is written and green, plus
**W14** (the CanSat light transport) and the snapshot-portability test that
`docs/lessons.md` had been asking for. The table below is what landed in the
last sessions, each row with the evidence behind it; the roadmap's own rows
carry the same claims in more detail.

| Item | Where | What it does |
|---|---|---|
| W2.1 + leftover | `crates/canlab-solver/src/solutions.rs`, `json.rs` | The answer type with `Union`, `Approximation` and `Rejected`; every kind round-trips through JSON, unions are depth-bounded, a residual must be finite. |
| W2.4 rejected roots | `solutions.rs`, `native.rs` | `log₂x + log₂(x−2) = 3` answers `{4}` **and** names `−2` as rejected with the reason. `to_latex` prints the set; `rejections_latex` prints the working. |
| W2.8 nonlinear systems | `crates/canlab-solver/src/nonlinear.rs` | `SystemAnswer { None, All, Points }` and `solve_nonlinear`: circle ∩ line, circle ∩ circle (`±√15/2`), circle ∩ parabola, `None` for a miss, a decline for three unknowns. Every point checked against all equations. The panel renders one line per point. |
| W3.1 definite integrals | `crates/canlab-parse/src/integral.rs` | `\int_0^2 3x^2 dx = 8` exactly, `e^{-x²}` numerically, a pole refused. The field holds a finished calculus statement as one `Node::Raw` and hands it to the parser, so the two corpus rows pass. |
| W3.2 limits | `crates/canlab-parse/src/limit.rs` | `sin x/x → 1`, `(1−cos x)/x² → ½`, `0/0` by cancellation, `∞/∞` by degree, one-sided `∓∞`, a jump reported as "does not exist" with both sides. |
| W3.3 tangents and extrema | `crates/canlab-parse/src/calculus.rs`, `canlab sign` | `tangent`, `normal`, `extrema` including endpoints, `sign_table` with a touching zero of `f'` found, and the table shaded in the SVG. |
| W3.4 volumes and areas | `crates/canlab-parse/src/figure.rs` | `\vol` (discs and shells) and `\area` split at crossings: a sphere gives `32π/3`, a cone `2π/3`, swapping the curves does not change the area. |
| W7.5 the GPU probe | `crates/canlab-gpu` | The only crate that depends on a graphics stack. The probe reports `Vulkan llvmpipe (Cpu)` with `SHADER_F64`, and a shader's `f64` matches the CPU to `0e0`. `run` executes a compiled kernel per input row, which makes W7.2's differential test real: 512 points, worst relative `4.87·10⁻⁶`. |
| W7.2 at the roadmap's size | `crates/canlab-gpu/tests/sweep.rs` | 193 generated programs at 10³ points each — 193 000 points, 0 disagreements, 5.1 s. The tolerance is `1e-5` relative **or** absolute; the roadmap's `1e-6` relative is not reachable on cancellation-heavy programs and the measured bound is stated. |
| W7.4 on the adapter | `crates/canlab-gpu` | The photon kernel runs on Vulkan llvmpipe from the same source the interpreter runs: 15 photons, worst relative difference `0e0`, random stream included. The generator is a `u32` xorshift because WGSL has no `u64`. |
| W7.4 light kernel | `crates/canlab-optics/src/kernel.rs` | The photon kernel in the Lua subset: Fresnel, total internal reflection, a per-photon random stream, path length out. `compile` emits WGSL from the same text `interpret` runs, so one source has two executions. `f32` vs `f64` agree to `1e-6`; the kernel and a Rust reference agree to `1e-12`. **Nothing executes WGSL**, and the throughput figure is unmeasured. |
| W3.3 sign table in the GUI | `crates/canlab-gui/src/panels.rs` | The table now appears under the result, in the CLI's own words, computed where the curves are built and over the plot's own window. Three tests. |
| W8.5 picking and chrome | `crates/canlab-scene/src/pick.rs`, `panels.rs` | Ray picking by Möller–Trumbore on the CPU, and axes, grid and tick labels in the 3D mode. A sphere pick is on the surface and within one pixel. Hover picking names the point under the pointer. Zoom by scrolling and pan by right-dragging. A fit control returns to the framing. The ODE path is drawn by `canlab ode` **and in the panel's 3D mode**, from `y' = …` lines the field now holds whole. **No vector arrows.** |
| W8.4 the 3D viewport | `crates/canlab-gui/src/panels.rs` | The plot panel's 3D mode: the surface in the same canvas, orbited by dragging, in `egui` because Bevy does not fit here. One culling rule shared with `canlab surface`. **No lighting, no axes, no depth buffer.** |
| W8.4 surface as SVG | `crates/canlab-scene/src/svg.rs`, `canlab surface` | A surface written as an SVG with back-face culling and the painter's algorithm. A shallow dish renders as a dish. **Not a viewport:** no orbit, no zoom, no lighting. |
| W8.3 closed | `crates/canlab-scene/src/implicit.rs` | The surface net closes: a quad's four cells all have `c[axis] == corner[axis]`, and the first version moved the holder up one. A unit sphere went from 270 boundary edges and `V − E + F = −19` to **0 and 2**. |
| W8.3 surface nets | `crates/canlab-scene/src/implicit.rs` | `mesh_implicit` for `f(x,y,z) = 0`, with analytic normals from the gradient. A sphere's vertices are within one cell of the surface; an empty region is an empty mesh, not an error. **Not closed where the surface meets the grid boundary** — stated, not hidden. |
| W8.1 the scene crate | `crates/canlab-scene` | A pure orbit camera, perspective projection, frustum test, `fit` and a painter's-algorithm sort — no GPU, no window, no dependencies. A point behind the camera has no projection rather than a mirrored one. |
| W7.3 compile plumbing | `crates/canlab-solver/src/kernel.rs`, `canlab compile` | `compile_expression`, `compile_lua`, `Artefact`, `Refusal` (naming the stage), an in-memory `KernelCache` whose key is a digest **and** the exact source, and a 200-program `sweep`. `canlab compile` prints the Lua and the WGSL; `canlab compile-sweep` reports the numbers. **Nothing executes WGSL or Lua**, and the sweep says so on its own output. |
| W7.2 Lua subset → WGSL | `crates/canlab-core/src/wgsl.rs` | A tokenizer, a parser for the subset and a WGSL emitter. Seven constructs outside the subset are refused with the construct named and the line. **The differential test is not run** — it needs the W7.3 plumbing to execute a kernel. |
| W7.1 Expr → Lua | `crates/canlab-core/src/lua.rs` | The printer, with named refusals for a `Hold`, an unsupported function and a free symbol. The emitted text agrees with `Program::eval` to `1e-10` through an independent evaluator in the test. **`mlua` is not available** (no network, no vendored Lua), so the roadmap's `mlua` check is unrun. |
| W15.2 dimension checks | `crates/canlab-core/src/dimension.rs` | `dimensions`, `check_equality`, `unit_of` and `solve_with_units`. `N = kg·m/s²` is consistent, `3 m + 2 s` is refused naming both dimensions, and a solved velocity carries `m/s`. A unit is a symbol whose name parses as one, which is stated. |
| W15.3 significant figures | `crates/canlab-core/src/sigfig.rs` | `Precision` and `Measured` with the school rules: a product takes the fewer figures, a sum the fewer decimal places, an exact count limits nothing. `1.2 × 3.45 = 4.1`, `1.2 + 3.45 = 4.7`. Rounding is half-up because that is what an exam marks. |
| W15.1 units | `crates/canlab-core/src/quantity.rs` | `Unit` (exponents plus a scale) and `Quantity` with unit algebra, SI prefixes and conversion. `kg·m/s²` reduces to `N`; `m + s` is refused naming both dimensions; `1 km + 500 m = 1.5 km` exactly. |
| W6.4 physics scenarios | `crates/canlab-ode/tests/scenarios.rs` | Air (drag against `tanh`), sound (speed of sound and Doppler), water (wave equation against d'Alembert and its energy). A companion test runs the wrong drag sign and asserts the check fails. |
| W6.3 ODE plotting | `crates/canlab-ode/src/plot.rs` | `trace_curve`, `phase_curve` and `component_curve` as `canlab_plot::Curve`. A blow-up is split into segments by a median-step threshold; a non-finite point ends a segment. |
| W6.2 ODE front end | `crates/canlab-ode/src/system.rs` | `OdeSystem`, `rk4_system`, `dopri_system` and `parse_system` for `y' = …` / `y(0) = …` lines. `y' = y` reproduces `eᵗ`, three unknowns trace a circle, and `y' = 1 + y'` is refused. The CLI and GUI do not call it yet. |
| W6.1 ODE crate | `crates/canlab-ode` | Euler, RK4, Dormand–Prince 5(4) with dense Hermite output, symplectic Euler, velocity Verlet, `Trace` and `first_event`. Orders measured, not claimed; the oscillator's energy is conserved to `1e-6` with Verlet. |
| W5.3 statistics | `crates/canlab-num/src/stats.rs` | `mean`, `median`, both variances, both standard deviations, and `least_squares` with its correlation. `y = 2x + 1` is recovered exactly; the one-pass variance agrees with the two-pass form on exact data. |
| W5.2 distributions | `crates/canlab-num/src/distribution.rs` | `erf`, `normal_cdf`, `normal_quantile`, and a `Binomial` with pmf/cdf/moments/quantile. `Φ(0) = 0.5` exactly, `Φ⁻¹(0.975) = 1.96`, the binomial sums to 1, and `P(X ≤ k) ≥ 0.9` for `n = 20, p = ½` is `k = 13`. |
| W5.1 combinatorics | `crates/canlab-solver/src/combinatorics.rs` | `factorial`, `binomial`, `permutations` exact through `33!`; Pascal's identity and the symmetry checked; `C(60,30)` exact by the product formula. |
| W4.1 the value type | `crates/canlab-solver/src/value.rs` | `Value { Scalar, Vector, Matrix, Complex }` with arithmetic, LaTeX printing and a parser path for `[[1,2],[3,4]]` and `(1,2,3)`. Every kind round-trips through its own LaTeX; broadcasting is refused. |
| W4.2 linear algebra | `crates/canlab-solver/src/linalg.rs` | `determinant`, `rank`, `inverse`, `eigenvalues` (2×2 and 3×3, exact where exact) and `solve_linear` on the existing elimination. `A·A⁻¹ = I` exactly for 200 integer matrices. |
| W4.3 vector geometry | `crates/canlab-solver/src/geometry.rs` | `dot`, `cross`, `norm`, `angle`, `projection`, point-to-line and point-to-plane distances, and two-plane intersection. Exact throughout: `(1,2,2)` has norm `3`, `(1,0,0)` and `(1,1,0)` are at `π/4`, and `1/√2` is recognised as `π/4` rather than left as `arccos`. |
| W4.4 complex numbers | `crates/canlab-solver/src/complex.rs` | `Complex { re, im }` as exact `Expr` pairs: `(1+i)⁴ = −4`, `z·z̄ = \|z\|²`, `arg` exact on the axes and diagonals, `roots_of_unity`. The solver answers `x² + 1 = 0` with `{i, −i}` and `x² + 2x + 5 = 0` with `{−1 ± 2i}`. |
| W14 CanSat light transport | `crates/canlab-optics` | Transport, SiPM model, Cherenkov source, closed-form validation, and a picosecond time base whose histogram recovers a known delay exactly. `muon_study --waveform` prints a table, a text histogram and an SVG. |
| W16 Muon direction study | `crates/canlab-muon` | The can envelope, the mission's parameters and an audit, the four parts' jitter budget, the least-squares heading from timestamps, and the Cramér–Rao bound on it. The bound is 81° at the worst heading — the timing signal is 224 ps against 47.4 ps of jitter, and the block's four sensor faces are a poor tetrahedron (the two end sensors are 10 mm apart); the block's corners give 16° at best. A position-sensitive face gives 1.5°. Six designs compared in `design_study`; the short tracker is the only one that passes the audit, at 245 EUR, 23.4 degrees and 13.3 muons. Only the timing angle is a verified bound — the tracker angles are a closed form with no simulated hits behind them, and the table says so. `docs/muon/` is the study's documentation. |
| Snapshot portability | `crates/canlab-gui/src/snapshot.rs` | Two tests ask the font system whether every character the app prints has a glyph; the frame test found `→` missing and the Linux font paths absent. |

**What is not done**, stated rather than implied:

- **W4–W15 are built and tested, but almost none of it is wired to a user.** The
  value type, combinatorics, distributions, statistics, the ODE crate, units,
  significant figures, the Lua and WGSL printers and the 3D scene all have
  tests; the CLI and the GUI expose almost none of it. Some of it is wired:
  `canlab compile` and `canlab compile-sweep`, `crates/canlab-optics/src/bin/muon_study.rs`
  (W14), which prints the study and its waveform, and `solve_nonlinear` (W2.8),
  which the GUI reaches through its `SolveSystem` job. This line used to say
  "W4 onwards … are untouched", which contradicted the table above it — the table
  was right and the line was stale.
- **Nothing executes WGSL or Lua.** `mlua` is not a dependency (no network, no
  vendored Lua), so the W7.1 `mlua` check is unrun, and the W7.3 sweep compares
  the CPU's `f64` against an `f32` *replay* of the emitter's arithmetic. There is
  no measured CPU/GPU difference anywhere in this tree. The sweep's seed-1
  result is not a general bound: at seed 7 it reports a worst relative
  difference of `3.88·10⁻²` and two failures, and `canlab compile-sweep --seed
  7` exits 2.
- **The 3D viewport has no lighting, axes or depth buffer.** The plot panel draws
  the surface with the painter's algorithm, which is wrong for a surface that
  intersects itself.
- **Surface nets at the grid boundary are not closed.** An interior grid is a
  proper surface net; where the surface reaches the box's own face a few quads
  share one vertex between two faces, so the seam is slightly smoothed. It **is**
  closed: `V − E + F = 2` with no boundary edges, which a test checks.
- **The kernel cache is in memory only**, not under `%LOCALAPPDATA%`.
- The SiPM model has no dark counts and no per-wavelength PDE curve; both need a
  real part number before they are worth inventing.
- The nonlinear solver's resultant is built exactly but its roots are found
  numerically.


## Test counts, last run

`cargo test --offline -j 2` on the tree this note describes: **1287 passed, 0
failed, 5 ignored.** The five ignored are the snapshot frames, and they are
ignored because this machine is not the one they were rendered on (see below).

| Suite | Count |
|---|---:|
| `canlab-solver` | 216 |
| `canlab-gui` | 142 |
| `canlab-cli` | 47 |
| `canlab-core` | 117 |
| `canlab-parse` | 107 |
| `canlab-optics` | 121 |
| `canlab-input` | 160 |
| `canlab-plot` | 65 |
| `canlab-poly` | 26 |
| `canlab-ode` | 34 |
| `canlab-mathview` | 45 |
| `canlab-scene` | 34 |
| `canlab-tex` | 43 |
| `canlab-num` | 45 |
| `canlab-corpus` | 79 |
| `canlab-gpu` | 6 |
| `canlab-muon` | 59 |

Each row is `cargo test -p <crate>`, which **includes that crate's doc-tests**, so
there is no separate doc-test row: adding one would count them twice. The rows sum to
**1274**, which is what the whole-workspace run reports.

| Corpus | 15 of 15 rows |

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
test that story 10 of `docs/lessons.md` asked for now exists — two glyph-coverage
tests in `snapshot.rs` — and it found a missing `→` and the absent Linux font
paths; the pixel snapshots still skip off Windows because DejaVu's outlines are
not Segoe's, which is a difference in shape and not in coverage.

**`canlab-optics`, the start of W14.** Seven modules, 112 tests:

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
- `block`, `fresnel`, `transport` — the solid, Snell and Fresnel for both
  polarisations, and one photon from birth to death with absorption and flight
  time. Every one was written from a closed form and against a test; three
  sign errors and two never-exit bugs were caught that way, and the fixture in
  `can-sat/sim/muon/events.json` is what stops the C++ simulation from drifting
  from them.

**W14.1's verification, and the seven bugs it raised.** A new test file,
`crates/canlab-optics/tests/transport_validation.rs`, checks the *composition*
against numbers from outside the crate: a million sampled Fresnel splits must
conserve energy, a bounced photon's path must be longer than its chord while its
time is the chord's, transmission must bracket `e^{−d/ℓ}`, the cone's blue and
red ends must arrive picoseconds apart, and every photon of a crossing must be
accounted for exactly once. Writing it found:

1. **The vertical-muon study was measuring an empty event.** `Block::contains`
   asked for a strictly negative signed distance, so a muon whose path began on
   an end face had its whole path rejected and the event reported zero photons.
   `contains` now counts the surface (within `SURFACE_MM`); `strictly_contains`
   is the material test, and the transport is the one that wants it.
2. **A `contains` that rejected "inside by a nanometre".** The same tolerance
   made `contains` false for a point just inside a face, so
   `Transport::trace` refused the birth points of a crossing entirely.
3. **The clip collapsed an entering muon's path to nothing.** Walking the path
   forward to find its end is only right when the start is outside; a muon that
   enters the block and leaves it exited at the first sample, i.e. at once.
4. **The bottom face was at `z = +thickness/2`.** The `−z` plane's offset is
   `+half`, not `−half`. Every downward photon found no exit and was reported as
   absorbed, which is where the first study's "trapped 96 %" came from.
5. **A photon bounced on one square nanometre of the same face.** After a
   reflection the photon is nudged back inside and the next exit is the face it
   just left, at the nudge's distance. `Block::exit_where` takes a face filter,
   the transport excludes the face it just reflected off, and the reflection
   nudge is 0.01 mm (four orders above the surface tolerance) along the face
   normal and not along the new heading.
6. **A reflection at normal incidence landed outside the block.** The old nudge
   was a nanometre along the new heading; at normal incidence that heading
   points straight back out.
7. **`survival` and a constant draw of 1.0.** Absorption is `draw > exp(−d/ℓ)`,
   so a test that passes a constant 1.0 absorbs the photon at its first step.
   `NO_REFLECTION` is 1.0 and works only with `>`; the two are one decision and
   the comment says so.

**W14.3, the time base.** `crates/canlab-optics/src/timebase.rs` — `TimeBase`
(1 ps bins over a nanosecond by default, and any window that is not a `NaN`),
`Histogram` (every time counted as binned, underflow, overflow or not-a-number,
so the bin counts can be checked against the photon total), `Waveforms` (the
four sensors and their sum) and `Histogram::curve` (a `canlab_plot::Curve`, so
the waveform is drawn by the plot crate rather than by a second renderer).
`tests/waveform.rs` checks it on *simulated* photons: the bins of twenty events
sum to the photons they detected, a known delay added to the timestamps is
recovered as exactly that many bins (including a negative one, which needs a
window that starts early), and the jitter's own standard deviation is 100 ps
where the sensor says 100 ps. `muon_study --waveform` prints one event as a
table, a text histogram and an SVG.

The bin index is `floor((t − start)/w)` after rounding the offset to a
millionth, because `0.3/0.1` is `2.9999999999999996` and a time on a bin edge
would otherwise land a bin low — the bug that a delayed-waveform test exists to
find, found in the module's own first test.

The study table in `muon_study` now says what the detector does — an across
muon reaches a sensor about half the time, a vertical one less often, and the
loss is dominated by the sensors covering 15 % of their faces — where before it
said "zero photons, zero detected, 96 % trapped" at every geometry.

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
cargo test -p canlab-optics --offline           # the optics crate, <1 s
cargo run -p canlab-cli --offline -- solve-all 'x^4 - x - 1'
cargo run -p canlab-cli --offline -- corpus run # 15 of 15 rows
cargo run -p canlab-cli --offline -- eval '\int_0^2 3x^2 dx'   # 8
cargo run -p canlab-cli --offline -- sign 'x^3 - 3x^2 + 1' --xmin -1 --xmax 3
cargo run -p canlab-optics --offline --bin muon_study -- --waveform
```

The calculus statements (`\int`, `\lim`, `\vol`, `\area`) are read by
`canlab-parse`, and the field holds a *finished* one as a single `Node::Raw` and
hands it over — so the corpus goes through the same grammar the CLI does.

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

## The stash

`stash@{0}` holds the old, half-built W2.8 from a previous session. **It is
superseded**: W2.8 is written, tested and merged on its own terms in
`crates/canlab-solver/src/nonlinear.rs`, and the stash's failure modes are
answered there rather than patched:

- The constant-value filter that decided a quadratic with a negative
  discriminant: the new path never substitutes a constant into a candidate
  before the polynomial test, because the resultant is built from polynomials
  and its roots are found by `canlab-num`.
- The independent acquisition of the two coordinates: the partner coordinate is
  found by solving the *first* equation with the candidate substituted, and the
  point is then checked against **every** original equation.
- `nested_root_roots` for `Δ ≤ 0`: replaced by the quadratic formula in
  `solve_for`, which returns no partner for a negative discriminant instead of
  one.

The stash can be dropped. Nothing was un-stashed to do this.


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
