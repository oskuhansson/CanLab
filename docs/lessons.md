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



