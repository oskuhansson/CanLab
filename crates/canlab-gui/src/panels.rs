//! The contents of the five dock panels.
//!
//! Every function here receives the app plus a UI already clipped to the
//! panel's body. Keeping the dock chrome in `layout.rs` means these functions
//! only have to answer one question each: what does this panel show?

use canlab_mathview::{DisplayList, MathView, Style};
use canlab_plot::implicit::{ImplicitOptions, sample_implicit};
use canlab_plot::{Curve, Intersection, SampleOptions, sample};
use canlab_tex::to_latex;
use egui::{
    self, Align, Align2, Button, Color32, FontId, Label, Layout, Rect, RichText,
    Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::{CasApp, SolveState};

/// Width of the editor's reading column. Wide enough for a display equation,
/// narrow enough that the eye does not lose the line.
const EDITOR_WIDTH: f32 = 900.0;

/// Size of the mathematics in the hero field, and the caret/padding around it.
const FIELD_BASE_SIZE: f32 = 30.0;
/// The field is the centre of the app, so it gets more than one text line even
/// when the expression is short.
/// One line of mathematics, plus the padding around it. The field grows with the
/// document; this is only the floor for an empty one.
const FIELD_MIN_HEIGHT: f32 = FIELD_BASE_SIZE * 1.9;
const FIELD_PADDING_X: f32 = 14.0;
const FIELD_PADDING_Y: f32 = 10.0;

pub fn editor(app: &mut CasApp, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .id_salt("editor-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_width().max(120.0);
            let width = available.min(EDITOR_WIDTH);
            let side = ((available - width) * 0.5).max(0.0);
            // Top-aligned, like a document: the first line sits where a text
            // editor's first line would, and the block grows downwards as lines
            // are added. Centring it vertically made a one-line expression float
            // in the middle of the panel, which read as a dialog, not an editor.
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    editor_column(app, ui);
                });
            });
        });
}

fn editor_column(app: &mut CasApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        let title = if app.field.system_count() > 1 || app.field.is_system() {
            format!(
                "Document · {} system(s), {} line(s)",
                app.field.system_count(),
                // Lines across the whole document: a two-system page whose cursor
                // is in the first system is not a one-line document.
                app.field.rows().count()
            )
        } else {
            String::from("Document")
        };
        ui.label(RichText::new(title).small().strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (text, color) = if app.error.is_some() {
                ("parse error", ui.visuals().error_fg_color)
            } else if app.field.is_incomplete() {
                ("incomplete", crate::theme::weak())
            } else {
                ("live parse", crate::theme::weak())
            };
            ui.label(RichText::new(text).small().color(color));
            // Undo and redo live on the document, so they belong beside the
            // document header rather than in a menu nobody opens.
            if ui
                .add_enabled(
                    app.undo.can_redo(),
                    Button::new(RichText::new("↷").size(14.0)).frame(false),
                )
                .on_hover_text("Redo (Ctrl+Shift+Z)")
                .clicked()
            {
                app.redo_edit();
            }
            if ui
                .add_enabled(
                    app.undo.can_undo(),
                    Button::new(RichText::new("↶").size(14.0)).frame(false),
                )
                .on_hover_text("Undo (Ctrl+Z)")
                .clicked()
            {
                app.undo_edit();
            }
        });
    });
    ui.add_space(4.0);

    let response = math_field(app, ui);
    handle_field_keys(app, ui, &response);

    // Abitti shows the LaTeX the math editor would emit; keeping that pane is
    // both familiar and the fastest way to see what a keystroke actually did.
    ui.add_space(6.0);
    let latex = app.field.to_latex();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("LaTeX")
                .small()
                .strong()
                .color(crate::theme::weak()),
        );
        ui.add(
            Label::new(
                RichText::new(if latex.is_empty() {
                    "—"
                } else {
                    latex.as_str()
                })
                .monospace()
                .size(12.0)
                .color(crate::theme::weak()),
            )
            .selectable(true)
            .wrap(),
        );
        if !latex.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(Button::new(RichText::new("copy LaTeX").small()).frame(false))
                    .on_hover_text(
                        "Copy the whole document as LaTeX (Ctrl+C). Paste with Ctrl+V.",
                    )
                    .clicked()
                {
                    app.copy_latex(ui.ctx());
                }
            });
        }
    });

    // The actions moved to the panels that show their answers: Solve and the
    // simplifications live with the Variables panel, Plot with the plot itself.
    // The editor is just the document.

    if let Some(error) = &app.error {
        ui.add_space(9.0);
        ui.label(
            RichText::new(error.as_str())
                .size(13.0)
                .color(ui.visuals().error_fg_color),
        );
    } else {
        ui.add_space(12.0);
        result_card(app, ui);
    }
}

/// The structural field: a line-number gutter, the mathematics itself, and the
/// focus target that the keyboard handler listens to.
///
/// The wrapper is a clickable rectangle above the field's own widget, so a
/// click in the padding still focuses the editor. `MathView` handles the part
/// that needs geometry: translating a click into one of the field's positions.
fn math_field(app: &mut CasApp, ui: &mut Ui) -> egui::Response {
    let style = Style {
        base_size: FIELD_BASE_SIZE,
        ..Style::default()
    };
    let display = MathView::layout(ui, &app.field, &style);
    let width = ui.available_width().max(FIELD_BASE_SIZE * 4.0);
    let height = (display.size.y + 2.0 * FIELD_PADDING_Y).max(FIELD_MIN_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    // No box. The mathematics sits straight on the panel: a filled, bordered
    // rectangle around an editor is chrome that costs space and says nothing the
    // caret does not already say.
    let mut inner = rect.shrink2(vec2(FIELD_PADDING_X, FIELD_PADDING_Y));

    // A gutter, once the document is long enough to need one. It is painted here
    // rather than laid out as a widget because it belongs to the *document*, and
    // its rows come from the layout that has already been computed.
    if display.line_bands.len() > 1 {
        let digits = display.line_bands.len().to_string().len();
        let gutter = digits as f32 * 8.0 + 12.0;
        paint_gutter(app, ui, &display, inner);
        inner.min.x += gutter;
    }

    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("canlab-math-field")
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(inner);
    let math = MathView::show(&mut child, &mut app.field, &style);

    // Focus `MathView`'s own widget: it is the focusable one, so Tab and clicks
    // agree about where the caret lives.
    if response.clicked() || math.clicked() {
        math.request_focus();
    }
    math
}

/// Paint line numbers beside the document.
///
/// The line the caret is in is marked with the accent colour, which is the
/// cheapest way to answer "which equation am I editing?" in a four-line system.
fn paint_gutter(app: &CasApp, ui: &Ui, display: &DisplayList, inner: Rect) {
    // The clip is widened to the left: the numbers live *beside* the maths, and
    // clipping them to the maths is how they disappear.
    let band = Rect::from_min_max(
        pos2(inner.left() - 48.0, inner.top()),
        pos2(inner.right(), inner.bottom()),
    );
    let painter = ui.painter().with_clip_rect(band);
    let weak = crate::theme::weak();
    let accent = crate::theme::accent();
    let current = display
        .line_refs
        .iter()
        .position(|&(system, line)| system == app.field.system() && line == app.field.line());
    for (index, band) in display.line_bands.iter().enumerate() {
        let selected = current == Some(index);
        let color = if selected {
            accent
        } else {
            weak.gamma_multiply(0.75)
        };
        painter.text(
            pos2(inner.min.x - 6.0, inner.min.y + band.y + band.h * 0.5),
            Align2::RIGHT_CENTER,
            (index + 1).to_string(),
            FontId::monospace(11.0),
            color,
        );
    }
}

/// What Enter means, in order:
///
/// 1. a half-typed `\name` is committed (`\bar` + Enter is the accent, not a new
///    line);
/// 2. Ctrl+Enter starts a new, separate system;
/// 3. after a *definition*, a new system too: a definition is not one of the
///    equations of a system, so bracing it with them would be a lie about what
///    the document is;
/// 4. otherwise the next equation of this system.
pub(crate) fn press_enter(app: &mut CasApp, command: bool) {
    if app.field.commit_command() {
        return;
    }
    if command || app.field.is_definition_row(app.field.root()) {
        app.field.new_system();
    } else {
        app.field.new_line();
    }
}

/// Route the frame's key events into the field.
///
/// `MathView` deliberately owns clicks only. `Event::Text` is the key here:
/// `type_str` already implements `/`, `*`, `^`, `_`, `\name` commands and the
/// Greek names, so this function must not special-case printable keys. Enter
/// goes through [`press_enter`], which starts the next line of the system (or a
/// whole new system, after a definition or with Ctrl). Every edit is
/// checkpointed before it is applied, so it can be taken back.
///
/// The events are **taken out of the queue**, not merely read: egui also feeds
/// them to its focus system, so leaving an arrow key in place moved focus to the
/// next widget while the caret was walking the expression. Ctrl/⌘ combinations
/// and `Tab`/`Esc` are left alone on purpose — undo and focus traversal belong to
/// the shell.
fn handle_field_keys(app: &mut CasApp, ui: &mut Ui, response: &egui::Response) {
    if !response.has_focus() {
        // Losing focus ends a typing burst: resuming later must not silently
        // fuse into the same undo step.
        app.undo.break_burst();
        return;
    }

    let events = ui.input_mut(|input| {
        // A dead key produces **no text event at all**. On a Nordic layout the
        // `^~` key is a dead circumflex: pressing it emits only a key event
        // (physical `CloseBracket`), so `x^2` arrived as `x`, nothing, then `2`
        // and the exponent silently vanished. A frame carrying text means the
        // key produced a character (shift+`]` is `}` on a US layout), so the
        // text wins and the fallback does not fire.
        let frame_has_text = input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(_)));
        let mut ours = Vec::new();
        input.events.retain(|event| {
            let claim = match event {
                egui::Event::Text(text) => !text.is_empty() && !text.chars().all(char::is_control),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let arrow = matches!(
                        key,
                        egui::Key::ArrowLeft
                            | egui::Key::ArrowRight
                            | egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                    );
                    let plain = !modifiers.ctrl && !modifiers.command;
                    let editing = arrow
                        || matches!(
                            key,
                            egui::Key::Backspace | egui::Key::Delete | egui::Key::Enter
                        );
                    // The dead circumflex: claimed only when the frame produced
                    // no text, so a layout where this key *does* type a character
                    // keeps typing it.
                    let accent = plain && is_dead_circumflex(*key, modifiers.shift, frame_has_text);
                    // `Ctrl-A` belongs to the field while it has focus; every
                    // other command combination is left to the shell.
                    (plain && editing)
                        || accent
                        || (modifiers.shift && arrow)
                        || (modifiers.command && matches!(key, egui::Key::A | egui::Key::Enter))
                }
                _ => false,
            };
            if claim {
                ours.push(event.clone());
            }
            !claim
        });
        ours
    });

    let mut changed = false;
    for event in events {
        match event {
            egui::Event::Text(text) => {
                app.note_edit(false);
                app.field.type_str(&text);
                changed = true;
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => match key {
                egui::Key::A if modifiers.command => {
                    app.field.select_all();
                    changed = true;
                }
                // The dead circumflex: open an exponent, the same template the
                // palette's `xⁿ` button inserts. Reached only when the frame had
                // no text event, which the claim above already decided.
                egui::Key::CloseBracket if modifiers.shift => {
                    app.note_edit(false);
                    app.field.power();
                    changed = true;
                }
                egui::Key::Backspace => {
                    app.note_edit(false);
                    app.field.backspace();
                    changed = true;
                }
                egui::Key::Delete => {
                    app.note_edit(false);
                    app.field.delete_forward();
                    changed = true;
                }
                egui::Key::ArrowLeft if modifiers.shift => app.field.extend_left(),
                egui::Key::ArrowRight if modifiers.shift => app.field.extend_right(),
                egui::Key::ArrowUp if modifiers.shift => app.field.extend_up(),
                egui::Key::ArrowDown if modifiers.shift => app.field.extend_down(),
                egui::Key::ArrowLeft => changed |= app.field.move_left(),
                egui::Key::ArrowRight => changed |= app.field.move_right(),
                egui::Key::ArrowUp => changed |= app.field.move_up(),
                egui::Key::ArrowDown => changed |= app.field.move_down(),
                // Enter means four things, in this order:
                // 1. a half-typed `\name` is committed (`\bar` + Enter is the
                //    accent, not a new line);
                // 2. Ctrl+Enter starts a new, separate system;
                // 3. after a definition, a new system too: a definition is not
                //    one of the equations of a system;
                // 4. otherwise the next equation of this system.
                egui::Key::Enter => {
                    // A structural change is always its own undo step.
                    app.note_edit(true);
                    press_enter(app, modifiers.command);
                    changed = true;
                }
                _ => {}
            },
            _ => {}
        }
    }

    if changed {
        app.recompute();
        ui.ctx().request_repaint();
    }
}

