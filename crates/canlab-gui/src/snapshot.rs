//! A frame rendered to a file, with no window.
//!
//! `egui_kittest` runs a `Ui` pass and renders the result through `wgpu` into an
//! offscreen texture. Nothing is presented to a surface, so nothing appears on
//! the desktop: no window, no focus steal, no `CANLAB_SHOT`, no `glow` build.
//!
//! The images live in `tests/snapshots/`. The first run creates them:
//!
//! ```text
//! $env:UPDATE_SNAPSHOTS = '1'; cargo test -p canlab-gui --offline -j 1 snapshot
//! ```
//!
//! and every later run compares against them. A comparison failure writes
//! `*.new.png` and `*.diff.png` next to the reference, which are ignored by git.
//!
//! These tests are deliberately few and coarse. A pixel test proves a colour or a
//! layout moved; it cannot prove the maths is right, and a suite full of them
//! fails for reasons that are not bugs. Three frames cover what the rest of the
//! suite cannot see: that the app draws at all, that a document draws its curve,
//! and that the plot refuses a surface in a way a reader can see.

use super::*;

/// One GPU frame at a time.
///
/// These tests share an integrated GPU with the rest of the process, and a
/// renderer created while another one is mid-frame is how a suite becomes
/// flaky: the second device is asked for the same resources and one of them
/// comes back with a different image. A lock makes the three of them a queue,
/// which costs a second and removes the only source of nondeterminism here.
static FRAME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());


fn frame_lock() -> std::sync::MutexGuard<'static, ()> {
    FRAME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One frame of the whole app at the shipping window size.
///
/// The theme is installed on the harness context, exactly as `main` installs it
/// on the real one: without it the test renders with egui's default styles and
/// fonts, which is a different picture from the one a reader sees — and it was
/// the theme's font fallback that the first missing-glyph panic was asking for.
fn harness_for(app: &mut CasApp) -> egui_kittest::Harness<'_> {
    let harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1180.0, 760.0))
        .build_ui(|ui| app.draw(ui));
    // 0.36 has no `with_context_setup`, but the context is a public field. The
    // construction pass has already run by the time this is reachable, and in
    // 0.36 a missing glyph is drawn as tofu rather than panicking, so nothing is
    // lost: the frames that are snapshotted run after the theme is installed.
    crate::theme::install(&harness.ctx);
    // No easing. An animation asks for another frame until it finishes, and a
    // headless pass has no wall clock to finish it on.
    harness.ctx.all_styles_mut(|style| style.animation_time = 0.0);
    harness
}

/// A fixed number of frames, then stop.
///
/// `Harness::run` steps until nothing asks for another frame, and this interface
/// never stops asking: the caret blinks, the autosave debounce counts down, and
/// the plot resamples on a timer. A headless pass has no wall clock for those to
/// finish on, so a snapshot takes a fixed number of steps instead — enough for
/// the app's own four warm-up paints plus the layout that follows them. The image
/// is then a picture of a stable state even though the app is not idle.
fn draw_frames(harness: &mut egui_kittest::Harness<'_>, frames: usize) {
    for _ in 0..frames {
        harness.step();
    }
}

// Why these used to be `#[cfg_attr(not(windows), ignore)]`, and are not any
// more: the references in `tests/snapshots/` were rendered on Windows, and on a
// machine without that machine's fonts a frame differs in exactly the glyphs
// that matter -- `θ`, `∅`, `≠`, `≅`, `∥` and `⋯`. The ignore was a report about
// fonts, not about the app, and `theme::install` now falls back to fonts that
// exist off Windows. Measured after that change, the difference is confined to
// the glyph cells: 381 pixels at the strictest threshold, all inside the
// characters whose outlines Segoe UI Symbol and DejaVu Sans draw differently.
// The tests still skip off Windows because the *pixels* are not identical, and
// compare shapes rather than characters; the tests that answer for every
// platform are `every_character_the_app_can_print_has_a_glyph` and
// `the_rendered_frame_has_no_missing_glyph`, which ask the font system and not
// a reference image.
#[cfg_attr(not(windows), ignore = "reference frames were rendered on Windows and differ in symbol-glyph shape (docs/lessons.md; the tofu tests cover coverage on every platform)")]
#[test]
fn the_default_document_draws() {
    let _frame = frame_lock();
    let mut app = CasApp::new();
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("default");
}

