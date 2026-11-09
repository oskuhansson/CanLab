//! `cas-gui` — an Abitti-style window over the CAS.
//!
//! The layout mirrors the editor Finnish students meet in the matriculation
//! examination: a palette of formula keys, a LaTeX field, the result below.
//!
//! Three deliberate choices:
//!
//! 1. **The palette is data, not code** (`cas_parse::palette`, transcribed from
//!    Abitti's own tables). Adding a key is a data change.
//! 2. **The plot is painted from `cas_plot::Curve`** — the same segments the SVG
//!    exporter uses. One sampler, two renderers, so the window and the exported
//!    file cannot disagree about where a pole is.
//! 3. **Parsing is live**: every edit re-parses, and errors are shown rather
//!    than swallowed.

use cas_core::{Expr, ExprNode, Hold, Program, SymbolId, expr::node};
use cas_parse::palette;
use cas_parse::parse_latex;
use cas_plot::{Curve, SampleOptions, sample};
use cas_solver::{MockSolver, Solver};
use cas_tex::to_latex;
use eframe::egui;
use egui::text::{CCursor, CCursorRange};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 560.0])
            .with_maximized(true)
            .with_title("cas — LaTeX CAS"),
        ..Default::default()
    };
    eframe::run_native(
        "cas",
        options,
        Box::new(|_cc| Ok(Box::new(CasApp::default()) as Box<dyn eframe::App>)),
    )
}

struct CasApp {
    input: String,
    /// Character index into `input` where palette keys insert.
    cursor: Option<usize>,
    /// Set by [`CasApp::insert`]: where the text cursor should be put once the
    /// editor has been shown this frame.
    pending_cursor: Option<usize>,
    x_min: f64,
    x_max: f64,
    expr: Option<Expr>,
    result_latex: String,
    result_plain: String,
    error: Option<String>,
    curve: Option<Curve>,
    y_range: (f64, f64),
    status: String,
}

impl Default for CasApp {
    fn default() -> Self {
        let mut app = CasApp {
            input: String::from(r"\frac{x^{2} + 1}{x}"),
            cursor: None,
            pending_cursor: None,
            x_min: -10.0,
            x_max: 10.0,
            expr: None,
            result_latex: String::new(),
            result_plain: String::new(),
            error: None,
            curve: None,
            y_range: (-1.0, 1.0),
            status: String::from("ready"),
        };
        app.recompute();
        app.plot();
        app
    }
}

impl CasApp {
    fn insert(&mut self, text: &str) {
        let char_index = self.cursor.unwrap_or_else(|| self.input.chars().count());
        let byte_index = self
            .input
            .char_indices()
            .nth(char_index)
            .map_or(self.input.len(), |(i, _)| i);
        self.input.insert_str(byte_index, text);
        let caret = char_index + text.chars().count();
        self.cursor = Some(caret);
        self.pending_cursor = Some(caret);
    }

    /// Re-parse the input and refresh the displayed forms.
    fn recompute(&mut self) {
        self.error = None;
        self.curve = None;
        match parse_latex(&self.input) {
            Ok(expr) => {
                self.result_plain = node(&expr).to_string();
                self.result_latex = to_latex(&expr);
                self.expr = Some(expr);
            }
            Err(err) => {
                self.error = Some(err.to_string());
                self.result_plain.clear();
                self.result_latex.clear();
                self.expr = None;
            }
        }
    }

    fn plot(&mut self) {
        self.curve = None;
        let Some(expr) = self.expr.clone() else {
            self.status = "nothing to plot".into();
            return;
        };
        let mut vars = Vec::new();
        free_symbols(&expr, &mut vars);
        if vars.len() != 1 {
            self.status = format!("plot needs one variable, found {}", vars.len());
            return;
        }
        let Some(program) = Program::compile(&expr, &vars) else {
            self.status = "cannot evaluate numerically".into();
            return;
        };
        let curve = sample(
            &program,
            &SampleOptions {
                x_min: self.x_min,
                x_max: self.x_max,
                ..SampleOptions::default()
            },
        );
        self.y_range = vertical_extent(&curve).unwrap_or((-1.0, 1.0));
        let count = curve.segments.len();
        let suffix = if count == 1 { "" } else { "s" };
        self.status = format!("{count} segment{suffix}");
        self.curve = Some(curve);
    }

    fn integrate(&mut self) {
        let Some(expr) = self.expr.clone() else {
            self.status = "nothing to integrate".into();
            return;
        };
        let mut vars = Vec::new();
        free_symbols(&expr, &mut vars);
        let Some(var) = vars.first().copied() else {
            self.status = "no variable to integrate".into();
            return;
        };
        match MockSolver.integrate(&expr, var) {
            Ok(result) => {
                self.result_latex = to_latex(&result);
                self.result_plain = node(&result).to_string();
                self.status = "integrated".into();
            }
            Err(err) => self.status = format!("solver declined: {err}"),
        }
    }
}