/// The buttons that act on the *document*, shown in the Variables panel because
/// that is where their answers appear.
fn solver_row(app: &mut CasApp, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let size = vec2(92.0, 30.0);
        let expression = app.expr.is_some();
        // A half-typed field has no document behind it: its equations were just
        // cleared, so Solve is disabled for the same reason it would send nothing.
        let equation = !app.equations.is_empty() && !app.is_typing();
        if ui
            .add_enabled(expression, Button::new("Simplify").min_size(size))
            .on_hover_text("Simplify the expression above")
            .clicked()
        {
            app.start_simplify();
        }
        if ui
            .add_enabled(expression, Button::new("Integrate").min_size(size))
            .on_hover_text("Integrate the expression with respect to its variable")
            .clicked()
        {
            app.start_integrate();
        }
        if ui
            .add_enabled(equation, Button::new("Solve").min_size(size))
            .on_hover_text("Solve an equation, or the first equation of a system")
            .clicked()
        {
            app.start_solve();
        }
        if app.jobs.in_flight() > 0 {
            ui.spinner();
        }
    });
}

/// The session changed from the plot panel: a view gesture, an axis or a
/// parameter. Remember it, and redraw at the chosen setting.
///
/// None of these are document edits, but all of them are saved in `session()`,
/// so each one has to ask for a save or it is lost on quit.
pub(crate) fn plot_changed(app: &mut CasApp) {
    app.mark_dirty();
    build_curve_fitting(app, false);
}

/// The plot's own controls, in the plot panel: the button, which variables go on
/// which axis, and the window it draws into. They belong together.
fn plot_row(app: &mut CasApp, ui: &mut Ui) {
    let mut axes_changed = false;
    ui.horizontal_wrapped(|ui| {
        // No Plot button: the picture follows the document. What is worth
        // showing here is what it is showing, and why.
        axes_changed |= axis_picker(app, ui, "x", Axis::Horizontal);
        axes_changed |= axis_picker(app, ui, "y", Axis::Vertical);

        // The window controls need room for two number boxes and their labels. In
        // a narrow dock they used to wrap onto a second line *inside a strip sized
        // for one*, so labels and values landed on top of each other. They are
        // better absent than broken there; the window is still reachable by
        // dragging and scrolling the plot.
        let mut range_changed = false;
        if ui.available_width() > 300.0 {
            ui.separator();
            ui.label(RichText::new("x from").small());
            range_changed |= ui
                .add(egui::DragValue::new(&mut app.x_min).speed(0.5))
                .changed();
            ui.label(RichText::new("to").small());
            range_changed |= ui
                .add(egui::DragValue::new(&mut app.x_max).speed(0.5))
                .changed();
        }

        if axes_changed {
            // A new axis can turn a parameter into a coordinate or back.
            app.refresh_parameters();
        }
        if axes_changed || range_changed {
            plot_changed(app);
        }
    });
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

/// Put `variable` on `axis`, swapping it with the other axis when needed.
///
/// Every free variable is offered on either axis, including the one already on
/// the other axis. Taking that one swaps the pair: the replaced choice moves to
/// the other axis, so the same variable never ends up on both. Two axes on one
/// variable made the sampler evaluate it twice, and `q² + q² = 1` drew two
/// vertical lines instead of a curve.
///
/// A pick made on an empty axis, when the other axis holds the only variable, is
/// a no-op: there is no replaced choice to move across, and moving the variable
/// off the only axis it is on would silently drop the horizontal one.
fn choose_axis(app: &mut CasApp, axis: Axis, variable: canlab_core::SymbolId) {
    match axis {
        Axis::Horizontal => {
            let replaced = app.x_axis;
            if replaced.is_none() && app.y_axis == Some(variable) {
                return;
            }
            app.x_axis = Some(variable);
            if app.y_axis == Some(variable) {
                app.y_axis = replaced;
            }
        }
        Axis::Vertical => {
            let replaced = app.y_axis;
            if replaced.is_none() && app.x_axis == Some(variable) {
                return;
            }
            app.y_axis = Some(variable);
            if app.x_axis == Some(variable) {
                app.x_axis = replaced;
            }
        }
    }
}

/// One "which variable is this axis?" selector.
///
/// Any free variable is offered on either axis, which is what makes
/// `i² + j² = 1` plottable and a swap one click: nothing about the maths cares
/// what the letters are called, only which one is horizontal. Picking the
/// other axis's variable goes through [`choose_axis`], so it swaps the pair.
fn axis_picker(app: &mut CasApp, ui: &mut Ui, label: &str, axis: Axis) -> bool {
    let current = match axis {
        Axis::Horizontal => app.x_axis,
        Axis::Vertical => app.y_axis,
    };
    let mut changed = false;
    let label_response = ui.label(RichText::new(label).small().strong());
    let selected = current.map_or_else(|| String::from("—"), |v| v.as_str().to_string());
    let combo = egui::ComboBox::from_id_salt(("axis", label))
        .selected_text(RichText::new(selected).monospace().size(12.0))
        .width(46.0)
        .show_ui(ui, |ui| {
            for variable in app.free_vars.clone() {
                let is_current = current == Some(variable);
                if ui
                    .selectable_label(is_current, variable.as_str().to_string())
                    .clicked()
                {
                    choose_axis(app, axis, variable);
                    changed = true;
                }
            }
        });
    // The letter beside the box is the box's name. Without this the combo has no
    // accessible name at all: a screen reader says "combo box" and the headless
    // frame test refuses the picture (kittest checks for unnamed inputs), which
    // is how this was found.
    combo.response.labelled_by(label_response.id);
    changed
}

/// The result, with its LaTeX source beside it in small type.
///
/// No frame: a bordered card around a result costs height and draws attention to
/// the box rather than to the answer.
fn result_card(app: &CasApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("RESULT").small().strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("display form")
                    .small()
                    .color(crate::theme::weak()),
            );
        });
    });
    ui.add_space(4.0);
    if app.result_latex.is_empty() {
        ui.label(
            RichText::new("Type an expression above; it is parsed as you type.")
                .small()
                .color(crate::theme::weak()),
        );
    } else {
        result_body(app, ui);
        // The decimal belongs under the exact form, not instead of it.
        if let Some(approximation) = &app.approximation {
            ui.add_space(2.0);
            ui.label(
                RichText::new(approximation.as_str())
                    .small()
                    .color(crate::theme::weak()),
            );
        }
        // The sign table is the answer to "where is this increasing", which the
        // plot shows as a shape and cannot say in words.
        if !app.sign_table.is_empty() {
            sign_table_body(app, ui);
        }
    }
}

/// The sign table of the document's expression, or an empty one.
///
/// Only a one-variable document has one: a sign table is "where is this
/// increasing", and a surface has no single answer. The interval is the plot's
/// own `x` window, so the table and the picture describe the same piece of the
/// curve — the same rule `canlab sign` follows.
pub fn build_sign_table(app: &CasApp) -> Vec<canlab_parse::calculus::Piece> {
    let (Some(expression), Some(var)) = (&app.expr, app.x_axis) else {
        return Vec::new();
    };
    // A sign table that failed is not an error to show: the expression may be
    // outside what the search can certify, and the result panel already says
    // what the answer is.
    canlab_parse::calculus::sign_table(expression, var, app.x_min, app.x_max).unwrap_or_default()
}

/// The sign table, as the words `canlab sign` prints.
///
/// The same three-line answer the CLI gives, in the same words, so a reader who
/// has seen one recognises the other.
fn sign_table_body(app: &CasApp, ui: &mut Ui) {
    ui.add_space(6.0);
    ui.label(
        RichText::new("sign table")
            .small()
            .color(crate::theme::weak()),
    );
    let weak = crate::theme::weak();
    for piece in &app.sign_table {
        let (word, colour) = match piece.direction {
            1 => ("increasing", crate::theme::accent()),
            -1 => ("decreasing", weak),
            _ => ("constant", weak),
        };
        ui.label(
            RichText::new(format!(
                "{word} on ({:.4}, {:.4})",
                piece.from, piece.to
            ))
            .size(12.0)
            .color(colour),
        );
    }
}

/// The typeset result, with its LaTeX source beside it in small type.
///
/// The LaTeX is the thing that travels — to the clipboard, into an exam answer, to
/// the solver — so it stays visible next to the picture rather than being hidden
/// behind it. When the string is outside the subset the renderer understands
/// (a `cases` block, say), the card falls back to showing the source alone.
fn result_body(app: &CasApp, ui: &mut Ui) {
    let style = Style {
        base_size: 28.0,
        ..Style::default()
    };
    let weak = crate::theme::weak();
    let rendered = canlab_input::MathField::from_latex(&app.result_latex).ok();

    ui.horizontal_top(|ui| {
        if let Some(field) = &rendered {
            MathView::display(ui, field, &style);
        } else {
            ui.label(RichText::new(app.result_plain.as_str()).size(24.0));
        }

        ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(app.result_latex.as_str())
                        .monospace()
                        .size(11.0)
                        .color(weak),
                );
                ui.label(RichText::new("LaTeX").size(9.0).color(weak));
            });
        });
    });
}

/// Apply one palette key to the field.
///
/// The button's own `insert` string is the command Abitti's editor would run, so
/// it is the thing we interpret: structures become structures, `\alpha` becomes a
/// token, `½` becomes a real fraction. Entries the model does not have (tables,
/// `\overleftarrow`) report why instead of pasting raw LaTeX into the field.
fn apply_palette(app: &mut CasApp, button: &canlab_parse::palette::Button) {
    app.note_edit(true);
    if let Err(error) = app.field.apply_snippet(button.insert) {
        app.status = format!("{}: {error}", button.label);
    }
    app.recompute();
}

pub fn palette(app: &mut CasApp, ui: &mut Ui) {
    ui.label(
        RichText::new("Abitti palette")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.add_space(2.0);
    egui::ScrollArea::vertical()
        .id_salt("palette-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for &(category, buttons) in canlab_parse::palette::ALL {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(category.to_uppercase())
                        .small()
                        .strong()
                        .color(crate::theme::weak()),
                );
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    for button in buttons {
                        if ui
                            .add(
                                Button::new(button.label)
                                    .min_size(vec2(34.0, 29.0))
                                    .corner_radius(5),
                            )
                            .on_hover_text(button.insert)
                            .clicked()
                        {
                            apply_palette(app, button);
                            ui.ctx().request_repaint();
                        }
                    }
                });
            }
        });
}

