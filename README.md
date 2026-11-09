# cas

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
| `cas-core` | `Expr` tree, exact `Number`, canonicalization, compiled `f64` evaluation |
| `cas-tex` | `Expr` → LaTeX |
| `cas-parse` | LaTeX → `Expr` (lexer + Pratt parser) |
| `cas-solver` | `Solver` trait, tagged-JSON wire format, SymPy worker subprocess |
| `cas-plot` | Adaptive sampling with pole/domain breaks, SVG export |
| `cas-cli` | `cas` binary: one-shot subcommands and a REPL |

The dependency arrow always points inward: everything depends on `cas-core`,
and `cas-core` depends on nothing.

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
cargo test                  # everything
cargo run -p cas-cli -- eval "2x + 3x"
cargo run -p cas-cli -- tex  "\frac{1}{2} + \frac{1}{3}"
cargo run -p cas-cli -- plot "\sin x" -o sin.svg --xmin 0 --xmax 6.28
cargo run -p cas-cli -- int  "x^2" -v x
cargo run -p cas-cli            # REPL
```

---

## Constraints of this build (honest list)

**No third-party dependencies.** This machine has no network access to
crates.io, so every crate uses only the standard library. That is a constraint,
not a preference — `serde`, `num-bigint` and `egui` are all unavailable here.

**Exact arithmetic is `i128`, not arbitrary precision.** Every operation is
checked and degrades to a float on overflow rather than wrapping or panicking.
`2^100` is therefore approximate; everyday algebra is exact. Swapping in a
bigint later is contained to `number.rs`.

**There is no GUI.** `egui`/`eframe` cannot be fetched. The plotting is
nonetheless real: `cas-plot` produces SVG, which any browser or document can
consume.

**The solver is a mock by default.** There is no Python on this machine, so the
SymPy worker (`cas-solver/python/worker.py`) is written but not exercised here.
What *is* tested is the wire format and the protocol session, in memory. The
`Solver` trait is the seam: swapping in the real backend changes one file.

---

## Testing strategy

- Unit tests at the bottom of each module.
- The parser/printer round-trip is the load-bearing property test.
- `cas-cli/tests/cli.rs` drives the compiled binary, so it fails if any two
  crates disagree about an API or an output format.

Current state: **232 tests**, all green — `cas-core` 28, `cas-parse` 60,
`cas-tex` 43, `cas-plot` 28, `cas-solver` 61, `cas-cli` end-to-end 8, plus 4
doctests.

```bash
cargo test --workspace
```

The end-to-end milestone the whole thing is aimed at:

```bash
$ cargo run -q -p cas-cli -- int "x^2" -v x
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
and the palette itself is data in `cas-parse/src/palette.rs`.

The point is not to clone the look; it is that a student can type
`\frac{1}{2}`, `\sqrt[3]{x}`, `\varepsilon` or `\mathbb{Z}` and have it mean
what it means in the exam.

