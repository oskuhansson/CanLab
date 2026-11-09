# The Abitti / MathQuill interaction model — an observed specification

This document is the behavioural contract that `crates/canlab-input` implements. It is
written from two kinds of evidence, and every claim is tagged:

| Tag | Meaning |
| --- | --- |
| **[OBSERVED]** | I did it in the live editor at <https://math-demo.abitti.fi/> and read the result back out of the DOM and the LaTeX pane. |
| **[SOURCE]** | Read in `digabi/rich-text-editor` (MIT, © Matriculation Examination Board) at `master`. |
| **[INFERRED]** | Neither — a design decision or a guess. Always flagged, never smuggled in. |

The live demo is Abitti's own build (`Päivitetty 5.11.2026`), and its palette
`data-testid` list is *identical* to the `action` strings in
`src/app/components/toolbar/math-shortcut-data.ts` at `master`, so the demo and the
source agree for everything quoted here.

## 0. The three layers, and why the difference matters

* The **answer box** is a `contenteditable` rich-text editor. Formulas in it are
  `<img class="equation" data-latex="…">` atoms — inert pictures with the LaTeX kept
  in an attribute. **[SOURCE]** `src/app/state/index.tsx`, **[OBSERVED]** in the DOM.
* `Ctrl-E` opens **one** `MathQuill` math field plus a read/write **LaTeX pane** on
  the right. **[OBSERVED]** The pane is editable: setting it to `\frac{3}{4}` and
  firing `input` re-renders the field to `\frac{3}{4}`. So the pane is a real second
  input path, not just a debug view. **[SOURCE]** `src/app/components/math-editor/index.tsx`.
  After `Ctrl-E` the field shows empty + no placeholder text — an empty field is
  literally an empty root block with a cursor. **[OBSERVED]**
* The field is MathQuill v2 (`MQ.MathField(node, {handlers:{edit, enter, …}})`) with a
  hidden `<textarea>` that receives the real key events. You cannot `fill()` that
  textarea; every observation below was made with real key events. **[OBSERVED]**
* Consequences: the model we need is **not** "text + insert strings". It is
  "a tree with a cursor inside it, where `/ ^ _ ( )` and Backspace are *structural*
  operations". The whole point of the rest of this document.

## 1. The cursor model

The cursor is a position **between children of a block**, and blocks nest. MathQuill's
DOM is exactly this: `mq-root-block` → `mq-fraction` → `mq-numerator` /
`mq-denominator`, each a block that can hold the cursor. **[OBSERVED]** directly in
`innerHTML`.

The single most useful fact for a reimplementation: **Left/Right move along a flattened,
depth-first ordering of all cursor positions, and that ordering is exactly
"root:0, then every slot's positions in order, then root:1"**. Measured on
`\frac{1}{2}` with the cursor starting at the end of the denominator **[OBSERVED]**:

| Key | Position | LaTeX |
| --- | --- | --- |
| (typed `1/2`) | `den[1]` (after `2`) | `\frac{1}{2}` |
| `←` | `den[0]` | `\frac{1}{2}` |
| `←` | `num[1]` (after `1`) | `\frac{1}{2}` |
| `←` | `num[0]` | `\frac{1}{2}` |
| `←` | root, **before** the fraction | `\frac{1}{2}` |
| `→` | `num[0]` | `\frac{1}{2}` |
| `→` | `num[1]` | `\frac{1}{2}` |
| `→` | `den[0]` | `\frac{1}{2}` |
| `→` | `den[1]` | `\frac{1}{2}` |
| `→` | root, **after** the fraction | `\frac{1}{2}` |

So there is no "up a level" key for horizontal motion — leaving a structure is just
walking past its last position. The same holds for a superscript: `x^2` visits
`root:0 → sup:0 → sup:1 → root:2` **[OBSERVED]**, and for `\sqrt{y}`:
`body:0 → body:1 → root:…` with **no** position for the empty index slot
`\sqrt{y}` has no `[]` at all. **[OBSERVED]**

