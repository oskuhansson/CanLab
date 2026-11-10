# Capability roadmap

This began as a plan: eight items, an estimate in hours for each, and the order
to do them in. All eight are now in the code, with tests where the machine allows
them. The hours below are what each item was judged at before it was built —
judgement, not measurement.

The remaining work is in [What is left](#what-is-left), estimated in the same
units. There is no total.

| # | Item | Hours | What shipped |
|---|---|---|---|
| 1 | Function definitions and derivatives | 20 | `f(x) = x² + 1` defines a name for the whole document. Lines resolve in dependency order, so a definition can be used above its line; cycles are refused. `f(3)` substitutes, including built-in calls in the body, so `f(x) = x + sin x` gives `f(2) = 2 + sin 2`; `f'(x)` and `f''(x)` differentiate the body, and a space before the bracket is still a call. A definition that depends on a broken one is reported, not built against a missing name. Definitions are listed in the Variables panel, are skipped when equations are collected, and plot as `y = f(x)`. |
| 2 | Undo and redo | 10 | A ring of 100 `MathField` snapshots, with typing coalesced over 400 ms and structural edits taking their own step. Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y, plus buttons in the editor header. |
| 3 | Parameters the user can set | 8 | A variable off both axes gets a value and a drag control in the plot overlay, default 1. Automatic axis choice looks for the familiar names — `x`/`i`/`t`/`u`/`s` horizontally, `y`/`j`/`v` vertically — and no longer invents a vertical axis from an arbitrary letter, so that variable stays a parameter until the user picks it. The `l = 1 assumed` note is gone. |
| 4 | SymPy for real (packaging) | 16 | `canlab solver` reports discovery; `canlab solver --probe` starts the worker and round-trips a request. Discovery tries `CANLAB_PYTHON` (a full path may contain spaces), then `py -3`, `python`, `python3`, `py`, with a 700 ms probe per candidate, and finds the worker through `CANLAB_WORKER`, beside the executable, or in the checkout. Python plus `worker.py` is not proof: `open()` sends one trivial `simplify`, and only an answer makes the GUI label the backend SymPy; otherwise it stays native and says why. A SymPy decline on a system is retried with native elimination, and a fatal SymPy failure is retried once on native with the panel saying so. `CANLAB_SOLVER=native` forces native. The worker has still not run on this machine. See [sympy.md](sympy.md). |
| 5 | Native elimination for 3+ unknowns | 10 | Gaussian elimination over the exact rationals, with no special case for 2×2; tested to 4×4. A unique point is returned, a contradiction is reported as no solutions — including one reached through substitution, where `x = 1, x = 2` used to return `x = 1` — and an underdetermined system is declined rather than narrowed to one point. |
| 6 | Plot interaction | 16 | Drag to pan and scroll to zoom about the pointer, on the equal-aspect view. Intersections of two curves are refined by Newton's method into points, drawn as dots, and the one under the pointer shows `(x, y)` to four decimals. The view fits the intersections when there are any. |
| 7 | Persistence | 6 | A versioned text session, written 1.2 s after the last change to `%APPDATA%/canlab/session.canlab`, `$HOME/.config/canlab/session.canlab`, or the path in `CANLAB_SESSION`. It stores the document (systems and lines), palette, plot window, axes and parameter values. A file from a newer version is refused rather than half-read; `CANLAB_SESSION=none` turns saving off. |
| 8 | Editor polish (paste, history, gutter) | 12 | Ctrl+C copies the document as LaTeX. Ctrl+V inserts a single expression at the caret, or replaces the page with a whole document. History entries load back into the editor on click, and a gutter marks the caret's line in the accent colour. |

Two things arrived that were not on the list. The old dark/light setting and
accent colour became seven whole schemes — Midnight, Graphite, Nord, Solarized
Dark, Contrast, Solstice and Solarized Light — each with its own surfaces, text,
accent and six-colour curve wheel. A palette pins light or dark, so there is no
separate theme toggle. And `CANLAB_SHOT` writes one frame through the renderer;
it needs a `glow` build, which is why that feature exists.

## Where it stands

Everything in the table is wired into the GUI, not just the reader. The workspace
has over 500 tests; the GUI suite paints whole frames headlessly, and a
63-document acceptance corpus asserts its own results and prints the Markdown
table kept in `docs/acceptance.md`.

One part has still never run end to end on this machine: the SymPy worker,
because there is no Python here. That is in the list below.

## What is left

Six things are worth naming. The order is roughly the order they hurt a user, not
the order they are easiest to fix.

| # | Item | Hours | What is wrong, and what done means |
|---|---|---|---|
| 1 | A mixed document's `cases` brace | 3 | `to_latex` braces every multi-line system, so a definition typed *into a later line of an existing system* still ends up inside that system's brace. The common path is handled: Enter after a definition starts a new system, so a definition is not braced with the equations that follow it. Done means the printer separates definitions from equations wherever they sit, and the document still round-trips. |
| 2 | `2f(3)` after a number | 6 | The editor glues `2` and `f` into one word run, and a run is read as a product, so a defined `f` is not seen as a call. `2*f(3)` works. Done means a run is split at a defined name before a bracket, without breaking `2x` or `xy`. |
| 3 | Plot resampling during pan and zoom | 8 | Every drag or scroll event rebuilds the whole 240×240 implicit trace. One curve is fine; a large system is visibly slow. Done means the gesture stays responsive, most likely by sampling coarser while moving and refining when it stops. |
| 4 | Window resize on the wgpu build | 12 | The window is created at 1180×760; resizing it on this machine leaves the surface wrong. The `glow` build is for screenshots, not a shipping answer. Done means a resize is a surface reconfigure that neither blanks nor stretches the frame. This is the least certain estimate here. |
| 5 | SymPy, actually run | 4 | Discovery and the protocol are tested in memory, but with no Python on this machine the worker has never imported SymPy here. The GUI probes before it claims a backend, so the first real check is `canlab solver --probe`. Done means that passes on a machine with Python and the GUI reports SymPy; whatever the first real request turns up is extra. |
| 6 | A plot-level test for a broken definition | 3 | The definition reader tests what it refuses, but no test builds a document whose definition is fine until the plot tries to compile or sample it. Done means a test asserts the plot's note and that the rest of the document still draws. |

## Known sharp edges

- **Memory**: a full rebuild needs `-j 1` here; `-j 2` exhausts commit. Not code,
  but it shapes how the project is developed.
- **Screenshots**: a normal build cannot be read back as an image. `--features
  glow` exists for that, and is not meant as the shipping renderer.

## What I would do next

Fix the mixed-document brace and the `2f(3)` glue first. Both are things a
student would meet in the first ten minutes, and both are confined to printing
and to how the reader handles a run. The plot resampling is next: a system that
drags slowly feels broken even when the answer is right. The wgpu resize is the
least certain estimate, so it is the one I would not promise a date for. SymPy
waits for a machine with Python.