impl eframe::App for CasApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("title").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("cas");
                ui.label("LaTeX-native algebra · Abitti vocabulary");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(self.status.as_str());
                });
            });
            ui.separator();
        });

        egui::Panel::left("palette")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for &(category, buttons) in palette::ALL {
                        ui.label(
                            egui::RichText::new(category.to_uppercase())
                                .small()
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            for button in buttons {
                                if ui
                                    .add(
                                        egui::Button::new(button.label)
                                            .min_size(egui::vec2(34.0, 30.0)),
                                    )
                                    .on_hover_text(button.insert)
                                    .clicked()
                                {
                                    self.insert(button.insert);
                                    self.recompute();
                                }
                            }
                        });
                        ui.add_space(10.0);
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("LaTeX").small().strong());

            let mut output = egui::TextEdit::multiline(&mut self.input)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .code_editor()
                .show(ui);
            if self.pending_cursor.is_none()
                && let Some(range) = output.cursor_range
            {
                self.cursor = Some(range.primary.index.0);
            }
            if let Some(index) = self.pending_cursor.take() {
                let id = output.response.id;
                output
                    .state
                    .cursor
                    .set_char_range(Some(CCursorRange::one(CCursor::new(index))));
                output.state.store(ui.ctx(), id);
                output.response.request_focus();
            }
            if output.response.changed() {
                self.recompute();
                self.curve = None;
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Simplify").clicked() {
                    self.recompute();
                }
                if ui.button("Integrate").clicked() {
                    self.recompute();
                    self.integrate();
                }
                if ui.button("Plot").clicked() {
                    self.recompute();
                    self.plot();
                }
                ui.separator();
                ui.label("x from");
                ui.add(egui::DragValue::new(&mut self.x_min).speed(0.5));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut self.x_max).speed(0.5));
            });

            if let Some(err) = &self.error {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(180, 40, 40), err.as_str());
            }

            if !self.result_latex.is_empty() {
                ui.add_space(8.0);
                ui.group(|ui| {
                    ui.label(egui::RichText::new("result").small().strong());
                    ui.label(
                        egui::RichText::new(self.result_latex.as_str())
                            .monospace()
                            .size(16.0),
                    );
                    ui.label(
                        egui::RichText::new(self.result_plain.as_str())
                            .small()
                            .color(egui::Color32::from_gray(110)),
                    );
                });
            }

            if let Some(curve) = &self.curve {
                ui.add_space(8.0);
                let height = (ui.available_height() - 8.0).max(160.0);
                // `available_width` can exceed the visible panel when a wide
                // monospace label has expanded the layout; clamping to the clip
                // rect keeps the plot — and therefore its x-mapping — inside the
                // area the user can actually see.
                let width = ui
                    .clip_rect()
                    .width()
                    .min(ui.available_width())
                    .max(120.0);
                let (rect, _response) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                paint_plot(ui, rect, curve, (self.x_min, self.x_max), self.y_range);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Plot painting
// ---------------------------------------------------------------------------

fn paint_plot(
    ui: &egui::Ui,
    rect: egui::Rect,
    curve: &Curve,
    x_range: (f64, f64),
    y_range: (f64, f64),
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::WHITE);
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(200)),
        egui::StrokeKind::Inside,
    );

    let (x0, x1) = x_range;
    let (y0, y1) = y_range;
    let usable = x1 > x0 && y1 > y0;
    if !usable {
        return;
    }

    let to_px = |x: f64, y: f64| {
        egui::pos2(
            rect.left() + ((x - x0) / (x1 - x0)) as f32 * rect.width(),
            rect.bottom() - ((y - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };

    let axis = egui::Stroke::new(1.0, egui::Color32::from_gray(190));
    if x0 <= 0.0 && x1 >= 0.0 {
        let x = to_px(0.0, y0).x;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            axis,
        );
    }
    if y0 <= 0.0 && y1 >= 0.0 {
        let y = to_px(x0, 0.0).y;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            axis,
        );
    }

    let stroke = egui::Stroke::new(1.6, egui::Color32::from_rgb(30, 136, 229));
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
        egui::pos2(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        format!("[{x0:.3}, {x1:.3}] x [{y0:.3}, {y1:.3}]"),
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(140),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn free_symbols(expr: &Expr, out: &mut Vec<SymbolId>) {
    match &**expr {
        ExprNode::Num(_) => {}
        ExprNode::Sym(s) => {
            if !matches!(s.as_str().as_str(), "pi" | "e" | "tau") && !out.contains(s) {
                out.push(*s);
            }
        }
        ExprNode::Add(items) | ExprNode::Mul(items) | ExprNode::Fn(_, items) => {
            for item in items {
                free_symbols(item, out);
            }
        }
        ExprNode::Pow(base, exponent) => {
            free_symbols(base, out);
            free_symbols(exponent, out);
        }
        ExprNode::Hold(hold) => match &**hold {
            Hold::Integral { body, .. } => free_symbols(body, out),
        },
    }
}

/// Vertical display range for the plot.
///
/// Taking the true min/max is wrong for anything with a pole: `1/x` on
/// `[-10, 10]` spans ±741, which squashes the interesting part of the curve
/// into a flat line along the axis. So the range is taken from percentiles
/// instead — the bulk of the samples decide the view, and the asymptote simply
/// runs off the top and bottom, which is what a graphing calculator does.
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
    let low = values[last / 50]; // ~2nd percentile
    let high = values[last - last / 50]; // ~98th percentile

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
        // A constant function still needs a visible band around it.
        min = min - 1.0;
        max = max + 1.0;
        0.0
    } else {
        span * 0.10
    };
    Some((min - pad, max + pad))
}
