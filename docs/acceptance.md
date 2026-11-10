# Acceptance corpus

Every row is a document a student could type, and what it should do. The table is
executable: it lives in
[`crates/canlab-input/tests/acceptance.rs`](../crates/canlab-input/tests/acceptance.rs)
and prints itself.

```bash
cargo test -p canlab-input --test acceptance -- --nocapture
```

Two layers appear here, and mixing them up was the first mistake this file
caught:

- **Typed form** — what the LaTeX pane shows, which is what the editor holds;
- **Value** — what the result card shows, which is what it means.

| Document | Why | Expected | Kind |
|---|---|---|---|
| `2x+3x` | like terms collect | `5x` | value |
| `\frac{1}{2}+\frac{1}{3}` | exact rational arithmetic | `\frac{5}{6}` | value |
| `\sqrt{2}` | a radical is a value | 1.4142135623730951 | number |
| `2*3` | `*` is a multiplication sign | `6` | value |
| `2x` | juxtaposition is multiplication | reads as `2x` | reads |
| `80°` | a unit marker is notation, not an operator | 80 | number |
| `\log_2(10)` | a subscript on a function is its base | 3.321928094887362 | number |
| `e^{-x^2}` | a negative exponent keeps its sign | `e^{-x^{2}}` | value |
| `-x^2` | a leading minus is a sign, not a factor | `-x^{2}` | value |
| `\bar{x}` | an accent is a decoration, not a scalar | refused | refused |
| `(x+1)^2` | bare parentheses are a group | `\left(1 + x\right)^{2}` | value |
| `x^2+y^2=1` | an equation is not an expression | refused | refused |
| `f'(x)` | derivatives need function definitions (roadmap P0) | refused | refused |
| `x_1` | a subscript on a variable is not algebraic | refused | refused |

## What the corpus found, on its first run

Written down because it is the point of the exercise:

1. **Four of my own expectations were wrong.** `to_latex()` returns the *typed*
   form, not the simplified one — `2x+3x` prints as typed, and only the result
   card prints `5x`. The corpus now has a `Simplified` expectation for the value
   layer, which made the distinction explicit instead of accidental.
2. **`\log_2(10)`** — I had written an imprecise constant; the app was right.
3. **`-1x^{2}`** — a real wart: a unit coefficient printed as a factor. `-x²` is
   what everyone writes. Fixed in `canlab-tex`.
4. **`e^{\left(-x^{2}\right)}`** — the exponent was parenthesised even though the
   braces already group. Fixed; `x^{-2}`, `x^{a + b}` now read cleanly.
5. **`\bar{x}` is refused**, and that is deliberate: an accent is a decoration,
   not a scalar. If you disagree — if `\bar{x}` should read as *the mean of x* —
   that is a semantics decision worth making explicitly rather than by accident.

## Cases that are not here yet, and why

- **Systems and their solutions** — `x+y=3`, `2x-y=0` → `x = 1, y = 2`. Belongs in
  a solver corpus with the same table format, and should be added when the
  solver's reach is settled.
- **Plot expectations** — the intersection count of two circles. Needs the GUI to
  expose its curves in a testable way; today those live in `canlab-gui`'s unit
  tests.
- **Anything involving `f`** — blocked on definitions (roadmap P0).
