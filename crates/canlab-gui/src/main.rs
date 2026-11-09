//! `canlab-gui` — the math editor is the centre of the app; the other panels are
//! a rearrangeable dock around it.
//!
//! The shell keeps the Abitti palette and replaces the first version's text
//! box with the structural math field, and adds three things:
//!
//! * a small docking tree (`layout.rs`), so the workbench can be rearranged
//!   without a docking crate;
//! * a background solver (`jobs.rs`), so a slow backend never blocks a frame;
//! * tuned dark and light palettes (`theme.rs`), with the mathematics as the
//!   only saturated thing on screen.
//!
//! The app is intentionally a single binary: the modules are implementation
//! details of the frame loop, not a library API.

mod jobs;
mod layout;
mod panels;
mod theme;

use canlab_core::{Expr, ExprNode, Hold, SymbolId, expr::node};
use canlab_input::{MathField, Node, ToExprError};
use canlab_plot::Curve;
use canlab_tex::to_latex;
use eframe::egui;

use jobs::{Job, JobKind, JobOp, JobOutcome, JobResult, Jobs};
use layout::{Dock, PanelKind, Workspace};
use theme::Mode;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_maximized(true)
            .with_title("CanLab — LaTeX algebra"),
        ..Default::default()
    };
    eframe::run_native(
        "canlab",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(CasApp::new()) as Box<dyn eframe::App>)
        }),
    )
}

/// The latest Solve outcome, kept separate from the editor's displayed LaTeX
/// so the Variables panel can tell "no answer" from "never asked".
#[derive(Default)]
pub enum SolveState {
    #[default]
    Idle,
    Solved {
        var: SymbolId,
        values: Vec<Expr>,
    },
    Declined,
    Failed(String),
}

struct HistoryEntry {
    label: String,
    latex: String,
}

struct CasApp {
    /// The structural math field. The tree is the source of truth; the text
    /// cursor is not a byte offset into a string but a position inside it.
    field: MathField,
    x_min: f64,
    x_max: f64,
    expr: Option<Expr>,
    free_vars: Vec<SymbolId>,
    result_latex: String,
    result_plain: String,
    /// A human-readable reason the field cannot be read as an expression.
    /// Empty or half-typed fields are deliberately *not* errors.
    error: Option<String>,
    curve: Option<Curve>,
    y_range: (f64, f64),
    status: String,
    solve: SolveState,
    history: Vec<HistoryEntry>,
    dock: Dock,
    mode: Mode,
    jobs: Jobs,
    /// Bumped on every re-parse. A solver result from an older generation is
    /// stale and must not replace the current expression's result.
    generation: u64,
}

impl CasApp {
    fn new() -> Self {
        let mut app = Self {
            field: default_field(),
            x_min: -10.0,
            x_max: 10.0,
            expr: None,
            free_vars: Vec::new(),
            result_latex: String::new(),
            result_plain: String::new(),
            error: None,
            curve: None,
            y_range: (-1.0, 1.0),
            status: String::from("ready"),
            solve: SolveState::Idle,
            history: Vec::new(),
            dock: Dock::default(),
            mode: Mode::Dark,
            jobs: Jobs::spawn(),
            generation: 0,
        };
        app.recompute();
        app
    }

    /// True while the field is only half-typed: an empty argument slot, or a
    /// `\name` command still being accumulated. Those are legal editing states,
    /// not errors, so the previous result stays on screen.
    fn is_typing(&self) -> bool {
        self.field.is_incomplete() || self.field.pending_command().is_some()
    }

    /// Rebuild the algebra expression from the structural field and refresh the
    /// displayed forms.
    fn recompute(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.error = None;
        self.curve = None;
        self.solve = SolveState::Idle;

        // Do not flash an error at someone who is halfway through a fraction
        // or a `\sqrt` command. Keep whatever was last readable.
        if self.is_typing() {
            return;
        }

        // A system is several equations held together. There is no single
        // expression to hand the algebra engine, so the result card shows the
        // system itself and the actions stay disabled until the backend grows a
        // system solver.
        if self.field.is_system() {
            self.expr = None;
            self.free_vars.clear();
            self.result_latex = self.field.to_latex();
            match self.field.build_lines() {
                Ok(lines) => {
                    let mut vars = Vec::new();
                    for line in &lines {
                        let mut line_vars = Vec::new();
                        free_symbols(line, &mut line_vars);
                        for var in line_vars {
                            if !vars.contains(&var) {
                                vars.push(var);
                            }
                        }
                    }
                    self.free_vars = vars;
                    self.result_plain = format!("system of {} equations", lines.len());
                }
                Err(error) => {
                    self.error = Some(format!(
                        "One of the equations cannot be read yet: {}.",
                        friendly_error(&error)
                    ));
                    self.result_plain.clear();
                }
            }
            return;
        }

        match self.field.build_expr() {
            Ok(expr) => {
                let mut vars = Vec::new();
                free_symbols(&expr, &mut vars);
                self.free_vars = vars;
                self.result_plain = node(&expr).to_string();
                self.result_latex = to_latex(&expr);
                self.expr = Some(expr);
            }
            Err(error) => {
                self.error = Some(format!(
                    "This expression cannot be read yet: {}.",
                    friendly_error(&error)
                ));
                self.free_vars.clear();
                self.result_plain.clear();
                self.result_latex.clear();
                self.expr = None;
            }
        }
    }