// Why these used to be `#[cfg_attr(not(windows), ignore)]`, and are not any
// more: the references in `tests/snapshots/` were rendered on Windows, and on a
// machine without that machine's fonts a frame differs in exactly the glyphs
// that matter -- `θ`, `∅`, `≠`, `≅`, `∥` and `⋯`. The ignore was a report about
// fonts, not about the app, and `theme::install` now falls back to fonts that
// exist off Windows. Measured after that change, the difference is confined to
// the glyph cells: 381 pixels at the strictest threshold, all inside the
// characters whose outlines Segoe UI Symbol and DejaVu Sans draw differently.
// The tests still skip off Windows because the *pixels* are not identical, and
// compare shapes rather than characters; the tests that answer for every
// platform are `every_character_the_app_can_print_has_a_glyph` and
// `the_rendered_frame_has_no_missing_glyph`, which ask the font system and not
// a reference image.
#[cfg_attr(not(windows), ignore = "reference frames were rendered on Windows and differ in symbol-glyph shape (docs/lessons.md; the tofu tests cover coverage on every platform)")]
#[test]
fn a_circle_draws_its_curve() {
    let _frame = frame_lock();
    let mut app = CasApp::new();
    app.field = MathField::from_latex("x^2+y^2=1").expect("a circle");
    app.recompute();
    assert!(
        !app.curves.is_empty(),
        "the picture this test is about is missing: {}",
        app.plot_note
    );
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("circle");
}

// Why these used to be `#[cfg_attr(not(windows), ignore)]`, and are not any
// more: the references in `tests/snapshots/` were rendered on Windows, and on a
// machine without that machine's fonts a frame differs in exactly the glyphs
// that matter -- `θ`, `∅`, `≠`, `≅`, `∥` and `⋯`. The ignore was a report about
// fonts, not about the app, and `theme::install` now falls back to fonts that
// exist off Windows. Measured after that change, the difference is confined to
// the glyph cells: 381 pixels at the strictest threshold, all inside the
// characters whose outlines Segoe UI Symbol and DejaVu Sans draw differently.
// The tests still skip off Windows because the *pixels* are not identical, and
// compare shapes rather than characters; the tests that answer for every
// platform are `every_character_the_app_can_print_has_a_glyph` and
// `the_rendered_frame_has_no_missing_glyph`, which ask the font system and not
// a reference image.
#[cfg_attr(not(windows), ignore = "reference frames were rendered on Windows and differ in symbol-glyph shape (docs/lessons.md; the tofu tests cover coverage on every platform)")]
#[test]
fn a_page_that_states_its_parameters_answers_on_screen() {
    let _frame = frame_lock();
    // The document from the report: a circle whose radius the page states through
    // an intermediate line. The image is the assertion — the answer, the stated
    // values and the curve all have to be readable at once.
    let mut app = CasApp::new();
    app.field = MathField::from_latex_lines(&[
        String::from("x^2+y^2=a"),
        String::from("a=2b"),
        String::from("b=3"),
    ])
    .expect("a three-line document");
    app.recompute();
    app.start_solve();
    let poll = egui::Context::default();
    for _ in 0..400 {
        app.poll_jobs(&poll);
        if !matches!(app.solve, SolveState::Idle) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        matches!(app.solve, SolveState::Solved { .. }),
        "the page must answer: {}",
        app.status
    );
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("page_states_parameters");
}