pub fn variables(app: &mut CasApp, ui: &mut Ui) {
    // Scrollable: a document with definitions, free variables and an answer is
    // taller than a dock panel, and clipping the definitions was how they
    // appeared to be missing.
    egui::ScrollArea::vertical()
        .id_salt("variables-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| variables_body(app, ui));
}

/// Is this keypress the **dead circumflex** — the `^~` key on a Nordic layout?
///
/// A dead key emits a key event and no text at all, so `x^2` arrives as `x`, the
/// key, then `2`, and the exponent vanishes unless the reader knows the key. The
/// `frame_has_text` guard is what keeps this from stealing shift+`]` on a layout
/// where that key types `}`: if the frame produced any text, the text wins.
///
/// Kept as a function of its inputs rather than inlined so the rule can be
/// asserted without a window: simulating focus in a headless frame is the sort of
/// test that passes for the wrong reason.
pub(crate) fn is_dead_circumflex(key: egui::Key, shift: bool, frame_has_text: bool) -> bool {
    shift && !frame_has_text && key == egui::Key::CloseBracket
}
fn variables_body(app: &mut CasApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("SOLVER")
                .small()
                .strong()
                .color(crate::theme::weak()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(app.jobs.backend_label())
                    .small()
                    .color(crate::theme::weak()),
            )
            .on_hover_text(
                app.jobs
                    .backend()
                    .note
                    .unwrap_or_else(|| String::from("The solver runs in the background.")),
            );
        });
    });
    ui.add_space(4.0);
    solver_row(app, ui);
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);

    // Values the page states. They are not *free*, so they are listed once, as
    // values, and not again as variables: `a` in `a = 2b`, `b = 3` is 6, and
    // calling it a free variable would invite a reader to look for a knob.
    let stated = app.document_assignments();

    ui.label(
        RichText::new("FREE VARIABLES")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.add_space(4.0);
    let free: Vec<canlab_core::SymbolId> = app
        .free_vars
        .iter()
        .copied()
        .filter(|variable| !stated.iter().any(|(symbol, _)| symbol == variable))
        .collect();
    if free.is_empty() {
        ui.label(
            RichText::new("No free variables.")
                .small()
                .color(crate::theme::weak()),
        );
    } else {
        for var in &free {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 2.5, crate::theme::accent());
                ui.label(RichText::new(var.as_str()).monospace().size(15.0).strong());
            });
        }
    }

    // Values the page states, listed as values. `b = 3` is a statement, not a
    // choice, so it is not a slider — and when the page says `a = 2b` the reader
    // can see that the answer is 6 rather than a knob they did not touch.
    if !stated.is_empty() {
        ui.add_space(10.0);
        ui.label(
            RichText::new("FROM THE DOCUMENT")
                .small()
                .strong()
                .color(crate::theme::weak()),
        );
        ui.add_space(4.0);
        for (symbol, value) in &stated {
            ui.label(
                RichText::new(format!(
                    "{} = {}",
                    symbol.as_str(),
                    to_latex(value)
                ))
                .monospace()
                .size(14.0),
            );
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        RichText::new("DEFINITIONS")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.add_space(4.0);
    if app.definitions.is_empty() && app.definition_problems.is_empty() {
        ui.label(
            RichText::new("None. Write f(x) = x² + 1 to define one.")
                .small()
                .color(crate::theme::weak()),
        );
    } else {
        for (name, function) in app.definitions.functions() {
            let params: Vec<String> = function
                .params
                .iter()
                .map(|parameter| to_latex(&canlab_core::sym(parameter.as_str().as_str())))
                .collect();
            ui.label(
                RichText::new(name.to_string())
                    .monospace()
                    .size(15.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!(
                    "{name}({}) = {}",
                    params.join(", "),
                    to_latex(&function.body)
                ))
                .monospace()
                .size(11.0)
                .color(crate::theme::weak()),
            );
        }
        for (name, why) in &app.definition_problems {
            ui.label(
                RichText::new(format!("{name}: {why}"))
                    .small()
                    .color(ui.visuals().warn_fg_color),
            );
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        RichText::new("SOLUTIONS")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.add_space(4.0);
    match &app.solve {
        SolveState::Idle => {
            ui.label(
                RichText::new("Run Solve to see values here.")
                    .small()
                    .color(crate::theme::weak()),
            );
        }
        SolveState::Declined { label, why } => {
            ui.label(
                RichText::new(label.as_str())
                    .small()
                    .color(crate::theme::weak()),
            );
            ui.label(
                RichText::new("declined")
                    .small()
                    .strong()
                    .color(ui.visuals().warn_fg_color),
            );
            ui.label(
                RichText::new(why.as_str())
                    .small()
                    .color(crate::theme::weak()),
            );
        }
        SolveState::Points(answer) => {
            use canlab_solver::nonlinear::SystemAnswer;
            match answer {
                SystemAnswer::None => {
                    ui.label(
                        RichText::new("No solutions.")
                            .small()
                            .color(crate::theme::weak()),
                    );
                }
                SystemAnswer::All => {
                    ui.label(RichText::new("Every point.").monospace().size(15.0));
                }
                SystemAnswer::Points(points) => {
                    // One line per point: two points are two answers, and a
                    // single line would lose one of them.
                    for point in points {
                        let parts: Vec<String> = point
                            .iter()
                            .map(|(variable, value)| {
                                let name = to_latex(&canlab_core::sym(&variable.as_str()));
                                format!("{name} = {}", to_latex(value))
                            })
                            .collect();
                        ui.label(
                            RichText::new(format!("\\left({}\\right)", parts.join(",\\ ")))
                                .monospace()
                                .size(15.0),
                        );
                    }
                }
            }
        }
        SolveState::System { assignments } => {
            if assignments.is_empty() {
                ui.label(
                    RichText::new("No solutions.")
                        .small()
                        .color(crate::theme::weak()),
                );
            } else {
                for (variable, value) in assignments {
                    let name = to_latex(&canlab_core::sym(&variable.as_str()));
                    ui.label(
                        RichText::new(format!("{name} = {}", to_latex(value)))
                            .monospace()
                            .size(15.0),
                    );
                }
            }
        }
        SolveState::Failed(message) => {
            ui.label(
                RichText::new("solver failed")
                    .small()
                    .strong()
                    .color(ui.visuals().error_fg_color),
            );
            ui.label(
                RichText::new(message.as_str())
                    .small()
                    .color(crate::theme::weak()),
            );
        }
        SolveState::Solved { var, solutions } => {
            use canlab_solver::Solutions;
            match solutions {
                Solutions::Values(values) if values.is_empty() => {
                    ui.label(
                        RichText::new("No solutions.")
                            .small()
                            .color(crate::theme::weak()),
                    );
                }
                Solutions::None => {
                    ui.label(
                        RichText::new("No solutions.")
                            .small()
                            .color(crate::theme::weak()),
                    );
                }
                Solutions::All => {
                    ui.label(RichText::new("Every number.").monospace().size(15.0));
                }
                // A numeric root is approximate by construction, so it is
                // written with `\approx` and carries its residual: the number
                // alone would look as certain as an exact answer.
                Solutions::Approximation { value, residual } => {
                    let name = to_latex(&canlab_core::sym(&var.as_str()));
                    ui.label(
                        RichText::new(format!(
                            "{name} \\approx {}",
                            crate::approximation_value(value)
                        ))
                        .monospace()
                        .size(15.0),
                    )
                    .on_hover_text(format!(
                        "found numerically: the relative residual is {residual:.1e}, so \
                         |f(x)| is that fraction of the function's scale, not zero"
                    ));
                    ui.label(
                        RichText::new(format!("residual {residual:.1e}"))
                            .small()
                            .color(crate::theme::weak()),
                    )
                    .on_hover_text(
                        "|f(x)| at the root divided by the function's scale. A true root \
                         lands far below 1e-10; a sign change at a pole does not.",
                    );
                }
                _ => {
                    for line in crate::solution_lines(solutions, *var) {
                        ui.label(RichText::new(line).monospace().size(15.0));
                    }
                }
            }
        }
    }
}

/// The data window a canvas shows.
///
/// One unit is the same number of pixels on both axes: that is the whole point,
/// because a circle that renders as an ellipse is a wrong picture. When the panel
/// is wider than the window it was fitted to, the x *range* grows and the same
/// picture sits in a wider plane; when it is taller, y grows. The axes never
/// stretch.
///
/// The stored `x_min..x_max` and `y_range` stay the window the document was
/// fitted to (or the reader panned to). This type is what the canvas shows of
/// it, so widening the panel does not change the session's window or the scale
/// at which the picture was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) x: (f64, f64),
    pub(crate) y: (f64, f64),
}

impl View {
    /// The view of `window` on a canvas of `canvas` points.
    ///
    /// `None` is the state before the plot has been drawn: builds that run
    /// before the first frame, and tests that never open a panel. The stored
    /// window is then used unchanged, which is what sampling did before the
    /// canvas existed.
    pub(crate) fn new(canvas: Option<egui::Vec2>, x: (f64, f64), y: (f64, f64)) -> Self {
        let stored = Self { x, y };
        let Some(canvas) = canvas else {
            return stored;
        };
        if !(x.1 > x.0 && y.1 > y.0) || canvas.x <= 0.0 || canvas.y <= 0.0 {
            return stored;
        }
        let units_x = x.1 - x.0;
        let units_y = y.1 - y.0;
        // The scale the letterboxed picture used to be drawn at: the shorter
        // side of the panel decided it. Keeping that number is what the user
        // called good; only how much plane fits around the picture may change.
        let pixels_per_unit =
            (f64::from(canvas.x) / units_x).min(f64::from(canvas.y) / units_y);
        if !pixels_per_unit.is_finite() || pixels_per_unit <= 0.0 {
            return stored;
        }
        let visible_x = f64::from(canvas.x) / pixels_per_unit;
        let visible_y = f64::from(canvas.y) / pixels_per_unit;
        // Keep the centre: the fitted picture stays where it was, and the extra
        // plane appears on both sides of it rather than only on the right.
        let centre_x = (x.0 + x.1) * 0.5;
        let centre_y = (y.0 + y.1) * 0.5;
        Self {
            x: (centre_x - visible_x * 0.5, centre_x + visible_x * 0.5),
            y: (centre_y - visible_y * 0.5, centre_y + visible_y * 0.5),
        }
    }

    fn drawable(&self) -> bool {
        self.x.1 > self.x.0 && self.y.1 > self.y.0
    }

    /// Where `(x, y)` is painted on `rect`.
    ///
    /// Both axes use the same scale by construction: `rect.width() / x-span`
    /// equals `rect.height() / y-span`, so a circle stays a circle.
    fn to_px(&self, rect: Rect, x: f64, y: f64) -> egui::Pos2 {
        pos2(
            rect.left() + ((x - self.x.0) / (self.x.1 - self.x.0)) as f32 * rect.width(),
            rect.bottom() - ((y - self.y.0) / (self.y.1 - self.y.0)) as f32 * rect.height(),
        )
    }

    /// The data coordinate under `point`, for a zoom anchor.
    fn from_px(&self, rect: Rect, point: egui::Pos2) -> (f64, f64) {
        (
            self.x.0 + ((point.x - rect.left()) / rect.width()) as f64 * (self.x.1 - self.x.0),
            self.y.0 + ((rect.bottom() - point.y) / rect.height()) as f64 * (self.y.1 - self.y.0),
        )
    }
}

pub fn plot(app: &mut CasApp, ui: &mut Ui) {
    // The canvas is the whole panel, and the controls float over its top edge.
    // A row the controls claim for themselves is graph nobody sees, and the
    // graph is the panel's answer.
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // The canvas shape decides how much plane is visible, and a build may have
    // run before this panel was ever drawn: the document is plotted on every
    // keystroke, while the panel is only drawn once a frame. Trace again when
    // the shape changed, so the curve covers the window actually shown. The
    // fitted window itself is not touched — a wider panel shows more plane, it
    // does not choose a different window.
    let canvas = rect.size();
    if app.plot_canvas != Some(canvas) {
        app.plot_canvas = Some(canvas);
        if app.can_plot() && !app.is_typing() {
            build_curve_fitting(app, false);
        }
    }

    let view = app.plot_view();
    let mut moved = false;
    if view.drawable() && rect.width() > 1.0 && rect.height() > 1.0 {
        // Drag to pan: the content follows the pointer, so the window moves the
        // other way. The delta is converted with the view the picture is drawn
        // in, so a drag covers exactly the data the pointer travelled over.
        let delta = response.drag_delta();
        if response.dragged() && delta != egui::Vec2::ZERO {
            let dx = -(delta.x as f64) * (view.x.1 - view.x.0) / rect.width() as f64;
            let dy = (delta.y as f64) * (view.y.1 - view.y.0) / rect.height() as f64;
            app.x_min += dx;
            app.x_max += dx;
            app.y_range = (app.y_range.0 + dy, app.y_range.1 + dy);
            moved = true;
        }

        // Scroll to zoom about the pointer, so the point under the cursor stays
        // where it is. The anchor is a data coordinate in `view`; scaling the
        // stored window about the same coordinate scales the view about it too,
        // because showing more plane is a linear map of the stored window.
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll.abs() > 0.5 {
                let factor = (-(scroll as f64) * 0.0020).exp().clamp(0.25, 4.0);
                let anchor = response
                    .hover_pos()
                    .map(|position| view.from_px(rect, position))
                    .unwrap_or(((view.x.0 + view.x.1) * 0.5, (view.y.0 + view.y.1) * 0.5));
                app.x_min = anchor.0 + (app.x_min - anchor.0) * factor;
                app.x_max = anchor.0 + (app.x_max - anchor.0) * factor;
                app.y_range = (
                    anchor.1 + (app.y_range.0 - anchor.1) * factor,
                    anchor.1 + (app.y_range.1 - anchor.1) * factor,
                );
                moved = true;
            }
        }
    }
    if moved {
        // Not fitted: the user has chosen this window, and refitting would fight
        // the gesture that just set it.
        plot_changed(app);
        ui.ctx().request_repaint();
    }

    // The window may have moved, so read it again after the gesture.
    let view = app.plot_view();
    if app.curves.is_empty() && app.intersections.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Nothing to plot yet.",
            FontId::proportional(13.0),
            crate::theme::weak(),
        );
    } else if view.drawable() {
        // Axes once, then every curve on top of them in its own colour, so a
        // system of two equations reads as two lines rather than one thick one.
        // Everything is painted into the same rectangle the sampler was given,
        // so one unit is the same number of pixels along both axes.
        paint_axes(&painter, rect, &view);
        for (index, curve) in app.curves.iter().enumerate() {
            let paint = app
                .curve_paints
                .get(index)
                .copied()
                .unwrap_or(CurvePaint::Wheel);
            let color = match paint {
                CurvePaint::Wheel => curve_color(index),
                CurvePaint::Family { equation, centre } => {
                    family_curve_color(equation, centre)
                }
            };
            paint_curve(&painter, rect, curve, &view, color);
        }
        paint_intersections(&painter, &response, app, &view, rect);
    }

    plot_overlay(app, ui, rect);
}

