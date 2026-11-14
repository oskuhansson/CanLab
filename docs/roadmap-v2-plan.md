# Roadmap V2 — a concrete implementation plan

This is a plan for the four capability areas and the workflow in `Roadmap V2.md`,
written against the code as it stands today: nine crates, ~26 000 lines of Rust,
624 tests, 77 acceptance documents. It is
not a promise of dates. It is an order of work, with the design of each piece
decided far enough that the next commit is obvious, and with the parts I think
are wrong in the roadmap named as such.

Read [Decisions taken](#decisions-taken) first — they override the
recommendations below where they differ. Then [Comments](#5-comments-on-the-roadmap)
if you want the argument, and [Questions](#6-questions) for what is still open.
The plan itself is sections 3 and 4.

## Decisions taken

Answered on 11 November 2026, before any of this was built. Where an answer
contradicts a recommendation below, the answer wins and the text has been
rewritten to match.

| Question | Answer | What it changed |
|---|---|---|
| Priority | Exam maths first, then simulation | M1–M2 stay ahead of every simulation tier |
| Deadline | None | The tiers may slip; the gates are the only schedule |
| Exam scope | Both syllabi, short first | M2 survives, after M1 |
| Framework | **Bevy from the start, full UI** | D5 rewritten; the egui 0.36 downgrade is now unconditional; `eframe`, `shot.rs` and the `glow` feature retire |
| GPU precision | f32 by default, f64 where the adapter offers it | W7.1 keeps its probe; the single-precision marker becomes permanent UI |
| Autofill | `±` in one go when both branches exist, cycle when they do not | W10.2 rewritten |
| CanSat | Light transport in a high-index glass block, SiPM detection, picosecond time evolution | New W14; the glass item moved out of W7 and the light scenario out of W6 |
| Start | W0 now | Being implemented, beginning with the stack move (W0.0) |

The CanSat answer is the most consequential of the eight, because it describes a
**detector**, not an ODE. A block of glass with photodetectors on it, asking where
the photons went and when, is a Monte Carlo over independent photons: no coupling
between them, no time-stepping a coupled state, and therefore the ideal shape for
a GPU. That is why W14 exists as its own workstream and why W6 (the ODE crate)
moved behind it: the roadmap's water, air and sound scenarios still need an
integrator, but the CanSat case does not.

## 0. The verdict in one paragraph

The roadmap is achievable and roughly fifteen times the size of the last one by
weight. Two of its parts are cheap and should have been done already: 2D
visualisation polish (W13), and the workflow — continuous solving and autofill
(W10). Three are large but conventional: the maths for real exam problems, the
differential equations, and embedded scripting. Two more are not in the roadmap at
all and turned out to be real work: units and significant figures, because a
physics answer is `4.2 m/s` (W15), and the CanSat case itself, which is a Monte
Carlo over photons with picosecond timing rather than an ODE (W14). One is the
place where the roadmap, as written, would spend months on the wrong thing: a
compiler that turns expressions into GPU code, where interpreting the opcode
stream we already emit does the job. And Bevy was decided rather than measured;
the measurement moved inside the migration, to its first step.

The gap that matters most is not on the roadmap at all: **the CAS cannot yet do
most of an exam.** It solves linear and quadratic equations in one variable,
linear systems of any size, and integrates integer powers. Everything else — a
limit, a trig equation, a vector, a binomial coefficient, an inequality, a
decimal answer to three significant figures — it declines. That is the work.

## 1. What the roadmap asks for, and what exists

| Roadmap says | Engineering target | Today |
|---|---|---|
| Solve upper-secondary problems from AbiMix | A measured capability set, one coverage table per topic | degree ≤ 2 in one variable; linear systems; `∫xⁿ` |
| Simulate physical phenomena through time (Euler, RK4) | An ODE crate, a front end that reads `y' = …`, scenarios with reference solutions | nothing |
| Compile expressions to GPU code | Evaluate many trajectories or rays at once | a flat f64 interpreter (`Program`), CPU only |
| 2D and 3D view | A pure camera/scene crate and a viewport | 2D only: sampled functions, implicit curves, intersections |
| Python or Lua | A script cell that produces values and tables | nothing |
| Solve in the background, no buttons | Debounced re-solve with cancellation and a cache | explicit buttons, no debounce, no cache |
| Autofill `x =` with `√(1−y²)`, Tab accepts | Parametric solution + ghost text in the field | no parametric solve, no ghost text |
| Test with AbiMix problems and physical simulations | A corpus format, a batch runner, an invariant suite | 77 acceptance documents, no exam problems, no physics |
| Physics problems too (AbiMix fysik) | Quantities with units, significant figures, a detector simulation | no units, no significant figures, no light transport |
| Screenshots without a window | Headless rendering inside `cargo test` | a `glow` build, a real window, a BMP |

Two things the roadmap does not say, and should:

- **Exact or bounded, never approximate-and-unmarked.** An answer is either exact
  or numeric with a stated precision and a visible `≈`. A CAS that prints
  `2.449` where the student needs `√6` is worse than one that refuses.
- **Nothing counts until it is checked against something outside itself.** Every
  capability in section 3 carries its own verification, and section 7 says what
  that means in general.

## 2. Six decisions that shape everything else

**D1 — Capability lives in crates; the UI stays thin.** Already true; keep it.
New capability gets a crate (`canlab-poly`, `canlab-num`, `canlab-ode`,
`canlab-lua` for the compiler, `canlab-gpu` for execution, `canlab-scene`,
`canlab-script`), a library API, and tests that never open a frame. The GUI only
ever displays a `Result`.

**D2 — The answer type must grow before the solver can.** `Solver::solve`
returns `Result<Vec<Expr>>`. That cannot say "no solution", "every number",
`x = π/6 + 2kπ`, or `x > 2`, and three of those are ordinary exam answers. The
type changes once, early, and the change ripples through `native`, `sympy`'s
JSON protocol, `jobs`, the panels, the CLI and the acceptance corpus. Doing it
after autofill or 3D would mean doing it twice.

Two things were found while building the first half of it, and both belong in the
same change rather than a second one:

- **A union.** `sin x = ½` is `π/6 + 2kπ` **or** `5π/6 + 2kπ`. One `Family` is one
  arithmetic progression, and that answer is two of them. Without `Union`,
  W2.5 reopens the type.
- **A per-value approximation.** `≈ 2.45` with its own bound is what §8 and W15
  require, and there is nowhere in `Values(Vec<Expr>)` to put it. Without it,
  W2.7 reopens the type.

So the shape to build towards is `Values(Vec<Answer>)` with
`Answer { expr, approximation: Option<Approximation> }`, plus
`Union(Vec<Solutions>)`. The additive `solve_all` already shipped, so the old
`solve` keeps working while the rest lands.

**D3 — One document, several kinds of row.** An equation is not a definition is
not an initial condition is not a script. Today the document is systems of
equations plus a definition table harvested by scanning. That works until
`y'(0) = 1` and `#lua` arrive, at which point the reader has to guess. Add an
explicit row kind now (equation, definition, derivative, initial, script, data)
and let the printer decide what to brace. This is also what fixes the `cases`
brace bug and makes "answer row" possible for autofill.

**D4 — An expression is interpreted; a *program* is compiled, and only on
request.** These were being conflated in my first draft, and the distinction is
the whole design.

An expression — a row of the document — is what `Program` already evaluates: a
flat f64 stack machine, fast, tested, and the right answer for plotting, for the
solver's numeric fallback and for an ODE right-hand side. It needs no code
generation and should not get any.

A *simulation* is not an expression. Photon transport has a loop over scatter
events, a random draw per event, and a branch per interface. An expression VM
cannot express any of those, which is why "compile it" was right about the CanSat
case after all. So the pipeline is:

```
LaTeX → Expr → Lua (readable program) → { mlua on the CPU, WGSL on the GPU }
```

The Lua is not an internal artefact. It is a real deliverable: readable, editable,
runnable, and portable out of the application — the same generated file runs in a
ground-station script with no CanLab around it. It also gives a *third*
implementation of the same maths, so the compiler can be checked against the
interpreter and the interpreter against the reader (section 7).

**And the compilation is explicit — a button, a command, or a script call, never
a reaction to typing.** That is what makes the cost acceptable: compiling a shader
takes a hundred milliseconds to a couple of seconds, and someone who pressed
Compile will wait; someone typing a formula will not. It also means the artefact
can be inspected before it is trusted, and cached under its content hash so the
second compile is free.

**D5 — Bevy owns the window, the UI and the 3D view. Decided, against my
recommendation, and planned accordingly.** `eframe` is retired; the app becomes a
Bevy app whose egui frame comes from `bevy_egui`, and `CasApp::draw(&mut self, ui)`
keeps its signature because Bevy hands it the same `egui::Ui`. The consequences
are real and they are priced in W0.0:

- `bevy_egui` 0.43 pins egui **0.36**, so every crate that touches egui moves down
  one minor version, and `egui_kittest` moves with it. That downgrade is now
  unconditional, which has one good effect: the fallback (eframe 0.36 plus
  `egui_wgpu::Callback`) sits on the same egui, so the fallback costs no extra
  version churn if the Bevy build turns out to be unaffordable here.
- `eframe`, the `glow` feature and `shot.rs` disappear. Screenshots come from
  `egui_kittest`, not from reading a framebuffer.
- 3D stops being a renderer to write. Bevy supplies cameras, lights, meshes and a
  render graph, so W8 shrinks to the maths (sampling, meshing) plus a camera and a
  `RenderTarget::Image` shown in an egui panel.
- The risk is the build: ~400 crates on a machine that already builds with `-j 1`
  to stay inside its commit charge. That is being measured before anything is
  ported, and the measurement is the first thing W0.0 delivers.

**D6 — Python over the subprocess boundary; Lua in process.** The project
already has a tested line-JSON session over a child process for SymPy. Python
support is a second worker on that channel, not `pyo3`: a Python segfault must
not take the editor down, and a linked libpython pins one ABI. Lua has no such
problem and embeds vendored with `mlua`, so Lua is the in-process language and
Python is the optional one.

## 3. The work, by workstream

Effort is in working days of implementation including its tests, not including
review rounds (add ~30%). "Verify" is the test that has to exist before the item
is done.

A note on those numbers, because the last roadmap's were wrong in one direction:
it was judged at 98 hours, and shipped in about two days. The project so far is
25 000 lines and was built in a few working days. So a "day" here is priced for
someone learning the code, not for the rate this project has actually run at; it
is a weight, not a schedule. One thing does move an item's price by more than its
size: a design change mid-item. Which is why section 2 exists.

### Where the problems come from

Two subjects, four sources, and one rule about what gets committed.

| Source | What it is | How it is used |
|---|---|---|
| **AbiMix mathematics** — [svenska.yle.fi/abimix/matematik](https://svenska.yle.fi/abimix/matematik) | Every exam since 2018, short and long syllabus, with the exam board's *beskrivningar av goda svar*. The **2023–2025 papers are HTML**, not PDFs: every formula is a MathJax SVG whose `<title>` carries the original LaTeX (`\displaystyle A=\int_{1}^{4} {3x^2 \, dx}`), so transcription can be partly automated. The older papers are PDFs. | **M1's yardstick.** Extract 2023+ where the LaTeX is machine-readable; transcribe the rest by hand. |
| **AbiMix physics** — [svenska.yle.fi/abimix/fysik](https://svenska.yle.fi/abimix/fysik) | The same structure, back to **2009**; the 2025 examinations are at [yle.fi/a/7-10070812](https://yle.fi/a/7-10070812). Recent papers are HTML for the same reason. Problems carry figures, tables and graphs, and the good answers give both a formula and a number with a unit. | **M3's yardstick.** A row can be written now with `unit` in its expectation; the unit *check* waits for W15, and until then the row is reported `pass (unit unchecked)` rather than claiming a check that does not happen. |
| **OlympiadBench** — [dataset](https://huggingface.co/datasets/Hothan/OlympiadBench), [arXiv:2402.14008](https://arxiv.org/abs/2402.14008) | 8,476 olympiad-level mathematics and physics problems, bilingual, already in LaTeX, with final answers, answer type and unit columns. Apache-2.0. **Parquet only** — 18 configs, no JSON or CSV — so the fetch reads the Hugging Face datasets-server JSON API at 100 rows per config rather than pulling in a parquet reader. | stress cases, not coverage. A corpus that is all olympiad problems measures the wrong thing. |
| **UGPhysics** — [dataset](https://huggingface.co/datasets/UGPhysics/ugphysics), [arXiv:2502.00334](https://arxiv.org/abs/2502.00334) | 5,520 undergraduate physics problems across 13 subjects, JSONL, `\boxed{}` answers, a unit column, a multiple-answer flag and a difficulty level. **CC BY-NC-SA 4.0** — non-commercial and share-alike. | the bridge between school physics and the CanSat calculations. Stress-only, and never committed. |
| **SciBench** — [dataset](https://huggingface.co/datasets/xw27/scibench), [project](https://scibench-ucla.github.io/) | 692 college textbook problems, JSON, MIT licence, with `answer_latex`, `answer_number` and `unit`. Chemistry-heavy, with a physics subset. | a small, clean set of numeric answers with units. |

JEEBench, PhysUniBench and TheoremQA were not examined and may be better than
these; OpenStax's textbooks (CC-BY) are a legitimate source of problems whose
answers are printed in the book.

**The rule: the statement is not committed, the model is.** A corpus row holds
what the CAS receives — the equations, in our LaTeX — plus the expected answer and
a citation. The prose of a problem, its figures and its tables stay out of the
repository: they are copyrighted, they are not what the reader parses anyway, and
C2 says the modelling step is the student's. Third-party datasets are fetched,
not vendored: `canlab corpus fetch` downloads to `%LOCALAPPDATA%/canlab/corpus`,
and the rows in `tests/cases/` reference them by id. With no Python on this
machine the fetch is an HTTP request and a JSON parse — JSONL for UGPhysics and
SciBench, and the datasets-server JSON API for OlympiadBench's parquet — never the
`datasets` library, and the format is documented in
[corpus.md](corpus.md) so the runner and the fetch agree.


### W0 — The stack, then the instruments

Nothing after this is measurable without it, and it retires the annoyance in the
roadmap about opening a window to take a screenshot.

| # | What | Verify | Days |
|---|---|---|---|
| ✓ W0.0 | **The stack move.** Bevy 0.20.1 + `bevy_egui` 0.43.1 own the window and the event loop; egui 0.36 is the interface inside it; `eframe`, `shot.rs`, the `glow` feature and the `Renderer` choice are gone. `CasApp::draw(&mut Ui)` kept its signature, which is why the port was a boot function and a `Jobs` fix (`Receiver` is not `Sync`) rather than a rewrite. *Measured before the port:* 25.8 min cold (`-j 1`, no OOM), 2m03–2m35 incremental — Bevy's link — and **188 ms** to the first egui frame on an Intel UHD adapter, Vulkan. *After:* the window opens, the app draws, 99 GUI tests pass on 0.36. Two things the probe and the port found: `bevy_egui`'s primary context lives on a **camera** (without one every frame silently does nothing), and Windows links `bevy_dylib` with **LNK1189** unless the performance profiles are set, so `bevy/dynamic_linking` is recorded but not enabled. Dev profile is now `debug = "line-tables-only"`, because debug info was most of every link. | a window that survives a resize; `cargo test -p canlab-gui` green on 0.36; the app opens from a sandboxed shell with no approval | 5 (spent) |
| ✓ W0.1 | `egui_kittest` 0.37 with `wgpu` + `snapshot`: render a frame to a PNG inside `cargo test`, no window, no display handle. *Shipped:* `src/snapshot.rs`, three images in `tests/snapshots/`, `kittest` config, `.gitignore` entries, and `theme::install` called on the harness context so the picture matches the app's. The first three frames found **two real UI defects**: every panel menu's `▾` and then `⋯` were glyphs the bundled fonts do not have (boxes, always), and the two axis combo boxes had no accessible name, which kittest refuses to snapshot. Both fixed; a symbol-font fallback was added to the theme. **Follow-up:** two tests now ask the font system whether every character the app prints has a glyph (`every_character_the_app_can_print_has_a_glyph`, `the_rendered_frame_has_no_missing_glyph`), and they found `→` and the missing Linux font paths. The five pixel snapshots still compare only on Windows, for shape rather than coverage. | a snapshot fails when the accent colour changes by 10%, passes otherwise; a second run compares and passes | 3 (spent) |
| W0.2 | Evaluator: `Program::eval` allocates a `Vec` per call. Add `max_stack` at compile time, `eval_into(&self, args, &mut [f64])`, keep `eval` as a wrapper, and `eval_batch(&self, rows: &[f64], stride: usize, out: &mut [f64])`. | identical results to today on 10⁴ random expressions; allocation count zero in the hot loop (a counting allocator in the test) | 2 |
| W0.3 | `canlab bench`: time the sampler at 240×240, `Program::eval` at 10⁶ calls, RK4 steps/s once W6 lands, GPU rays/s once W7 lands. Writes `docs/bench.md`. | the file regenerates, and a 20% regression is visible in a diff | 2 |
| W0.4 | Corpus runner: `canlab corpus run [--tag X] [--batch N] [--update]`, reading `tests/cases/**/*.toml`, each batch in a subprocess so one crash or one leak cannot take the run down, writing `docs/abimix-coverage.md` and exiting non-zero on a failure. The row format, the exit codes, `--tag`'s prefix matching and the meaning of `--update` (re-check each cited dataset upstream, never rewrite an expectation) are specified in [corpus.md](corpus.md) and are implemented against it. | a deliberately wrong expected value fails the run and the report says which file | 3 |

### W1 — Algebra

The layer everything in W2 and W3 stands on. `canlab-core` today has
`normalize` — flatten, fold constants, collect like terms, sort — and nothing
else: no expand, no factor, no identities.

| # | What | Verify | Days |
|---|---|---|---|
| ✓ W1.1 | New crate `canlab-poly`: sparse multivariate polynomials, `Poly<C>` generic over the coefficient type from the start — `Number` today, `Expr` for parametric solving. *Shipped:* `Monomial`, `Poly`, the `Coeff` trait, `add`/`sub`/`mul`/`pow`/`scale`/`evaluate`/`derivative`, `from_expr`/`to_expr`, univariate `div_rem`/`gcd`/`resultant`, and `Poly<Number>::rational_roots`. 13 tests, among them 200 randomised products against a slow reference and a refusal for every non-polynomial input. Two bugs the tests caught in the first hour: the Sylvester matrix's second block was shifted from the wrong column (a determinant, but not the resultant), and a gcd test's *expectation* was wrong while the code was right. | Kronecker-style identities: `gcd(p·q, p·r)` divides `p`; `div_rem` reconstructs; 200 random cases against a slow reference | 4 |
| ✓ W1.2 | Use it: `expand`, `factor`, `collect`, `cancel` and polynomial division, as functions on `Expr` in `canlab-poly` rather than in `canlab-core` — the dependency runs one way and core stays ignorant of polynomials. *Shipped:* `expand` (polynomials through the poly layer, anything else distributed structurally), `collect` (other symbols become coefficients, which is what `Poly<Expr>` is for), `divide`, `cancel`, and a partial `factor` that divides out rational roots and refuses with `NoFactors` rather than handing the polynomial back. 10 tests, among them `expand(factor(p)) == p` and three refusals. | for random `p`, `expand(factor(p)) == p`; `factor` never returns a product that fails to multiply back | 3 |
| ✓ W1.3 | `rewrite.rs`: a small term-rewriting engine — ordered rules, a cost function, a step budget so it always terminates — and the rule sets for `ln`, `exp`, `sqrt`, `abs`, trig. Not an e-graph. *Shipped:* one rule is one `fn(&Expr) -> Option<Expr>`, applied bottom-up; each step must make the expression **cheaper** or the walk stops, a repeat ends it, and the budget is the backstop. Rules: `ln 1`, `ln e`, `ln∘exp`, `exp 0`, `exp∘ln`, `√(u²) = \lvert u\rvert`, `\lvert number\rvert`, `\lvert u\rvert² = u²`, `sin/cos/tan` at `0`, `sin π`, `cos π`, and `sin²u + cos²u = 1` inside a larger sum. `NativeSolver::simplify` now uses it, so the Simplify button does something for the first time. 7 agreement tests, each sampling 20 random points and requiring the rewrite to match the input wherever both are defined. | every rule tested by numeric agreement at 20 random points | 4 |
| ✓ W1.4 | Exact constants and identities: `√2·√2 = 2`, `ln e = 1`, `e^{ln x} = x` for `x > 0`, `sin²+cos² = 1`, `sin(π/6) = 1/2`. *Shipped:* the first two came with W1.3's rules; this item added `√u·√u = u` and the eight standard angles (`sin`/`cos`/`tan` at `π/6`, `π/4`, `π/3`, `π/2`), with `π/7` and other non-standard angles left alone. The walk now accepts an **equal-cost** rewrite, because `sin(π/4) = √2/2` is the same size as the sine it replaces and refusing it on cost would be refusing a correct answer. **Rationalised denominators are deliberately not a rule**: `√2/2` costs more than `1/√2` under any honest cost function, so rationalising is a *presentation* choice and belongs in the printer, not in a rewrite that must shrink. | the standard angles are exact and agree numerically; `√2·√2 = 2`; `sin(π/7)` unchanged | 3 |
| ✓ W1.5 | Numeric formatting: an exact value prints with a decimal to three significant figures and a `≈`; an integer gets no second form. *Shipped:* `canlab_core::approx` — `value_of` (only when there is no free variable, so a parametric answer has no value rather than being evaluated at 1), `significant` (3 s.f., trailing zeros trimmed, `-0` normalised), `decimal_of`, `is_exact_decimal`; the GUI shows `≈ 0.5` under `x = 1/2` and nothing under `x = 2`. 5 core tests and 1 GUI test. | `1/3 → 0.333`, `√2 → 1.41`, `π/6 → 0.524` | 2 |

### W2 — Equations and inequalities

| # | What | Verify | Days |
|---|---|---|---|
| ✓ W2.1 | The answer type (D2): `Solutions { None, All, Values(Vec<Expr>), Family { base, period, param }, Inequality { var, intervals } }`, its LaTeX, its JSON, and an **additive** `solve_all` beside the old `solve`. *Shipped:* the type, `Bound`, `to_latex`, `encode_solutions`/`decode_solutions`, `NativeSolver::solve_all`, `SympyWorker::solve_all` and `canlab solve-all`; 18 new solver tests and 2 CLI tests; the worker op is reviewed but unrun (no Python here). Still to come in this item: `Union`, `Answer`/`Approximation`. **Follow-up:** `Union` and `Approximation` shipped, and the JSON decoder now reads both — the "encoded but not decoded" asymmetry is gone, unions are depth-bounded, and a residual must be a finite number. `every_kind` covers all seven kinds in both round-trip tests. | the protocol round-trips every kind; a family renders as `π/6 + 2kπ` | 5 (3 spent) |
| ✓ W2.1b | W2.1 switched the trait, not the panels: `JobOutcome` still carries `Vec<Expr>` and the result card still prints the old shape. Cut the GUI over to `Solutions`, render all five kinds, and use `is_empty` for "no solution". *Shipped:* every kind renders in the result panel — `None` and `All` as words rather than symbols, a value set ascending and one line per value, a family as `x = π/4 + kπ`, a union as one line per member (so an approximation inside it keeps its `≈`), an inequality as `x ∈ (-∞, -2] ∪ [1, ∞)`, and an approximation as `x ≈ 1.2599` with the residual on hover and in a line of its own. The status bar counts each kind in words ("2 approximate solutions", "an interval", "a family of solutions"). Snapshots cover the value set, the family, the refusal and the stated-parameter page. | a family, an inequality, a value set, `All` and `None` each render in the panel, and each has a snapshot (W0.1) | 2 |
| ✓ W2.2 | Parametric solve: extract coefficients as `Expr` (`Poly<Expr>`) and apply the linear/quadratic formulas symbolically, so `x² + y² = 1` for `x` gives `±√(1−y²)`. *Shipped:* `canlab_solver::parametric::solve_parametric`, wired into `NativeSolver::solve` and `solve_all`, 6 tests. The square factor of the discriminant is taken out by **scaling the polynomial**, not by dividing the printed expression — dividing left `(1/4)(4 − 4y²)` under a root, which is the same number and unreadable. `sin x = 0` and cubics are refused. This is what the editor's Solve button now shows for a two-variable equation, and what autofill (W10.2) will insert. | `x²+y²=1` → two branches; `2x+y=3` → `(3−y)/2`; substitution back gives zero | 3 |
| ✓ W2.3 | Higher degree: rational root theorem, deflation, then factorisation from W1.2; refuse cleanly above that. *Shipped:* `Poly::<Number>::rational_roots` (x-powers divided out first, so `x³ − x` answers `{−1, 0, 1}`), `Poly::<Number>::deflate` (synthetic division, refusing a non-root), and `higher_degree_roots` in `canlab-solver`, which divides rational roots out one at a time and finishes the leftover linear or quadratic factor exactly — so `(x−1)(x²−2)` answers `1, ±√2` rather than the `1` a search-only implementation returned. A leftover of degree three or more declines. 6 poly tests and 5 solver tests. The corpus has `cubic-three-roots` and `quartic-four-roots`. *Not done:* a quartic whose real roots are all irrational (`x⁴ − x − 1`) declines. | cubics and quartics with rational and surd roots; `x⁴−5x²+4` → `±1, ±2` | 2 |
| W2.4 | Exponential and logarithmic equations: isolate, take logs, `ln`/`exp` identities, reject extraneous roots by substitution. *Both shapes shipped:* `canlab_solver::exponential::solve_exponential` (`c·a^u + d = 0`: `2^x = 8 → {3}`, `2^x = 7 → ln7/ln2`, `2^{x^2} = 8 → ±√3`, `2^x = −4 → Solutions::None`) and `canlab_solver::logarithmic::solve_logarithmic` (`Σ log_b u = d` over one base: `log₂x + log₂(x−2) = 3 → {4}`, with `−2` computed and *rejected* by substituting into each logarithm's domain — `log₂x + log₂(x−2) = 1` keeps the surd `1+√3` and rejects `1−√3`; two bases decline). Wired into `solve`/`solve_all`; 12 tests; the corpus has `log-combine-two-roots`; checked end-to-end through the real parser (`canlab solve-all '\log_2 x + \log_2(x-2) - 3'` → `{4}`). **And the rejected root is reported:** `Solutions::Rejected { solutions, rejected }` carries the answer *and* the candidates that were computed and do not satisfy the equation, each with a `Rejection` reason (`OutsideLogarithmDomain`, `DoesNotSatisfy`). `\log_2 x + \log_2(x-2) - 3` prints `\left\{4\right\}` and, beside it, `x = −2 is rejected: the argument of a logarithm must be positive`. The answer set itself is unchanged — `to_latex` prints the set, `rejections_latex` prints the working — so the corpus row still compares equal, and the kind round-trips through JSON. | `2^x = 8 → 3`, `log₂ x + log₂(x−2) = 3` → `4`, and the rejected `−2` is reported as rejected | 2 |
| ✓ W2.5 | Trig equations in general form, including the second branch and the odd-half-turn family (`cos x = c`, `tan x = c`). *Partly shipped:* `canlab_solver::trig::solve_trigonometric` answers `tan u = r` as the single family `arctan r + kπ` — exact for the school values (`tan x = 1` → `\frac{\pi}{4} + \pi k`, `tan x = 0` → `\pi k`, `tan x = 2` → `\arctan 2 + \pi k`) — 5 tests, a corpus row, and checked through the parser (`canlab solve-all '\tan x - 1'`). *Blocked on a type:* `sin x = ½` needs **two** families (`π/6 + 2kπ` and `5π/6 + 2kπ`), and `Solutions::Family` holds one progression, so sine and cosine **decline** rather than answer half their roots. The fix is `Solutions::Union(Vec<Solutions>)` — the leftover W2.1 lists — followed by the branch construction for `sin`/`cos`. | `sin x = ½` → `π/6 + 2kπ`, `5π/6 + 2kπ`; a numeric spot-check at k = −2…2 | 3 |
| ✓ W2.6 | Absolute value and inequalities with a sign chart over the exact critical points. | `\lvert x−1\rvert < 2` → `(−1, 3)`; `(x−1)/(x+2) ≥ 0`; endpoint openness is checked | 3 |
| W2.7 | New crate `canlab-num`: bracketing + Brent in one variable, a scan that finds all roots in an interval, deflation for repeated roots, and every answer carrying its residual. | residual ≤ 1e-12 relative on 50 equations with known roots; a double root is found once | 3 |
| W2.8 | Nonlinear systems: substitute the linear ones out, then a resultant for two polynomial equations, then Newton from the remaining candidates. | circle ∩ line, circle ∩ circle, two conics; every reported point substitutes to zero | 4. **Done.** `crates/canlab-solver/src/nonlinear.rs`: `SystemAnswer { None, All, Points }` and `Solver::solve_nonlinear` (default decline, implemented by the native backend). The method is the note's: substitute the equations that pin one unknown, a resultant for the two that remain, back-substitution by the quadratic formula, and every point checked against **all** the original equations to `POINT_TOLERANCE`. Circle ∩ line gives `(−2,−1)` and `(1,2)`; two circles give `x = ½, y = ±√15/2`; a circle and a parabola give exactly two: substituting `y = x² − 1` into `x² + y² = 5` gives `y² + y − 4 = 0`, whose roots are `(−1 ± √17)/2`, and the second gives `x² = y + 1 < 0`, which has no real `x`; a non-intersecting pair answers `None` and not an empty list; three unknowns decline. **And the panel renders it:** `JobOutcome::System` and `SolveState::Points` show one line per point, `None` as "no solutions" and `All` as "every point"; a circle and a line answer with both points in the panel, which a test checks against `(−2,−1)` and `(1,2)`. **Not done:** the resultant is evaluated numerically at the end (the polynomial is built by exact interpolation, its roots found by `canlab-num`). |

### W3 — Calculus

| # | What | Verify | Days |
|---|---|---|---|
| W3.1 | Definite integrals: parse `∫_a^b`, and Gauss–Kronrod with an error estimate in `canlab-num`. Symbolic first, numeric with a bound when the symbolic path declines. | `∫₀^π sin x = 2` exactly; `∫₀^1 e^{−x²}` numeric with a bound that brackets the true value | 3. **Library done, not the grammar it was specified against.** `crates/canlab-parse/src/integral.rs`: a polynomial term integrates exactly in rational arithmetic, `sin`/`cos`/`exp`/`1/x` have closed forms evaluated at the limits, everything else goes to adaptive Simpson returning `Approximate { value, error }`. `∫₀² 3x² = 8`, `∫₁⁴ 3x² = 63`, `∫₀¹ e^{−x²} = 0.7468…`, a pole is refused. **Not done:** the error is adaptive Simpson's Richardson estimate and not Gauss–Kronrod's interval bound, `π` in a limit is refused because a limit must be a number the parser can see, and the *field* holds an `\int` line as one `Node::Raw` (`canlab-input`'s `is_calculus_statement`) that `canlab-parse` reads — see the note below. |
| W3.2 | Limits: the school set — polynomial and rational at a point, `0/0` by cancellation, `∞/∞` by degree, `(1+1/n)ⁿ`, one-sided. Numeric bisection as a fallback with a stated uncertainty. | `lim (sin x)/x = 1`, `(1−cos x)/x² = ½`, a jump function reports "does not exist" with both sides | 3. **Library done.** `crates/canlab-parse/src/limit.rs`, reached as a statement like the integral: substitution (exact where the expression is a polynomial at a rational point), the standard limits recognised structurally, cancellation of a common polynomial factor by exact Euclid, degree at infinity, and a scaled numeric sequence that reports divergence with its sign. `sin x/x → 1`, `(1−cos x)/x² → ½`, `(x²−1)/(x−1) → 2`, `3x²/2x² → 3/2` at infinity, `1/(x−2)` from each side `∓∞` and two-sided "does not exist" with both sides named, `1/x² → +∞` from both. **Not done:** the numeric fallback is a scaled sequence with a stated uncertainty rather than bisection, and the *field* holds a `\lim` line as a `Node::Raw` for the parser, as it does for `\int` — see the note under W3.1. |
| W3.3 | Tangents, normals, extrema over an interval (critical points + endpoints), monotonicity, and the sign table as a rendered artefact. | a cubic on `[−1, 3]`: global max at an endpoint, local max inside; tangent at `x₀` touches the curve there and nowhere else in a small window | 3. **Library done, not the artefact.** `crates/canlab-parse/src/calculus.rs`: `tangent` and `normal` as `Line { at, slope }` (a vertical tangent is a line, not a failure), `extrema` over `[a, b]` including both endpoints, and `sign_table` as contiguous `Piece`s between critical points. `x³−3x²+1` on `[−1,3]` gives critical points 0 and 2, the global maximum at the endpoint `x=3`, the tangent at 1 with slope `−3`, and increasing/decreasing/increasing. **Rendered in three places:** `SvgOptions::bands` shades the pieces in the SVG, `canlab sign 'x^3-3x^2+1' --xmin -1 --xmax 3` prints the table and writes the picture from one call, and the **GUI's result panel** now shows the table under the answer, in the CLI's own words and over the plot's own window — computed where the curves are built, because a panel draws on every frame and a sign table searches for critical points. Three renderings, one source, so they cannot disagree. **And a touching zero is found:** `critical_points` minimises `\lvert f'\rvert` on the bracket around **every** sampled local minimum, so the threshold only confirms the refined value instead of gating the search; `x³` at 0 is a critical point and its sign table has two increasing pieces. `x⁴`'s flat minimum is one point, not three: a local-minimum test and a golden-section minimum of `\lvert f'\rvert` replaced a search that stopped wherever the magnitude was flat. **Not every critical point is certified:** the scan's resolution is one of its 1024 cells, so two zeros inside one cell can be missed; a derivative that is zero on a whole interval has no isolated point to report; and points closer than a thousandth of the interval (with a `10⁻¹⁵` floor) are reported as one. The doc says "the points the searches certify" for that reason. |
| W3.4 | Volume of revolution and area between curves. | sphere from a semicircle, cone from a line; compared to the closed form | 1. **Done.** `crates/canlab-parse/src/figure.rs` plus the `\vol` and `\area` statements: `\vol_a^b y\,dx` is discs `π∫y²`, `\vol_a^b y\,dy` is shells `2π∫xy`, and `\area_a^b f, g` splits at the curves' crossings so it is an area and not a signed area. A sphere of radius 2 gives `32π/3` and a cone `2π/3` to 1e-9 against the closed forms; swapping the two curves in `\area` does not change the answer. **Not done:** `\vol` refuses a curve given in the differential's own variable rather than reading it as an inverse function. |

**The integral's syntax has two readers, and the field now defers to the
parser.** A document enters through the field (`canlab-input`), whose structural
model is a tree of text runs, fractions, scripts and groups — there is no node
for a differential, and `dx` arrives as the text `dx`, which the field's own
reader factorises into `d·x`. The decision was to add the smallest node that
lets the field *hold* such a statement rather than parse it: `Node::Raw` carries
pre-rendered LaTeX, and a line that starts with `\int`, `\lim`, `\vol` or
`\area` **and has no empty slot left** is held as one `Raw` and read by
`canlab-parse`. A line still being typed — the palette's `\int_{X}^{X}` — stays
structural, so the cursor still lands in its slots. The corpus rows pass, and
there is still one LaTeX grammar for calculus, not two.

### W4 — Vectors, matrices, complex numbers

A new value type, not an expression: `Expr` has no matrix.

| # | What | Verify | Days |
|---|---|---|---|
| W4.1 | `Value { Scalar(Expr), Vector(Vec<Expr>), Matrix(Vec<Vec<Expr>>), Complex(Expr, Expr) }` in core, with arithmetic, printing to LaTeX, and a parser path for `[[1,2],[3,4]]` and `(1,2,3)`. | every operation round-trips through LaTeX; a matrix prints as `pmatrix` | 3. **Done, in `canlab-solver` and not in core.** `crates/canlab-solver/src/value.rs`: the four kinds, `add`/`sub`/`neg`/`mul`/`pow`, `to_latex` (a matrix as `pmatrix`, a vector as `\left(…\right)`, a complex number as `a + bi`) and `from_latex` for `[[1,2],[3,4]]`, `(1,2,3)` and the `pmatrix` the printer writes. **Verified:** every kind round-trips through its own LaTeX; a scalar times a matrix is a matrix; a matrix times a vector is `(3, 7)` for `[[1,2],[3,4]]·[[1],[1]]`; a scalar plus a vector is *refused* rather than broadcast. **Not done:** the type lives in the solver rather than in `canlab-core` (it needs `canlab-parse` for `from_latex`, and core is below both), and the GUI has no panel that shows a vector or a matrix. |
| W4.2 | Linear algebra: determinant, inverse, rank, eigenvalues of 2×2 and 3×3 exactly where they are rational or surd, `Ax = b` on top of the existing elimination. | `A·A⁻¹ = I` for 200 random integer matrices; eigenvalues of a symmetric 2×2 checked by the characteristic polynomial | 3. **Done.** `crates/canlab-solver/src/linalg.rs`: `determinant` by cofactor expansion (exact, and `O(n!)` — right for the sizes an exam asks and the doc says so), `rank` by exact elimination (a count, not a tolerance), `inverse` by Gauss–Jordan, `eigenvalues` for 2×2 (quadratic formula, complex roots included) and 3×3 (rational roots then a quadratic factor), and `solve_linear` on top of the existing elimination. **Verified:** `A·A⁻¹ = I` exactly for 200 deterministic integer matrices; `[2 1; 1 2]` gives `λ = 1, 3`; `[0 −1; 1 0]` gives `±i`; a singular matrix is refused. **Not done:** a 3×3 with three irrational eigenvalues is refused (no cubic formula), and there is no `Value`/parser path for matrix literals — that is W4.1. |
| W4.3 | Geometry: dot, cross, norm, angle, projection, line and plane equations, distance from a point to a line and a plane, intersection of two planes. | a right triangle with known angles; a plane and a line that miss, and one that meets | 2. **Done.** `crates/canlab-solver/src/geometry.rs`: vectors as `Vec<Expr>`, so `dot`, `cross`, `norm` (`(1,2,2)` has norm exactly `3`), `angle` (exact for `π/2`, `π/3`, `π/4`, `π`; a name otherwise), `projection`, `distance_point_line`, `distance_point_plane` (unsigned), and `plane_intersection` answering a line, `None` or `Coincident`. A mismatch of lengths and a zero direction are refused with both lengths named. **Not done:** lines and planes are data (`point`, `direction`/`normal`) rather than a `Value` the parser produces — that is W4.1's matrix/vector type, and the parser path for `(1,2,3)` is not written. |
| W4.4 | Complex numbers: `i` as an exact constant, arithmetic, modulus, argument, conjugate, roots of unity, and the quadratic case with a negative discriminant. | `(1+i)⁴ = −4`; the three cube roots of 1 sum to zero; `z·z̄ = \lvert z\rvert²` | 3. **Done.** `crates/canlab-solver/src/complex.rs`: `Complex { re, im }` as a pair of `Expr`s, so arithmetic, integer powers, the conjugate and the modulus are exact — `(1+i)⁴` is the integer `−4`. `argument` is exact on the axes and the diagonals (`arg(−1+i) = 3π/4`) and a *name* (`arctan 2`) elsewhere, never a decimal. `roots_of_unity(n)` returns the fourth roots as `1, i, −1, −i`. And the solver answers a negative discriminant with complex roots: `x² + 1 = 0` is `{i, −i}` and `x² + 2x + 5 = 0` is `{−1 + 2i, −1 − 2i}` — the division is done term by term so the answer is simplified. The evaluator refuses `i` unless the caller supplies it as a variable, so a document that uses `i` as an axis name still plots. **Not done:** `arg` of a general point is a name rather than a number, and there is no complex `exp`/`ln`/root beyond integer powers. |

### W5 — Probability and statistics

| # | What | Verify | Days |
|---|---|---|---|
| W5.1 | Exact combinatorics: `n!`, `binom`, `perm`, with big-integer growth to 10³⁸ before falling back to a float. | Pascal's identity, `C(n,k) = C(n,n−k)`, a 20-element draw | 1. **Done.** `crates/canlab-solver/src/combinatorics.rs`: `factorial`, `binomial`, `permutations`, and checked variants that refuse a negative argument. Exact through `33!` (`3.68·10³⁷`, the roadmap's `10³⁸` bound); `34!` is the first that `i128` cannot hold, and `Number` says so by becoming a `Float`. **Verified:** Pascal's identity for `n ≤ 20`, the symmetry `C(n,k) = C(n,n−k)` for `n ≤ 30`, `C(20,5) = 15504` and `P(20,5) = 1860480`, and `C(60,30)` exact. **Not done:** no `n!`/`\binom` syntax in the parser, and no big-integer type — beyond `i128` the answer is a float and not an exact integer of arbitrary size. |
| W5.2 | Distributions: binomial pmf/cdf, normal cdf and inverse cdf, expected value and variance, and the inverse path so `P(X ≤ k) ≥ 0.9` solves for `k`. | `Φ(0) = 0.5`; `Φ⁻¹(0.975) = 1.96` to 4 decimals; binomial sums to 1 | 2. **Done.** `crates/canlab-num/src/distribution.rs`: `erf` (Abramowitz–Stegun 7.1.26, with its stated `1.5·10⁻⁷` bound), `normal_cdf`, `normal_pdf`, `normal_quantile` by bisection, the `N(μ, σ²)` forms of both, and `Binomial { trials, probability }` with `pmf`, `cdf`, `mean`, `variance` and `quantile`; plus `binomial_pmf_exact` for a rational `p`, where `P(X = 3)` of five fair coin tosses is exactly `5/16`. **Verified:** `Φ(0) = 0.5` exactly, `Φ⁻¹(0.975) = 1.96` to `5·10⁻⁵`, the binomial sums to 1 for four `(n, p)` pairs, moments from the pmf agree with `np` and `np(1−p)`, and `P(X ≤ k) ≥ 0.9` for `n = 20, p = ½` is `k = 13`. **Not done:** the normal cdf's accuracy is the erf approximation's `1.5·10⁻⁷` and not better, and there is no parser syntax for a distribution. |
| W5.3 | Descriptive statistics and regression: mean, median, variance (sample and population), standard deviation, least-squares line, correlation. | a dataset with a known line; variance against the two-pass formula | 1. **Done.** `crates/canlab-num/src/stats.rs`: `mean`, `median`, `variance_population` (`/n`), `variance_sample` (`/(n−1)`), the two standard deviations, and `least_squares` returning `{intercept, slope, correlation}`. Exact wherever exactness is possible: `mean(1,2,3,4) = 5/2`, `variance_population(2,4,4,4,5,5,7,9) = 4` and the sample variance `32/7` (the ratio is `n/(n−1)`), and `y = 2x + 1` is recovered exactly with `r = 1`. **Verified:** the one-pass variance agrees exactly with the two-pass form on exact data, including a dataset with mean `10⁶` and spread 1 where a float implementation loses digits. **Not done:** no parser or panel syntax for a dataset, and `√` of a non-square variance is a `Float` because `Number` has no surd variant. |

### W6 — Differential equations

| # | What | Verify | Days |
|---|---|---|---|
| W6.1 | New crate `canlab-ode`: explicit Euler, RK4, Dormand–Prince 5(4) with adaptive step and dense (Hermite) output, symplectic Euler and velocity Verlet, a `Trace`, and events (a zero crossing located by bisection). | measured convergence order: RK4 error falls by 16 when `h` halves, DOPRI by 32; a harmonic oscillator conserves energy to 1e-6 with Verlet | 4. **Done.** `crates/canlab-ode`: `euler`, `rk4`, `Dopri::integrate` and `Dopri::integrate_dense` (cubic Hermite), `symplectic_euler`, `velocity_verlet`, `Trace` with `at`/`extent`, and `first_event` by bisection. **Verified:** Euler is first order (error halves with `h`), RK4 fourth (16×), Dormand–Prince's error falls with the tolerance; velocity Verlet holds the oscillator's energy to `1e-6` at `h = 0.001`; symplectic Euler's energy has no trend. **Not done:** the state is a scalar, not a vector — a coupled system needs a `System` type that is not here; and the front end (`y' = …`, W6.2) does not exist, so nothing in the CLI or the GUI can ask for one. |
| W6.2 | Front end: `y' = −ky`, `x' = y` alongside `y' = −x`, `y(0) = 1`, a time span and a step, and a refusal when the system is underdetermined or a derivative is not isolated. Coupled systems build one `OdeSystem`. | `y' = y, y(0) = 1` reproduces `eᵗ` to 1e-6; three unknowns in three lines; `y' = 1 + y'` is refused with a reason | 3. **Done.** `crates/canlab-ode/src/system.rs`: `OdeSystem` with a vector state, `rk4_system` and `dopri_system` (the error estimate is the largest component), `SystemTrace` with interpolation, and `parse_system` reading `y' = …` and `y(0) = …` lines. **Verified:** `y' = y, y(0) = 1` reproduces `eᵗ` to `1e-9`; three unknowns in three lines trace a circle with a constant radius; `y' = 1 + y'` is refused as not isolated; a missing, duplicate or orphan initial condition and an undefined reference are each refused with their own message; the scalar and system paths agree to `1e-12` on one unknown. **Not done:** the CLI and the GUI do not call it — there is no `canlab ode` subcommand and no panel — and the time span and step are the caller's, not part of the document. |
| W6.3 | Plot: `y` against `t`, a phase portrait, and a 3D path once W8 lands. The trace is a `Curve` like any other, so the sampler and the axes need no new concepts. | the plotted trace matches the tabulated trace; a blowing-up solution is split into segments, not joined | 2. **Done, except the 3D path.** `crates/canlab-ode/src/plot.rs`: `trace_curve` (`y` against `t`), `phase_curve` (`y₂` against `y₁` for a two-dimensional system) and `component_curve`, all returning `canlab_plot::Curve`. **Verified:** the plotted points are the tabulated points in order; a trajectory that blows up is split into three segments with the two gaps unjoined; a non-finite point ends a segment rather than being skipped over; a smooth trajectory is one polyline. **Not done:** the 3D path waits for W8's viewport, and nothing in the GUI draws an ODE trace yet. |
| W6.4 | Three scenarios as corpus files, each with an analytic reference or an invariant: **water** (1D wave equation against d'Alembert, energy drift < 0.1% over 10⁴ steps), **air** (projectile with `v²` drag against the `tanh` closed form, terminal velocity), **sound** (Doppler shift; `c = 331.3√(1+T/273.15)`). The fourth, light, turned out to be a Monte Carlo rather than an integrator and lives in W14. | each scenario fails when its physics is wrong, not when a number is retyped | 3. **Done as tests, not corpus rows.** `crates/canlab-ode/tests/scenarios.rs`: **air** — a `v²`-drag projectile against `v_t·tanh(g·t/v_t)` and its distance against `−(v_t²/g)·ln cosh`, plus the terminal velocity; **sound** — `c = 331.3√(1+T/273.15)` and the Doppler shift `f·c/(c∓v)` with its asymmetry; **water** — the explicit wave-equation scheme against d'Alembert's `sin(πx)·cos(πct)` and its discrete energy over `10⁴` steps (drift under `10⁻³`). **Why not corpus rows:** the corpus format is document-to-number, and an ODE scenario is an integration over a parameter range — a `document` field would not be a document. The roadmap's own criterion is met: the air scenario has a companion test that runs the *wrong* drag sign and asserts the check fails. **Not done:** the water scenario is a finite-difference scheme written in the test, not a crate — the ODE crate integrates first-order systems in time and the wave equation is second order in both. |

The roadmap is right that closed-form ODE solving is optional and that "reworking
them into shapes that are easily simulatable" is what matters. W6.2's refusal
when a derivative is not isolated is exactly that rework, with a message instead
of a silence. Separable and linear first-order closed forms are cheap once W1
lands, and are not in the plan until the corpus asks for one.

### W7 — The compilation pipeline

LaTeX → Expr → Lua → GPU, explicitly requested and cached by content. This is the
workstream that the "compile to GPU code" line in the roadmap turns into, and it
subsumes the scripting engine, so W9 got smaller by one item.

| # | What | Verify | Days |
|---|---|---|---|
| W7.0 | A sandboxed Lua engine in `canlab-script`: `mlua` 0.12 with `lua54` + `vendored` (no system Lua), no `io`, no `os`, no package loading, and an instruction budget that stops a runaway loop with a message rather than a hang. | an infinite loop stops in under 100 ms with a diagnostic; `io.open` is refused; a script cannot reach the filesystem | 3 |
| W7.1 | `Expr` → Lua: a printer that emits a named function per document row, with the constants folded and the operators spelled as Lua's. Every `Hold` and every function the subset cannot express is a named refusal, not a wrong number. | `Expr → Lua → mlua` agrees with `Program::eval` to 1e-1 — 2 on 10⁴ random expressions (transcendentals included), and the emitted source parses as Lua | 3. **Printer done; the `mlua` comparison is not.** `crates/canlab-core/src/lua.rs`: `to_lua` and `to_lua_program`, with `math.*` for the six functions the subset has, `math.pi` and `math.exp(1)` for the constants, a rational as a division, and a named refusal for a `Hold`, an unsupported function and a free symbol. **Verified:** the emitted text, parsed by an independent evaluator written in the test, agrees with `Program::eval` to `1e-10` on eight expressions at three points. **Not done:** `mlua` is not a dependency — there is no network and no vendored Lua — so the roadmap's `Expr → Lua → mlua` check is unrun, and the test says so. W7.2 and W7.3 are done (their rows below); W7.4 is untouched. |
| W7.2 | The Lua subset → WGSL compiler: literals, locals, assignment, `+ − × ÷ ^`, the `math` functions, `if`/`else`, `while`, fixed `for`, `break`, and calls to a small builtin table (including a per-invocation random stream). Anything outside the subset is a diagnostic that names the construct and the line. | every rejection names the construct; a differential test runs 200 generated programs on both backends and compares 10³ sample points each — **and compares NaN as "is a NaN", not bit-for-bit**: two compilers propagate different NaN payloads, and chasing that is chasing codegen | 8. **Compiler done; the differential test is not.** `crates/canlab-core/src/wgsl.rs`: a tokenizer, a recursive-descent parser for the subset (`local`, assignment, `if`/`elseif`/`else`, `while`, fixed `for`, `break`, `return`, `+ − * / % ^`, comparisons, `and`/`or`/`not`, the `math` table and `rand()`), and a WGSL emitter. **Verified:** a literal expression, a local, the math functions, an `if`, a `while`, a fixed `for`, right-associative `^` as `pow`, comparisons as `select(0.0, 1.0, …)` because a kernel's values are `f32`, and `rand(seed)`. Seven constructs outside the subset — a table, a string, a function definition, `repeat`, `goto`, a field access, a method call — are each refused with the construct named and the line. **Not done:** **the differential test now runs:** `crates/canlab-gpu` executes the compiled WGSL on this machine's adapter, so eight generated programs at 64 points each — **512 points, worst relative `4.87·10⁻⁶`** — and four programs that draw — **256 draws** — are compared against the interpreter. **At the roadmap's size:** `crates/canlab-gpu/tests/sweep.rs` runs **193 programs at 10³ points each — 193 000 points, 0 disagreements, in 5.1 s**, with seven programs refused by the compiler and counted as such. The tolerance is `1e-5` relative **or** `1e-5` absolute, and the absolute half is not a loosened bound: near zero the relative error is meaningless, and 14 points needed it, with a worst absolute difference of `1.2·10⁻⁷` — one ulp of an order-one value. **The roadmap's `1e-6` relative is not achieved** on cancellation-heavy programs, because the quantity the two `f32` evaluations disagree about is smaller than their own rounding; the measured bound is stated instead. |
| W7.3 | Compile and run plumbing: content-hash cache under `%LOCALAPPDATA%/canlab/kernels`, an explicit Compile action in the simulation panel and a `canlab compile` command, viewers for the generated Lua and the generated WGSL, and a validation sweep whose result — the measured worst-case difference between the CPU and GPU runs — is stored with the artefact and shown in the UI. **Nothing compiles in the background; nothing recompiles without being asked.** | a second compile of an unchanged document is instant; the panel shows the error bound the sweep measured; editing the document marks the artefact stale instead of rebuilding it | **Done, minus the GPU and the disk cache.** `crates/canlab-solver/src/kernel.rs`: `compile_expression` (Lua then WGSL), `compile_lua` for a hand-written kernel, `Artefact { hash, key, lua, wgsl, parameters, locals }`, `Refusal` naming the stage (`lua`/`wgsl`) and the construct, an in-memory `KernelCache` keyed by a length-prefixed digest **and** the exact source (so a hash collision is a miss, never a wrong kernel), and `sweep` over 200 generated programs at 64 points each. `canlab compile` and `canlab compile-sweep` in the CLI. **Verified:** 19 tests in `crates/canlab-solver/src/kernel.rs` and 26 in `crates/canlab-cli/tests/cli.rs`, 12 of the CLI tests covering `compile`/`compile-sweep`; the digest matches the published FNV-1a values; a second compile is a hit and a changed source a miss; a refusal is cached as itself; the sweep at seed 1 gives 200/200 compiled, 12,800 points, worst relative difference `8.42·10⁻⁵` against a `1·10⁻⁴` bound, with 21 points where the kernel's `f32` overflows and the CPU's `f64` does not (counted, not failed). **That bound is a property of seed 1, not of `f32`.** The code's own comment on `SWEEP_TOLERANCE` says "the measured worst case is the one to read" and that "`1e-4` is therefore also the margin"; the same sweep at seed 7 reports a worst relative difference of `3.88·10⁻²` and two failures — `tan(exp(y + 4))` sampled near a pole — and `canlab compile-sweep --seed 7` exits 2. The seed-1 number is a measurement, not a guarantee. **Not done:** nothing executes WGSL or Lua — the sweep compares the CPU's `f64` against an `f32` *replay* of the emitter's arithmetic, so there is **no measured CPU/GPU difference** and no artefact field claiming one; the cache is in memory, not under `%LOCALAPPDATA%`; and the viewers are the CLI's printed text, not a panel. |
| W7.4 | The light-transport kernel template: one invocations per photon, a per-photon random stream, the Fresnel split, total internal reflection, and the photon-event output. Written once in the Lua subset so the CPU and GPU runs share a source. | CPU and GPU agree on exit angle, path length and arrival time to 1e-6 relative; throughput at least 10⁷ photons per second on this machine, measured by W0.3 and not assumed | 3. **Done, as a slab and without a GPU.** `crates/canlab-optics/src/kernel.rs`: the kernel source, a `run` that interprets it at `f32` or `f64`, and a `reference` that does the same arithmetic in Rust with the same splitmix64 stream. It refracts in, bounces with the Fresnel reflectance, is trapped by total internal reflection past the critical angle, and returns the path length. **One source:** `canlab_core::wgsl::compile` emits the WGSL from the same text `interpret` runs, so the two cannot drift — a Rust re-implementation would have been a second source, and `mlua` is not a dependency. **Verified:** the kernel compiles to WGSL; `f32` and `f64` agree to `1e-6` relative at five angles and eight seeds, which is the floating-point difference a GPU run would show; the kernel and the Rust reference agree to `1e-12` at four angles, four indices and eight seeds; a photon past the critical angle is trapped and its path is the bounce limit times the step for every seed; the reflectance matches `((n−1)/(n+1))²` at normal incidence. **Not done:** **and it now runs on the adapter:** `crates/canlab-gpu` executes the compiled kernel, and 15 photons across five angles and three indices — including past the critical angle, where total internal reflection traps them — agree with the interpreter to **`0e0`**, while the interpreter agrees with the Rust reference to `1e-12`. One source, three executions, no disagreement. The roadmap's 10⁷ photons per second is still **unmeasured**, because the adapter here is llvmpipe, which executes on the CPU, and a number from a software rasteriser is not a GPU's. The geometry is a slab and not the block: the subset has no tables, so the box's four-face loop would have to be unrolled by hand. |
| W7.5 | Probe `wgpu::Features::SHADER_F64` on this machine (DX12 may provide it, Metal will not). Decide once: f64 where available, else f32 with a visible "single precision" marker, else double-double for the transport only. | a probe binary prints the adapter, its features, and a 1e-15 accuracy check; the marker appears on every artefact compiled in single precision | 2. **Done, and the answer is f64.** New crate `crates/canlab-gpu`, the only one in the workspace that depends on a graphics stack. The probe reports one adapter — `Vulkan llvmpipe (LLVM 21.1.8, 256 bits)`, device type `Cpu` — with `SHADER_F64` present, and a shader's `f64` agrees with the CPU's to **0e0** relative on Newton's method for `√2` and a sum of a hundred thirds, against the roadmap's `1e-15`. So the transport can run in double precision and the single-precision marker is not needed. **And WGSL now actually runs:** `canlab_gpu::run` executes a compiled kernel once per input row, which makes W7.2's differential test real — eight generated programs at 64 points each, **512 points compared, worst relative difference `4.87·10⁻⁶`**. **Not done:** llvmpipe executes on the CPU, so the roadmap's 10⁷ photons per second is still **unmeasured** and a number from a software rasteriser would not be a GPU's. |

### W8 — 3D visualisation

| # | What | Verify | Days |
|---|---|---|---|
| W8.1 | New crate `canlab-scene`, pure: camera (orbit, azimuth, elevation, distance, perspective), projection, view frustum, clipping, depth sort. No GPU, no `egui`. | a known cube projects to known screen coordinates; a point behind the camera is clipped; the camera round-trips through a `fit` | 3. **Done.** New crate `crates/canlab-scene`, with no dependencies at all: `Vec3`, an orbit `Camera` (azimuth, elevation, distance, perspective), `project`, `to_pixels`, `contains`, `fit`, `project_triangle` and `depth_sort`. **Verified:** a camera at `(0,0,5)` with a 90° field of view puts `(5,5,0)` exactly at the frame's corner and the origin at the centre; a point behind the camera has *no* projection rather than a mirrored one, and a triangle with a corner behind is refused; `fit` puts every corner of a cube inside the frustum and is idempotent; the camera's three axes are orthonormal at every azimuth and elevation. **Not done:** the GUI has no 3D viewport. W8.2 (surface grids) and W8.3 (surface nets) are done and tested — `crates/canlab-scene/src/surface.rs` and `implicit.rs`, 21 tests in the crate — and their own rows below carry the detail. |
| W8.2 | Surface `z = f(x, y)`: a grid sampler with an adaptive split by screen-space error, indexed triangles, analytic normals from `∂f/∂x` and `∂f/∂y` (we can differentiate), palette-ramp colour, and non-finite corners dropping their cells — the same "a break means not connected" rule as 2D. | a plane and a paraboloid: triangle count, normal directions, no NaN vertex ever | 2. **Done.** `crates/canlab-scene/src/surface.rs`: `mesh_surface(expression, options, camera)` builds an indexed mesh of `z = f(x, y)` — corners deduplicated by position, two triangles per cell, an adaptive split driven by the *screen-space* error (a cell splits while its corners' heights differ by more than `pixel_error` world-units-per-pixel, computed from the camera's field of view and distance), analytic normals `(−∂f/∂x, −∂f/∂y, 1)` from `canlab_core::differentiate`, and a height→shade ramp over the mesh's own range. A cell with a non-finite corner is **dropped and not split** — the 2D sampler's break rule — so a pole is a hole and never a spike. **Verified:** a plane gives exactly 32 triangles and 25 vertices over a 4×4 grid with one constant normal; a paraboloid's normals match `(−2x, −2y, 1)` normalised at every vertex and its shade runs 0→1; `1/(xy)` never produces a NaN vertex and its axial cells are gone; an adaptive split produces more triangles than the plain grid. **Not done:** the GUI has no 3D viewport, so nothing draws the mesh yet. |
| W8.3 | Implicit `f(x, y, z) = 0` by surface nets (marching squares' 3D counterpart, ~150 lines instead of a 1 500-line table, and smoother). | a sphere's vertices all lie within one cell of the true surface; an empty region produces no triangles | **Done, with a stated limitation at the grid boundary.** `crates/canlab-scene/src/implicit.rs`: `mesh_implicit(expression, options)` builds `f(x, y, z) = 0` by surface nets — one vertex per sign-changing cell at the average of its edge crossings, neighbouring cells joined in a ring, normals `∇f/\lvert∇f\rvert` from three `canlab_core::differentiate` partials, and the sign of `f` deciding which way they face. A non-finite corner is a hole; a zero corner counts as outside so no crossing divides by zero; a zero gradient drops the cell. **Verified:** a sphere's vertices all lie within one cell of the true surface; an empty region gives an empty mesh and not an error; the negated expression flips the normals; `1/(xyz)` never produces a NaN vertex. **Closed, and measured:** a unit sphere gives `V − E + F = 2` with **zero** boundary edges, on a grid it touches and on one it does not. The first version named a quad's four cells from the cell *above* the edge along its own axis, which does not contain the edge, so most lookups failed and the quad was dropped: 270 of 540 edges were on a boundary and `V − E + F = −19`. `a_sphere_is_a_closed_surface` is the test that could not be written before. **Not done:** a cell cut on two faces still shares one vertex between them, so the seam is slightly smoothed; a vertex per side would be a different surface nets. |
| W8.4 | The viewport, on Bevy (D5): a second camera rendering to a `RenderTarget::Image`, that image shown in an egui rect beside the maths, with depth testing, one directional light and palette colours. | a snapshot test (W0.1) of a lit sphere; orbit drag changes the image and nothing else | **Done, in `egui` and not Bevy.** The plan names Bevy (D5); the roadmap's own C9 note says its build does not fit on this machine, so the viewport is the plot panel's 3D mode: the same canvas, the same drag, the surface instead of the curve. Drag orbits, the elevation clamps at ±1.5 rad so the camera cannot reach the pole where its up vector degenerates, and the toggle appears only for a two-variable document. `canlab_scene::svg::visible_triangles` holds the back-face cull and the painter's sort, and both the panel and `canlab surface` call it, so there is one answer to which triangles are visible. **Verified:** a two-variable document builds a surface whose vertices are on it; the panel's own draw emits polygons in 3D and not in 2D; the toggle clears the mode for a document it cannot draw. **Not done:** no lighting, no axes, no grid, and no depth buffer — the painter's algorithm is wrong for a surface that intersects itself. |
| W8.5 | 3D chrome: axes, grid, tick labels as screen-space text, arrows for vectors, hover picking by CPU ray–triangle, orbit/zoom/pan, fit to bounds, and the ODE path from W6.3 drawn in the same scene. | picking a known sphere returns its surface within a pixel; fit frames the whole mesh | **Partly done.** `crates/canlab-scene/src/pick.rs` casts a ray from the eye through a normalised device coordinate and returns the nearest triangle by Möller–Trumbore — on the CPU, on the same `Mesh` the renderer draws, so the answer cannot disagree with the picture. The plot panel's 3D mode draws axes labelled `x`, `y`, `z`, a grid on the base plane at the surface's own minimum `z`, and tick labels at the window's own coordinates. **Verified:** a unit sphere picked at three points returns a hit on the sphere that projects back to the picked pixel **within one pixel** of an 800-wide frame; a ray outside a triangle misses; the nearest of two parallel triangles wins; and the panel's own draw labels all three axes and ticks the window. **Not done:** **hover picking is wired** — the panel draws a ring at the hit and the point's three coordinates, read from the input rather than from `response.hover_pos()` because egui decides \"hovered\" from the previous frame's pointer, and the test asserts the label's `z` is the surface's own value at its `x, y`. **And the reader can zoom and pan:** scroll zooms about the centre, exponentially so a notch is the same ratio at every scale, clamped to `[0.2, 5]`; right-drag pans in the camera's own screen plane. Right-drag and not shift-drag because `RawInput` has no modifier field and a `PointerMoved` carries none — measured, a shift-drag orbited. **And there is a fit control:** it resets the zoom and pan to the framing `framing_camera` computes, and its hover text names the three gestures. **And the ODE path is drawn**, twice: `canlab ode` reads `y' = …` and `y(0) = …` lines, integrates with Dormand–Prince and writes the trajectory with the same camera and projection as `canlab surface`; and the plot panel's 3D mode does the same from the same lines, which `canlab-input` now holds whole as `Node::Raw` the way it holds `\int`. One turn of `x' = y, y' = -x` is a closed circular orbit in the command's SVG and a circle of radius 1 in the panel, both checked. **Accents parse, and the arrow is still not drawn.** `\vec{v}` is now `Hold::Accent { kind, body }` — a decoration that prints and compares and has no number, which is what `Hold` is for, so no `ExprNode` variant was needed. `\vec`, `\overrightarrow`, `\hat`, `\bar`, `\tilde`, `\overline` and `\underline` are read and printed back; `\overleftarrow` is refused by name. **The axes are drawn as arrows:** each carries a head, because an axis is a vector. **Not done:** the viewport does not draw an arrow for a `\vec{v}` in the document, and a vector's *value* — what `\vec{v}` is equal to — has no document form yet, so there is nothing for such an arrow to point at. |

### W9 — Scripting

The engine itself is W7.0, because the compiler needed it first. What is left is
the Python bridge and putting a script into the document.

| # | What | Verify | Days |
|---|---|---|---|
| W9.1 | `PythonEngine` over the existing `session::Session` on a `python3` child, reusing the discovery from `sympy.rs` and `CANLAB_PYTHON`. | protocol tests in memory; discovery reports "no interpreter" as a normal state, exactly as the SymPy worker already does | 2 |
| W9.2 | Document integration: a script cell with a name, access to the document's variables, a return value or a table, and a table plotting as a curve. A script can call the compiled kernel from W7.3 by name. | a script that returns `sin` samples plots; a script that raises shows the message on the cell; a script that calls a compiled kernel gets the same numbers as the kernel's own run | 3 |

Rhai is the zero-C alternative and is rejected for two reasons: Lua is what
students have met, and it is what most CanSat ground tooling already speaks.

### W10 — Workflow

| # | What | Verify | Days |
|---|---|---|---|
| W10.1 | Continuous solving: 200 ms after the last edit, submit; a job carries the document's generation and a stale result is dropped; an LRU cache keyed by (document hash, operation) so a keystroke and its undo do not re-solve. | typing ten characters in a row starts at most two jobs; a cached re-solve returns in under a millisecond | 3 |
| W10.2 | Autofill: caret at the end of `x =`, another row mentions `x`, so the parametric solve from W2.2 runs and becomes a ghost in `theme::weak()`. **When the two branches are `±` of each other, insert both at once** — `x = ±√(1−y²)` — because that is one equation and the honest one. When they are not symmetric (a linear branch plus a quadratic one), insert the first and let Tab and Shift+Tab cycle the rest, with the count shown. Esc dismisses. Tab is intercepted **only while a ghost is live**, so focus traversal is otherwise untouched. | scripted keystrokes through a real frame: type `x =`, see the ghost, Tab, see `±√(1−y²)`; a non-symmetric case inserts the first branch and cycles; refusing to guess is a visible state | 3 |
| W10.3 | Results inline: each row shows its own answer and its own LaTeX code beside it, in the row, as it changes; an error is attached to the row that caused it, not to the panel. | a document with one broken row keeps the other rows' answers | 2 |

### W11 — Pasting exam LaTeX

An exam problem is already LaTeX. Pasting it should mostly work, and the
refusals should be named. Today `canlab-parse` covers the MathQuill/Abitti
subset. The delimiters matter as much as the commands: `\left| … \right|` and
`\left| … \right.` appear in every absolute-value or evaluation-at-a-point
problem, and `\lvert` is the same construct spelled differently.

| # | What | Verify | Days |
|---|---|---|---|
| W11.1 | Accept and print `\frac`, `\sqrt[n]{…}`, absolute-value delimiters, `\vec`, `\overline`, `\binom`, `\begin{pmatrix}`, `\begin{cases}`, `\text{…}`, `\int_a^b`, `\lim_{x\to a}`, `\sum_{k=1}^n`, `\begin{aligned}`. | a pasted real exam formula parses; every construct in the list round-trips | **Partly done.** `\frac`, `\sqrt[n]`, `\vec`, `\overline` and now **`\left\lvert … \right\rvert` with `\right.`** round-trip: the delimiters become `abs(x)`, a value the evaluator and the differentiator both know, and the printer writes `abs` back as `\left\lvert…\right\rvert` rather than `\operatorname{abs}(x)`. **Not done:** `\lvert`, `\binom`, `\begin{pmatrix}`, `\begin{cases}` and `\text` are all still refused, measured against this row's own list. |
| W11.2 | A refusal list with reasons, shown in the UI: "`\text` inside a formula is prose", "a table is not an expression". | 20 pasted snippets, each either accepted or refused by name | 1 |
| W11.3 | Subscripts as names: `v_1` and `v_2` are two variables, not `v` twice. Today the field refuses a subscript and the parser silently drops it, which means the same text means different things on two paths — the one thing a document reader must never do. Decide which, then make both paths agree and print the name back. | `v_1` and `v_2` in one document plot as two curves with two labels; a subscript on a function name is refused by name | **Done.** A simple subscript is part of the name: `v_1`, `v_2` and `v_max` are symbols whose own LaTeX is a subscript, so `v_1` and `v_2` are two variables and the round trip needs nothing special. An index expression — `x_{n+1}`, `x_1^2` — has no symbol and is refused with a reason that names the shape it reads. **Both readers changed together**, which the agreement test in `canlab-input` enforces: it failed the moment the parser accepted `x_1` while the field still refused it. Six stale tests changed with the rule, each keeping a refusal for the case that is still refused. |

### W12 — (retired)

This was the Bevy spike with a go/no-go. The decision was taken before the spike:
Bevy from the start, full UI. It is now W0.0, and it is a migration rather than a
measurement — though it still starts by measuring, because a build that does not
fit on this machine changes the answer no matter what was decided.

### W13 — 2D plotting, to match the maths

The 2D view is the part that already works, and the part the new maths will
outgrow: an inequality that cannot be shaded, an integral that cannot show its
region, a tangent that has to be imagined. Small items, on the critical path for
M1.

| # | What | Verify | Days |
|---|---|---|---|
| W13.1 | Shaded regions: a half-plane for an inequality answer, and the area between two curves for a definite integral. Drawn under the curves, from the palette, never opaque. | `y > x²` shades the right side; `∫₀¹ (x − x²)` shades the lens, and the shaded area reads the same as the symbolic answer | 3 |
| W13.2 | Tangent and normal lines, point and extremum markers, and the `(x, y)` readout for the point under the pointer. | the tangent at `x₀` matches the sampled curve's slope within 1e-3; a marker sits on the curve it marks | 2 |
| W13.3 | Resampling during a gesture: coarse while dragging or scrolling, full resolution when it stops. This is the known slow pan — one curve is fine, a system is visibly slow. | 30 frames of a simulated drag on a six-curve document stay under 16 ms each, measured by W0.3 | 3 |
| W13.4 | A legend when there are several curves, and a fit that never zooms into a degenerate window. | six curves get six palette colours and six labels; a single point does not produce an empty viewport | 2 |

### W14 — Light transport and detectors

The CanSat case as it was specified: photons through a high-index block of glass,
SiPMs watching its faces, and the whole thing resolved in picoseconds. It is not
an ODE (see C7): every photon is independent, which is what makes it a Monte Carlo
and what makes W7 able to run it.

| # | What | Verify | Days |
|---|---|---|---|
| W14.1 | Photon transport: a ray inside the block, a Fresnel split at every dielectric interface (reflect or refract by sampling, not by branching to both), total internal reflection, absorption by path length, optional `n(λ)` dispersion, and time of flight accumulated as `t += d·n/c`. A photon ends as an event: exit point, direction, wavelength, arrival time, and whether it was absorbed or reached a detector. | `R + T = 1` to 1e-9 at a flat interface over 10⁶ photons; the refracted angle matches Snell to 1e-12; after two internal reflections the distance travelled satisfies `d = c·t/n`; transmission falls as `e^{−αd}` | 5 | **Done.** crates/canlab-optics (`block`, `fresnel`, `transport`); `tests/transport_validation.rs`. **Partly verified:** `R + T = 1` is checked over a million *traced* photons rather than at one flat interface, and the transmission law is checked as a bound rather than against `e^{−αd}` exactly.
| W14.2 | The SiPM model: photon-detection efficiency per wavelength, a microcell count with saturation, dark counts, avalanche time jitter, and the waveform they make together. | at low occupancy the counts are Poisson with the expected mean; occupancy bends the response where the microcell count says it should; a fixed jitter reproduces its own standard deviation in the histogram | 4 | **Done.** crates/canlab-optics (`detector`); dark counts and the per-wavelength PDE curve are **not** modelled — see the note below the table.
| W14.3 | The time base: 1 ps bins over a nanosecond window, `f64` throughout (`2⁻⁵²` relative means a 500 ps transit resolves to 1e-13 s), and the waveform plotted as a curve of its own. | a known cable delay shifts the whole histogram by exactly that delay; the bin counts sum to the photon total | 3 | **Done.** crates/canlab-optics (`timebase`), drawn by `canlab-plot`; `tests/waveform.rs`.
| W14.4 | Validation against closed forms: Fresnel's curves for both polarisations from 0° to 90°, the Brewster angle, the critical angle for total internal reflection, and a pulse's spread through the slab against `L·n/c` plus the geometric estimate. | each check fails when the formula in the code is altered, and passes where the closed form says | 3 | **Done.** crates/canlab-optics (`fresnel`, `cherenkov` unit tests and `transport_validation.rs`).
| W14.5 | The source: a point emitter with a wavelength distribution, and — if the detector is a Cherenkov one — the cone `cos θ = 1/(nβ)` as an emission distribution rather than a point. | the cone angle matches `arccos(1/(nβ))`; a β below threshold emits nothing | 3 | **Done.** crates/canlab-optics (`cherenkov`, `source`).

W14 is done in the library and not in the UI, and two parts of W14.2 are
deliberately absent rather than forgotten: **dark counts** (a sensor's own noise
floor) and **a PDE curve** (efficiency against wavelength, where the generic
model uses one number). Both need a part number to be worth modelling, and
inventing them would be a model with no measurement behind it — the same
decision `docs/lessons.md` records for surface roughness. The 3D view's waveform
panel is W8's work, and the histogram already produces the `Curve` it would
draw.

### W15 — Units and quantities

A physics answer is `4.2 m/s`, not `4.2`. This is the workstream the AbiMix physics
corpus exposed, and no part of it exists today.

| # | What | Verify | Days |
|---|---|---|---|
| W15.1 | `Quantity { value: Expr, unit: Unit }` with unit algebra (product, quotient, power), SI prefixes, and conversion. Printing carries the unit into the LaTeX. | `4.2 m/s` survives a round-trip; `kg·m/s²` reduces to `N`; `m + s` is refused with both dimensions named | 5. **Done.** `crates/canlab-core/src/quantity.rs`: `Unit` as a map from base units to exponents with a scale, `parse_unit` (`m/s`, `kg*m/s^2`, `kg/(m*s^2)`, `km`, `μs`), `Quantity` with `add`/`sub`/`mul`/`div`/`pow`, and the SI derived units as a *lookup* so `kg·m/s²`, `N/m²` and `kg/(m·s²)` all print as `N` and `Pa`. **Verified:** `1 km + 500 m = 1.5 km` exactly, `10 m / 2 s = 5 m/s`, `kg*m/s^2` prints `N`, and `m + s` is refused naming `length` and `time`. **Not done:** the LaTeX printing takes the value's printer from the caller (`canlab-core` is below `canlab-tex`), no parser syntax for a quantity, and no significant figures — that is W15.3. |
| W15.2 | Dimensions checked where it matters: an equation whose sides disagree in dimension is refused before solving, and a solved value inherits its unit. | `F = m·a` is consistent; `x = 3 m + 2 s` is refused; a solved velocity prints `m/s` | 3. **Done.** `crates/canlab-core/src/dimension.rs`: `dimensions` walks an expression and returns a `Unit`, `check_equality` compares two sides, `unit_of` reads the unit an unknown must have, and `solve_with_units` checks before solving. **Verified:** `N = kg·m/s²` is consistent; `3 m + 2 s` is refused naming `length` and `time`; `v − 3m/2s = 0` gives `v` the unit `m/s`; `sin(3 m)` is refused; `√(4 m²)` is `m` and `√(m)` is refused. **Not done:** a unit is a *symbol whose name parses as a unit*, so `m` is always a metre and never a mass — the ambiguity is stated in the module doc, and a document that needs `m` as a variable is one this checker cannot help. |
| W15.3 | Significant figures: an answer states its precision, propagates it through one operation by the school rule, and never prints more digits than it has. | `1.2 m × 3.45 m` gives `4.1 m²`; the digit count matches the inputs'; an exact answer still prints exactly | 3. **Done.** `crates/canlab-core/src/sigfig.rs`: `Precision` (significant figures *and* decimal places, because the two rules differ), `Measured` with `mul`/`div`/`add`/`sub`, and `to_text` printing exactly the digits the precision allows. **Verified:** `1.2 × 3.45 = 4.1`, `1.2 + 3.45 = 4.7` (the decimal-place rule), `2 × 1.2 = 2.4` (an exact count does not limit), `120` has two figures and `120.` three, and an exact answer prints `4` and not `4.0`. **Rounding is half-up, not half-even:** `4.65` is a tie, and every exam answer key says `4.7`; the statistically correct rule would say `4.6` and be marked wrong. **Not done:** the precision does not travel with a `Quantity` yet, and no parser syntax carries a literal's digits through the CAS — the module is the arithmetic, not the wiring. |

## 4. Sequence and effort

197 units of implementation, 256 with the review rounds section 3 allows for.
Tiers are shipping points: each one is a coherent tool, not a half-built one.

| Milestone | Workstreams | Units | What you can do at the end |
|---|---|---|---|
| **M0 Stack and instruments** | W0 | 15 | the app runs on Bevy with egui inside it, screenshots happen in `cargo test`, a benchmark table exists, and the corpus runner works. No new maths. |
| **M1 Short syllabus and workflow** | W1, W2 (W2.1, W2.1b, W2.2–W2.7), W3, W5.1, W10, W11, W13 | 78 | type a formula and get an answer without pressing anything; exact and decimal side by side; inequalities, logs, trig, limits, definite integrals, combinatorics; shaded regions and tangents; autofill; paste an exam formula. |
| **M2 Long syllabus** | W2.8, W4, W5.2, W5.3 | 18 | vectors, matrices, complex numbers, nonlinear systems, normal and binomial distributions, regression. |
| **M3 Physics problems** | W15 | 11 | a physics answer is a quantity with a unit and a sensible number of digits, and the AbiMix physics corpus can be scored. |
| **M4 The compilation pipeline** | W7 | 23 | a formula becomes readable Lua, the Lua runs on the CPU, and a Lua program compiles to WGSL when you press Compile — with the measured CPU/GPU difference shown beside the artefact. |
| **M5 Light transport and detectors** | W14 | 18 | photons through a high-index block, Fresnel and total internal reflection, a SiPM waveform in picoseconds, validated against closed forms on both backends. |
| **M6 Differential equations** | W6 | 12 | `y' = …` with initial conditions, RK4 and DOPRI5, phase plots, and the water, air and sound scenarios passing their invariants. |
| **M7 3D** | W8 | 17 | surfaces, implicit meshes, orbit, picking, the ODE path and the photon paths in one scene. |
| **M8 Scripting** | W9 | 5 | Python where it exists, and script cells that turn tables into plots and call a compiled kernel. |

Two ways to read the total. At four and a half days of work a week — how the last
roadmap actually ran — 197 units is about 44 weeks. At the 20 h/week we agreed,
which is closer to two and a half days, it is well over a year. The last
projection was several times too pessimistic, which is why the numbers are
labelled as weights: I would bet on the lower end and would not bet a date on
either. What is a commitment is the order, and the gates: M1 is the first version
that answers a real exam question, and M5 is the first that answers the CanSat
question. If neither is doing that, later tiers are the wrong place to be.

**What I would cut if time is short.** W14.5 (Cherenkov) unless the detector is a
Cherenkov one — Q3 decides it. W15.3 (significant figures) after W15.1, since one
unit is worth more than correct rounding. W5.2 and W5.3 only if probability is not
on the exam you care about. W8.5's picking. W9.1 (Python) — Lua alone proves the
concept and this machine cannot run Python anyway. Nothing in W0, W1, W2, W7.1 or
W7.2: those are the difference between a demo and a tool.


## 5. Comments on the roadmap

**C1 — "Nearly all AbiMix problems" is not measurable as written.** The exams
are PDFs and the practice tool is an app; the questions are not machine-readable
LaTeX. The only honest definition is a transcribed corpus — one small TOML per
problem with the LaTeX you would type and the expected answer — and a report that
gives a pass rate *per capability*, because a CAS that does 95% of derivatives
and 0% of probability is not half done. W0.4 and W11 exist for this.

**C2 — The modelling step is yours.** A problem says a ladder leans against a
wall and a boy pulls it; the CAS gets `x² + y² = 25, y = 3`. Expect to be the
one who writes the equations. This is not a limitation to fix, it is where the
exam marks are, and the plan should not pretend otherwise.

**C3 — Exam answers are often three significant figures.** The CAS currently
has no way to say "≈ 2.45". W1.5 is small and it is on the critical path for
looking like the tool the exam expects.

**C4 — `Vec<Expr>` is the wrong answer type, and it is load-bearing.** See D2.
The change is cheap now and expensive after autofill, 3D and scripting all read
the answer type.

**C5 — Compile programs, not formulas, and compile them on request. (Revised.)**
The first draft of this document said not to compile at all. That was wrong for
one case and right for the other. A formula does not need a compiler: `Program`
evaluates it, cheaply, and a compiler would add shader latency and a pipeline
cache for a factor of two. A Monte Carlo loop does need one: it has control flow
and randomness, which an expression VM cannot express, and that is precisely the
CanSat case. The codegen cost is real and it is paid, not avoided — so make it
explicit and cached (W7.3): a Compile button, a content hash, a viewer for what
came out, and a measured CPU/GPU difference stored beside the artefact.

**C6 — WGSL has no `f64`.** The single most consequential unknown for "performance
is critical". f32 may be plenty for a rendered ray path and is not plenty for a
simulation whose numbers you intend to trust. W7.5 decides it with a probe, and
the answer changes the design of the glass block.

**C7 — The glass block is a ray tracer, not an ODE.** A homogeneous slab is Snell
plus Fresnel plus Monte Carlo scattering: a photon is independent of every other
photon, which is what makes the GPU the right engine and an integrator the wrong
one. An *inhomogeneous* medium is the case that needs an ODE in
`(position, momentum)` with `∇n`, and a Cherenkov source with dispersion needs
`n(λ)`. So W6 is not a prerequisite for the CanSat work; it is a prerequisite for
the graded-index variant.

**C8 — Bevy costs a downgrade, and it was accepted.** `bevy_egui` 0.43 targets
egui 0.36; the editor is on 0.37. Cargo can hold both, but a `Ui` from one cannot
be passed to the other, so the whole UI stack moves to 0.36 and `egui_kittest`,
`eframe` and the screenshot path move with it. The decision was to take it
(Decisions taken, D5). The consolation is that the fallback — eframe 0.36 plus
`egui_wgpu::Callback` — sits on the same egui, so nothing is paid twice if the
Bevy build turns out not to fit on this machine.

**C9 — Compile times, now measured.** The first draft of this comment guessed. The
numbers from this machine (6 cores, 7.6 GiB, `-j 1`), all for one changed file in
`canlab-gui`:

| Configuration | Time |
|---|---|
| Full debug info, `link.exe` | 79.7 s |
| `debug = "line-tables-only"`, `link.exe` | 40.5 s |
| `debug = "line-tables-only"`, `rust-lld` | 37.3 s |

So debug info was half the cost and the linker is worth 8%, not the 3× that
"use lld" folklore promises — because the remaining time is dominated by writing
and scanning a ~1 GB executable, not by the linker's algorithm. `rust-lld` is
kept because it is free, not because it is dramatic.

The one big lever left is `bevy/dynamic_linking`, and it does not link on Windows
without the performance profiles: `link.exe` fails with **LNK1189, too many
exported symbols**. The recipe is `[profile.dev] opt-level = 1` plus
`[profile.dev.package."*"] opt-level = 3`, which rebuilds the dependency tree
once and then links in seconds — worth doing when a milestone starts, not in the
middle of one. Every test binary also needs the Bevy dylib on `PATH` afterwards.

What does not help: `sccache` cannot cache crates that invoke the linker (`bin`,
`dylib`, `cdylib`, `proc-macro`), which is exactly what a test binary is, so a
remote compile cache speeds up the 500 dependencies on a cold build and nothing
in the daily loop.

**C10 — Simulation tests should assert physics, not numbers.** "RK4 error falls
16× when `h` halves", "energy drifts less than 0.1%", "`R + T = 1`", "the ray in
`n(y) = n₀ + αy` is an exact arc" — these survive a refactor. A stored array of
1 000 doubles does not, and it will be regenerated whenever it fails, which makes
it worse than no test.

**C11 — Autofill on Tab collides with a deliberate decision.** `Tab` and `Esc`
were left to focus traversal on purpose. Intercepting `Tab` only while a ghost is
live keeps that, but the `EventFilter` and the key handler have to agree or the
ghost will never see the key. W10.2 is written with that in mind.

**C12 — Every approximate answer must be marked.** Numeric roots, numeric
integrals, a refused factorisation, a declined limit — each needs a visible
state. The project already has the discipline for declines ("declined" is not an
error); extend it to "approximate, with a bound".

**C13 — Ambition and review are in tension, and review has been right every
time.** Twelve wrong-answer bugs came out of review in the last round, plus three
regressions from fixing them. More capability means more of that. The mitigation
is not more care, it is the soundness gate in section 7: a rule that cannot be
checked numerically does not ship.

## 6. Questions still open

The eight that were open have answers; see [Decisions taken](#decisions-taken).
These five remain, each with the default I will use if an answer never comes.

**Q1 — Physics: short or long syllabus, or both?** The AbiMix physics exam has the
same two tracks as the mathematics one. *Default: both, short first.*

**Q2 — The hardware.** The glass — refractive index, size, absorption coefficient,
and whether dispersion `n(λ)` matters — and the SiPM: a part number with its
datasheet, or a generic model. Real numbers let W14.2 be checked against a
datasheet curve instead of against itself. *Default: a generic borosilicate-like
block (`n = 1.52`, 100 × 100 × 25 mm, `α` small) and a generic SiPM (PDE 25% at
420 nm, 3 600 microcells, 50 ps jitter, 100 kHz dark rate).*

**Q3 — What emits the light?** A point emitter (an LED, a scintillator flash) or
Cherenkov radiation from a charged particle crossing the block. The second makes
`n(λ)` mandatory and makes W14.5 real; the first makes it unnecessary. *Default:
point emitter first, Cherenkov if this is a Cherenkov detector.*

**Q4 — Python: the system interpreter over the subprocess, or pure-Rust
RustPython?** The first needs Python installed (not on this machine) and cannot
crash the editor; the second needs nothing and is slower and incomplete.
*Default: the subprocess.*

**Q5 — Does an answer need to be exportable** — a Markdown or PDF sheet of the
document and its results, for handing in? Not on the roadmap, and a real gap in
the workflow. *Default: Markdown now, PDF later if asked.*

## 7. Definition of done for a capability

A capability is finished when all five hold. This is the answer to C13, and it
should be the reviewer's checklist. An internal item — the evaluator's allocation,
the compiler's IR — is judged by its own Verify column and item 3 alone: a corpus
row and a UI snapshot are for things a user can see.

1. **An API in a crate**, with no `egui` in it, and a unit test.
2. **A refusal that names its reason** for every input it cannot handle, **and a
   test that asserts the refusal and the reason**. A panic is not a refusal, and a
   refusal without a test is a claim. See [lessons.md](lessons.md) for the plot
   that drew a surface's slice because nothing tested a refusal.
3. **A numerical check.** A symbolic rewrite must agree with the plain evaluator
   at 20 random points (a wrong simplification is otherwise invisible). A solve
   must substitute back to zero. A derivative must match a central difference of
   `Program::eval`. An integral must match the antiderivative's difference. An ODE
   must show the right convergence order. A compiled kernel must agree with the
   interpreter on 10³ sample points, and the worst difference it found must be
   stored with the artefact rather than assumed.
4. **A corpus row** that a student could have typed, with the expected answer, in
   the batch runner.
5. **A visible state in the UI** for success, decline and approximation — and a
   snapshot test that the three look different.

## 8. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| `egui_kittest` finds no adapter on this machine | no headless snapshots, W0 slips | add a software-adapter fallback; a missing adapter skips, it does not fail |
| No `SHADER_F64` and f32 is not enough | the glass result cannot be trusted numerically | double-double path for the transport only; the CPU reference is always available |
| Bevy's cold build exhausts memory or takes an hour | M0 stalls before any maths | the probe is built first and timed; the fallback (eframe 0.36 + `egui_wgpu::Callback`) sits on the same egui, so the downgrade work is not lost |
| The SiPM model is only as good as a datasheet we do not have | W14.2 measures itself | the generic model is labelled generic in the UI, and Q2 asks for a part number |
| Algebra scope creeps (a full CAS is a decade) | M1 never finishes | the corpus defines the scope: a rewrite rule lands only with a corpus row that needs it |
| Numeric answers get treated as exact | wrong physics, wrong exam answers | exact-or-bounded is enforced by the answer type, not by convention |
| Review capacity | wrong answers ship | section 7, and the reviewer stays read-only |