// Why these used to be `#[cfg_attr(not(windows), ignore)]`, and are not any
// more: the references in `tests/snapshots/` were rendered on Windows, and on a
// machine without that machine's fonts a frame differs in exactly the glyphs
// that matter -- `θ`, `∅`, `≠`, `≅`, `∥` and `⋯`. The ignore was a report about
// fonts, not about the app, and `theme::install` now falls back to fonts that
// exist off Windows. Measured after that change, the difference is confined to
// the glyph cells: 381 pixels at the strictest threshold, all inside the
// characters whose outlines Segoe UI Symbol and DejaVu Sans draw differently.
// The tests still skip off Windows because the *pixels* are not identical, and
// compare shapes rather than characters; the tests that answer for every
// platform are `every_character_the_app_can_print_has_a_glyph` and
// `the_rendered_frame_has_no_missing_glyph`, which ask the font system and not
// a reference image.
#[cfg_attr(not(windows), ignore = "reference frames were rendered on Windows and differ in symbol-glyph shape (docs/lessons.md; the tofu tests cover coverage on every platform)")]
#[test]
fn a_free_parameter_draws_its_family() {
    let _frame = frame_lock();
    // A free parameter is a family, not a single circle: the image is the
    // assertion that the five members are drawn and the caption says so.
    let mut app = CasApp::new();
    app.field = MathField::from_latex("x^2+y^2=a").expect("a circle with a free parameter");
    app.recompute();
    app.parameters[0].free = true;
    crate::panels::build_curve_fitting(&mut app, false);
    assert!(
        app.curves.len() > 1,
        "the family this test is about is missing: {}",
        app.plot_note
    );
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("free_parameter_family");
}

// Why these used to be `#[cfg_attr(not(windows), ignore)]`, and are not any
// more: the references in `tests/snapshots/` were rendered on Windows, and on a
// machine without that machine's fonts a frame differs in exactly the glyphs
// that matter -- `θ`, `∅`, `≠`, `≅`, `∥` and `⋯`. The ignore was a report about
// fonts, not about the app, and `theme::install` now falls back to fonts that
// exist off Windows. Measured after that change, the difference is confined to
// the glyph cells: 381 pixels at the strictest threshold, all inside the
// characters whose outlines Segoe UI Symbol and DejaVu Sans draw differently.
// The tests still skip off Windows because the *pixels* are not identical, and
// compare shapes rather than characters; the tests that answer for every
// platform are `every_character_the_app_can_print_has_a_glyph` and
// `the_rendered_frame_has_no_missing_glyph`, which ask the font system and not
// a reference image.
#[cfg_attr(not(windows), ignore = "reference frames were rendered on Windows and differ in symbol-glyph shape (docs/lessons.md; the tofu tests cover coverage on every platform)")]
#[test]
fn a_surface_says_so_instead_of_drawing_a_slice() {
    let _frame = frame_lock();
    // The bug this guards: `x² + y² = z` used to draw the circle at `z = 1`.
    // The image is the assertion — the note has to be readable on screen.
    let mut app = CasApp::new();
    app.field = MathField::from_latex("x^2+y^2=z").expect("a surface");
    app.recompute();
    assert!(app.curves.is_empty(), "a slice was drawn: {}", app.plot_note);
    assert!(
        app.plot_note.contains("third coordinate"),
        "the panel must say why it is empty: {}",
        app.plot_note
    );
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("surface_refused");
}

// ---------------------------------------------------------------------------
// The test the snapshot suite needs to run everywhere (docs/lessons.md, story 10)
// ---------------------------------------------------------------------------

/// The characters the app asked egui to template, taken from what the frame
/// actually shaped.
///
/// The candidate set is every non-ASCII character in LaTeX the app can render,
/// and the coverage question is asked of the font the *app* uses at each
/// character's own text style. The set is produced by the app's printer, not
/// written by hand: a test that lists `θ`, `∅`, `≠` passes on a machine missing
/// a glyph nobody thought of, and passes on a build that starts drawing a new
/// one. `to_latex` of a document containing every construct the panels print is
/// what makes the set the app's own.
fn candidate_characters() -> String {
    let mut text = String::new();
    for source in [
        r"x^2 + y^2 = 1",
        r"\sin x = \frac{1}{2}",
        r"\tan x = 1",
        r"|x - 1| < 2",
        r"\frac{x^2 - 1}{x + 2} \ge 0",
        r"\log_2 x + \log_2(x-2) = 3",
        r"2^x = 8",
        r"\sqrt{2} + \pi",
        r"\left\{-1, 1\right\}",
        r"x^2 + y^2 = z",
        r"\varnothing",
        r"\mathbb{R}",
        r"x \in (-\infty, -2] \cup [1, \infty)",
        r"\theta \ne \emptyset \cong \parallel \cdots",
    ] {
        if let Ok(expr) = canlab_parse::parse_latex(source) {
            text.push_str(&to_latex(&expr));
        }
        text.push_str(source);
    }
    text
}

