//! The contents of the five dock panels.
//!
//! Every function here receives the app plus a UI already clipped to the
//! panel's body. Keeping the dock chrome in `layout.rs` means these functions
//! only have to answer one question each: what does this panel show?

use canlab_mathview::{MathView, Style};
use canlab_plot::implicit::{ImplicitOptions, sample_implicit};
use canlab_plot::{Curve, SampleOptions, sample};
use canlab_tex::to_latex;
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Label, Layout, Rect, RichText,
    Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::theme::ACCENT;
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
                app.field.build_equations().map(|e| e.len()).unwrap_or(0)
            )
        } else {
            String::from("Document")
        };
        ui.label(RichText::new(title).small().strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (text, color) = if app.error.is_some() {
                ("parse error", ui.visuals().error_fg_color)
            } else if app.field.is_incomplete() {
                ("incomplete", ui.visuals().weak_text_color())
            } else {
                ("live parse", ui.visuals().weak_text_color())
            };
            ui.label(RichText::new(text).small().color(color));
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
                .color(ui.visuals().weak_text_color()),
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
                .color(ui.visuals().weak_text_color()),
            )
            .selectable(true)
            .wrap(),
        );
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

/// The structural field: a painted background, the maths itself, and the focus
/// target that the keyboard handler listens to.
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
    let inner = rect.shrink2(vec2(FIELD_PADDING_X, FIELD_PADDING_Y));
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

