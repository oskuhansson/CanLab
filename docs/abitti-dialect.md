# The Abitti / MathQuill LaTeX dialect

The input vocabulary this tool targets, taken from the editor Finnish upper
secondary students actually use in the matriculation examination.

**Source:** [digabi/rich-text-editor](https://github.com/digabi/rich-text-editor)
**Licence:** MIT, Copyright 2017 Matriculation Examination Board, Finland.
The palette tables below are derived from that repository
(`src/app/components/toolbar/math-char-data.ts` and `src/latexCommands.ts`) and
retain the same MIT terms. Attribution is kept in `NOTICE`.

**Live demo:** <https://math-demo.abitti.fi/> — "Kaavaeditorin kehitysversio".

---

## Why this matters

Abitti's editor is built on **MathQuill**. That is the important fact: the
dialect is not "LaTeX in general", it is MathQuill's LaTeX subset plus a fixed
palette. A student who types `\frac{1}{2}` in the exam expects it to look the
same here, and expects `\le` rather than `\leq` to be the thing the palette
inserts (it is — the palette emits `\leq`, and MathQuill renders both).

Design consequence: **accept what the palette can produce, and print in a form
the palette can re-parse.** Anything else is a nice-to-have.

---

## The keyboard shortcuts (from the demo page)

| Action (Finnish) | Key |
|---|---|
| Paste image | Ctrl-V |
| Write formula | Ctrl-E |
| Division line | `/` |
| Multiplication sign | `*` |
| Exponent | `^` |
| Close formula | `Esc` |
| Formula on next line | `Enter` |
| Undo / redo | Ctrl-Z / Ctrl-Y |

The behaviours worth copying: `/` becomes a proper fraction, `*` becomes `·`,
`^` opens a superscript box, and **Esc leaves the formula** rather than
cancelling it.

---

## Palette: quick symbols (row 1)

Raw characters unless a `latex` is given.

`°` · `·`→`\cdot` · `×`→`\times` · `±`→`\pm` · `∞`→`\infty` · `²`→`^2` ·
`³`→`^3` · `½`→`\frac{1}{2}` · `⅓`→`\frac{1}{3}` · `π`→`\pi` · `‰` ·
`α`→`\alpha` · `β`→`\beta`

### Greek, complete set
`\Gamma \gamma \Delta \delta \varepsilon \zeta \eta \theta \vartheta \iota
\kappa \Lambda \lambda \mu \nu \Xi \xi \Pi \rho \Sigma \sigma \tau \Upsilon
\upsilon \Phi \phi \chi \Psi \psi \Omega \omega \partial \varphi`

Note `\varepsilon` (not `\epsilon`), `\vartheta`, `\varphi`. Getting those
wrong is immediately visible to a Finnish student.

## Palette: algebra

`≠`→`\neq` · `≈`→`\approx` · `≤`→`\leq` · `≥`→`\geq` · `<` · `>` ·
`∼`→`\sim` · `≡`→`\equiv` · `≢`→`\not\equiv` · `∘`→`\circ` ·
`…`→`\ldots` · `∝`→`\propto`

## Palette: geometry

`∢`→`\sphericalangle` · `|`→`\mid` · `‖`→`\parallel` ·
`⇌`→`\xrightleftharpoons` · `⇅` · `∠`→`\angle` · `↑`→`\uparrow` ·
`↗`→`\nearrow` · `↘`→`\searrow` · `↓`→`\downarrow` ·
`↔`→`\leftrightarrow` · `⊥`→`\perp`

## Palette: sets and logic

`→`→`\rightarrow` · `⇒`→`\Rightarrow` · `⇔`→`\Leftrightarrow` · `∈`→`\in` ·
`ℤ`→`\mathbb{Z}` · `ℝ`→`\mathbb{R}` · `∃`→`\exists` · `∀`→`\forall` ·
`ℕ`→`\mathbb{N}` · `ℚ`→`\mathbb{Q}` · `∩`→`\cap` · `∪`→`\cup` ·
`∖`→`\setminus` · `⊂`→`\subset` · `⊄`→`\not\subset` · `∉`→`\notin` ·
`∅`→`\varnothing` · `∧`→`\wedge` · `∨`→`\vee` · `¬`→`\neg` · `∇`→`\nabla`

## Palette: structures (rows 2–3)

These are the entries that build *shape*, not just a glyph. `X` marks where the
cursor lands.

| Label | Inserted LaTeX |
|---|---|
| √ | `\sqrt{X}` |
| xⁿ | `x^{X}` |
| a/b | `\frac{X}{X}` |
| ∛ | `\sqrt[X]{X}` |
| \|x\| | `\left\|X\right\|` |
| ( ) | `\left(X\right)` |
| [ ] | `\left[X\right]` |
| xₙ | `x_X` |
| ∫ | `\int_{X}^{X}` |
| ∑ | `\sum_{X}^{X}` |
| lim | `\lim_{X}` |
| lim x→∞ | `\lim_{x\rightarrow\infty}` |
| sin cos tan | `\sin` `\cos` `\tan` |
| →v | `\overrightarrow{X}` |
| ←v | `\overleftarrow{X}` |
| x̲ | `\underrightarrow{X}` |
| x̄ | `\overline{X}` |
| î ĵ k̂ | `\bar{\text{i}}` `\bar{\text{j}}` `\bar{\text{k}}` |
| binom | `\binom{X}{X}` |
| cases | `\begin{cases} … \\ … \end{cases}` |
| matrix | `\begin{matrix} … & … \\ … & … \end{matrix}` |
| array | `\begin{array}{l\|l} … & … \\ \hline … & … \end{array}` |
| roman | `\mathrm{T}` |
| int-as-fraction | `\bigg/_{\!\!\!\!X}^{X}` |

---

## Support matrix for `cas-parse`

The parser targets what the palette emits, in priority order.

### Supported now (expression-valued)
`\frac` `\sqrt` `\sqrt[n]` `\binom` `^` `_` (subscript parsed and folded into
the name) `\left(...\right)` `\left|...\right|` `\left[...\right]`
`\cdot \times \div` `\pm` `\infty` `\pi` `\alpha…\omega` (all Greek above)
`\sin \cos \tan \ln \log \exp \arcsin \arccos \arctan \sinh \cosh \tanh`
`\operatorname{...}` `\mathrm{...}` `\text{...}` `\operatorname{}` `\%` `\,`

`\le \ge \ne \approx` and friends need a **relation** node; they are parsed
into a `Rel` value at the `Statement` level, which this crate does not yet have.
Until then they produce a clear error: `unsupported relation \leq`.

### Deliberately deferred
- `cases` / `matrix` / `array` environments — need a matrix type, which the
  `Expr` tree does not have and an expression CAS does not need until Phase 9.
- `\int` / `\sum` / `\lim` with bounds — need binder nodes. `cas-core` has
  `Hold::Integral` for the unevaluated form, so `\int` can land there later.
- `\overrightarrow`, `\overline`, `\bar` — decoration, not mathematics; the
  solver has nothing to do with them.

### Printing back
`to_latex` must emit forms this list can re-parse, so the round-trip property
holds for every expression the tool itself produces. In particular:
`\frac{a}{b}` for rationals, `\sqrt{x}`, `\left(x\right)` for grouping,
`\sin\left(x\right)` for functions, `\pi` for π, `\leq`/`\neq` if relations are
ever added.

---

## Attribution

`cas-palette` data is MIT, Copyright 2017 Matriculation Examination Board,
Finland, derived from `digabi/rich-text-editor`. See `NOTICE` in the repository
root. The implementation here is independent Rust; only the vocabulary — which
glyph maps to which LaTeX command — is shared, because that vocabulary is the
whole point of matching the exam interface.
