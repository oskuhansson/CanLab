# Acceptance corpus

Every row is a document a student could type and what it should do. The table is
**generated from the test itself**, so it cannot drift:

```bash
cargo test -p canlab-input --test acceptance -- --nocapture
```

Three layers appear here, and mixing them up was the first mistake this file
caught:

- **typed form** - what the LaTeX pane shows, which is what the editor holds;
- **value** - what the result card shows, which is what it means;
- **number** - what it evaluates to.

| Document | Why | Expected | Actual |
|---|---|---|---|
| `1+2*3` | precedence | `7` | `7` |
| `2*3` | star is a multiplication sign | `6` | `6` |
| `10^6` | an exact integer power | `1000000` | `1000000` |
| `2^100` | beyond i128, degrades to a float | 1267650600228229400000000000000 | 1267650600228229400000000000000 |
| `2^10` | a modest power | `1024` | `1024` |
| `2^{-1}` | a negative power is a reciprocal | `\frac{1}{2}` | `\frac{1}{2}` |
| `(1+2)*3` | bare parentheses group | `9` | `9` |
| `-3+5` | a leading minus | `2` | `2` |
| `-7` | a negative literal | `-7` | `-7` |
| `0` | zero | `0` | `0` |
| `x--y` | double minus is a plus | `x + y` | `x + y` |
| `\frac{2}{4}` | reduces to lowest terms | `\frac{1}{2}` | `\frac{1}{2}` |
| `\frac{1}{2}+\frac{1}{3}` | exact rational arithmetic | `\frac{5}{6}` | `\frac{5}{6}` |
| `\frac{0}{5}` | zero over anything | `0` | `0` |
| `\frac{1}{3}*3` | the thirds cancel exactly | 1 | 1 |
| `1/2x` | the denominator keeps the x (Abitti) | `\frac{1}{2x}` | `\frac{1}{2x}` |
| `\frac{1}{2}x` | a coefficient is not part of a fraction | `\frac{1}{2}x` | `\frac{1}{2}x` |
| `x^2` | a power | `x^{2}` | `x^{2}` |
| `-x^2` | a leading minus is a sign, not a factor | `-x^{2}` | `-x^{2}` |
| `e^{-x^2}` | a negative exponent keeps its sign | `e^{-x^{2}}` | `e^{-x^{2}}` |
| `x^2y` | a script absorbs what follows it | `x^{2y}` | `x^{2y}` |
| `x^2+1` | LaTeX is unambiguous: the script ends at the operator | `x^2+1` | `x^2+1` |
| `\sqrt{16}` | a square root of a square | 4 | 4 |
| `\sqrt[3]{8}` | a cube root | 2 | 2 |
| `\sqrt{2}` | an irrational radical | 1.4142135623730951 | 1.4142135623730951 |
| `\sqrt{2}+\sqrt{2}` | like surds collect | 2.8284271247461903 | 2.8284271247461903 |
| `2^{0.5}` | a fractional exponent | 1.4142135623730951 | 1.4142135623730951 |
| `\sin 0` | sin of zero | 0 | 0 |
| `\cos 0` | cos of zero | 1 | 1 |
| `\ln 1` | ln of one | 0 | 0 |
| `\exp 0` | e to the zero | 1 | 1 |
| `\log_2(8)` | a logarithm with a base | 3 | 2.9999999999999996 |
| `\log_2(10)` | and an irrational one | 3.321928094887362 | 3.3219280948873626 |
| `2x` | juxtaposition is multiplication | `2x` | `2x` |
| `2*x` | an explicit sign is the same thing | `2x` | `2x` |
| `2x+3x` | like terms collect | `5x` | `5x` |
| `2.5x` | a decimal coefficient | `2.5x` | `2.5x` |
| `3xy` | three factors | reads | reads, `3x y` |
| `x2` | a digit after a letter | `2x` | `2x` |
| `abc` | a run of variables | reads | reads, `a b c` |
| `80°` | a degree marker is notation | 80 | 80 |
| `50%` | so is a percent sign | 50 | 50 |
| `x=1` | an equation is not an expression | refused | refused |
| `x<y` | neither is a relation | refused | refused |
| `x_1` | a subscript on a variable | refused | refused |
| `\bar{x}` | an accent is a decoration | refused | refused |
| `\vec{v}` | so is a vector arrow | refused | refused |
| `f'` | a derivative needs definitions (roadmap P0) | refused | refused |
| `(x+1)^2` | bare parentheses are a group | `\left(1 + x\right)^{2}` | `\left(1 + x\right)^{2}` |
| `\frac{x}{x}` | policy: never cancelled silently | reads | reads, `1` |
| `x/0` | division by zero has no value | refused | refused |

## What the corpus has found so far

Each of these was a real defect or a wrong belief, caught by writing the
expectation down before checking the behaviour:

1. **Four of my own expectations were wrong.** `to_latex()` returns the *typed*
   form, not the simplified one - `2x+3x` prints as typed, and only the result
   card prints `5x`. Hence the separate `value` layer above.
2. **`-1x^{2}`** - a unit coefficient printed as a factor. Now `-x^2`.
3. **`e^{\left(-x^{2}\right)}`** - an exponent parenthesised although the braces
   already group. Now `e^{-x^{2}}`.
4. **`2^10` read as `2^1 * 0`.** The snippet reader took a single character after
   `^`; it now takes the whole run, so `x^23` is x^23 and `x^2y` is x^(2y).
5. **`1/2x` read as literal text.** `/` is the fraction key when typing and now is
   one when reading, so a pasted `1/2` renders as a fraction.
6. **`\sqrt[3]{8}` was refused.** An indexed root is `x^(1/n)`; refusing a cube
   root was a limitation, not a policy.
7. **`x/0` became a NaN constant** and silently poisoned everything downstream.
   Division by zero is now refused.

## Deliberate refusals

`\bar{x}`, `\vec{v}`, `x_1`, `x=1` and `f'` are refused, and each is a decision
rather than a gap:

- an accent and a vector arrow are *decorations*, not scalars;
- a subscript on a variable has no algebraic meaning here (a subscript on a
  *function* does: it is the logarithm base);
- a relation is not a value - solve takes equations, it does not evaluate them;
- `f'` needs function definitions, which is roadmap P0.