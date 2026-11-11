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

#[test]
fn the_default_document_draws() {
    let _frame = frame_lock();
    let mut app = CasApp::new();
    let mut harness = harness_for(&mut app);
    draw_frames(&mut harness, 6);
    harness.snapshot("default");
}

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