/// Route the frame's key events into the field.
///
/// `MathView` deliberately owns clicks only. `Event::Text` is the key here:
/// `type_str` already implements `/`, `*`, `^`, `_`, `\name` commands and the
/// Greek names, so this function must not special-case printable keys. Enter is
/// swallowed rather than committed because a dock panel has nowhere to put a
/// new line.
///
/// The events are **taken out of the queue**, not merely read: egui also feeds
/// them to its focus system, so leaving an arrow key in place moved focus to the
/// next widget while the caret was walking the expression. Ctrl/⌘ combinations
/// and `Tab`/`Esc` are left alone on purpose — undo and focus traversal belong to
/// the shell.
fn handle_field_keys(app: &mut CasApp, ui: &mut Ui, response: &egui::Response) {
    if !response.has_focus() {
        return;
    }

    let events = ui.input_mut(|input| {
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
                    // `Ctrl-A` belongs to the field while it has focus; every
                    // other command combination is left to the shell.
                    (plain && editing)
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
                egui::Key::Backspace => {
                    app.field.backspace();
                    changed = true;
                }
                egui::Key::Delete => {
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
                // Enter means three things, in this order:
                // 1. a half-typed `\name` is committed (`\bar` + Enter is the
                //    accent, not a new line);
                // 2. Ctrl+Enter starts a new, separate system;
                // 3. otherwise the next equation of this system.
                egui::Key::Enter => {
                    if app.field.commit_command() {
                        // The keystroke was consumed by the command.
                    } else if modifiers.command {
                        app.field.new_system();
                    } else {
                        app.field.new_line();
                    }
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
        let equation = !app.equations.is_empty();
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

/// The plot's own controls, in the plot panel: the button, which variables go on
/// which axis, and the window it draws into. They belong together.
fn plot_row(app: &mut CasApp, ui: &mut Ui) {
    let mut axes_changed = false;
    ui.horizontal_wrapped(|ui| {
        // No Plot button: the picture follows the document. What is worth
        // showing here is what it is showing, and why.
        axes_changed |= axis_picker(app, ui, "x", Axis::Horizontal);
        axes_changed |= axis_picker(app, ui, "y", Axis::Vertical);

        ui.separator();
        ui.label(RichText::new("x from").small());
        let mut range_changed = ui
            .add(egui::DragValue::new(&mut app.x_min).speed(0.5))
            .changed();
        ui.label(RichText::new("to").small());
        range_changed |= ui
            .add(egui::DragValue::new(&mut app.x_max).speed(0.5))
            .changed();

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(app.plot_note.as_str())
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        });

        if axes_changed || range_changed {
            app.plot();
        }
    });
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

/// One "which variable is this axis?" selector.
///
/// Any variable in the document can go on either axis, which is what makes
/// `i² + j² = 1` plottable: nothing about the maths cares what the letters are
/// called, only which one is horizontal.
fn axis_picker(app: &mut CasApp, ui: &mut Ui, label: &str, axis: Axis) -> bool {
    let current = match axis {
        Axis::Horizontal => app.x_axis,
        Axis::Vertical => app.y_axis,
    };
    let mut changed = false;
    ui.label(RichText::new(label).small().strong());
    let selected = current.map_or_else(|| String::from("—"), |v| v.as_str().to_string());
    egui::ComboBox::from_id_salt(("axis", label))
        .selected_text(RichText::new(selected).monospace().size(12.0))
        .width(52.0)
        .show_ui(ui, |ui| {
            for variable in app.free_vars.clone() {
                let is_current = current == Some(variable);
                if ui
                    .selectable_label(is_current, variable.as_str().to_string())
                    .clicked()
                {
                    match axis {
                        Axis::Horizontal => app.x_axis = Some(variable),
                        Axis::Vertical => app.y_axis = Some(variable),
                    }
                    changed = true;
                }
            }
        });
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
                    .color(ui.visuals().weak_text_color()),
            );
        });
    });
    ui.add_space(4.0);
    if app.result_latex.is_empty() {
        ui.label(
            RichText::new("Type an expression above; it is parsed as you type.")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    } else {
        result_body(app, ui);
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
    let weak = ui.visuals().weak_text_color();
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
            .color(ui.visuals().weak_text_color()),
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
                        .color(ui.visuals().weak_text_color()),
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
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("SOLVER")
                .small()
                .strong()
                .color(ui.visuals().weak_text_color()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("native · linear & quadratic")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        });
    });
    ui.add_space(4.0);
    solver_row(app, ui);
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);

    ui.label(
        RichText::new("FREE VARIABLES")
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(4.0);
    if app.free_vars.is_empty() {
        ui.label(
            RichText::new("No free variables.")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    } else {
        for var in &app.free_vars {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 2.5, ACCENT);
                ui.label(RichText::new(var.as_str()).monospace().size(15.0).strong());
            });
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        RichText::new("SOLUTIONS")
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(4.0);
    match &app.solve {
        SolveState::Idle => {
            ui.label(
                RichText::new("Run Solve to see values here.")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        SolveState::Declined { label, why } => {
            ui.label(
                RichText::new(label.as_str())
                    .small()
                    .color(ui.visuals().weak_text_color()),
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
                    .color(ui.visuals().weak_text_color()),
            );
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
                    .color(ui.visuals().weak_text_color()),
            );
        }
        SolveState::Solved { var, values } => {
            if values.is_empty() {
                ui.label(
                    RichText::new("No solutions.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            } else {
                let name = to_latex(&canlab_core::sym(&var.as_str()));
                for value in values {
                    ui.label(
                        RichText::new(format!("{name} = {}", to_latex(value)))
                            .monospace()
                            .size(15.0),
                    );
                }
            }
        }
    }
}

pub fn plot(app: &mut CasApp, ui: &mut Ui) {
    plot_row(app, ui);
    ui.add_space(6.0);

    let size = ui.available_size();
    let width = size.x.max(80.0);
    let height = size.y.max(90.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();

    // No frame around the plot either: the axes and the grid are the frame.
    if app.curves.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Nothing to plot yet.",
            FontId::proportional(13.0),
            visuals.weak_text_color(),
        );
        return;
    }

    // Axes once, then every curve on top of them in its own colour, so a system
    // of two equations reads as two lines rather than one thick one.
    paint_axes(&painter, rect, (app.x_min, app.x_max), app.y_range);
    for (index, curve) in app.curves.iter().enumerate() {
        paint_curve(
            &painter,
            rect,
            curve,
            (app.x_min, app.x_max),
            app.y_range,
            curve_color(index),
        );
    }
}

/// A distinct colour per curve: the accent first, then a small wheel.
fn curve_color(index: usize) -> Color32 {
    const WHEEL: [Color32; 6] = [
        ACCENT,
        Color32::from_rgb(0x62, 0xB0, 0xF5),
        Color32::from_rgb(0x8B, 0xC3, 0x4A),
        Color32::from_rgb(0xE5, 0x7A, 0xC8),
        Color32::from_rgb(0xF2, 0xC9, 0x4C),
        Color32::from_rgb(0x7A, 0xE0, 0xD4),
    ];
    WHEEL[index % WHEEL.len()]
}

fn paint_axes(painter: &egui::Painter, rect: Rect, x_range: (f64, f64), y_range: (f64, f64)) {
    let visuals = painter.ctx().global_style().visuals.clone();
    let axis_color = visuals.widgets.noninteractive.bg_stroke.color;
    let label_color = visuals.weak_text_color();
    let (x0, x1) = x_range;
    let (y0, y1) = y_range;
    if !(x1 > x0 && y1 > y0) {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "x range is empty",
            FontId::proportional(13.0),
            visuals.warn_fg_color,
        );
        return;
    }

    let to_px = |x: f64, y: f64| {
        pos2(
            rect.left() + ((x - x0) / (x1 - x0)) as f32 * rect.width(),
            rect.bottom() - ((y - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };

    // A subtle grid: four vertical and four horizontal divisions.
    let grid = Stroke::new(1.0, axis_color.gamma_multiply(0.35));
    for step in 1..4 {
        let t = step as f64 / 4.0;
        let x = rect.left() + rect.width() * t as f32;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], grid);
        let y = rect.top() + rect.height() * t as f32;
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], grid);
    }

    let axis = Stroke::new(1.0, axis_color);
    if x0 <= 0.0 && x1 >= 0.0 {
        let x = to_px(0.0, y0).x;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], axis);
    }
    if y0 <= 0.0 && y1 >= 0.0 {
        let y = to_px(x0, 0.0).y;
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], axis);
    }

    painter.text(
        pos2(rect.left() + 8.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
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
    x_range: (f64, f64),
    y_range: (f64, f64),
    color: Color32,
) {
    let (x0, x1) = x_range;
    let (y0, y1) = y_range;
    if !(x1 > x0 && y1 > y0) {
        return;
    }
    let to_px = |x: f64, y: f64| {
        pos2(
            rect.left() + ((x - x0) / (x1 - x0)) as f32 * rect.width(),
            rect.bottom() - ((y - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };
    let stroke = Stroke::new(1.8, color);
    for segment in &curve.segments {
        let points: Vec<egui::Pos2> = segment
            .iter()
            .filter(|point| point[0].is_finite() && point[1].is_finite())
            .map(|point| to_px(point[0], point[1]))
            .collect();
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

/// Display-only: `canlab-input` has no LaTeX-to-tree parser, so a row cannot be loaded back into the field.
pub fn history(app: &CasApp, ui: &mut Ui) {
    if app.history.is_empty() {
        ui.label(
            RichText::new("No results yet. Simplify, integrate or solve something.")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        return;
    }

    ui.label(
        RichText::new("NEWEST FIRST")
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("history-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &app.history {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(entry.label.as_str())
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add(
                    Label::new(RichText::new(entry.latex.as_str()).monospace().size(14.0))
                        .wrap()
                        .selectable(true),
                );
                ui.add_space(4.0);
            }
        });
}

/// Sample the document and keep the curves for the Plot panel.
///
/// Two kinds of picture come out of the same button:
///
/// * a *function* `y = f(x)` — the field holds one expression, so it is sampled
///   along x;
/// * an *equation* — a line like `x + y = 3` is not a function of x, so it is
///   traced as the set of points where `left − right = 0`, which is what
///   students mean by plotting an equation. A system gives one curve per line,
///   and several systems give all of their lines.
pub fn build_curve(app: &mut CasApp) {
    app.curves.clear();
    if !app.x_max.is_finite() || !app.x_min.is_finite() || app.x_max <= app.x_min {
        app.plot_note = String::from("x range is empty");
        return;
    }

    let equations = app.field.build_equations();
    if let Ok(equations) = &equations
        && equations.iter().any(|(_, right)| right.is_some())
    {
        plot_equations(app, equations);
        return;
    }

    let Some(expr) = app.expr.clone() else {
        app.plot_note = String::from("nothing to plot");
        return;
    };
    if app.free_vars.len() != 1 {
        app.plot_note = format!("plot needs one variable, found {}", app.free_vars.len());
        return;
    }
    let Some(program) = canlab_core::Program::compile(&expr, &app.free_vars) else {
        app.plot_note = String::from("cannot evaluate numerically");
        return;
    };
    let curve = sample(
        &program,
        &SampleOptions {
            x_min: app.x_min,
            x_max: app.x_max,
            ..SampleOptions::default()
        },
    );
    app.y_range = vertical_extent(&curve).unwrap_or((-1.0, 1.0));
    let count = curve.segments.len();
    let suffix = if count == 1 { "" } else { "s" };
    app.plot_note = format!("{count} segment{suffix}");
    app.curves.push(curve);
}

/// Trace every equation in the document as an implicit curve.
///
/// The two axes come from the document's variable list, so the letters are
/// irrelevant: `i² + j² = 1` with `i` on x and `j` on y plots exactly the same
/// circle as `x² + y² = 1` does. An equation with a third free variable cannot be
/// drawn on two axes, so it is skipped rather than guessed at.
fn plot_equations(app: &mut CasApp, equations: &[(canlab_core::Expr, Option<canlab_core::Expr>)]) {
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

    // An equation has no preferred y range, so the window is square: the two
    // axes get the same span, which is also what makes a circle look round.
    let span = (app.x_max - app.x_min).abs().max(1.0);
    let y_min = -span / 2.0;
    let y_max = span / 2.0;
    let options = ImplicitOptions {
        resolution: 240,
        x_min: app.x_min,
        x_max: app.x_max,
        y_min,
        y_max,
    };

    let mut plotted = 0;
    let mut skipped = 0;
    for (left, right) in equations {
        let expression = match right {
            // `left − right`, so the curve is where the equation holds.
            Some(right) => canlab_core::add(vec![
                left.clone(),
                canlab_core::mul(vec![canlab_core::int(-1), right.clone()]),
            ]),
            None => left.clone(),
        };
        let Some(program) = canlab_core::Program::compile(&expression, &variables) else {
            skipped += 1;
            continue;
        };
        let curve = if variables.len() == 2 {
            sample_implicit(|x, y| program.eval(&[x, y]), &options)
        } else {
            // One variable: the zero set is a set of vertical lines.
            sample_implicit(|x, _| program.eval(&[x]), &options)
        };
        if curve.segments.is_empty() {
            skipped += 1;
        } else {
            plotted += 1;
            app.curves.push(curve);
        }
    }

    app.y_range = (y_min, y_max);
    app.plot_note = match (plotted, skipped) {
        (0, _) => format!("no curve found in this window ({axes_label})"),
        (n, 0) => format!("{n} equation curve(s), {axes_label}"),
        (n, s) => format!("{n} equation curve(s), {s} skipped, {axes_label}"),
    };
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
mod tests {
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