/// The plot's controls, drawn over the top edge of the canvas.
///
/// The strip has a background but no widget of its own: a drag that starts on
/// the graph — even on an empty part of the strip — still pans it, and only
/// where a picker or a number box actually is does the overlay take the pointer.
fn plot_overlay(app: &mut CasApp, ui: &mut Ui, rect: Rect) {
    let margin = 8.0;
    if rect.width() <= 2.0 * margin || rect.height() <= 2.0 * margin {
        return;
    }
    let bounds = rect.shrink(margin);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(bounds)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(bounds);
    egui::Frame::new()
        .fill(crate::theme::overlay())
        .stroke(Stroke::new(1.0, crate::theme::line()))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(&mut child, |ui| {
            // Tight on purpose: in a narrow dock every row the strip takes is a
            // row of graph it covers. The controls stay one line high wherever
            // they fit; wrapping is what makes a small panel unreadable.
            ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
            ui.spacing_mut().interact_size.y = 22.0;
            plot_row(app, ui);
            if !app.parameters.is_empty() {
                parameter_row(app, ui);
            }
            plot_note_row(app, ui);
        });
}

/// The plot's caption: what is drawn, on which axes, at which parameter values.
///
/// Not selectable: it is a caption rather than a control, and a drag that starts
/// on it must still pan the picture underneath.
fn plot_note_row(app: &CasApp, ui: &mut Ui) {
    ui.add(
        Label::new(
            RichText::new(app.plot_note.as_str())
                .small()
                .color(crate::theme::weak()),
        )
        .truncate()
        .selectable(false),
    );
}

/// Draw the intersections as points, and name the one under the pointer.
fn paint_intersections(
    painter: &egui::Painter,
    response: &egui::Response,
    app: &CasApp,
    view: &View,
    rect: Rect,
) {
    if app.intersections.is_empty() {
        return;
    }
    let to_px = |x: f64, y: f64| view.to_px(rect, x, y);
    for intersection in &app.intersections {
        let point = to_px(intersection.x, intersection.y);
        painter.circle_filled(point, 4.5, marker_color());
        painter.circle_stroke(point, 4.5, Stroke::new(1.5, crate::theme::current().panel));
    }

    let Some(pointer) = response.hover_pos() else {
        return;
    };
    let nearest = app
        .intersections
        .iter()
        .map(|intersection| (intersection, to_px(intersection.x, intersection.y)))
        .filter(|(_, point)| (*point - pointer).length() <= 16.0)
        .min_by(|a, b| (a.1 - pointer).length().total_cmp(&(b.1 - pointer).length()));
    let Some((intersection, at)) = nearest else {
        return;
    };
    let text = format!("({:.4}, {:.4})", intersection.x, intersection.y);
    let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), crate::theme::text());
    let mut label = Rect::from_min_size(
        at + vec2(10.0, -20.0),
        galley.size() + vec2(8.0, 4.0),
    );
    if label.right() > rect.right() {
        label = label.translate(vec2(rect.right() - label.right() - 2.0, 0.0));
    }
    if label.top() < rect.top() {
        label = label.translate(vec2(0.0, at.y - label.top() + 8.0));
    }
    painter.rect_filled(label, 3.0, crate::theme::current().panel);
    painter.galley(label.min + vec2(4.0, 2.0), galley, crate::theme::text());
}

/// One row per *parameter*: a variable in the document that is not on an axis.
///
/// This is what replaces `l = 1 assumed`: the variable still starts at 1, but the
/// value is now a control, and the plot says what it is set to.
fn parameter_row(app: &mut CasApp, ui: &mut Ui) {
    let mut changed = false;
    // The heading is the first thing to go in a narrow strip: the name and its
    // number identify the control on their own, and the heading costs a row.
    let compact = ui.available_width() < 220.0;
    ui.horizontal_wrapped(|ui| {
        if !compact {
            ui.label(
                RichText::new("parameters")
                    .small()
                    .strong()
                    .color(crate::theme::weak()),
            );
        }
        for parameter in &mut app.parameters {
            ui.label(
                RichText::new(parameter.name.as_str())
                    .monospace()
                    .size(12.0)
                    .strong(),
            );
            // A free parameter is a family, not a number: the toggle is the
            // control that says which of the two the parameter is. The drag
            // stays, because the centre member of the family is the value the
            // drag shows.
            changed |= ui
                .add(egui::Checkbox::new(&mut parameter.free, "free"))
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut parameter.value)
                        .speed(0.02)
                        .range(-1.0e6..=1.0e6),
                )
                .changed();
        }
    });
    if changed {
        plot_changed(app);
    }
}

/// A distinct colour per curve, from the palette's wheel.
fn curve_color(index: usize) -> Color32 {
    crate::theme::curve_color(index)
}

/// How a curve is painted when it is not simply the next wheel colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurvePaint {
    /// The next colour in the wheel: a normal curve, or a definition curve.
    Wheel,
    /// A member of a free parameter's family. `equation` is the equation's
    /// index in the system, so its colour is the same as the single curve's
    /// would have been; `centre` is the member at the parameter's current value.
    Family { equation: usize, centre: bool },
}

/// The colour of a family member: the equation's colour, full strength at the
/// centre and translucent for the members either side.
///
/// The curves are the *explanation* of the family, so the side members are
/// faint enough that overlaps read as overlaps; the centre is the picture the
/// user was already looking at, so it stays full strength.
fn family_curve_color(equation: usize, centre: bool) -> Color32 {
    let base = curve_color(equation);
    if centre {
        base
    } else {
        base.gamma_multiply(0.5)
    }
}

/// The colour of an intersection marker: full strength, because the answer has
/// to stand out against the faint curves of a family.
fn marker_color() -> Color32 {
    crate::theme::accent()
}

/// Draw the grid, the axes and the window label into `rect`.
///
/// `view` is the window the canvas shows, so the label names the plane that is
/// actually visible, not just the fitted window inside it.
fn paint_axes(painter: &egui::Painter, rect: Rect, view: &View) {
    // Colours come from the palette, not from egui's derived text colours: the
    // grid is a *line* colour and the labels are *weak* text, which is what makes
    // the axes recede in every scheme instead of just the default one.
    let axis_color = crate::theme::weak();
    let label_color = crate::theme::weak();
    let grid_color = crate::theme::line();
    let (x0, x1) = view.x;
    let (y0, y1) = view.y;
    if !view.drawable() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "x range is empty",
            FontId::proportional(13.0),
            crate::theme::current().warn,
        );
        return;
    }

    let to_px = |x: f64, y: f64| view.to_px(rect, x, y);

    // A subtle grid: four vertical and four horizontal divisions.
    let grid = Stroke::new(1.0, grid_color);
    for step in 1..4 {
        let t = step as f64 / 4.0;
        let x = rect.left() + rect.width() * t as f32;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], grid);
        let y = rect.top() + rect.height() * t as f32;
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], grid);
    }

    let axis = Stroke::new(1.4, axis_color);
    if x0 <= 0.0 && x1 >= 0.0 {
        let x = to_px(0.0, y0).x;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], axis);
    }
    if y0 <= 0.0 && y1 >= 0.0 {
        let y = to_px(x0, 0.0).y;
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], axis);
    }

    // Along the bottom: the top of the plot is where the controls float, and a
    // label underneath them is a label nobody can read.
    painter.text(
        pos2(rect.left() + 8.0, rect.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        format!("x [{x0:.3}, {x1:.3}]   y [{y0:.3}, {y1:.3}]"),
        FontId::monospace(10.0),
        label_color,
    );
}

/// Draw one curve; the axes are drawn once, before any of them.
fn paint_curve(
    painter: &egui::Painter,
    rect: Rect,
    curve: &Curve,
    view: &View,
    color: Color32,
) {
    if !view.drawable() {
        return;
    }
    let stroke = Stroke::new(2.2, color);
    for segment in &curve.segments {
        let points: Vec<egui::Pos2> = segment
            .iter()
            .filter(|point| point[0].is_finite() && point[1].is_finite())
            .map(|point| view.to_px(rect, point[0], point[1]))
            .collect();
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

/// The results so far, newest first. Clicking one puts it back into the editor.
pub fn history(app: &mut CasApp, ui: &mut Ui) {
    if app.history.is_empty() {
        ui.label(
            RichText::new("No results yet. Simplify, integrate or solve something.")
                .small()
                .color(crate::theme::weak()),
        );
        return;
    }

    ui.label(
        RichText::new("NEWEST FIRST · click one to edit it again")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.add_space(4.0);
    let mut load: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt("history-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &app.history {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(entry.label.as_str())
                        .small()
                        .color(crate::theme::weak()),
                );
                let response = ui.add(
                    Label::new(RichText::new(entry.latex.as_str()).monospace().size(14.0))
                        .wrap()
                        .selectable(false)
                        .sense(Sense::click()),
                );
                let response = response.on_hover_text("Load this back into the editor");
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    load = Some(entry.latex.clone());
                }
                ui.add_space(4.0);
            }
        });
    if let Some(latex) = load {
        app.load_latex(&latex);
    }
}

