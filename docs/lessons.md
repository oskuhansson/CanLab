# Lessons

One entry per bug that reached a screen. Each states what happened, why the code
did it, why no test caught it, and what changed so the next one is caught. The
point is not the fix — it is the reason the fix was needed.

## A surface plotted as a slice

**What happened.** The document `x² + y² = z` drew a circle. `z` was given the
value 1.00 and the tracer drew the level set `x² + y² = 1`. Nothing on screen said
that the circle was one slice of a paraboloid, and the picture is exactly what
`x² + y² = 1` would have drawn.

**Why the code did it.** The parameter feature was built for coefficients: `a` in
`a·x = 1` is a knob, and a knob needs a value, so every free variable that was
not on an axis got 1.0. "Not on an axis" is a *positional* test. A third
coordinate is also not on an axis, so `z` qualified. The rule was right for the
case that motivated it and wrong for a case with the same symptom.

**Why no test caught it.** Every plotting test asked whether a curve *appeared*.
None asked whether a plot should be *refused*. One test —
`an_equation_with_extra_variables_plots_them_as_parameters` — asserted the
behaviour for `a·x + b·y = 1`, which is correct there, and the same shape was
never written for a symbol that is a whole side. The suite had no negative space:
it proved the intended cases worked without proving the unintended ones were
refused, and refusal is most of what a CAS is trusted for.

**What changed.**

1. The test is now structural rather than positional: a symbol that forms a whole
   side of an equation is a *coordinate* of that relation unless the other side
   is an axis or a constant. `x² + y² = z` refuses and names `z`;
   `x² + y² = r²` keeps its radius slider; `y = k·x + m` keeps both sliders.
2. Three tests: the refusal, plus two guards that the refusal did not spread to
   the forms that must keep working.
3. The rule below, in `docs/AGENTS.md` and in §7 of the roadmap plan.

**The rule.** Every guard gets a refusal test. If the code can decide not to
draw, not to solve, or not to evaluate, a test asserts the refusal *and* its
reason. And a feature that supplies a value the user did not type must justify it
structurally — never positionally.

**Why this class recurs.** A positive test proves the intended case works. It
cannot prove the neighbouring cases are refused, and the neighbouring cases are
where a CAS is dangerous: a wrong picture, like a wrong number, is worse than no
picture.

## Two tests that only passed on a permissive machine

**What happened.** `cargo test --workspace` went red in `canlab-cli` with
`could not write C:\Users\…\AppData\Local\Temp\cas-e2e-sin.svg: Access is denied`.
Both failing tests plot to an SVG under `std::env::temp_dir()`. Nothing about
plotting had changed.

**Why it appeared.** The tests assumed `%TEMP%` is writable. It is on a normal
desktop and is not when the process runs inside a file sandbox. The suite had
been green for months because it had only ever run outside one.

**Why no test caught it.** This is the mirror of the plot bug: the plot bug was a
missing refusal test, and this is a test that leaks an assumption about the
*environment* instead of asserting something about the code. Both are the same
failure — the suite proved what was convenient to prove.

**What changed.** `temp_svg` now builds its path from `CARGO_TARGET_TMPDIR`, which
is inside `target/` and therefore inside the workspace, so the test writes where
the build writes. A test that needs to write outside the build directory now has
to say so explicitly, which is the point.

## The picture found what the state assertions could not

**What happened.** The first headless frame refused to render three times before
it produced an image. Twice it was a missing glyph: a `▾` in every panel's menu,
and `⋯` after that, neither of them in the bundled fonts — so the panel menu had
always drawn a box, and nobody had looked. Then kittest's accessibility check
refused the frame because the two axis combo boxes had no accessible name.

**Why the code did it.** The glyphs were chosen by how they look, never checked
against the font that has to draw them. The combo boxes have a small letter beside
them, which reads as a label to someone looking at the screen and is nothing at
all to a screen reader.

**Why no test caught it.** The suite asserted application *state* — curves,
notes, undo depth — and never an image, and never the accessibility tree. Both
defects are invisible to `assert_eq!` on a field and obvious to a renderer. The
suite was also never drawing the palette, because the default layout does not
show it and no test opened it.

**What changed.**

1. Three headless frame tests (`src/snapshot.rs`) render the app through `wgpu`
   into a texture and compare with `tests/snapshots/*.png`. No window, no
   desktop, no `glow` build.
