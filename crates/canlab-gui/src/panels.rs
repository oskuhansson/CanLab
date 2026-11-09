//! The contents of the five dock panels.
//!
//! Every function here receives the app plus a UI already clipped to the
//! panel's body. Keeping the dock chrome in `layout.rs` means these functions
//! only have to answer one question each: what does this panel show?

use canlab_input::Template;
use canlab_mathview::{MathView, Style};
use canlab_plot::{Curve, SampleOptions, sample};
use canlab_tex::to_latex;
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Label, Layout, Margin, Rect, RichText,
    Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::theme::ACCENT;
use crate::{CasApp, SolveState};

/// Width of the editor's reading column. Wide enough for a display equation,
/// narrow enough that the eye does not lose the line.
const EDITOR_WIDTH: f32 = 880.0;

/// Size of the mathematics in the hero field, and the caret/padding around it.
const FIELD_BASE_SIZE: f32 = 30.0;
/// The field is the centre of the app, so it gets more than one text line even
/// when the expression is short.
const FIELD_MIN_HEIGHT: f32 = FIELD_BASE_SIZE * 2.5;
const FIELD_PADDING_X: f32 = 14.0;
const FIELD_PADDING_Y: f32 = 10.0;

pub fn editor(app: &mut CasApp, ui: &mut Ui) {
    let viewport_height = ui.available_height();
    egui::ScrollArea::vertical()
        .id_salt("editor-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_width().max(120.0);
            let width = available.min(EDITOR_WIDTH);
            let side = ((available - width) * 0.5).max(0.0);
            // The result card height varies with the expression, so this is
            // an estimate rather than a measurement. It is close enough at
            // normal window sizes, and the scroll area takes over when the
            // content outgrows the panel.
            let top = ((viewport_height - 340.0) * 0.5).clamp(8.0, 400.0);
            ui.add_space(top);
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
        ui.label(RichText::new("Expression").small().strong());
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

    ui.add_space(9.0);
    action_row(app, ui);

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

    let fill = ui.visuals().text_edit_bg_color();
    let border = ui.visuals().widgets.inactive.bg_stroke.color;
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    let inner = rect.shrink2(vec2(FIELD_PADDING_X, FIELD_PADDING_Y));
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("canlab-math-field")
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(inner);
    let math = MathView::show(&mut child, &mut app.field, &style);

    // Focus `MathView`'s own widget, not the decorative rectangle: it is the
    // focusable widget, so Tab and clicks agree about where the caret lives.
    if response.clicked() || math.clicked() {
        math.request_focus();
    }
    if math.has_focus() {
        ui.painter().rect_stroke(
            rect,
            8.0,
            Stroke::new(1.5, ACCENT),
            egui::StrokeKind::Inside,
        );
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
                    !modifiers.ctrl
                        && !modifiers.command
                        && matches!(
                            key,
                            egui::Key::Backspace
                                | egui::Key::Delete
                                | egui::Key::ArrowLeft
                                | egui::Key::ArrowRight
                                | egui::Key::ArrowUp
                                | egui::Key::ArrowDown
                                | egui::Key::Enter
                        )
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
                key, pressed: true, ..
            } => match key {
                egui::Key::Backspace => {
                    app.field.backspace();
                    changed = true;
                }
                egui::Key::Delete => {
                    app.field.delete_forward();
                    changed = true;
                }
                egui::Key::ArrowLeft => changed |= app.field.move_left(),
                egui::Key::ArrowRight => changed |= app.field.move_right(),
                egui::Key::ArrowUp => changed |= app.field.move_up(),
                egui::Key::ArrowDown => changed |= app.field.move_down(),
                // Enter starts the next equation of the system.
                egui::Key::Enter => {
                    app.field.new_line();
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

fn action_row(app: &mut CasApp, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let enabled = app.expr.is_some();
        let size = vec2(92.0, 30.0);
        if ui
            .add_enabled(enabled, Button::new("Simplify").min_size(size))
            .clicked()
        {
            app.start_simplify();
        }
        if ui
            .add_enabled(enabled, Button::new("Integrate").min_size(size))
            .clicked()
        {
            app.start_integrate();
        }
        if ui
            .add_enabled(enabled, Button::new("Solve").min_size(size))
            .clicked()
        {
            app.start_solve();
        }
        let plot = Button::new(RichText::new("Plot").color(Color32::WHITE))
            .fill(ACCENT)
            .min_size(size);
        if ui.add_enabled(enabled, plot).clicked() {
            app.plot();
        }

        ui.separator();
        ui.label(RichText::new("x from").small());
        let mut range_changed = ui
            .add(egui::DragValue::new(&mut app.x_min).speed(0.5))
            .changed();
        ui.label(RichText::new("to").small());
        range_changed |= ui
            .add(egui::DragValue::new(&mut app.x_max).speed(0.5))
            .changed();
        if range_changed && app.curve.is_some() {
            app.plot();
        }

        if app.jobs.in_flight() > 0 {
            ui.spinner();
        }
    });
}

fn result_card(app: &CasApp, ui: &mut Ui) {
    let visuals = ui.visuals();
    Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(8)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
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
            ui.add_space(6.0);
            if app.result_latex.is_empty() {
                ui.label(
                    RichText::new("Type an expression above; it is parsed as you type.")
                        .color(ui.visuals().weak_text_color()),
                );
            } else {
                ui.label(
                    RichText::new(app.result_latex.as_str())
                        .monospace()
                        .size(21.0),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(app.result_plain.as_str())
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
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
        SolveState::Declined => {
            ui.label(
                RichText::new("no solver backend")
                    .small()
                    .strong()
                    .color(ui.visuals().warn_fg_color),
            );
            ui.label(
                RichText::new("The backend declined this request, so no values are shown.")
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

pub fn plot(app: &CasApp, ui: &mut Ui) {
    let size = ui.available_size();
    let width = size.x.max(80.0);
    let height = size.y.max(90.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 6.0, visuals.extreme_bg_color);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );

    let Some(curve) = &app.curve else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No curve yet — press Plot in the editor.",
            FontId::proportional(13.0),
            visuals.weak_text_color(),
        );
        return;
    };

    paint_curve(&painter, rect, curve, (app.x_min, app.x_max), app.y_range);
}

fn paint_curve(
    painter: &egui::Painter,
    rect: Rect,
    curve: &Curve,
    x_range: (f64, f64),
    y_range: (f64, f64),
) {
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

    let stroke = Stroke::new(1.8, ACCENT);
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

    painter.text(
        pos2(rect.left() + 8.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        format!("x [{x0:.3}, {x1:.3}]   y [{y0:.3}, {y1:.3}]"),
        FontId::monospace(10.0),
        label_color,
    );
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

/// Sample the current expression and keep the curve for the Plot panel.
pub fn build_curve(app: &mut CasApp) {
    app.curve = None;
    let Some(expr) = app.expr.clone() else {
        app.status = String::from("nothing to plot");
        return;
    };
    if app.free_vars.len() != 1 {
        app.status = format!("plot needs one variable, found {}", app.free_vars.len());
        return;
    }
    if !app.x_max.is_finite() || !app.x_min.is_finite() || app.x_max <= app.x_min {
        app.status = String::from("x range is empty");
        return;
    }
    let Some(program) = canlab_core::Program::compile(&expr, &app.free_vars) else {
        app.status = String::from("cannot evaluate numerically");
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
    app.status = format!("{count} segment{suffix}");
    app.curve = Some(curve);
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
    use super::*;

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