/// Sample the document and keep the curves for the Plot panel.
///
/// Three kinds of picture come out of one document:
///
/// * an *equation* — a line like `x + y = 3` is not a function of x, so it is
///   traced as the set of points where `left − right = 0`. A system gives one
///   curve per line, and where two of them cross the crossing is refined to a
///   point rather than left as a ring of samples;
/// * a *function* `y = f(x)` — the field holds one expression, so it is sampled
///   along x;
/// * a *definition* — `f(x) = x² + 1` is a curve of its own, drawn as `y = f(x)`
///   against the variable it is a function of.
pub fn build_curve(app: &mut CasApp) {
    build_curve_fitting(app, true);
}

/// As [`build_curve`], but saying whether the view should be fitted to what was
/// drawn. Automatic plotting fits (a unit circle in a -10..10 window is tiny);
/// changing the range by hand, or panning, must not, or the two would fight.
pub fn build_curve_fitting(app: &mut CasApp, fit: bool) {
    app.curves.clear();
    app.curve_paints.clear();
    app.intersections.clear();
    if !app.x_max.is_finite() || !app.x_min.is_finite() || app.x_max <= app.x_min {
        app.plot_note = String::from("x range is empty");
        return;
    }

    let equations = app
        .field
        .build_equations_with(&app.definitions)
        .unwrap_or_default();
    if equations.iter().any(|(_, right)| right.is_some()) {
        plot_equations(app, &equations, fit);
        return;
    }

    // No equations: a function, and any definitions beside it.
    let mut noted: Vec<String> = Vec::new();
    if let Some(expr) = app.expr.clone() {
        if app.free_vars.len() != 1 {
            app.plot_note = format!("plot needs one variable, found {}", app.free_vars.len());
            return;
        }
        let Some(program) = canlab_core::Program::compile(&expr, &app.free_vars) else {
            app.plot_note = String::from("cannot evaluate numerically");
            return;
        };
        // Fitting must not read the filled view: the y range it is about to
        // choose is what decides how wide that view is, so fitting on it would
        // walk the range outward on every keystroke. Fit on the stored x window,
        // then trace the plane the panel will show without moving the range.
        let window = if fit { (app.x_min, app.x_max) } else { app.plot_view().x };
        let mut curve = sample_window(&program, window);
        if fit {
            app.y_range = vertical_extent(&curve).unwrap_or((-1.0, 1.0));
            let drawn = app.plot_view().x;
            if drawn != window {
                curve = sample_window(&program, drawn);
            }
        }
        let count = curve.segments.len();
        noted.push(match count {
            1 => String::from("1 segment"),
            n => format!("{n} segments"),
        });
        app.curves.push(curve);
        app.curve_paints.push(CurvePaint::Wheel);
    }
    noted.extend(plot_definitions(app, fit));
    if app.curves.is_empty() && app.intersections.is_empty() {
        app.plot_note = String::from("nothing to plot");
        return;
    }
    if fit {
        fit_to_curves(app, app.curves.len());
    }
    app.plot_note = noted.join(", ");
}

/// Sample `program` across an x window, for the plot's width.
fn sample_window(program: &canlab_core::Program, window: (f64, f64)) -> Curve {
    sample(
        program,
        &SampleOptions {
            x_min: window.0,
            x_max: window.1,
            ..SampleOptions::default()
        },
    )
}

/// Draw `y = f(x)` for every definition whose parameter is the horizontal axis.
///
/// Returns at most one note fragment, which is what the plot caption says when a
/// definition was drawn.
fn plot_definitions(app: &mut CasApp, fit: bool) -> Vec<String> {
    let Some(horizontal) = app.x_axis else {
        return Vec::new();
    };
    let definitions: Vec<(String, Vec<canlab_core::SymbolId>, canlab_core::Expr)> = app
        .definitions
        .functions()
        .map(|(name, function)| {
            (
                name.to_string(),
                function.params.clone(),
                function.body.clone(),
            )
        })
        .collect();
    // Only the definitions decide the vertical range when nothing else was drawn:
    // an expression has already been sampled into a range that fits it.
    let first = app.curves.is_empty();
    // Same rule as the expression path: a definition-only page fits its y range
    // from the stored x window, not from the filled one, so the range cannot
    // chase itself. Otherwise the window is whatever the canvas shows.
    let window = if fit && first {
        (app.x_min, app.x_max)
    } else {
        app.plot_view().x
    };
    let mut curves = definition_curves(&definitions, horizontal, window);
    if first && fit {
        let mut extent: Option<(f64, f64)> = None;
        for (_, curve) in &curves {
            if let Some((low, high)) = vertical_extent(curve) {
                extent = Some(match extent {
                    Some((existing_low, existing_high)) => {
                        (existing_low.min(low), existing_high.max(high))
                    }
                    None => (low, high),
                });
            }
        }
        if let Some(range) = extent {
            app.y_range = range;
        }
        // The fitted y range may have changed how much x the panel shows; trace
        // that plane too, without touching the range that was just fitted.
        let drawn = app.plot_view().x;
        if drawn != window {
            curves = definition_curves(&definitions, horizontal, drawn);
        }
    }
    let mut labels: Vec<String> = Vec::new();
    for (label, curve) in curves {
        labels.push(label);
        app.curves.push(curve);
        app.curve_paints.push(CurvePaint::Wheel);
    }
    if labels.is_empty() {
        Vec::new()
    } else {
        vec![format!("definition(s) of {}", labels.join(", "))]
    }
}

/// Sample one curve per definition that is a function of the horizontal axis.
fn definition_curves(
    definitions: &[(String, Vec<canlab_core::SymbolId>, canlab_core::Expr)],
    horizontal: canlab_core::SymbolId,
    window: (f64, f64),
) -> Vec<(String, Curve)> {
    let mut out = Vec::new();
    for (name, params, body) in definitions {
        // A function of two variables is a surface; a function of a *different*
        // variable does not belong on this horizontal axis.
        if params.len() != 1 || params[0] != horizontal {
            continue;
        }
        let Some(program) = canlab_core::Program::compile(body, &[horizontal]) else {
            continue;
        };
        let curve = sample_window(&program, window);
        if curve.segments.is_empty() {
            continue;
        }
        out.push((format!("{name}({})", horizontal.as_str()), curve));
    }
    out
}

/// Grow the view to contain everything that was drawn.
///
/// The window is only ever *shrunk* to the curves, never grown past a sane size,
/// and never below a minimum: a curve that is a single point would otherwise ask
/// for a zero-width window.
fn fit_to_curves(app: &mut CasApp, limit: usize) {
    // No equation curve was produced. `plot_definitions` may still have added a
    // definition curve to the same list, and fitting to that is what the comment
    // on `plot_equations` promises not to do.
    if limit == 0 || app.curves.is_empty() {
        return;
    }
    let mut low_x = f64::MAX;
    let mut high_x = f64::MIN;
    let mut low_y = f64::MAX;
    let mut high_y = f64::MIN;
    let considered = limit.clamp(1, app.curves.len());
    for curve in app.curves.iter().take(considered) {
        for segment in &curve.segments {
            for point in segment {
                low_x = low_x.min(point[0]);
                high_x = high_x.max(point[0]);
                low_y = low_y.min(point[1]);
                high_y = high_y.max(point[1]);
            }
        }
    }
    // A curve's bounding box can be degenerate or absurd (a pole, a failed
    // sample); fitting to that is how the range once jumped to -163.
    let sane = low_x.is_finite() && high_x.is_finite() && low_y.is_finite() && high_y.is_finite();
    let reasonable = (high_x - low_x) < 1.0e6 && (high_y - low_y) < 1.0e6;
    // `sane` already rules out NaN, so a plain `>=` is the right emptiness test.
    if !sane || !reasonable || low_x >= high_x || low_y >= high_y {
        return;
    }
    let pad = |low: f64, high: f64| {
        let margin = ((high - low) * 0.12).max(0.5);
        (low - margin, high + margin)
    };
    let (low_x, high_x) = pad(low_x, high_x);
    let (low_y, high_y) = pad(low_y, high_y);
    // Only ever *shrink*: an implicit curve is traced across the whole window, so
    // its bounding box is the window, and fitting to that would grow the view on
    // every redraw — each rebuild widening the range by the margin again.
    app.x_min = app.x_min.max(low_x);
    app.x_max = app.x_max.min(high_x);
    app.y_range = (app.y_range.0.max(low_y), app.y_range.1.min(high_y));
}

/// Frame the intersections: they are what a system's picture is *about*.
///
/// Fitting to the curves instead does not work for a linear equation, which
/// spans the whole window and therefore has no useful extent of its own.
fn fit_to_intersections(app: &mut CasApp) {
    if app.intersections.is_empty() {
        return;
    }
    let mut low_x = f64::MAX;
    let mut high_x = f64::MIN;
    let mut low_y = f64::MAX;
    let mut high_y = f64::MIN;
    for point in &app.intersections {
        low_x = low_x.min(point.x);
        high_x = high_x.max(point.x);
        low_y = low_y.min(point.y);
        high_y = high_y.max(point.y);
    }
    if !(low_x.is_finite() && high_x.is_finite() && low_y.is_finite() && high_y.is_finite()) {
        return;
    }
    let centre_x = (low_x + high_x) * 0.5;
    let centre_y = (low_y + high_y) * 0.5;
    // Enough context to see the curves meeting, not just a dot in the middle:
    // at least four units across, and half again the spread of the answers.
    let spread = (high_x - low_x).max(high_y - low_y);
    let half = (spread * 1.5).max(2.0);
    app.x_min = centre_x - half;
    app.x_max = centre_x + half;
    app.y_range = (centre_y - half, centre_y + half);
}

/// True when two refined crossings are the same point.
///
/// The tolerance is relative because a coordinate can be arbitrarily large or
/// small: a constant 1e-6 is looser than the plot's own `1e-9 * max(1, |x|)`
/// refinement near zero and far tighter than it near 1e6. Scaling by the larger
/// coordinate keeps this merge rule consistent with `locate` at both ends.
fn same_point(a: &Intersection, b: &Intersection) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    close(a.x, b.x) && close(a.y, b.y)
}

/// How many members a free parameter's family draws.
const FAMILY_MEMBERS: usize = 5;

/// The member drawn at the parameter's current value: the centre of the five.
const FAMILY_CENTRE: usize = FAMILY_MEMBERS / 2;

/// The five levels a free parameter's family is drawn at.
///
/// The centre is the parameter's current value, so the picture the user was
/// already looking at stays in the middle. The other four are evenly spaced
/// between half and three-halves of that value, in the parameter's own units:
/// the visible range of the curve, not a new scale. A value of zero has no half
/// to speak of, so the family is a unit step around it instead.
fn family_levels(value: f64) -> [f64; FAMILY_MEMBERS] {
    if !value.is_finite() {
        return [0.0; FAMILY_MEMBERS];
    }
    if value.abs() < 1e-12 {
        return [-0.5, -0.25, 0.0, 0.25, 0.5];
    }
    let step = value / 4.0;
    [
        value - 2.0 * step,
        value - step,
        value,
        value + step,
        value + 2.0 * step,
    ]
}