2. `theme::install` appends a system symbol font as the last fallback, so `ϑ`,
   `∢`, `≢` and the rest of the palette have glyphs; if the font is absent the app
   falls back to the boxes it had, which is the only thing that function promises.
3. The axis pickers are wired to their visible letters with `labelled_by`, so the
   access tree names them.
4. The rule: **a UI change is not finished until a frame test renders it.** The
   reviewer can read a diff; only a renderer can see a missing glyph.

## The window arrived with somebody else's title bar

**What happened.** The first launch on Bevy opened a correctly drawn dark app
under a bright white Windows title bar.

**Why the code did it.** eframe had been setting the viewport up for us; the Bevy
`Window` was created without `window_theme`, so winit drew the frame in the
system's light theme regardless of the app's palette. Nothing in the app's own
rendering could fix it — the frame belongs to the OS.

**Why no test caught it.** Every test renders the *client area*. A title bar is
outside the texture, so no snapshot can contain it, and no assertion about app
state can either. This is the boundary of what the headless harness proves: it
covers what egui paints and nothing that winit draws.

**What changed.** `window_theme: Some(WindowTheme::Dark)` on the primary window,
next to the size and title. And a note in the plan's W0.0 that the two failures
of the port were both *silent*: a missing camera meant `bevy_egui` had no context
and every frame drew nothing, and a missing window theme meant the frame was
light. Neither is a compile error.




## A rule that was right about z and wrong about aa

**What happened.** After x² + y² = z was fixed not to plot a slice, x² + y² = a
stopped plotting at all: a was classified as a third coordinate, the equation
was skipped as a surface, and the panel said "Nothing to plot yet."

**Why the code did it.** The fix drew its line on structure — a bare symbol on one
side, both variables on the other — and z and a are structurally identical.
The rule was right about the case it was written for and wrong about its twin,
which is the same shape of mistake as the original bug: a rule that is right for
the case in hand and untested on its neighbour.

**Why no test caught it.** The new test used z only. The neighbour was never
written, so "the rule refuses a surface" passed while "the rule accepts a
parameter" had no test at all.

**What changed.** The refusal is now limited to the names that mean another
coordinate (z, w); every other bare symbol becomes a parameter, whose value is
visible on a slider. A test now covers x² + y² = a plotting with a as a
parameter, next to the z test that keeps the refusal. **A rule drawn on
structure needs a test on each side of the line it draws**, or the next symbol
someone types will fall on the wrong side.

## 5x² read as (5x)²

**What happened.** A new corpus row for the quartic solver -
`x^4 - 5x^2 + 4 = 0`, the roots ±1 and ±2 - printed *declined: the backend
declined*. The same polynomial typed into `canlab solve-all` answered correctly,
so the solver was fine and the reading was not: the document went through the
editor's field, and the field read `- 5x^2` as `-(5x)² = -25x²`. The row's real
equation was `x^4 - 25x^2 + 4 = 0`, which has no rational root at all.

**Why the code did it.** The field glues adjacent word characters into one *text
run* - that is what makes `1/2x` put the whole `2x` in the denominator, and it is
Abitti's behaviour, not an accident. `text_expr` already knew a run is notation
for a product (`2x` is `2·x`), but the builder in `product` treated a run as one
atom and gave a following superscript the whole run as its base. Two readings of
one node, and the code picked the reading that was easier to implement. A run
followed by a power is the only construct where the two readings differ, and it
is the construct every earlier test happened to avoid.

**Why no test caught it.** Three suites, three different blind spots, and the bug
lived in the gap between them:

1. `acceptance.rs` had `x^2`, `-x^2`, `2x`, `3xy`, `x2`, `abc`, `(x+1)^2` - every
   one of them either a bare power or a run with *no* power. The compound case
   was never written, and a compound is exactly where a one-node-two-meanings
   representation goes wrong.
2. The solver's own tests build `Expr` by hand, so they never pass through the
   field: `solve_all_answers_a_quartic_with_four_rational_roots` proved the
   solver and could not prove the sentence the user would type.
3. The corpus rows that *do* go through the field were quadratics with a
   coefficient of 1 on `x^2`, plus `3x` and `2x` terms - no row had a number in
   front of a power.

