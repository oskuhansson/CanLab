# Capability roadmap

Written to be reviewed, not followed. Each item says what it unlocks, what it
costs, and what it puts at risk. "Effort" is calendar time at 5–10 h/week.

## Where it stands

Working and tested (443 tests): exact `i128` rationals, LaTeX parse and print,
the Abitti structural editor, typeset layout, implicit plotting, a native solver
for linear/quadratic equations and polynomial calculus, a dockable UI.

Honest gaps, in the order they hurt:

1. **No function definitions.** `f(x) = x²` reads as the product `f·x`, and `f'`
   has no meaning, so the whole line fails. This is the single biggest thing
   between the app and how a student actually writes mathematics.
2. **No undo.** Ctrl+Z does nothing in the editor. Every other editor has it;
   its absence is felt on the first mistyped structure.
3. **Parameters have no control.** A variable that is not on an axis silently
   becomes `1` (`l = 1 assumed`). That is a placeholder, not a feature.
4. **The solver is narrow by design.** Linear and quadratic in one variable,
   integer powers, no systems beyond substitution and 2×2 elimination.
5. **Nothing persists.** Closing the app loses the document, the theme and the
   accent.

---

## P0 — Function definitions and derivatives

**Unlocks:** `f(x) = ax² + b` then `f(3)`, `f'(x)`, plotting `f`. Without it the
document cannot express the most common exam task.

**Design (the part to agree on):**

- A *definition* is a line whose left side is `name(parameters)` and whose right
  side is an expression. It defines; it does not draw a curve of its own unless
  asked.
- Definitions are **document-wide and order-independent** (a line may use a
  function defined below it), which is simpler to explain than forward-only and
  avoids a class of "why doesn't this work" questions.
- A call `f(a, b)` substitutes arguments into the body. Recursion is refused at
  definition time (a name may not appear in its own body) — this is a calculator,
  not a programming language.
- `f'` is a call to the backend's `diff` on the body. `f''` likewise. Anything
  higher is refused rather than half-supported.
- The reader needs the definition table at parse time. That is the real work:
  `expr::build` gains a `Definitions` argument, and the GUI builds it before
  reading any line.

**Cost:** ~2 weeks. Mostly `canlab-input` (reader + substitution + tests), plus
`canlab-gui` for the definition table and the "which lines are definitions"
display.

**Risk:** definitions make every read dependent on every other line, so an error
in one definition must not blank the document. Plan: read line by line, and let a
broken definition degrade to "`f` is undefined" on the lines that use it.

## P0 — Undo and redo

**Unlocks:** editing without fear. A structural editor that cannot undo a
backspace that swallowed a bracket is painful.

**Design:** the field is already a value type (`MathField: Clone`). Keep a
`Vec<MathField>` snapshot ring (say 100) and push a snapshot on each *structural*
change, coalescing typing by a 400 ms timer so that undo steps back a word, not a
keystroke.

**Cost:** ~3 days. Contained to `canlab-gui`; no model change.

**Risk:** snapshots of a large document per keystroke — fine at this scale, but
cap the ring and measure before trusting it.

## P0 — Parameters with a value the user controls

**Unlocks:** the `l` in `x² + y² = l²` becomes a slider instead of a silent 1.

**Design:** every variable that is not on an axis gets a row in the plot overlay:
name, drag value, default 1. The value feeds the same sampling path that
currently hard-codes 1.0. Sliders are the last 10% — a drag value is enough.

**Cost:** ~3 days. `canlab-gui` only.

**Risk:** none to speak of; this replaces an acknowledged placeholder.

---

## P1 — Solver breadth

**Unlocks:** the tasks the native backend declines today.

Two separate decisions:

- **Wire SymPy for real.** The worker exists and the protocol is tested in
  memory, but there is no Python on this machine, so it has never run. This is a
  *packaging* problem (bundle a Python, or require one) rather than a coding one.
- **Widen the native backend.** Systems of three or more linear unknowns
  (Gaussian elimination — the 2×2 special case is already there), and equations
  that are linear in one variable but with symbolic coefficients.

**Cost:** SimPy wiring 1 week (mostly packaging); Gaussian ~4 days.

**Risk:** a bundled Python is a distribution question, not just code. For a hobby
project, "install Python and SymPy yourself" is a legitimate answer.

## P1 — Plot interaction

**Unlocks:** reading a graph rather than looking at it.

- Drag to pan, scroll to zoom (the equal-aspect window is already the only state).
- Refine intersection markers with Newton from each traced blob instead of
  drawing the ε-ring — the current marker size depends on how steep the surface is.
- Show the numeric coordinates of an intersection on hover.

**Cost:** ~1 week. Contained to `canlab-gui`; the tracer already returns points.

## P1 — Persistence

**Unlocks:** closing the app stops losing work.

`eframe`'s persistence feature stores app state to disk; the document, theme and
accent are all plain data. A `serde` dependency arrives with it — the first
third-party crate outside the GUI.

**Cost:** ~2 days.

**Risk:** a saved document format is a promise. Version it from the first commit.

---

## P2 — Editor polish

- **LaTeX in and out**: paste LaTeX into the field (the reader now handles bare
  brackets and `\left…\right`), and copy the document as LaTeX.
- **History reload**: the history panel is display-only; `from_latex` now exists,
  so an entry can be clicked back into the editor.
- **Line numbers** in the editor gutter, once documents are long enough to need
  them.

## P2 — Known sharp edges

- **Window resizing**: a maximised window came up blank on this machine, and the
  workaround is "size it at creation and don't resize". The real fix is a surface
  reconfigure on resize; it is a `wgpu`/`eframe` problem, not ours, but it is
  visible to the user.
- **Memory**: a full rebuild needs `-j 1` here; `-j 2` runs the machine out of
  commit. Not a code issue, but it shapes how the project is developed.
- **`l` and other one-letter parameters collide with `l` the digit-like glyph**;
  worth a lint that warns when a parameter shadows a common constant.

---

## What I would do next, if only one thing

**Function definitions.** Everything else is polish on a calculator; definitions
are what make it a tool a student can use for a real task. Undo is a close second
because it is cheap and its absence is felt every session.
