# CanLab

A LaTeX-native computer algebra system with graphing, in Rust.

The design principle is the one the plan started from:

> **LaTeX is the interchange format, and round-tripping is the invariant.**

`parse(print(e)) == e` for every canonical expression. That single property test
ties the parser and the printer together and catches precedence bugs in either
one.

---

## Layout

| Crate | Responsibility |
|---|---|
| `canlab-core` | `Expr` tree, exact `Number`, canonicalization, compiled `f64` evaluation |
| `canlab-tex` | `Expr` → LaTeX |
| `canlab-parse` | LaTeX → `Expr` (lexer + Pratt parser) + the Abitti palette as data |
| `canlab-input` | the **structural math field**: a caret that lives in the tree, empty slots, MathQuill key semantics, document-wide definitions |
| `canlab-mathview` | layout and painting for the field (fractions, scripts, radicals, placeholders, carets) |
| `canlab-solver` | `Solver` trait, tagged-JSON wire format, native exact solver, SymPy discovery and worker subprocess |
| `canlab-plot` | adaptive sampling with pole/domain breaks, Newton-refined intersections, SVG export |
| `canlab-gui` | the desktop shell: centred editor, dockable panels, seven palettes, undo, sessions, background jobs |
| `canlab-cli` | `canlab` binary: one-shot subcommands, `solver --probe`, and a REPL |

Two dependency arrows matter. Everything depends on `canlab-core`, and
`canlab-core` depends on nothing. The GUI depends on the field and the view, but
the field and the view know nothing about the GUI — which is what would make a
different shell (Bevy, say) a one-crate port rather than a rewrite.

---

## What it does

| Capability | Behaviour |
|---|---|
| Definitions | `f(x) = x² + 1` defines a name for the whole document, resolved in dependency order; `f(3)` substitutes including built-in calls, `f'(x)` and `f''(x)` differentiate, cycles are refused, and a definition that depends on a broken one is reported. A space before the bracket is still a call. |
| Undo and redo | A 100-snapshot ring with 400 ms typing coalescing; Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y, and buttons in the editor header. |
| Parameters | A variable that is not on an axis gets a drag control in the plot overlay, default 1. Automatic axis choice prefers the familiar axis names. |
| Systems | Linear systems beyond 2×2 by exact native Gaussian elimination, including when SymPy declines a system: a point, no solution, or a declined underdetermined system, never one point from a family. |
| Plot | Drag to pan, scroll to zoom about the pointer; intersections are Newton-refined, drawn as dots and named on hover; the view fits them. |
| Sessions | The document, palette, plot window, axes and parameter values are saved 1.2 s after the last change, in a versioned text format. |
| Editor | Ctrl+C copies the document as LaTeX, Ctrl+V pastes; history entries load back; the gutter marks the caret's line. |
| Palettes | Seven whole schemes, each with its own surfaces, text and six curve colours; a palette pins light or dark, so there is no separate theme switch. |

---

## Three invariants worth knowing before you touch anything

**1. Raw constructors never normalize.** `ExprNode::add_raw` stores exactly what
you give it. `normalize::add` folds constants, collects like terms and sorts.
Mixing the two up is how normalization becomes recursive and quadratic.

**2. Canonical form.** `Add`/`Mul` are flattened, never empty, never length 1.
`Mul` may start with a numeric coefficient, and the remaining factors are
sorted. Comparison is structural, so `Expr: PartialEq` is *syntactic* equality
on canonical trees — it cannot see that `x/x = 1`.

**3. Exactness is a property of the value.** `2` and `2.0` are different
numbers and both survive. Anything that silently turns `1/3` into `0.333…` is a
bug, even when it "looks right".

---

## Quick start

```bash
cargo run -p canlab-gui        # the app
cargo test --workspace         # everything
cargo run -p canlab-cli -- eval "2x + 3x"
cargo run -p canlab-cli -- tex  "\frac{1}{2} + \frac{1}{3}"
cargo run -p canlab-cli -- plot "\sin x" -o sin.svg --xmin 0 --xmax 6.28
cargo run -p canlab-cli -- int  "x^2" -v x
cargo run -p canlab-cli -- solver          # what discovery found, or what is missing
cargo run -p canlab-cli -- solver --probe  # start the worker, import SymPy, answer one request
cargo run -p canlab-cli        # REPL
```

The GUI opens at 1180×760 with the editor centred: the LaTeX field is the point
of the screen, the solver runs on a worker thread behind it, and the panels dock
around the edges. Each panel has its own menu for splitting and closing, the
seams between them drag, and the **Workspace** menu swaps a whole preset layout.
The **Settings** menu lists the seven palettes — Midnight, Graphite, Nord,
Solarized Dark, Contrast, Solstice and Solarized Light — each button painted in
its own colours. Solstice and Solarized Light are light; the rest are dark. There
is no separate theme toggle.

These choose a backend, a layout or a file without changing the code:

