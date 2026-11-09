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
| `canlab-input` | the **structural math field**: a caret that lives in the tree, empty slots, MathQuill key semantics |
| `canlab-mathview` | layout and painting for the field (fractions, scripts, radicals, placeholders, carets) |
| `canlab-solver` | `Solver` trait, tagged-JSON wire format, SymPy worker subprocess |
| `canlab-plot` | Adaptive sampling with pole/domain breaks, SVG export |
| `canlab-gui` | the desktop shell: centred editor, dockable panels, background jobs, dark mode |
| `canlab-cli` | `canlab` binary: one-shot subcommands and a REPL |

Two dependency arrows matter. Everything depends on `canlab-core`, and
`canlab-core` depends on nothing. The GUI depends on the field and the view, but
the field and the view know nothing about the GUI — which is what would make a
different shell (Bevy, say) a one-crate port rather than a rewrite.

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
cargo run -p canlab-cli        # REPL
```

The GUI opens maximised with the editor centred: the LaTeX field is the point of
the screen, the solver runs on a worker thread behind it, and the panels dock
around the edges. Each panel has its own menu for splitting and closing, the
seams between them drag, the **Workspace** menu swaps a whole preset layout, and
the **Theme** button switches dark and light.

---

## Constraints of this build (honest list)

**Exact arithmetic is `i128`, not arbitrary precision.** Every operation is
checked and degrades to a float on overflow rather than wrapping or panicking.
`2^100` is therefore approximate; everyday algebra is exact. Swapping in a
bigint later is contained to `number.rs`.

**The solver is a mock by default.** There is no Python on this machine, so the
SymPy worker (`canlab-solver/python/worker.py`) is written but not exercised
here. What *is* tested is the wire format and the protocol session, in memory.
The `Solver` trait is the seam: swapping in the real backend changes one file.

**A declined solve is a real answer.** When the backend cannot do something it
says so, and the UI shows the expression unevaluated rather than inventing a
result. `SolverError::NotImplemented` travels all the way to the screen.

---

## Testing strategy

- Unit tests at the bottom of each module.
- The parser/printer round-trip is the load-bearing property test.
- `canlab-cli/tests/cli.rs` drives the compiled binary, so it fails if any two
  crates disagree about an API or an output format.

Current state: **366 tests**, all green — `canlab-core` 28, `canlab-parse` 68,
`canlab-tex` 43, `canlab-plot` 28, `canlab-solver` 61, `canlab-input` 75,
`canlab-mathview` 32, `canlab-gui` 12, `canlab-cli` end-to-end 8, plus
integration and doc tests.

```bash
cargo test --workspace
```

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