    fn plot(&mut self) {
        panels::build_curve(self);
    }

    fn start_simplify(&mut self) {
        self.start_job(JobKind::Simplify);
    }

    fn start_integrate(&mut self) {
        self.start_job(JobKind::Integrate);
    }

    fn start_solve(&mut self) {
        self.start_job(JobKind::Solve);
    }

    fn start_job(&mut self, kind: JobKind) {
        // Send the field's own tree. Re-parsing `to_latex()` here would be a
        // second, subtly different interpretation of what the user built.
        let expr = match self.field.build_expr() {
            Ok(expr) => expr,
            Err(error) => {
                self.status = format!("nothing to send: {}", friendly_error(&error));
                return;
            }
        };
        let (label, op) = match kind {
            JobKind::Simplify => (String::from("Simplify"), JobOp::Simplify(expr)),
            JobKind::Integrate => {
                let Some(var) = self.free_vars.first().copied() else {
                    self.status = String::from("no variable to integrate");
                    return;
                };
                (
                    format!("Integrate with respect to {}", var.as_str()),
                    JobOp::Integrate(expr, var),
                )
            }
            JobKind::Solve => {
                let Some(var) = self.free_vars.first().copied() else {
                    self.status = String::from("no variable to solve for");
                    return;
                };
                (
                    format!("Solve for {}", var.as_str()),
                    JobOp::Solve(expr, var),
                )
            }
        };
        let label_for_status = label.clone();
        let generation = self.generation;
        if self.jobs.submit(Job {
            label,
            op,
            generation,
        }) {
            self.status = format!("{label_for_status}…");
        } else {
            self.status = String::from("solver worker unavailable");
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let results = self.jobs.poll();
        if results.is_empty() {
            return;
        }
        for result in results {
            self.apply_result(result);
        }
        ctx.request_repaint();
    }

    fn apply_result(&mut self, result: JobResult) {
        let JobResult {
            label,
            kind,
            var,
            outcome,
            generation,
        } = result;
        if generation != self.generation {
            self.status = String::from("discarded a stale solver result");
            return;
        }
        match outcome {
            JobOutcome::Value(expr) => {
                self.result_latex = to_latex(&expr);
                self.result_plain = node(&expr).to_string();
                self.status = format!("{label}: done");
                self.push_history(&label, self.result_latex.clone());
            }
            JobOutcome::Solutions(values) => {
                let Some(var) = var else {
                    self.status = String::from("solver returned solutions without a variable");
                    return;
                };
                let name = to_latex(&canlab_core::sym(&var.as_str()));
                if values.is_empty() {
                    self.result_latex = String::from("\\varnothing");
                    self.result_plain = String::from("no solutions");
                    self.status = format!("{label}: no solutions");
                } else {
                    let latex: Vec<String> = values
                        .iter()
                        .map(|value| format!("{name} = {}", to_latex(value)))
                        .collect();
                    self.result_latex = latex.join(",\\quad ");
                    self.result_plain = latex.join(", ");
                    let suffix = if values.len() == 1 { "" } else { "s" };
                    self.status = format!("{label}: {} solution{suffix}", values.len());
                }
                self.solve = SolveState::Solved { var, values };
                self.push_history(&label, self.result_latex.clone());
            }
            JobOutcome::Declined => {
                self.status = format!("{label}: no solver backend");
                if kind.is_solve() {
                    self.solve = SolveState::Declined;
                }
            }
            JobOutcome::Failed(message) => {
                self.status = format!("{label}: {message}");
                if kind.is_solve() {
                    self.solve = SolveState::Failed(message);
                }
            }
        }
    }

    fn push_history(&mut self, label: &str, latex: String) {
        self.history.insert(
            0,
            HistoryEntry {
                label: label.to_owned(),
                latex,
            },
        );
        self.history.truncate(100);
    }

    fn show_panel(&mut self, ui: &mut egui::Ui, kind: PanelKind) {
        match kind {
            PanelKind::Editor => panels::editor(self, ui),
            PanelKind::Palette => panels::palette(self, ui),
            PanelKind::Variables => panels::variables(self, ui),
            PanelKind::Plot => panels::plot(self, ui),
            PanelKind::History => panels::history(self, ui),
        }
    }
}

impl eframe::App for CasApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_jobs(ui.ctx());
        menu_bar(self, ui);
        status_bar(self, ui);

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.painter()
                .rect_filled(rect, 0.0, ui.visuals().window_fill);
            let mut layout = self.dock.take_layout();
            layout.show(ui, rect, |ui, kind| self.show_panel(ui, kind));
            self.dock.restore(layout);
        });

        if self.jobs.in_flight() > 0 {
            ui.ctx().request_repaint();
        }
    }
}

