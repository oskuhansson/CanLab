# Capability roadmap

Written to be reviewed, not followed. Effort is given in **hours of work**, then
as calendar time **at 20 h/week** — the rate this is now being built at.

At 20 h/week the whole list below is about **four weeks**. The estimates are my
judgement, not measurements, and the two "sharp edges" at the bottom are the ones
most likely to overrun.

| # | Item | Hours | Calendar (20 h/wk) | State |
|---|---|---|---|---|
| 1 | Function definitions and derivatives | 20 | 1 week | reader done, wiring left |
| 2 | Undo and redo | 10 | 2–3 days | not started |
| 3 | Parameters the user can set | 8 | 2 days | not started |
| 4 | SymPy for real (packaging) | 16 | 4 days | worker written, never run |
| 5 | Native elimination for 3+ unknowns | 10 | 2–3 days | 2×2 only |
| 6 | Plot interaction | 16 | 4 days | not started |
| 7 | Persistence | 6 | 1.5 days | not started |
| 8 | Editor polish (paste, history, gutter) | 12 | 3 days | `from_latex` exists |
| | **Total** | **98 h** | **~5 weeks** | |

## Where it stands

Working and tested (459 tests): exact `i128` rationals, LaTeX parse and print,
the Abitti structural editor, typeset layout, implicit plotting with intersection
tracing, a native solver for linear/quadratic equations, polynomial calculus, a
real symbolic differentiator (sums, product, power, chain, `u^v`, any order), and
the reader half of function definitions. A dockable UI with dark/light and accent
colours, and a 53-document acceptance corpus that generates its own review table.

Honest gaps, in the order they hurt:

1. **Definitions are not wired into the editor.** The reader resolves `f(3)`,
   `f'(x)`, `f''(x)` and has tests for all of it, but the GUI never builds the
   table, so typing `f(x) = x²` still reads as a product. This is half a feature.
2. **No undo.** Ctrl+Z does nothing. Its absence is felt on the first mistyped
   structure.
3. **Parameters have no control.** A variable that is not on an axis silently
   becomes `1` (`l = 1 assumed`).
4. **The solver is narrow by design.** Linear and quadratic in one variable, no
   systems beyond substitution and 2×2 elimination.
5. **Nothing persists.** Closing the app loses the document, theme and accent.

---

## 1. Function definitions and derivatives — 20 h

**Reader: done** (7 tests). `f(x) = x² + 1` makes `f` a name; `f(3)` substitutes;
`f'(5)` differentiates the body then substitutes; `f''`, `f'''` repeat the rule;
recursion is refused at definition time; wrong arity is an error.

**Remaining, ~12 h:**

- Harvest definitions from a document: a line whose left side is `name(params)`
  defines, and does not draw a curve of its own.
- `MathField::build_expr_with(&Definitions)` and `build_equations_with`, so the
  GUI can pass the table everywhere it currently reads without one.
- Show defined names in the Variables panel, and plot each definition's curve on
  its own variable.
- **Decide:** are definitions document-wide and order-independent (my choice) or
  forward-only? Document-wide is easier to explain and avoids a class of "why
  doesn't this work" questions, at the cost of allowing a definition to be used
  above its line.

**Risk:** definitions make every read depend on every other line, so one broken
definition must not blank the document. Plan: read line by line and let a broken
definition degrade to "`f` is undefined" only on the lines that use it.

## 2. Undo and redo — 10 h

The field is already `Clone`. Keep a ring of snapshots (say 100), pushing on each
*structural* change and coalescing typing on a 400 ms timer so undo steps back a
word rather than a keystroke. Contained to `canlab-gui`; no model change.

**Risk:** one snapshot per keystroke on a long document — cap the ring and measure.

## 3. Parameters the user can set — 8 h

Every variable that is not on an axis gets a row in the plot overlay: name and a
drag value, default 1. That replaces the acknowledged `l = 1 assumed` placeholder.
Sliders are the last 10 % — a drag value is enough.

---

## 4. SymPy for real — 16 h

The worker exists, the protocol is tested in memory, and it has never run: there
is no Python on this machine. This is a **packaging** problem, not a coding one —
bundle an interpreter, or require one and document it. Prefer the latter for a
hobby project; a bundled runtime is a distribution question.

## 5. Native elimination for 3+ unknowns — 10 h

Gaussian elimination over the existing exact `Number` arithmetic. The 2×2 case is
there; generalising it is bookkeeping, not new mathematics, and it removes the
"three unknowns → declined" cliff for linear systems.

## 6. Plot interaction — 16 h

- Drag to pan, scroll to zoom (equal-aspect window is already the only state).
- Newton-refine each intersection blob into a point instead of drawing the ε-ring,
  whose size currently depends on how steep the surface is.
- Show coordinates of an intersection on hover.

## 7. Persistence — 6 h

`eframe`'s persistence stores app state to disk; the document, theme and accent
are plain data. Brings the first third-party dependency outside the GUI (`serde`).

**Risk:** a saved format is a promise. Version it from the first commit.

## 8. Editor polish — 12 h

- Paste LaTeX into the field and copy the document as LaTeX (`from_latex` reads
  bare brackets, `/`, indexed roots and definitions now — this is mostly wiring).
- Click a history entry to load it back into the editor.
- Line numbers in the gutter, once documents are long enough to need them.

---

## Known sharp edges (not in the estimate)

- **Window resizing**: a window created maximised came up blank on this machine,
  and the workaround is "size it at creation and don't resize". The real fix is a
  surface reconfigure on resize; `wgpu`/`eframe` territory.
- **Memory**: a full rebuild needs `-j 1` here; `-j 2` exhausts commit. Not code,
  but it shapes how the project is developed.

## What I would do next, regardless of the rate

**Finish item 1.** Everything else is polish on a calculator; definitions are what
let a student use it for a real task, and the reader half is already paid for.
Undo is the cheap second because it is felt every session.