| Variable | Effect |
|---|---|
| `CANLAB_PALETTE` | palette for this run, for example `CANLAB_PALETTE=Nord` |
| `CANLAB_SOLVER` | `native` forces the native backend; otherwise SymPy is used only after its probe request succeeds |
| `CANLAB_SESSION` | session file path, or `none` to turn saving and loading off |
| `CANLAB_OPEN` | document to open, lines separated by a vertical bar |
| `CANLAB_WORKSPACE` | preset layout: `editing`, `solving` or `plotting` |
| `CANLAB_PYTHON`, `CANLAB_WORKER` | interpreter and `worker.py` location; an existing interpreter path may contain spaces; see [docs/sympy.md](docs/sympy.md) |

---

## Constraints of this build (honest list)

**Exact arithmetic is `i128`, not arbitrary precision.** Every operation is
checked and degrades to a float on overflow rather than wrapping or panicking.
`2^100` is therefore approximate; everyday algebra is exact. Swapping in a
bigint later is contained to `number.rs`.

**The solver has two backends, and only one of them has run here.** The native
backend is always available: exact linear and quadratic equations, polynomial
calculus, and Gaussian elimination for systems. SymPy is used only after one probe
request comes back: discovery finds an interpreter and `worker.py`, giving each
candidate 700 ms, but `open()` starts the worker and asks a trivial simplification
before the panel says SymPy, so a Python without the `sympy` package falls back to
native and the note explains why. `CANLAB_SOLVER=native` forces native. If SymPy
declines a system, native elimination answers it; a fatal SymPy failure is retried
once on native with the panel saying so. Discovery and the worker protocol are
tested in memory, but there is no Python on this machine, so the worker has never
imported SymPy here. `canlab solver --probe` is the check, and
[docs/sympy.md](docs/sympy.md) is the packaging note.

**A declined solve is a real answer.** When the backend cannot do something it
says so, and the UI shows the expression unevaluated rather than inventing a
result. `SolverError::NotImplemented` travels all the way to the screen.

---

## Testing strategy

- Unit tests at the bottom of each module.
- The parser/printer round-trip is the load-bearing property test.
- `canlab-cli/tests/cli.rs` drives the compiled binary, so it fails if any two
  crates disagree about an API or an output format.
- `canlab-input/tests/abitti_parity.rs` is a corpus of **observed** behaviour:
  every case was typed into the live editor at <https://math-demo.abitti.fi/> with
  Playwright and the expected LaTeX is what that editor answered. If the model and
  the corpus disagree, the model is wrong.
- `canlab-input/tests/acceptance.rs` is a corpus of student documents and their
  expected typed form, value or number. It prints the Markdown table kept in
  `docs/acceptance.md`, so the document and the test cannot drift.
- The exam corpus format and the `canlab corpus run` contract are specified in [docs/corpus.md](docs/corpus.md); the runner is W0.4 and does not exist yet.
- `canlab-gui` paints whole frames headlessly with a real `egui::Context`, which
  is what would catch a panic that shows up as a blank window.

Current state: **over 500 tests**, all green.

```bash
cargo test --workspace
```

### Looking at the maths without a window

A GPU-composited window cannot be screenshotted over a remote session
(`PrintWindow` returns a blank bitmap), but the layout is pure geometry:

```bash
cargo run -p canlab-mathview --example layout_svg -- layout.svg layout.json
python crates/canlab-mathview/tools/render_layout.py layout.json layout.png
```

`layout.svg` opens in a browser; the Python renderer needs Pillow and draws the
same items to a PNG, including every caret position and the faint half of a ghost
bracket.

### Capturing a frame, without a window

The interface renders itself in a test, through `wgpu`, into an offscreen
texture. Nothing is presented, so nothing appears on the desktop:

```bash
cargo test -p canlab-gui --offline -j 1 snapshot
```

The images live in `crates/canlab-gui/tests/snapshots/` and are compared on every
run; `UPDATE_SNAPSHOTS=1` regenerates them. Three frames are kept: the default
document, a circle, and the refusal a surface gets. `CANLAB_OPEN` and
`CANLAB_WORKSPACE` are not needed — a snapshot builds its own document.

The end-to-end milestone the whole thing is aimed at:

```bash
$ cargo run -q -p canlab-cli -- int "x^2" -v x
\frac{1}{3}x^{3}
```

LaTeX in, exact mathematics out, typeset as LaTeX.

---

## Input dialect: Abitti / MathQuill

The LaTeX vocabulary targets the editor Finnish upper-secondary students use in
the matriculation examination — MathQuill inside
[`digabi/rich-text-editor`](https://github.com/digabi/rich-text-editor) (MIT,
© 2017 Matriculation Examination Board). The palette, the shortcuts and the
support matrix are documented in [docs/abitti-dialect.md](docs/abitti-dialect.md),
and the palette itself is data in `canlab-parse/src/palette.rs`.

The point is not to clone the look; it is that a student can type
`\frac{1}{2}`, `\sqrt[3]{x}`, `\varepsilon` or `\mathbb{Z}` and have it mean
what it means in the exam. The interaction model — where the caret goes when you
type `/`, what backspace does inside an empty slot — was reverse-engineered and
is written down in [docs/mathquill-interaction.md](docs/mathquill-interaction.md).