**What changed.**

1. A run is split into its factors before the exponent applies, and only the
   superscript directly after the run may split it: `5x^2` is `5·x²`,
   `(5x)^2` is still `(5·x)²`. The pair is now a test on each side of the rule.
2. A corpus row (`quartic-four-roots`) and a `canlab-corpus` document test,
   because the document level is the only place that runs *field → solve →
   answer* rather than one of the two halves.
3. `5x^2` and `(5x)^2` sit next to each other in `acceptance.rs` with the reading
   written out, so the next person to touch the builder sees both meanings.

**The rule.** When one node carries two meanings, test every operator that can
follow it - not just the one that motivated the shared node. The run was built
for `1/2x`; the power after it was never asked what it meant.

## A comment that read "in ar"

**What happened.** Auditing for stray bytes after an older corruption found four
prose characters replaced by control codes: two `a`s and a `v` in doc comments
became U+0007 and U+000B. In `lessons.md` the paragraph about `x² + y² = a` had
read "wrong about " for weeks, and `native.rs` said "a polynomial in ar".

**Why the code did it.** Those files had been written through a shell path where
`\a` and `\v` are escape sequences, so the letter vanished into a control code
instead of being stored. No compiler sees it: comments are free text, and a
Markdown file is not compiled at all.

**Why no test caught it.** Nothing reads the prose. The NUL byte that made
`main.rs` look binary was caught only because a tool refused to parse the file;
these were subtler and survived review because a missing `a` is still a
word-shaped hole.

**What changed.** `scripts/check-text.ps1` scans every tracked text file for
control characters and exits non-zero, so the class is one command away from
being caught. The rule for agents is unchanged and now enforced by that script:
**edit source with the file tools, never with shell string substitution.**

## The result panel answered the question with the question

**What happened.** A three-line page — `x² + y² = a`, `a = 2b`, `b = 3` — answered
*Solve 3 equations* with a decline, and the result panel showed "system of 3
equations" and the document's own LaTeX back. The plot put `a` on a slider at
1.00 while the page says `a = 6`.

**Why the code did it.** Two readings of one page, both missing the point that a
page *states* things:

1. Every line was sent to the solver as one system of four unknowns. `a = 2b` and
   `b = 3` are not unknowns to solve together; they are the page telling you what
   `a` and `b` are. The solver therefore received a nonlinear system it declines,
   and its honest decline looked on screen like no answer at all.
2. The parameter list recognised only `s = 3` — a symbol equal to a literal — as a
   stated value, so `a = 2b` left `a` on a slider at the default 1.0. A value the
   user never chose was shown as if they had.

**Why no test caught it.** Three separate reasons:

1. Every system test was a *linear* system (`x + y + z = 6`, `x − y = 0`), where
   sending everything to the solver together is exactly right. The page that
   states its parameters is nonlinear after reduction, and no test had one.
2. Every parameter test used `s = 1`, where the stated value is a literal. A page
   that states a value *through another value* (`a = 2b`) was never written.
3. The GUI tests asserted `app.solve`, a state — not the text a user reads. A
   decline with a reason passed the state test while the screen showed the
   question echoed as the answer.

**What changed.**

1. `document_assignments` resolves the values a page states, in dependency order,
   and substitutes them before the solve; the lines that only stated a value leave
   the system. An axis variable is never an assignment (`x = 1` is a vertical
   line), and a value that mentions an axis is not a statement about its symbol
   (`x² + y² = a` is a circle, not `a = x² + y²`).
2. The Variables panel lists the stated values under **FROM THE DOCUMENT**, so
   `a = 6` is visible and the slider stops implying a choice.
3. Four GUI tests, including the one the screenshot was: the page answers
   `x = ±√(6 − y²)` instead of declining, `a` resolves to 6 through `b`, an
   equation is never read as an assignment of the symbol on its side, and a line
   that merely mentions a stated symbol stays an equation. The first draft of the
   fix had that last bug — the circle line was swallowed as "the line that stated
   `a`" — and only writing the test found it.

**The rule.** A document is not a bag of equations; it is a set of statements, and
"the page states this value" is a different statement from "these unknowns satisfy
this relation". And a test of what the panel says is a test of the product: assert
the text the user reads, not only the state the code keeps.