fn menu_bar(app: &mut CasApp, ui: &mut egui::Ui) {
    let fill = ui.visuals().window_fill;
    egui::Panel::top("canlab-menu")
        .frame(
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(12, 7)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("CanLab").size(20.0).strong());
                ui.label(
                    egui::RichText::new("LaTeX CAS")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.separator();

                let current = app.dock.workspace().label();
                let workspaces = ui.menu_button(format!("Workspace: {current}"), |ui| {
                    for workspace in Workspace::ALL {
                        if ui
                            .selectable_label(workspace == app.dock.workspace(), workspace.label())
                            .clicked()
                        {
                            app.dock.select_workspace(workspace);
                            ui.close();
                        }
                    }
                });
                workspaces.response.on_hover_text("Swap in a preset layout");

                ui.menu_button("Layout", |ui| {
                    if ui.button("Reset layout").clicked() {
                        app.dock.reset();
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(format!("Theme: {}", app.mode.label()))
                        .on_hover_text("Switch light/dark mode")
                        .clicked()
                    {
                        app.mode = app.mode.other();
                        theme::apply(ui.ctx(), app.mode);
                    }
                    ui.label(
                        egui::RichText::new("drag the seams · split from a panel menu")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                });
            });
        });
}

fn status_bar(app: &mut CasApp, ui: &mut egui::Ui) {
    let fill = ui.visuals().window_fill;
    egui::Panel::bottom("canlab-status")
        .frame(
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(12, 5)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if app.jobs.in_flight() > 0 {
                    ui.spinner();
                    let label = app
                        .jobs
                        .running_label()
                        .map_or("solver working", |label| label);
                    ui.label(egui::RichText::new(format!("{label}…")).small());
                    ui.ctx().request_repaint();
                } else {
                    ui.label(egui::RichText::new(app.status.as_str()).small());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} free variable(s)", app.free_vars.len()))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    if !app.jobs.available() {
                        ui.label(
                            egui::RichText::new("solver offline")
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        );
                    }
                });
            });
        });
}

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

/// The expression the app opens with: `(x^2 + 1) / x`.
///
/// Built as a tree because `canlab-input` deliberately has no LaTeX parser; a
/// `type_str` sequence would need a terminator after every command and would
/// treat braces as grouping, not as command arguments.
fn default_field() -> MathField {
    let mut field = MathField::from_row(vec![Node::Frac {
        num: vec![
            Node::text("x"),
            Node::Sup(vec![Node::text("2")]),
            Node::text("+"),
            Node::text("1"),
        ],
        den: vec![Node::text("x")],
    }]);
    // `from_row` starts at the first position; a fresh app should start with
    // the caret after the expression instead.
    if let Some(end) = field.positions().last().cloned() {
        let _ = field.set_cursor(end);
    }
    field
}

/// A short explanation for a field that is structurally valid LaTeX but not a
/// value the algebra engine can consume.
fn friendly_error(error: &ToExprError) -> String {
    match error {
        ToExprError::EmptySlot => String::from("the expression is empty"),
        ToExprError::DanglingSuperscript => String::from("a superscript needs a base"),
        ToExprError::Subscript => String::from("subscripts are not algebraic expressions"),
        ToExprError::UnsupportedConstruct(name) => format!("{name} is not supported"),
        ToExprError::UnsupportedCommand(name) => {
            format!("{name} is not an algebraic operator")
        }
        ToExprError::UnexpectedText(text) => {
            format!("cannot read {text:?} as an operand")
        }
        ToExprError::System => String::from("a system is more than one expression"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_expression_is_readable() {
        let field = default_field();
        assert_eq!(field.to_latex(), "\\frac{x^2+1}{x}");
        assert!(field.build_expr().is_ok());
    }

    #[test]
    fn a_half_typed_field_keeps_the_previous_result_quietly() {
        let mut app = CasApp::new();
        let previous_latex = app.result_latex.clone();
        // The default field is complete; a bare `/` opens an empty fraction.
        app.field.type_str("/");
        app.recompute();
        assert!(app.error.is_none());
        assert_eq!(app.result_latex, previous_latex);
    }

    #[test]
    fn a_complete_but_unreadable_field_gets_a_friendly_error() {
        let mut app = CasApp::new();
        app.field = MathField::new();
        app.field.type_str("x_1");
        app.recompute();
        assert!(
            app.error
                .as_deref()
                .is_some_and(|error| error.contains("subscript")),
            "{:?}",
            app.error
        );
    }
}