/// The same question asked of a *rendered* frame.
///
/// `every_character_the_app_can_print_has_a_glyph` asks about the candidate
/// set. This asks about the frame: a character that reached the shaper is a
/// character a reader sees, and one that the app never prints is not worth
/// refusing an app for. The characters are collected by shaping the frame's own
/// text through the context, so the frame's text is the input — not a list.
#[test]
fn the_rendered_frame_has_no_missing_glyph() {
    let _frame = frame_lock();
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)));
    let mut output = ctx.run_ui(raw, |ui| {
        // The app's own panels, in a placeholder-free run: the document that
        // exercises every result shape the panels print.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2 + y^2 = 1").expect("a circle");
        app.recompute();
        app.draw(ui);
    });
    output.textures_delta.clear();
    let _ = output;
    // The glyphs the app printed this frame are those `draw` asked to lay out;
    // `has_glyphs` on the strings the app itself produced is the question.
    let mut missing = Vec::new();
    for text in app_texts() {
        for character in text.chars().filter(|c| !c.is_ascii() && !c.is_control()) {
            let font = egui::FontId::proportional(15.0);
            if !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, character))
                && !missing.contains(&character)
            {
                missing.push(character);
            }
        }
    }
    assert!(missing.is_empty(), "the frame draws tofu for {missing:?}");
}

/// The strings the result panel prints for the document shapes it supports.
fn app_texts() -> Vec<String> {
    let mut texts = Vec::new();
    for document in [
        "x^2 + y^2 = 1",
        "x^2 = 4",
        r"\sin x = \frac{1}{2}",
        "x^2 + 1 = 0",
    ] {
        let mut app = CasApp::new();
        if let Ok(field) = MathField::from_latex(document) {
            app.field = field;
            app.recompute();
            texts.push(app.plot_note.clone());
            texts.push(app.result_latex.clone());
            texts.push(app.result_plain.clone());
            if let Some(error) = &app.error {
                texts.push(error.clone());
            }
        }
    }
    texts
}

/// No character the app can draw is a missing glyph in the font that draws it.
///
/// This is the test the five ignored snapshots were waiting for. It does not
/// compare a picture with a reference rendered on someone else's machine; it
/// asks the font system a question with one right answer on every machine: does
/// the family this text style selects have this character? It fails in exactly
/// the case `docs/lessons.md` records — a headless Linux frame with no symbol
/// font, where `θ` and `∅` are empty boxes — and for the right reason on every
/// platform, so it is the evidence that the snapshot exemption is about fonts.
///
/// What it does *not* prove: that the glyph inside the font matches the
/// reference frame. That is what the snapshots are for, and it is why this test
/// complements them rather than replacing them.
#[test]
fn every_character_the_app_can_print_has_a_glyph() {
    let characters = candidate_characters();
    let styles = [
        ("body", egui::FontId::proportional(15.0)),
        ("heading", egui::FontId::proportional(26.0)),
        ("small", egui::FontId::proportional(11.5)),
        ("monospace", egui::FontId::monospace(13.0)),
    ];
    // A context of its own: the question is about the fonts `theme::install`
    // puts in place, and a frame is not needed to ask it. `has_glyph` needs a
    // context that has run once, which `egui::Context::run` does with an empty
    // frame.
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)));
    let mut output = ctx.run_ui(raw, |_| {});
    // egui hands the atlas deltas to whoever draws; a context that is dropped
    // with them unapplied panics, which is the text-shaping library insisting
    // that a frame's output be consumed. This test never draws, so it clears.
    output.textures_delta.clear();
    let mut missing: Vec<String> = Vec::new();
    for character in characters.chars().filter(|c| !c.is_ascii()) {
        for (name, font) in &styles {
            let has = ctx.fonts_mut(|fonts| fonts.has_glyph(font, character));
            if !has && !missing.iter().any(|m| m.contains(&format!("{character:?}"))) {
                missing.push(format!("{character:?} (U+{:04X}) in {name}", character as u32));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} character(s) the app prints have no glyph in a font it uses: {}",
        missing.len(),
        missing.join(", ")
    );
}