/// Trace every equation in the document as an implicit curve.
///
/// The two axes come from the document's variable list, so the letters are
/// irrelevant: `i² + j² = 1` with `i` on x and `j` on y plots exactly the same
/// circle as `x² + y² = 1` does. Any variable that is neither on an axis nor a
/// definition parameter is a *parameter*, sampled at the value the user set in
/// the plot overlay.
fn plot_equations(
    app: &mut CasApp,
    equations: &[(canlab_core::Expr, Option<canlab_core::Expr>)],
    fit: bool,
) {
    let Some(horizontal) = app.x_axis else {
        app.plot_note = String::from("no variables to plot");
        return;
    };
    let vertical = app.y_axis;
    let mut variables = vec![horizontal];
    if let Some(vertical) = vertical {
        variables.push(vertical);
    }
    let axes_label = match vertical {
        Some(vertical) => format!(
            "{} → x, {} → y",
            horizontal.as_str(),
            vertical.as_str()
        ),
        None => format!("{} → x", horizontal.as_str()),
    };

    // An equation has no preferred y range. Fitting frames it square on the x
    // window, as it always has; otherwise the y range the reader is looking at
    // is kept — refitting it here would undo a vertical pan on the very next
    // rebuild.
    let (y_min, y_max) = if fit {
        let span = (app.x_max - app.x_min).abs().max(1.0);
        (-span / 2.0, span / 2.0)
    } else {
        app.y_range
    };
    // Trace the window the canvas shows, not only the fitted one: a line that
    // leaves the fitted window continues across the plane the panel adds. This
    // changes no stored range — it is the same picture seen in a wider plane.
    let view = View::new(app.plot_canvas, (app.x_min, app.x_max), (y_min, y_max));
    let options = ImplicitOptions {
        resolution: 240,
        x_min: view.x.0,
        x_max: view.x.1,
        y_min: view.y.0,
        y_max: view.y.1,
    };

    // `left − right`, so the curve is where the equation holds. An equation whose
    // third coordinate is a whole side is a *surface*, and the 2D view cannot draw
    // one: it is counted and named, never sliced at an invented value. Plotting
    // `x² + y² = z` at `z = 1` answers a question nobody asked, and the picture
    // does not say so.
    let mut expressions: Vec<canlab_core::Expr> = Vec::new();
    let mut surfaces = 0usize;
    for (left, right) in equations {
        // `s = 1` states a parameter's value; it is not a curve. Plotting it would
        // draw the whole plane, because with `s = 1` the relation holds everywhere.
        if let Some(right) = right {
            let assigned = match (left.as_symbol(), right.as_number()) {
                (Some(symbol), Some(_)) => Some(symbol),
                _ => match (left.as_number(), right.as_symbol()) {
                    (Some(_), Some(symbol)) => Some(symbol),
                    _ => None,
                },
            };
            if let Some(symbol) = assigned {
                if app.parameters.iter().any(|parameter| parameter.name == symbol) {
                    continue;
                }
            }
        }
        let mut symbols = Vec::new();
        crate::free_symbols(left, &mut symbols);
        if let Some(right) = right {
            crate::free_symbols(right, &mut symbols);
        }
        if symbols
            .iter()
            .any(|symbol| app.third_coordinates.contains(symbol))
        {
            surfaces += 1;
            continue;
        }
        expressions.push(match right {
            Some(right) => canlab_core::add(vec![
                left.clone(),
                canlab_core::mul(vec![canlab_core::int(-1), right.clone()]),
            ]),
            None => left.clone(),
        });
    }

    // The parameters, in the order the compiled programs expect them.
    let parameter_names: Vec<canlab_core::SymbolId> =
        app.parameters.iter().map(|parameter| parameter.name).collect();
    let parameter_values: Vec<f64> = app
        .parameters
        .iter()
        .map(|parameter| parameter.value)
        .collect();
    let mut sampling = variables.clone();
    sampling.extend(parameter_names.iter().copied());

    // A free parameter is a variable the user asked to see as a family rather
    // than as one number. Which free parameters actually occur in the equations
    // decides whether there is a family at all: a free parameter that no
    // equation mentions would only draw the same picture five times.
    let free_indices: Vec<usize> = app
        .parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| parameter.free)
        .map(|(index, _)| index)
        .collect();
    let mut used_symbols: Vec<canlab_core::SymbolId> = Vec::new();
    for expression in &expressions {
        crate::free_symbols(expression, &mut used_symbols);
    }
    let used_free: Vec<usize> = free_indices
        .iter()
        .copied()
        .filter(|index| used_symbols.contains(&app.parameters[*index].name))
        .collect();
    let unused_free: Vec<usize> = free_indices
        .iter()
        .copied()
        .filter(|index| !used_free.contains(index))
        .collect();
    let has_free = !free_indices.is_empty();
    let family = !used_free.is_empty();

    // One level per member, shared by every equation in that member: the same
    // point of the family's parameter, not a separate level per equation. That
    // is what makes two circles of a system meet at the same level.
    let levels: Vec<[f64; FAMILY_MEMBERS]> = used_free
        .iter()
        .map(|index| family_levels(app.parameters[*index].value))
        .collect();
    let members: Vec<Vec<f64>> = if family {
        (0..FAMILY_MEMBERS)
            .map(|member| {
                let mut values = parameter_values.clone();
                for (position, index) in used_free.iter().enumerate() {
                    values[*index] = levels[position][member];
                }
                values
            })
            .collect()
    } else {
        vec![parameter_values.clone()]
    };
    // The centre member is drawn last: a full-strength curve drawn first is
    // half covered by the translucent ones either side of it, and no longer
    // reads as full strength.
    let member_order: Vec<usize> = if family {
        vec![0, 1, 3, 4, FAMILY_CENTRE]
    } else {
        vec![0]
    };

    let evaluate = |program: &canlab_core::Program, axes: &[f64], values: &[f64]| -> f64 {
        let mut args = axes.to_vec();
        args.extend(values.iter().copied());
        program.eval(&args)
    };

    let mut plotted = 0;
    let mut skipped = 0;
    let mut programs: Vec<canlab_core::Program> = Vec::new();
    for expression in &expressions {
        match canlab_core::Program::compile(expression, &sampling) {
            Some(program) => programs.push(program),
            None => skipped += 1,
        }
    }

    for member in member_order {
        for (equation, program) in programs.iter().enumerate() {
            let values = &members[member];
            let curve = if variables.len() == 2 {
                sample_implicit(|x, y| evaluate(program, &[x, y], values), &options)
            } else {
                // One variable: the zero set is a set of vertical lines.
                sample_implicit(|x, _| evaluate(program, &[x], values), &options)
            };
            if curve.segments.is_empty() {
                skipped += 1;
            } else {
                plotted += 1;
                app.curves.push(curve);
                app.curve_paints.push(if family {
                    CurvePaint::Family {
                        equation,
                        centre: member == FAMILY_CENTRE,
                    }
                } else {
                    CurvePaint::Wheel
                });
            }
        }
    }

    // Where two curves cross, the tracer only sees a ring of near-zero samples:
    // the sum of squares touches zero from above instead of crossing it. That
    // ring is a *hint*, and Newton's method turns each hint into a coordinate,
    // which is a thing a reader can read off instead of a blob they cannot.
    // Pairing is only meaningful *inside* one system. `x² + y² = 1` and `x = y`
    // in two different blocks are not simultaneous, so a crossing between them is
    // a point that satisfies neither block's question — a wrong answer drawn as a
    // dot. The flat equation list has lost the block each line came from, and
    // until the plot carries it, a page with more than one system draws **no**
    // intersections: a refusal is a smaller lie than an invented crossing.
    //
    // A free parameter makes the set of crossings infinite, so a family draws
    // no intersections at all: one point per pair would be a sample of a
    // continuum, not an answer.
    let one_system = app.field.systems().len() <= 1;
    if !has_free && programs.len() >= 2 && variables.len() == 2 && one_system {
        let tolerance = ((options.x_max - options.x_min) / options.resolution as f64) * 3.0;
        for first in 0..programs.len() {
            for second in (first + 1)..programs.len() {
                let mut peak: f64 = 0.0;
                for step in 0..=4 {
                    let t = step as f64 / 4.0;
                    let x = options.x_min + (options.x_max - options.x_min) * t;
                    let y = options.y_min + (options.y_max - options.y_min) * t;
                    let a = evaluate(&programs[first], &[x, y], &parameter_values);
                    let b = evaluate(&programs[second], &[x, y], &parameter_values);
                    peak = peak.max(a * a + b * b);
                }
                let epsilon = (peak * 0.004).max(f64::MIN_POSITIVE);
                let hint = sample_implicit(
                    |x, y| {
                        let a = evaluate(&programs[first], &[x, y], &parameter_values);
                        let b = evaluate(&programs[second], &[x, y], &parameter_values);
                        a * a + b * b - epsilon
                    },
                    &options,
                );
                let found = canlab_plot::locate(
                    |x, y| evaluate(&programs[first], &[x, y], &parameter_values),
                    |x, y| evaluate(&programs[second], &[x, y], &parameter_values),
                    &hint.segments,
                    tolerance,
                );
                for point in found {
                    let duplicate = app
                        .intersections
                        .iter()
                        .any(|existing| same_point(existing, &point));
                    if !duplicate {
                        app.intersections.push(point);
                    }
                }
            }
        }
    }

    if fit {
        // The square window the trace was framed in; the fit below shrinks it
        // further. A rebuild at the same view must not move it.
        app.y_range = (y_min, y_max);
    }

    // A page can do both: define a function and ask a question about it. The fit
    // below only looks at the *equation* curves: `y = x²` runs off the top of a
    // plot of the unit circle, and fitting to it would make the circle a dot.
    let equation_curves = app.curves.len();
    let definitions = plot_definitions(app, fit);

    if fit {
        // The intersections if there are any: a linear equation spans the whole
        // window, so a system's curves have no useful extent of their own.
        if app.intersections.is_empty() {
            fit_to_curves(app, equation_curves);
        } else {
            fit_to_intersections(app);
        }
    }

    // A free parameter has no number in the caption: the family is the value,
    // and the centre member is the value the drag shows. The pinned parameters
    // keep the number they were given.
    let parameters: Vec<String> = app
        .parameters
        .iter()
        .filter(|parameter| !parameter.free)
        .map(|parameter| {
            format!(
                "{} = {}",
                parameter.name.as_str(),
                short_number(parameter.value)
            )
        })
        .collect();
    let intersections = app.intersections.len();

    let mut parts: Vec<String> = Vec::new();
    if plotted > 0 || skipped > 0 {
        parts.push(match (plotted, skipped) {
            (0, s) => format!("no curve found in this window ({axes_label}), {s} skipped"),
            (n, 0) => format!("{n} equation curve(s), {axes_label}"),
            (n, s) => format!("{n} equation curve(s), {s} skipped, {axes_label}"),
        });
    }
    if !used_free.is_empty() {
        let names: Vec<String> = used_free
            .iter()
            .map(|index| app.parameters[*index].name.as_str().to_string())
            .collect();
        let names = names.join(", ");
        parts.push(format!(
            "{names} free: {FAMILY_MEMBERS} level curves of {names}"
        ));
    }
    if !unused_free.is_empty() {
        let names: Vec<String> = unused_free
            .iter()
            .map(|index| app.parameters[*index].name.as_str().to_string())
            .collect();
        let list = names.join(", ");
        let verb = if names.len() == 1 { "is" } else { "are" };
        let pronoun = if names.len() == 1 { "it" } else { "them" };
        parts.push(format!(
            "{list} {verb} free but no equation uses {pronoun}"
        ));
    }
    if intersections > 0 {
        let suffix = if intersections == 1 { "" } else { "s" };
        parts.push(format!("{intersections} intersection{suffix}"));
    }
    if surfaces > 0 {
        let names: Vec<String> = app
            .third_coordinates
            .iter()
            .map(|symbol| symbol.as_str().to_string())
            .collect();
        let verb = if names.len() == 1 {
            "is a third coordinate"
        } else {
            "are third coordinates"
        };
        parts.push(format!(
            "{} {verb}: that equation is a surface, and the 2D view draws two variables",
            names.join(", ")
        ));
    }
    if !parameters.is_empty() {
        parts.push(parameters.join(", "));
    }
    if let Some(fragment) = definitions.first() {
        parts.push(fragment.clone());
    }
    app.plot_note = parts.join("; ");
}