Two asymmetries worth memorising:

* `←` from the **start of the first slot** of a construct lands *before the whole
  construct*: `←` from the numerator of `1/2` exits the fraction; `←` from the start of
  the sup in `x^2` lands *before the `x`*. **[OBSERVED]** There is no cursor position
  "between `x` and `^2`" — the superscript is a decoration on the preceding atom, not a
  container next to it.
* `↑`/`↓` are **geometry**, not structure: `\frac{123456}{7}` with the cursor at the
  denominator's left edge moves to `num[2]` on `↑`, because the denominator is centred
  under a much wider numerator. **[OBSERVED]** Up from the numerator does nothing.
  A headless model cannot reproduce this exactly (see §10).

## 2. Key bindings

| Key | Effect | Evidence |
| --- | --- | --- |
| any letter/digit | insert into the innermost block at the cursor | **[OBSERVED]** |
| space | inserts a space, LaTeX `\ ` (`pi ` → `pi\ `) | **[OBSERVED]** |
| `.` `+` `-` `=` | literal; `-` renders U+2212 but latex is `-` | **[OBSERVED]** |
| `,` | LaTeX `{,}` (`2,3` → `2{,}3`) | **[OBSERVED]** |
| `/` | **structural**: make a fraction from what is to the left | **[OBSERVED]** §3.1 |
| `^` | **structural**: begin a superscript | **[OBSERVED]** §3.2 |
| `_` | **structural**: begin a subscript | **[OBSERVED]** §3.2 |
| `*` | inserts `\cdot` (`x*2` → `x\cdot2`) — a real symbol, not `*` | **[OBSERVED]** |
| `+-` | **nothing special**: `x+-1` → `x+-1` | **[OBSERVED]** |
| `<=` , `>=` | become `\le` / `\ge` (`2<=3` → `2\le3`, `x<=y` → `x\le y`) | **[OBSERVED]** |
| `!=` , `->` , `=>` | **nothing special**, stay literal | **[OBSERVED]** |
| `(` | opens a `\left( … \right)` group, cursor inside | **[OBSERVED]** |
| `)` | closes the enclosing group (cursor moves outside); a stray `)` builds a ghost pair around the preceding run | **[OBSERVED]** |
| `[` `]` `\|` | same family: `\left[…\right]`, `\left\|…\right\|` | **[OBSERVED]** (palette) / **[INFERRED]** (typing) |
| `{` `}` | **visible braces**: `x^{2}` typed literally → `x^{\left\{2\right\}}` | **[OBSERVED]** |
| `\` + letters | LaTeX **command entry**, committed by the next non-letter | **[OBSERVED]** §4 |
| `sin` `cos` `tan` `ln` `log` `exp` `max` … | auto-converted to `\sin` … as soon as the letters match | **[OBSERVED]** §4.2 |
| `←` `→` | walk the flattened position list | **[OBSERVED]** §1 |
| `↑` `↓` | geometric vertical move (numerator ⇄ denominator) | **[OBSERVED]** §1 |
| `Backspace` | delete char; at a slot start, merge/unwrap the structure | **[OBSERVED]** §3.3 |
| `Delete` | forward delete a char / a whole following node | **[OBSERVED]** §3.4 |
| `Ctrl-Z` / `Ctrl-Y` | undo / redo the *whole formula* (driven by the outer editor, not MathQuill) | **[SOURCE]** `math-editor/index.tsx` |
| `Esc` | **closes the whole formula editor**, committing the LaTeX pane content as an `<img data-latex>` | **[OBSERVED]** |
| `Tab` | **closes the editor** and moves on (`Shift-Tab` closes backwards) | **[SOURCE]** + **[OBSERVED]** |
| `Enter` | **commits** the formula, inserts a `<br>`, opens a *fresh empty* editor on the next line | **[OBSERVED]** |

Note what is *absent*: there is no key that "leaves the structure" while staying in the
field. `Esc` is not a MathQuill notion at all here — Abitti binds it to close the
editor. `Tab` and `Shift-Tab` are the same close-and-commit, with a cursor hint for the
outer editor (`close('after')` / `close('before')`). **[SOURCE]**

## 3. Structural editing

### 3.1 `/` — the smart fraction

The rule, from four observations **[OBSERVED]**:

| Before `/` | Result | Cursor after |
| --- | --- | --- |
| empty field | `\frac{ }{ }` | **numerator** |
| `1` | `\frac{1}{ }` | **denominator** |
| `xy` | `\frac{xy}{ }` | denominator |
| `12` | `\frac{12}{ }` | denominator |
| `1+2` | `1+\frac{2}{ }` | denominator |
| `1/2` (cursor in den) | `\frac{1}{\frac{2}{ }}` | inner denominator |
| `x+1` **selected** | `\frac{x+1}{ }` | denominator |

So: take the selection if there is one; otherwise take the trailing run of plain
characters in the current block (**not** the last atom — `xy` moves *both* letters,
`1+2` moves only the `2` because `+` is an operator node); if there is nothing to take,
leave the numerator empty and put the cursor in the numerator.

Then the general template rule applies (see §6): content goes into the template's
content slot, and the cursor lands in the **first still-empty slot** of the new
template. With a numerator already filled, that is the denominator; with an empty
numerator it is the numerator. That single rule explains both "cursor in numerator"
and "cursor in denominator" without special cases.

### 3.2 `^` and `_`

`^` appends a superscript decoration to the node immediately to the left and puts the
cursor inside it: `x` then `^` → `x^{ }`. **[OBSERVED]**

With a selection, the selection *becomes* the superscript and there is **no base**:
select `x+1`, press `^` → `^{x+1}` with the cursor at the end of the superscript.
**[OBSERVED]** A base-less superscript is legal in the model (and `to_expr` must reject
it).

Because everything happens at the cursor's current level, `x_1^2` gives `x_{1^2}`,
**not** `x_1^2`: after `_` the cursor is inside the subscript, so `^` decorates the `1`
inside it. **[OBSERVED]** This is the single most surprising consequence of a cursor
that lives in the tree, and it is why a text box cannot emulate the editor.

Symmetrically `x^2y` is `x^{2y}` and `1/2x` is `\frac{1}{2x}`: **whatever you type next
goes where the cursor is**, and the cursor is inside the structure you just built.
**[OBSERVED]**

### 3.3 Backspace

Backspace acts at three different levels depending on where the cursor is. All
**[OBSERVED]**:

| Situation | Result | Cursor after |
| --- | --- | --- |
| char to the left in this block | deletes that char | stays |
| at the start of a **fraction denominator** | fraction **unwrapped**: `\frac{1}{2}` → `12` | at the `num`/`den` junction |
| at the start of a **fraction numerator** | fraction unwrapped: `\frac{1}{2}` → `12` | at the very start |
| at the start of an **empty** denominator (`\frac{1}{ }`) | fraction unwrapped → `1` | after the `1` |
| at the start of a **square-root body** | root unwrapped: `\sqrt{y}` → `y` | before the `y` |
| at the start of a **bracket body** | brackets unwrapped: `(x)` → `x` | before the `x` |
| at the start of an **empty superscript** (`x^{ }`) | the superscript is **removed** → `x` | after `x` |
| at the start of a **non-empty superscript** (`x^2`) | the **base** is deleted → `^2` | before the sup |
| at the very start of the root | nothing | — |

Two invariants fall out of the table:

1. Unwrapping **preserves the cursor's logical position**. It was between `num` and
   `den`, so after unwrapping it sits between the two pieces of content (`12` with the
   cursor in the middle for the denominator case, at the start for the numerator case).
2. An empty block that *is* the whole construct is deleted by Backspace at its start;
   a non-empty block escalates one level further out instead.

This is the behaviour that makes the editor usable: you can back out of a fraction you
did not mean to create without deleting its contents.

### 3.4 Delete (forward)

`Delete` at the start of a fraction numerator deletes the `1` and leaves `\frac{ }{2}`
with the cursor still in the numerator. **[OBSERVED]** At the root, `Delete` deletes the
whole following node — `1+\frac{2}{3}` with the cursor before the `1` becomes
`+\frac{2}{3}`. **[OBSERVED]** (i.e. forward delete removes one node, backward delete
removes one character.)

## 4. Typing LaTeX commands

### 4.1 `\name` is a modal text entry

Typing `\` starts a **command-accumulation block** (`mq-latex-command-input`) that shows
the raw characters, and the whole thing is committed when the next **non-letter** key
arrives. **[OBSERVED]**

| Typed | LaTeX | Rendered | Note |
| --- | --- | --- | --- |
| `\alpha` (still typing) | `\alpha` | `\alpha` (raw) | uncommitted |
| `\alpha` then space | `\alpha` | `α` | space is the terminator, not content |
| `\alpha` then `2` | `\alpha2` | `α2` | digit terminates, then is inserted normally |
| `\alpha` then `x` | `\alphax` | `\alphax` | `alphax` is not a command → raw text, forever |
| `\alp` then space | `\text{alp}` | `alp` | invalid name becomes upright text |
| `\alp` then Backspace | `\al` | `\al` | Backspace edits the accumulation |
| `\sqrt` then `2` | `\sqrt{2}` | `√2` | valid command, `2` becomes the body |
| `\sqrt` then `x` | `\sqrtx` | raw | letters keep accumulating |
| `\sqrt` then space then `x` | `\sqrt{x}` | `√x` | the reliable way to type a root |
| `\frac` then `1` | `\frac{1}{ }` | | command inserted, cursor into numerator |

So `\`-entry is a little state machine, not a keystroke map, and an invalid name has
two different fates (raw text if unterminated, `\text{…}` if terminated by a space).

### 4.2 Auto-operator names

There is a second, independent path: a bare run of letters that **ends with** one of a
fixed list of operator names is rewritten into the command as you type. **[OBSERVED]**

| Typed | LaTeX |
| --- | --- |
| `sin` | `\sin` |
| `sinx` | `\sin x` |
| `sin2` | `\sin2` |
| `xsin` | `x\sin` |
| `cosx` | `\cos x` |
| `arcsin` | `\arcsin` (longest match wins) |
| `pi` | `pi` (**not** a command) |
| `sqrt` | `sqrt` (**not** a command) |
| `ln` `log` `exp` `max` | `\ln` `\log` `\exp` `\max` |

The space in `\sin x` is not content — MathQuill inserts it in the LaTeX output because
`\sinx` would be a different (invalid) command. Same rule produces `x\le y` but `2\le3`.

### 4.3 What you cannot type

There is no auto-completion for Greek letters or `sqrt`: `pi` gives the letters `p`,`i`.
Greek and relations come from `\alpha`-style entry (§4.1) or from the palette. **[OBSERVED]**

## 5. Selection

* Drag across the fraction selects the **whole fraction as one unit**:
  `<span class="mq-selection">` wraps the entire `mq-fraction`. **[OBSERVED]**
* Typing over a selection **replaces** it (`9` over `\frac{1}{2}` → `9`). **[OBSERVED]**
* **Double-click does not select a word.** Double-clicking the numerator's `1` left a
  *collapsed* cursor at `num[0]`. **[OBSERVED]** Treat "double-click = place cursor" as
  the contract.
* `Ctrl-A` selects the whole root block. **[OBSERVED]** Used for every reset below.

## 6. The palette

The palette sends a **command** to the live MathQuill instance, never a string:
`mq[fn ?? 'cmd'](action)` where `fn` is `'write'` (parse as LaTeX) or `'typedText'`
(type key by key); the default is `cmd`, i.e. "run this MathQuill command". **[SOURCE]**
`src/app/components/toolbar/components/math-editor-buttons.tsx`. If the field is not
focused the same button inserts the human-readable `label` into the surrounding text
area instead — that is the only place the `label` is used. **[SOURCE]**

Observed landing spots after clicking, with the field empty and the cursor at `root:0`
**[OBSERVED]**:

| Button (`action`) | LaTeX after click | Cursor lands in |
| --- | --- | --- |
| `\frac` | `\frac{ }{ }` | numerator |
| `\sqrt` | `\sqrt{ }` | body |
| `\nthroot` | `\sqrt[]{ }` | **index** (`[]`) |
| `^` | `^{ }` | superscript |
| `_` | `_{ }` | subscript |
| `(` | `\left(\right)` | body |
| `\|` | `\left\|\right\|` | body |
| `\int` | `\int_{ }^{ }` | subscript (lower limit) |
| `\sum` | `\sum_{ }^{ }` | subscript |
| `\lim_` | `\lim_{ }` | subscript |
| `\sin` | `\sin` | after the symbol — **no argument slot** |
| `\binom` | `\binom{ }{ }` | top |
| `\overrightarrow` | `\overrightarrow{ }` | body |
| `\cases` | `\begin{cases}…\end{cases}` | first cell |

Unified rule: **fill the template's content slot with the selection (if any), then put
the cursor in the first still-empty slot, in document order.** It reproduces every row
above, and it explains the difference between `/` and the `\frac` button on a
non-empty field: `/` fills the numerator *before* creating, so the first empty slot is
the denominator; the button creates first, so the first empty slot is the numerator.
With a selection, the `\frac` button behaves exactly like `/`. **[OBSERVED]**

Palette entries with `fn: 'write'` insert the `action` string as literal LaTeX; the `X`
in the data file is only a placeholder in the *button image* (`label`), never in the
inserted text. Clicking `]x]` produces `]x]`, not `]X]`. **[OBSERVED]** + **[SOURCE]**

## 7. Empty slots

An empty argument block is a first-class, editable object. Its LaTeX rendering is a
single space inside braces: `\frac{ }{ }`, `\sqrt{ }`, `x^{ }`, `x_{ }`, `\int_{ }^{ }`.
**[OBSERVED]** Two exceptions: a bracket group with an empty body renders
`\left(\right)` with nothing inside **[OBSERVED]**, and an optional root index that is
absent is not written at all (`\sqrt{y}`, not `\sqrt[]{y}`).

Visually MathQuill draws these as placeholder boxes (`mq-empty`); the empty block also
carries the cursor class when focused. **[OBSERVED]** (`class="mq-denominator mq-empty"`).

## 8. LaTeX output catalogue

Ground truth for the emitter, all **[OBSERVED]** from the pane:

| Input | LaTeX |
| --- | --- |
| `x` | `x` |
| `x^2` | `x^2` (no braces for a single character) |
| `x^{12}` typed as `x^12` | `x^{12}` |
| `x_1^2` | `x_{1^2}` |
| `x^2y` | `x^{2y}` |
| `1/2x` | `\frac{1}{2x}` |
| `1/2x+1` | `\frac{1}{2x+1}` |
| `(x+1)^2` | `\left(x+1\right)^2` |
| `\sqrt x` (typed `\sqrt ` + `x`) | `\sqrt{x}` |
| `\sqrt` of `(x+1)` | `\sqrt{\left(x+1\right)}` |
| `sin(x)` | `\sin\left(x\right)` |
| `sin x` | `\sin\ x` |
| `2,3` | `2{,}3` |
| `x*2` | `x\cdot2` |
| `2<=3` | `2\le3` |
| `x<=y` | `x\le y` (space only because `y` is a letter) |
| `x^{2}` (literal braces) | `x^{\left\{2\right\}}` |
| `\frac{1}{ }` | `\frac{1}{ }` |
| `()` | `\left(\right)` |
| empty field | `` (empty string) |

`\left(…\right)` for every bracket is not cosmetic: Abitti round-trips this LaTeX
through its own renderer, and MathQuill always writes scalable delimiters.

## 9. What a reimplementation must not get wrong

1. The cursor is **inside** the tree; `1/2x` = `\frac{1}{2x}` is the acid test.
2. `x_1^2` = `x_{1^2}`. Anyone who writes `x_1^2` in a text box gets something else.
3. `/` moves the **whole trailing character run**, and it can nest (`1/2` then `/`).
4. Backspace **unwraps** rather than deletes, and preserves the logical position.
5. Templates land the cursor in the first **empty** slot, not after the template.
6. `\name` entry is modal and has an "invalid name" outcome.
7. Bare `sin`/`cos`/… become commands; bare `pi`/`sqrt` do not.
8. `*` is `\cdot`; `+-` is nothing.
9. Empty slots must be representable, or `\frac{1}{ }` cannot be edited.
10. `Esc`/`Tab`/`Enter` belong to the surrounding editor, not to the field.

## 10. Not determined, and where the model guesses

Honest list of things I could not pin down from observation, plus what
`crates/canlab-input` does about each (all guesses are isolated in one function, named
here):

| Question | Status | Model decision |
| --- | --- | --- |
| Exact pixel rule behind `↑`/`↓` | **[OBSERVED]** that it is geometric and x-preserving, but not the algorithm | `MathField::vertical_target` clamps the offset into the sibling slot. Diverges when the two slots have different widths. |
| Does `/` stop at a `Sym` to the left, or take it? | **not observed** | The numerator is the trailing `Text` node only; a `Sym` to the left stays put. `MathField::slash`. |
| Backspace with a *structure* immediately to the left at root level | **not observed** | Deletes the whole node. `MathField::backspace`. |
| `^` pressed twice in a row | **not observed** | The second press enters the existing superscript instead of nesting a new one. `MathField::power`. |
| Behaviour inside `cases` / `matrix` / `array` | **not observed** | Not modelled at all; `Template::from_palette_action` returns `Err(Unsupported)` for them. |
| Selection spanning two sibling structures | **not observed** | The model supports a selection as a contiguous `(start, end)` cursor pair and replaces it wholesale; cross-structure drag semantics are not reproduced. |
| `\overline` / `\underrightarrow` / `\overrightarrow` | palette observed; they behave like a one-slot body, same as `\sqrt` | Modelled as `Node::Accent`; `\mathrm` is not modelled at all. |
| Tables (`cases`, `matrix`, `array`) and `\lim_{x\to\infty}`, `\intsub` | palette observed, cells reachable | Not modelled; `Template::from_palette_action` returns `Err`. |

## 11. How `crates/canlab-input` maps onto this specification

| Observed fact | Where it lives |
| --- | --- |
| Cursor lives in a slot of the tree | `Cursor { path: Vec<PathStep>, offset }`, `PathStep { index, slot }` |
| Left/Right walk a flattened depth-first order | `MathField::positions`, `move_left`, `move_right` |
| No position between an atom and its superscript | `collect_positions` skips the position before a `Sup`/`Sub` |
| `↑`/`↓` are geometric | `MathField::vertical_target` (clamped — the documented approximation) |
| `1/2x` → `\frac{1}{2x}` | cursor is in `Slot::Den` after `slash()` |
| `xy/` vs `1+2/` | word characters share one `Node::Text`, operators do not |
| Backspace unwraps a fraction, a root, a bracket | `unwrap_construct`, `backspace_out_of_structure` |
| Backspace at a non-empty superscript start deletes the base | the `Slot::Sup` / `Slot::Sub` escalation in `backspace_out_of_structure` |
| Templates land the cursor in the first empty slot | `template_target` |
| `\name` entry, invalid names, swallowed terminator | `begin_command` / `push_command_char` / `finish_command` |
| `sin` → `\sin`, `pi` stays `pi` | `auto_operator_name` |
| `*` → `\cdot`, `<=` → `\le` | `insert_char`, `insert_equals` |
| Empty slots print `{ }`; `\left(\right)`; `\le` + letter grows a space | `src/latex.rs` |
| A half-typed field is not an expression | `build_expr` / `ToExprError` |