/// A number with its trailing zeros trimmed: `1`, `0.5`, `-2.25`.
fn short_number(value: f64) -> String {
    if !value.is_finite() {
        return String::from("0");
    }
    let text = format!("{value:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        String::from("0")
    } else {
        trimmed.to_string()
    }
}
/// Vertical display range for the plot.
///
/// Taking the true min/max is wrong for anything with a pole: `1/x` on
/// `[-10, 10]` spans ±741, which squashes the interesting part of the curve
/// into a flat line along the axis. Percentiles let the bulk of the samples
/// decide the view; the asymptote simply runs off the top and bottom, which is
/// what a graphing calculator does.
fn vertical_extent(curve: &Curve) -> Option<(f64, f64)> {
    let mut values: Vec<f64> = curve
        .segments
        .iter()
        .flatten()
        .map(|point| point[1])
        .filter(|y| y.is_finite())
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));

    let last = values.len() - 1;
    let low = values[last / 50];
    let high = values[last - last / 50];
    let (mut min, mut max) = if low < high {
        (low, high)
    } else {
        (values[0], values[last])
    };
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    let span = max - min;
    let pad = if span.abs() < 1e-9 {
        min -= 1.0;
        max += 1.0;
        0.0
    } else {
        span * 0.10
    };
    Some((min - pad, max + pad))
}

#[cfg(test)]
mod sign_table_tests {
    use super::*;

    fn app_with(document: &str) -> CasApp {
        let mut app = CasApp::new();
        app.field = canlab_input::MathField::from_latex(document).expect("a document");
        app.x_min = -1.0;
        app.x_max = 3.0;
        app.recompute();
        app
    }

    #[test]
    fn a_cubic_gets_the_table_the_cli_prints() {
        // `x³ − 3x² + 1` on `[−1, 3]`: increasing, decreasing, increasing, with
        // the critical points at 0 and 2. The same three pieces `canlab sign`
        // prints, so the panel and the CLI cannot disagree about the curve.
        let app = app_with("x^3 - 3*x^2 + 1");
        let directions: Vec<i8> = app.sign_table.iter().map(|piece| piece.direction).collect();
        assert_eq!(directions, vec![1, -1, 1], "{:?}", app.sign_table);
        assert!((app.sign_table[0].from - (-1.0)).abs() < 1e-9);
        assert!((app.sign_table[0].to - 0.0).abs() < 1e-6);
        assert!((app.sign_table[1].to - 2.0).abs() < 1e-6);
        assert!((app.sign_table[2].to - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_document_that_has_no_sign_table_gets_an_empty_one() {
        // An equation is not a function of one variable, so it has no sign
        // table — and the panel must not invent one.
        let app = app_with("x^2 + y^2 = 1");
        assert!(app.sign_table.is_empty(), "{:?}", app.sign_table);
        // A constant is one piece, and it is constant.
        let app = app_with("5");
        assert!(
            app.sign_table.iter().all(|piece| piece.direction == 0),
            "{:?}",
            app.sign_table
        );
    }

    #[test]
    fn the_table_follows_the_plot_window() {
        // The interval is the plot's own window, so a narrower window gives a
        // narrower table: the same rule `canlab sign --xmin --xmax` follows.
        let mut app = app_with("x^3 - 3*x^2 + 1");
        let wide = app.sign_table.len();
        app.x_min = 0.5;
        app.x_max = 1.5;
        app.recompute();
        assert!(
            app.sign_table.len() < wide,
            "narrowing the window did not narrow the table: {:?}",
            app.sign_table
        );
        assert!((app.sign_table[0].from - 0.5).abs() < 1e-9);
        assert!((app.sign_table.last().expect("a piece").to - 1.5).abs() < 1e-9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_to_no_equation_curves_leaves_the_window_alone() {
        // `y = x²` runs off a plot of the unit circle, so a definition curve
        // must not decide the window when no equation produced one.
        let mut app = crate::CasApp::new();
        app.x_min = -3.0;
        app.x_max = 3.0;
        app.y_range = (-2.0, 2.0);
        app.curves = vec![Curve {
            segments: vec![vec![[0.0, 0.0], [1.0, 1.0]]],
        }];
        fit_to_curves(&mut app, 0);
        assert_eq!((app.x_min, app.x_max), (-3.0, 3.0));
        assert_eq!(app.y_range, (-2.0, 2.0));
    }

    fn axes_of(app: &crate::CasApp) -> (Option<String>, Option<String>) {
        (
            app.x_axis.map(|v| v.as_str().to_string()),
            app.y_axis.map(|v| v.as_str().to_string()),
        )
    }

    #[test]
    fn choosing_the_other_axis_swaps_the_pair() {
        // x = q, y = p, then pick q for y: q moves to y and p moves to x. With
        // the old exclusion q was not even offered on y, so a swap was
        // impossible.
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("p^2+q^2=1").expect("a circle");
        app.recompute();
        let p = canlab_core::SymbolId::intern("p");
        let q = canlab_core::SymbolId::intern("q");
        app.x_axis = Some(q);
        app.y_axis = Some(p);

        choose_axis(&mut app, Axis::Vertical, q);
        assert_eq!(
            axes_of(&app),
            (Some(String::from("p")), Some(String::from("q")))
        );

        // And the same move in the other direction.
        choose_axis(&mut app, Axis::Horizontal, q);
        assert_eq!(
            axes_of(&app),
            (Some(String::from("q")), Some(String::from("p")))
        );
    }

    #[test]
    fn choosing_a_third_variable_leaves_the_other_axis_alone() {
        let mut app = crate::CasApp::new();
        app.field =
            canlab_input::MathField::from_latex("p^2+q^2+r^2=1").expect("three variables");
        app.recompute();
        let p = canlab_core::SymbolId::intern("p");
        let q = canlab_core::SymbolId::intern("q");
        let r = canlab_core::SymbolId::intern("r");
        app.x_axis = Some(p);
        app.y_axis = Some(q);

        choose_axis(&mut app, Axis::Vertical, r);
        assert_eq!(
            axes_of(&app),
            (Some(String::from("p")), Some(String::from("r")))
        );
    }

    #[test]
    fn a_vertical_pick_without_a_y_axis_keeps_x() {
        // One variable means the vertical axis is empty, so there is nothing to
        // swap with. Moving x across anyway left the plot without a horizontal
        // variable until the next reparse.
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("x=1").expect("one variable");
        app.recompute();
        let x = canlab_core::SymbolId::intern("x");
        assert_eq!(app.x_axis, Some(x));
        assert_eq!(app.y_axis, None);

        choose_axis(&mut app, Axis::Vertical, x);
        assert_eq!(app.x_axis, Some(x), "the horizontal axis was dropped");
        assert_eq!(app.y_axis, None, "there is no second variable to swap with");

        // The plot still has a horizontal variable to trace against.
        build_curve(&mut app);
        assert!(!app.curves.is_empty(), "plot note: {}", app.plot_note);
    }

    #[test]
    fn an_axis_choice_never_puts_one_variable_on_both_axes() {
        // The other axis's variable stays in the list so it can be swapped in;
        // the decision must move the replaced choice, never duplicate it.
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("p^2+q^2=1").expect("a circle");
        app.recompute();
        let p = canlab_core::SymbolId::intern("p");
        let q = canlab_core::SymbolId::intern("q");
        app.x_axis = Some(p);
        app.y_axis = Some(q);

        for variable in [p, q] {
            choose_axis(&mut app, Axis::Horizontal, variable);
            assert_ne!(app.x_axis, app.y_axis, "after choosing on x");
            choose_axis(&mut app, Axis::Vertical, variable);
            assert_ne!(app.x_axis, app.y_axis, "after choosing on y");
        }
    }

    #[test]
    fn crossings_far_from_the_origin_are_not_merged() {
        // The relative tolerance at 1e6 is about 1e-3; these two sit just
        // outside it, so the large-coordinate branch must not fall back to a
        // constant.
        let a = Intersection {
            x: 1.0e6,
            y: 1.0e6,
        };
        let b = Intersection {
            x: 1.0e6 + 1.0e-3,
            y: 1.0e6 + 1.0e-3,
        };
        assert!(!same_point(&a, &b));
    }

    #[test]
    fn crossings_within_the_relative_tolerance_are_one_point() {
        // At 0.5 the rule is 1e-9, so a 1e-12 shift is the same crossing.
        let a = Intersection { x: 0.5, y: 0.5 };
        let b = Intersection {
            x: 0.5 + 1.0e-12,
            y: 0.5,
        };
        assert!(same_point(&a, &b));
    }

    #[test]
    fn crossings_far_out_that_share_a_coordinate_are_one_point() {
        // Half a thousandth at 1e6 is inside the relative tolerance; the old
        // absolute 1e-6 kept this as a second point for the same crossing.
        let a = Intersection {
            x: 1.0e6,
            y: 1.0e6,
        };
        let b = Intersection {
            x: 1.0e6 + 5.0e-4,
            y: 1.0e6 + 5.0e-4,
        };
        assert!(same_point(&a, &b));
    }

    #[test]
    fn crossings_below_one_use_the_finer_relative_rule() {
        // At 0.5 the tolerance is 5e-10, so 1e-7 apart is two crossings. An
        // absolute 1e-6 would have merged them.
        let a = Intersection { x: 0.5, y: 0.5 };
        let b = Intersection {
            x: 0.5 + 1.0e-7,
            y: 0.5,
        };
        assert!(!same_point(&a, &b));
    }

    /// The app with a unit circle in it, fitted as a document change fits it.
    fn circle_app() -> crate::CasApp {
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("x^2+y^2=1").expect("a circle");
        app.recompute();
        app
    }

    /// The bounding box of every sampled point, in data coordinates.
    fn sample_bbox(app: &crate::CasApp) -> (f64, f64, f64, f64) {
        let mut low_x = f64::MAX;
        let mut high_x = f64::MIN;
        let mut low_y = f64::MAX;
        let mut high_y = f64::MIN;
        for curve in &app.curves {
            for point in curve.segments.iter().flatten() {
                low_x = low_x.min(point[0]);
                high_x = high_x.max(point[0]);
                low_y = low_y.min(point[1]);
                high_y = high_y.max(point[1]);
            }
        }
        assert!(
            low_x.is_finite() && high_x.is_finite() && low_y.is_finite() && high_y.is_finite(),
            "nothing was sampled"
        );
        (low_x, high_x, low_y, high_y)
    }

    /// One frame of just the plot panel in a fixed rectangle.
    fn plot_frame(
        app: &mut crate::CasApp,
        ctx: &egui::Context,
        plot_rect: Rect,
        events: Vec<egui::Event>,
    ) {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(plot_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            child.set_clip_rect(plot_rect);
            plot(app, &mut child);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_wider_canvas_widens_the_range_instead_of_stretching_the_axes() {
        let x = (-10.0, 10.0);
        let y = (-10.0, 10.0);

        // The old canvas was square, so a square window already filled it.
        let square = View::new(Some(vec2(400.0, 400.0)), x, y);
        assert_eq!(square, View { x, y });

        // 900 × 400 is 2.25:1, so x shows 45 units at the 20 px/unit the short
        // side sets; y is untouched.
        let wide = View::new(Some(vec2(900.0, 400.0)), x, y);
        assert!((wide.x.0 + 22.5).abs() < 1.0e-9 && (wide.x.1 - 22.5).abs() < 1.0e-9);
        assert!((wide.y.1 - wide.y.0 - 20.0).abs() < 1.0e-9);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 400.0));
        let unit_x = wide.to_px(rect, 1.0, 0.0).x - wide.to_px(rect, 0.0, 0.0).x;
        let unit_y = wide.to_px(rect, 0.0, 0.0).y - wide.to_px(rect, 0.0, 1.0).y;
        assert!(
            (unit_x - unit_y).abs() < 1.0e-3,
            "one scale: {unit_x} px/unit on x, {unit_y} on y"
        );

        // A tall canvas widens y instead.
        let tall = View::new(Some(vec2(400.0, 900.0)), x, y);
        assert!((tall.x.1 - tall.x.0 - 20.0).abs() < 1.0e-9);
        assert!((tall.y.1 - tall.y.0 - 45.0).abs() < 1.0e-9);

        // A canvas that is not a canvas, and a window that is not a window, are
        // shown unchanged rather than divided by zero.
        assert_eq!(View::new(Some(vec2(0.0, 0.0)), x, y), View { x, y });
        assert_eq!(
            View::new(Some(vec2(900.0, 400.0)), (1.0, 1.0), y),
            View {
                x: (1.0, 1.0),
                y
            }
        );

        // Before a canvas exists, the stored window is what is shown.
        assert_eq!(View::new(None, x, y), View { x, y });
    }

    #[test]
    fn at_the_old_square_size_the_window_and_samples_are_unchanged() {
        // The old plot canvas was a square fitted into the panel. At that shape
        // filling the panel is a no-op, so the window and the traced curve must
        // be exactly what they were.
        //
        // `y = x` rather than the circle: it runs corner to corner, so its
        // bounding box is the whole window and the fit leaves the stored range
        // alone — a comparison of like with like rather than one hidden refit.
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("y=x").expect("a diagonal line");
        app.plot_canvas = Some(vec2(400.0, 400.0));
        app.recompute();
        build_curve_fitting(&mut app, true);
        let ranges = ((app.x_min, app.x_max), app.y_range);
        let bbox = sample_bbox(&app);
        assert_eq!(ranges.0, (-10.0, 10.0), "x window: {ranges:?}");
        assert_eq!(ranges.1, (-10.0, 10.0), "y window: {ranges:?}");

        build_curve_fitting(&mut app, false);
        assert_eq!(
            ((app.x_min, app.x_max), app.y_range),
            ranges,
            "filling the panel moved the window"
        );
        assert_eq!(sample_bbox(&app), bbox, "filling resampled the same window differently");
    }

    #[test]
    fn a_wide_panel_shows_a_circle_with_equal_pixel_spans() {
        // The bug this guards: widening the panel must show more plane, not
        // stretch a unit of x away from a unit of y. A circle whose samples are
        // 500 px wide and 200 px tall is a wrong picture.
        let mut app = circle_app();
        let canvas = vec2(900.0, 400.0);
        app.plot_canvas = Some(canvas);
        build_curve_fitting(&mut app, false);

        let stored = (app.x_min, app.x_max);
        let view = View::new(Some(canvas), stored, app.y_range);
        assert!(
            view.x.1 - view.x.0 > stored.1 - stored.0 + 1.0,
            "the x range did not widen: {view:?}"
        );
        assert!(
            (view.y.1 - view.y.0 - (app.y_range.1 - app.y_range.0)).abs() < 1.0e-9,
            "y should keep its span: {view:?}"
        );

        let rect = Rect::from_min_size(pos2(0.0, 0.0), canvas);
        let (low_x, high_x, low_y, high_y) = sample_bbox(&app);
        let span_x = view.to_px(rect, high_x, 0.0).x - view.to_px(rect, low_x, 0.0).x;
        let span_y = view.to_px(rect, 0.0, low_y).y - view.to_px(rect, 0.0, high_y).y;
        assert!(
            (span_x - span_y).abs() <= 2.0,
            "a circle came out {span_x} px wide and {span_y} px tall"
        );
    }

    #[test]
    fn a_drag_from_the_middle_of_the_canvas_pans_and_is_not_swallowed_by_the_overlay() {
        let ctx = egui::Context::default();
        let mut app = circle_app();
        let plot_rect = Rect::from_min_size(pos2(40.0, 30.0), vec2(900.0, 400.0));
        plot_frame(&mut app, &ctx, plot_rect, Vec::new());

        let centre = plot_rect.center();
        let before = (app.x_min, app.x_max, app.y_range);
        let view = app.plot_view();
        let expected_dx = -60.0 * (view.x.1 - view.x.0) / plot_rect.width() as f64;

        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerMoved(centre)],
        );
        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerButton {
                pos: centre,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerMoved(centre + vec2(60.0, 0.0))],
        );

        assert!(
            (app.x_min - (before.0 + expected_dx)).abs() < 1.0e-6,
            "the overlay swallowed the drag: x went {} -> {}, expected {}",
            before.0,
            app.x_min,
            before.0 + expected_dx
        );
        assert_eq!(
            app.x_max - app.x_min,
            before.1 - before.0,
            "panning changed the zoom"
        );
        assert_eq!(app.y_range, before.2, "a horizontal drag moved y");
    }

    #[test]
    fn a_drag_on_the_overlay_background_still_pans() {
        // The strip has a background but no widget across it: only the controls
        // in it take the pointer. A drag that starts on the empty part of the
        // strip must pan the picture under it.
        let ctx = egui::Context::default();
        let mut app = circle_app();
        let plot_rect = Rect::from_min_size(pos2(40.0, 30.0), vec2(900.0, 400.0));
        plot_frame(&mut app, &ctx, plot_rect, Vec::new());

        // Between the overlay's outer edge and the frame's inner margin: strip,
        // not control.
        let start = plot_rect.min + vec2(10.0, 10.0);
        let before = app.x_min;
        let view = app.plot_view();
        let expected_dx = -40.0 * (view.x.1 - view.x.0) / plot_rect.width() as f64;

        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerMoved(start)],
        );
        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        plot_frame(
            &mut app,
            &ctx,
            plot_rect,
            vec![egui::Event::PointerMoved(start + vec2(40.0, 0.0))],
        );

        assert!(
            (app.x_min - (before + expected_dx)).abs() < 1.0e-6,
            "the strip background swallowed the drag: {before} -> {}, expected {}",
            app.x_min,
            before + expected_dx
        );
    }

    #[test]
    fn a_panel_too_small_for_the_controls_still_paints_its_canvas() {
        // `plot_overlay` refuses a rectangle smaller than its own margin. That
        // refusal must not take the canvas with it: the picture is still drawn,
        // and the panel is still recorded as the place the plot lives.
        let ctx = egui::Context::default();
        let mut app = circle_app();
        let plot_rect = Rect::from_min_size(pos2(5.0, 5.0), vec2(12.0, 12.0));
        plot_frame(&mut app, &ctx, plot_rect, Vec::new());
        assert_eq!(app.plot_canvas, Some(plot_rect.size()));
        assert!(!app.curves.is_empty(), "note: {}", app.plot_note);
    }

    /// The app with a unit circle and a free parameter, as the toggle makes it.
    fn free_circle_app() -> crate::CasApp {
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex("x^2+y^2=a").expect("a circle");
        app.recompute();
        app.parameters[0].free = true;
        build_curve_fitting(&mut app, false);
        app
    }

    #[test]
    fn a_free_parameter_draws_more_than_one_curve() {
        let app = free_circle_app();
        assert!(app.curves.len() > 1, "note: {}", app.plot_note);
        assert!(app.plot_note.contains("a free"), "note: {}", app.plot_note);
        assert!(
            app.plot_note.contains("5 level curves"),
            "note: {}",
            app.plot_note
        );
    }

    #[test]
    fn each_curve_of_the_family_sits_on_its_level() {
        // The five members are five circles, not five copies of one: each
        // curve's samples must lie on its own x² + y² = level. The level is the
        // mean of the samples, which is the value the tracer's own interpolation
        // gives; the spread is the tracer's error, not a second level.
        let app = free_circle_app();
        let mut levels: Vec<f64> = Vec::new();
        for curve in &app.curves {
            let mut sum = 0.0;
            let mut count = 0usize;
            for point in curve.segments.iter().flatten() {
                sum += point[0] * point[0] + point[1] * point[1];
                count += 1;
            }
            assert!(count > 0, "an empty curve");
            // The level is the mean of the samples, so a 1e-6 check on the
            // *level* is the precision the tracer's interpolation can give; the
            // samples themselves are only as close as the tracer's grid.
            let level = sum / count as f64;
            for point in curve.segments.iter().flatten() {
                let residual = (point[0] * point[0] + point[1] * point[1] - level).abs();
                assert!(
                    residual < 1.0e-4,
                    "point {point:?} is {residual} away from {level}"
                );
            }
            levels.push(level);
        }
        assert!(levels.len() > 1, "only one curve: {levels:?}");
        // The five levels are the five levels the family is meant to draw: the
        // centre is the parameter's value, and the others are half and three
        // halves of it. The level is the mean of the samples, so a 1e-6 check
        // on the *level* is the precision the tracer's interpolation can give.
        let mut expected = [0.5, 0.75, 1.0, 1.25, 1.5];
        expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        levels.sort_by(|a, b| a.total_cmp(b));
        for (drawn, wanted) in levels.iter().zip(expected) {
            assert!(
                (drawn - wanted).abs() < 1.0e-4,
                "the family drew {drawn} where the level {wanted} was asked for"
            );
        }
    }

    #[test]
    fn a_free_parameter_in_a_system_draws_no_intersections() {
        // The user's page: `x² + y² = a` and `x² + (y − 1)² = (a + b)²`, with
        // `a` free and `b` pinned. The family is a pair of circles at each
        // level; the crossings are not a finite set, so no marker is drawn.
        let mut app = crate::CasApp::new();
        app.field = canlab_input::MathField::from_latex_lines(&[
            String::from("x^2+y^2=a"),
            String::from("x^2+(y-1)^2=(a+b)^2"),
            String::from("b=0.01"),
        ])
        .expect("a page");
        app.recompute();
        let a = app
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name.as_str() == "a")
            .expect("a is a parameter");
        a.free = true;
        let b = app
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name.as_str() == "b")
            .expect("b is a parameter");
        b.free = false;
        build_curve_fitting(&mut app, false);
        assert!(app.intersections.is_empty(), "note: {}", app.plot_note);
        assert!(
            app.curves.len() > 2,
            "the family is more than one curve per equation: {}",
            app.plot_note
        );
        assert!(
            app.plot_note.contains("a free"),
            "note: {}",
            app.plot_note
        );
    }

    #[test]
    fn a_family_marker_is_full_strength_while_its_curves_are_not() {
        // The user's correction: the curves are translucent so overlaps read as
        // overlaps; the marker is the answer, so it stays at full strength.
        let marker = marker_color();
        assert_eq!(marker.a(), 255, "the marker is the answer");
        let centre = family_curve_color(0, true);
        assert_eq!(centre.a(), 255, "the centre member is full strength");
        let side = family_curve_color(0, false);
        assert!(side.a() < 255, "a side member is translucent");
        assert!(side.a() >= 128, "the side member is still readable");
    }

    #[test]
    fn every_palette_key_either_lands_or_says_why_not() {
        // The four entries the model deliberately lacks. Everything else — all
        // ~100 keys, symbols and structures alike — has to put something real in
        // the field.
        const NOT_MODELLED: &[&str] = &["cases", "matrix", "array"];

        for &(category, buttons) in canlab_parse::palette::ALL {
            for button in buttons {
                let mut field = canlab_input::MathField::new();
                let applied = field.apply_snippet(button.insert);
                if NOT_MODELLED.contains(&button.label) {
                    assert!(
                        applied.is_err(),
                        "{category}/`{}` should be refused, got {:?}",
                        button.label,
                        field.to_latex()
                    );
                    continue;
                }
                assert!(
                    applied.is_ok(),
                    "{category}/`{}` failed: {applied:?}",
                    button.label
                );
                assert!(
                    !field.to_latex().is_empty(),
                    "{category}/`{}` inserted nothing",
                    button.label
                );
            }
        }
    }

    #[test]
    fn a_palette_fraction_is_a_fraction_and_not_the_text_frac() {
        let mut field = canlab_input::MathField::new();
        field.apply_snippet(canlab_parse::palette::find("a/b").unwrap().insert).unwrap();
        assert_eq!(field.to_latex(), "\\frac{ }{ }");
        // The cursor is in the numerator, exactly where Abitti puts it.
        assert_eq!(field.cursor().path.len(), 1);
    }
}

