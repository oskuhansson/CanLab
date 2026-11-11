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
mod persist;
#[cfg(test)]
mod snapshot;
mod theme;
mod undo;

use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use canlab_core::{Expr, ExprNode, Hold, Number, SymbolId, expr::node};
use canlab_input::{Definitions, MathField, Node, ToExprError};
use canlab_plot::{Curve, Intersection};
use canlab_tex::to_latex;
use egui;

use jobs::{Job, JobKind, JobOp, JobOutcome, JobResult, Jobs};
use layout::{Dock, PanelKind, Workspace};

/// The application, as a Bevy resource.
///
/// `CasApp` is unchanged: it still draws one frame given a `Ui`, which is what
/// lets the same code run under Bevy and under the headless snapshot harness.
#[derive(Resource)]
struct OpenApp {
    app: CasApp,
    /// `theme::install` needs an egui context, and the context does not exist
    /// until `bevy_egui` has made one for the camera, so it happens on the first
    /// frame rather than in `main`.
    theme_installed: bool,
}

/// The camera `bevy_egui` hangs its primary context on.
///
/// Without one there is no context, and every frame silently draws nothing —
/// which is exactly how the boot probe spent twelve seconds looking idle.
fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// One frame of the interface, inside Bevy's egui pass.
fn draw_frame(mut contexts: EguiContexts, mut open: ResMut<OpenApp>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !open.theme_installed {
        open.theme_installed = true;
        theme::install(ctx);
    }
    draw_root(&mut open.app, ctx);
}

/// Draw the application as the root of an egui context, and report the rectangle
/// it was laid out in.
///
/// The rectangle is returned for the regression test: an `Area` remembers its
/// size between passes, so the first pass of a Bevy window — which happens before
/// `bevy_egui` knows the scale factor and reported a 1770×1140 screen for a
/// 1180×760 window — left the whole interface laid out for a window that did not
/// exist, and the right-hand column was simply off screen.
fn draw_root(app: &mut CasApp, ctx: &egui::Context) -> egui::Rect {
    let screen = ctx.content_rect();
    let mut laid_out = screen;
    egui::Area::new(egui::Id::new("canlab-root"))
        .fixed_pos(screen.min)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            // Explicit, every frame: this is what makes the Area follow the
            // window instead of its own memory of the last size.
            ui.set_width(screen.width());
            ui.set_height(screen.height());
            ui.set_clip_rect(screen);
            laid_out = ui.max_rect();
            app.draw(ui);
        });
    laid_out
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: String::from("CanLab — LaTeX algebra"),
                resolution: (1180u32, 760u32).into(),
                // Without this the Windows title bar is light above a dark app:
                // winit only switches the frame to dark when the window asks for
                // a dark theme.
                window_theme: Some(bevy::window::WindowTheme::Dark),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(OpenApp {
            app: CasApp::new(),
            theme_installed: false,
        })
        .add_systems(Startup, setup_camera)
        .add_systems(EguiPrimaryContextPass, draw_frame)
        .run();
}

/// The latest Solve outcome, kept separate from the editor's displayed LaTeX
/// so the Variables panel can tell "no answer" from "never asked".
#[derive(Default, Debug)]
pub enum SolveState {
    #[default]
    Idle,
    Solved {
        var: SymbolId,
        /// Every kind of answer the solver can give, not just a list of values.
        /// `sin x = ½` is two families, `x² + 1 = 0` is none, and an inequality is
        /// an interval — all of them are answers, so the panel holds the whole set.
        solutions: canlab_solver::Solutions,
    },
    Declined {
        /// What was asked, e.g. "Solve for x".
        label: String,
        /// What the backend can do instead.
        why: String,
    },
    /// A system's answer: one value per variable it could pin down.
    System {
        assignments: Vec<(SymbolId, Expr)>,
    },
    Failed(String),
}

struct HistoryEntry {
    label: String,
    latex: String,
}

/// A variable that is not on an axis, and the value the plot samples it at.
///
/// Before this existed, such a variable silently became `1` and the picture said
/// `l = 1 assumed`. It still defaults to 1 — but now it is a number the user can
/// change, and the caption says what it is actually set to.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// The variable's name.
    pub name: SymbolId,
    /// The value it is sampled at.
    pub value: f64,
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
    /// The curves currently plotted: one for y = f(x), one per equation for a system.
    curves: Vec<Curve>,
    /// The document's equations — the `left` and optional `right` of each line's
    /// `=`. A lone equation is one entry; a system is several. Empty when the
    /// document is a plain expression.
    equations: Vec<(Expr, Option<Expr>)>,
    /// Which variable is drawn along the horizontal axis. Any variable can be:
    /// `i² + j² = 1` plots with `i` on x and `j` on y just as well as `x² + y² = 1`
    /// does. `None` when the document has no variables at all.
    x_axis: Option<SymbolId>,
    /// Which variable is drawn vertically. `None` for a one-variable document,
    /// where the curve is `y = f(x)` and only the horizontal axis is a variable.
    y_axis: Option<SymbolId>,
    /// What the plot panel says about the last attempt: how many curves, on which
    /// axes, or why nothing was drawn. Separate from `status`, which belongs to
    /// the solver.
    plot_note: String,
    y_range: (f64, f64),
    status: String,
    solve: SolveState,
    history: Vec<HistoryEntry>,
    dock: Dock,
    /// The colour scheme chosen in Settings: surfaces, text, accent and the
    /// plot's curve wheel, as one thing.
    palette: &'static theme::Palette,
    jobs: Jobs,
    /// Bumped on every re-parse. A solver result from an older generation is
    /// stale and must not replace the current expression's result.
    generation: u64,
    /// How many frames have been painted. The first few force a repaint so the
    /// window is never left showing an unpainted surface.
    paints: u8,
    /// The functions the document defines, harvested from every line.
    definitions: Definitions,
    /// Definitions the document tried to make and could not, with the reason.
    definition_problems: Vec<(String, String)>,
    /// The decimal beside an exact answer, when it has one. 2/3 shows ≈ 0.667;
    /// x = √2 shows ≈ 1.41; an integer shows nothing, because it needs nothing.
    approximation: Option<String>,
    /// Values for the variables that are not on an axis.
    parameters: Vec<Parameter>,
    /// Symbols that are a whole side of an equation and not on an axis.
    ///
    /// `x² + y² = z` defines a surface: `z` is the third coordinate, not a knob.
    /// The 2D plot must leave that equation out rather than invent `z = 1` and
    /// draw the slice as if it were the graph.
    third_coordinates: Vec<SymbolId>,
    /// Points where two curves of a system cross, refined to a coordinate.
    intersections: Vec<Intersection>,
    /// Undo history for the document.
    undo: undo::Undo,
    /// When the document last changed, so a save can wait for a pause.
    dirty_since: Option<Instant>,
    /// False in tests: they must never read or write the user's session file.
    sessions: bool,
    /// The theme has been restored from a session but not applied yet, because
    /// applying it needs an egui context and `new` has none.
    theme_pending: bool,
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
            curves: Vec::new(),
            equations: Vec::new(),
            x_axis: None,
            y_axis: None,
            plot_note: String::new(),
            y_range: (-1.0, 1.0),
            status: String::from("ready"),
            solve: SolveState::Idle,
            history: Vec::new(),
            dock: Dock::default(),
            palette: &theme::PALETTES[0],
            jobs: Jobs::spawn(),
            generation: 0,
            paints: 0,
            definitions: Definitions::default(),
            definition_problems: Vec::new(),
            approximation: None,
            parameters: Vec::new(),
            third_coordinates: Vec::new(),
            intersections: Vec::new(),
            undo: undo::Undo::new(),
            dirty_since: None,
            // A test that restored the last session would depend on whichever
            // document the developer last had open — and would overwrite it.
            sessions: !cfg!(test),
            theme_pending: false,
        };
        app.restore_session();
        // `CANLAB_PALETTE=Nord` picks a scheme for this run, which is what a
        // screenshot and a shared machine want.
        if let Ok(name) = std::env::var("CANLAB_PALETTE") {
            if let Some(palette) = theme::by_name(name.trim()) {
                app.palette = palette;
                app.theme_pending = true;
            }
        }
        // `CANLAB_WORKSPACE=plotting` opens in a preset, which is what a
        // screenshot of a specific case wants.
        if let Ok(name) = std::env::var("CANLAB_WORKSPACE") {
            let workspace = match name.trim().to_ascii_lowercase().as_str() {
                "solving" => Some(Workspace::Solving),
                "plotting" => Some(Workspace::Plotting),
                "editing" => Some(Workspace::Editing),
                _ => None,
            };
            if let Some(workspace) = workspace {
                app.dock.select_workspace(workspace);
            }
        }
        app.recompute();
        // A debug aid worth keeping: `CANLAB_OPEN` loads a document at startup,
        // its lines separated by `|` and its systems by `||`. It is what makes a
        // screenshot of a specific case possible without typing into the window.
        if let Ok(document) = std::env::var("CANLAB_OPEN") {
            let systems: Vec<Vec<String>> = document
                .split("||")
                .map(|system| system.split('|').map(str::to_string).collect())
                .collect();
            if let Ok(field) = MathField::from_systems(&systems) {
                app.field = field;
                app.recompute();
            }
        }
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
        self.reparse();
        // Plotting is not a button: the picture follows the document, so it is
        // rebuilt whenever the document is rebuilt.
        self.autoplot();
    }

    /// Redraw the plot from the current document, or clear it when there is
    /// nothing drawable. Cheap enough to run on every keystroke: a 240×240
    /// implicit trace is a few milliseconds.
    fn autoplot(&mut self) {
        if self.is_typing() || !self.can_plot() {
            self.curves.clear();
            self.plot_note.clear();
            return;
        }
        panels::build_curve(self);
    }

    /// Rebuild the expression and everything derived from it.
    fn reparse(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.error = None;
        self.curves.clear();
        self.intersections.clear();
        self.approximation = None;
        self.solve = SolveState::Idle;

        // Definitions first: every later read depends on them.
        let harvest = self.field.harvest();
        self.definition_problems = harvest
            .problems
            .iter()
            .map(|(name, error)| (name.clone(), error.to_string()))
            .collect();
        self.definitions = harvest.definitions;

        // Do not flash an error at someone who is halfway through a fraction
        // or a `\sqrt` command. Keep whatever was last readable.
        //
        // The *derived* state does not survive, though: these equations, the
        // solve answer and the curves describe the last complete parse, and
        // `start_solve` would send that stale document under the new generation.
        // Clearing them is what makes "half-typed" mean "nothing to send".
        if self.is_typing() {
            self.equations.clear();
            self.solve = SolveState::Idle;
            self.intersections.clear();
            self.curves.clear();
            return;
        }

        // An inequality is a relation too, but `build_equations` only knows
        // `=`, and the expression reader refuses a comparison by design. It is
        // read here so the comparator survives re-parsing; folding the sides
        // into a difference at this point would lose which comparison was typed.
        match self.field.build_relation_with(&self.definitions) {
            Ok(Some(relation)) => {
                self.expr = None;
                self.equations.clear();
                self.result_latex = self.field.to_latex();
                self.result_plain = String::from("an inequality");
                let mut vars = Vec::new();
                for side in [&relation.left, &relation.right] {
                    let mut side_vars = Vec::new();
                    free_symbols(side, &mut side_vars);
                    for var in side_vars {
                        if !vars.contains(&var) {
                            vars.push(var);
                        }
                    }
                }
                for variable in self.definition_variables() {
                    if !vars.contains(&variable) {
                        vars.push(variable);
                    }
                }
                self.free_vars = vars;
                self.choose_axes();
                self.refresh_parameters();
                return;
            }
            Ok(None) => {}
            Err(error) => {
                self.error = Some(format!(
                    "The inequality cannot be read yet: {}.",
                    friendly_error(&error)
                ));
                self.result_plain.clear();
                self.equations.clear();
                self.refresh_parameters();
                return;
            }
        }

        // An *equation* is a first-class thing here, whether or not it is part of
        // a system: `x + y = 3` on its own line is a perfectly good object to
        // plot and to solve. `build_expr` refuses relations by design (a relation
        // is not a value), so equations are read separately and kept on the app,
        // and only a plain expression goes into `self.expr`.
        let has_relation = self.field.has_relation();
        if has_relation {
            self.expr = None;
            self.result_latex = self.field.to_latex();
            match self.field.build_equations_with(&self.definitions) {
                Ok(equations) => {
                    let mut vars = Vec::new();
                    for (lhs, rhs) in &equations {
                        for side in std::iter::once(lhs).chain(rhs.iter()) {
                            let mut side_vars = Vec::new();
                            free_symbols(side, &mut side_vars);
                            for var in side_vars {
                                if !vars.contains(&var) {
                                    vars.push(var);
                                }
                            }
                        }
                    }
                    self.free_vars = vars;
                    for variable in self.definition_variables() {
                        if !self.free_vars.contains(&variable) {
                            self.free_vars.push(variable);
                        }
                    }
                    self.choose_axes();
                    let named = equations.iter().filter(|(_, rhs)| rhs.is_some()).count();
                    let lines = equations.len();
                    self.result_plain = match (named, lines) {
                        (0, _) => String::from("an empty document"),
                        (1, 1) => String::from("an equation"),
                        (n, m) if n == m => format!("system of {n} equations"),
                        (n, m) => format!("system: {n} of {m} lines are equations"),
                    };
                    self.equations = equations;
                }
                Err(error) => {
                    self.error = Some(format!(
                        "One of the equations cannot be read yet: {}.",
                        friendly_error(&error)
                    ));
                    self.result_plain.clear();
                    self.equations.clear();
                }
            }
            self.refresh_parameters();
            return;
        }
        self.equations.clear();

        // No relation anywhere: expressions, or a page that only defines names.
        let rows: Vec<canlab_input::Row> = self
            .field
            .rows()
            .filter(|row| !row.is_empty() && !self.field.is_definition_row(row))
            .cloned()
            .collect();
        match rows.len() {
            0 => self.read_definitions(),
            1 => match canlab_input::build_with(&rows[0], &self.definitions) {
                Ok(expr) => {
                    let mut vars = Vec::new();
                    free_symbols(&expr, &mut vars);
                    for variable in self.definition_variables() {
                        if !vars.contains(&variable) {
                            vars.push(variable);
                        }
                    }
                    self.free_vars = vars;
                    self.choose_axes();
                    self.result_plain = node(&expr).to_string();
                    self.result_latex = to_latex(&expr);
                    self.expr = Some(expr);
                    self.error = self.definition_problems.first().map(|(name, why)| {
                        format!("{name} is not defined: {why}")
                    });
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
            },
            _ => {
                // Several lines of plain expressions and no `=`: not one value,
                // and not a system either.
                let reason = rows
                    .iter()
                    .find_map(|row| canlab_input::build_with(row, &self.definitions).err())
                    .map(|error| friendly_error(&error))
                    .unwrap_or_else(|| friendly_error(&ToExprError::System));
                self.error = Some(format!("This expression cannot be read yet: {reason}."));
                self.expr = None;
                self.free_vars.clear();
                self.result_plain.clear();
                self.result_latex.clear();
            }
        }
        self.refresh_parameters();
    }

    /// A page that only defines functions still has something to show: the
    /// definitions themselves.
    fn read_definitions(&mut self) {
        self.expr = None;
        self.equations.clear();
        let mut names: Vec<String> = Vec::new();
        let mut parts: Vec<String> = Vec::new();
        for (name, function) in self.definitions.functions() {
            let params: Vec<String> = function
                .params
                .iter()
                .map(|parameter| to_latex(&canlab_core::sym(parameter.as_str().as_str())))
                .collect();
            parts.push(format!(
                "{name}({}) = {}",
                params.join(", "),
                to_latex(&function.body)
            ));
            names.push(name.to_string());
        }
        let count = parts.len();
        self.result_latex = parts.join(",\\quad ");
        self.result_plain = if count == 1 {
            format!("1 definition: {}", names[0])
        } else {
            format!("{count} definitions: {}", names.join(", "))
        };
        self.free_vars = self.definition_variables();
        self.choose_axes();
        self.error = self
            .definition_problems
            .first()
            .map(|(name, why)| format!("{name} is not defined: {why}"));
    }

    /// The parameters of the document's definitions, as variables.
    ///
    /// Only a function of *one* variable is a curve, so only its parameter becomes
    /// a variable of the page. `g(a, b) = a·b` is a surface, and listing `a` and
    /// `b` as free variables would invite a plot that cannot exist.
    fn definition_variables(&self) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for (_, function) in self.definitions.functions() {
            if function.params.len() != 1 {
                continue;
            }
            for parameter in &function.params {
                if !out.contains(parameter) {
                    out.push(*parameter);
                }
            }
        }
        out
    }

    /// Equations of the form `s = 1`: a parameter the document itself defines.
    ///
    /// A system is allowed to state the value of its own parameter, and when it
    /// does, that value is the document's rather than the default 1.0. `x² + y² = s`
    /// with `s = 1` is a circle, not a surface: the second line is what makes the
    /// first plottable, so the plot must not treat `s` as a third coordinate when
    /// a row has already pinned it down.
    fn assigned_parameters(&self) -> Vec<(SymbolId, Number)> {
        self.document_assignments()
            .into_iter()
            .filter_map(|(symbol, value)| value.as_number().cloned().map(|number| (symbol, number)))
            .collect()
    }

    /// The values the document itself states, in the order they can be resolved.
    ///
    /// `b = 3` fixes `b`; `a = 2b` is only a number once `b` is one, so a single
    /// pass is not enough and this resolves repeatedly until nothing changes. A
    /// line whose symbol is still free afterwards fixes nothing — `r = 2s` with
    /// `s` unknown stays symbolic — and a line that mentions its own symbol
    /// (`s = s + 1`) is not iterated, because it has no value to reach.
    ///
    /// An **axis** variable is not an assignment: `x = 1` is a vertical line to
    /// plot and an equation to solve, not a definition of `x`. That is the
    /// difference between "the page pins a parameter" and "the page draws a
    /// curve", and it is why the axes are excluded here.
    pub fn document_assignments(&self) -> Vec<(SymbolId, Expr)> {
        let axes: Vec<SymbolId> = [self.x_axis, self.y_axis]
            .into_iter()
            .flatten()
            .collect();
        let mut candidates: Vec<(SymbolId, Expr)> = Vec::new();
        for (left, right) in &self.equations {
            let Some(right) = right else {
                continue;
            };
            let (symbol, value) = match (left.as_symbol(), right.as_symbol()) {
                (Some(symbol), _) => (symbol, right.clone()),
                (None, Some(symbol)) => (symbol, left.clone()),
                _ => continue,
            };
            if axes.contains(&symbol) {
                continue;
            }
            // A value that mentions an axis is not a value the page states.
            // `x² + y² = a` is a circle, not the definition `a = x² + y²`: read the
            // other way, `a` becomes a function of the variables it is plotted
            // against, and the circle's radius turns into a curve.
            let mut value_symbols = Vec::new();
            free_symbols(&value, &mut value_symbols);
            if value_symbols.iter().any(|symbol| axes.contains(symbol)) {
                continue;
            }
            // Two lines for one symbol: the first stated wins, as it does for a
            // parameter's value today.
            if !candidates.iter().any(|(seen, _)| *seen == symbol) {
                candidates.push((symbol, value));
            }
        }

        let mut known: Vec<(SymbolId, Expr)> = Vec::new();
        loop {
            let mut progressed = false;
            for (symbol, value) in &candidates {
                if known.iter().any(|(seen, _)| seen == symbol) {
                    continue;
                }
                let substituted = substitute_all(value, &known);
                let mut free = Vec::new();
                free_symbols(&substituted, &mut free);
                if !free.is_empty() {
                    continue;
                }
                known.push((*symbol, substituted));
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        known
    }

    /// Symbols that are a whole side of an equation and are not on an axis.
    ///
    /// Structure alone cannot tell `x² + y² = z` from `x² + y² = a`: both have a
    /// bare symbol on one side and both variables on the other. The difference is
    /// what the symbol *means*, and the only signal for that is the name. So the
    /// refusal is limited to the conventional third coordinate (`z`, and `w` after
    /// it), and every other bare symbol becomes a **parameter** — a knob whose
    /// value is shown on a slider, so a value that was invented is visible rather
    /// than implied. `x² + y² = r²` stays a family because `r` is inside a power; a
    /// symbol the document assigns (`s = 1`) is a parameter with that value.
    fn third_coordinate_symbols(&self) -> Vec<SymbolId> {
        let axes: Vec<SymbolId> = [self.x_axis, self.y_axis]
            .into_iter()
            .flatten()
            .collect();
        let assigned = self.assigned_parameters();
        let axis_or_constant = |expression: &Expr| {
            expression
                .as_symbol()
                .is_some_and(|symbol| axes.contains(&symbol))
                || expression.as_number().is_some()
        };
        let mut out: Vec<SymbolId> = Vec::new();
        for (left, right) in &self.equations {
            let Some(right) = right else {
                continue;
            };
            for (side, other) in [(left, right), (right, left)] {
                let Some(symbol) = side.as_symbol() else {
                    continue;
                };
                if axes.contains(&symbol) || assigned.iter().any(|(seen, _)| *seen == symbol) {
                    continue;
                }
                if !is_coordinate_name(symbol) {
                    continue;
                }
                if !axis_or_constant(other) && !out.contains(&symbol) {
                    out.push(symbol);
                }
            }
        }
        out
    }

    /// Recompute the parameter list: every free variable that is not on an axis.
    ///
    /// Values the user has set are kept; a new variable starts at 1, which is
    /// what it used to be forced to whether anyone wanted it or not. A symbol
    /// that is a whole side of an equation is not a parameter at all — see
    /// [`Self::third_coordinate_symbols`] — so it gets no value and no slider.
    /// A symbol the document assigns gets the document's value.
    pub fn refresh_parameters(&mut self) {
        let coordinates = self.third_coordinate_symbols();
        let assigned = self.assigned_parameters();
        let axes: Vec<SymbolId> = [self.x_axis, self.y_axis]
            .into_iter()
            .flatten()
            .collect();
        let mut next: Vec<Parameter> = Vec::new();
        for variable in &self.free_vars {
            if axes.contains(variable) {
                continue;
            }
            // The document's value wins over the slider: `s = 1` is a statement,
            // not a default. The control stays, so the value can be tried out;
            // the next re-parse puts it back.
            if let Some((_, number)) = assigned.iter().find(|(seen, _)| seen == variable) {
                next.push(Parameter {
                    name: *variable,
                    value: number.to_f64(),
                });
                continue;
            }
            if coordinates.contains(variable) {
                continue;
            }
            let value = self
                .parameters
                .iter()
                .find(|parameter| parameter.name == *variable)
                .map_or(1.0, |parameter| parameter.value);
            next.push(Parameter {
                name: *variable,
                value,
            });
        }
        self.third_coordinates = coordinates;
        self.parameters = next;
    }

    /// Is there anything the plot panel could draw?
    ///
    /// An equation is plottable even though it is not an expression — that is the
    /// whole point of the implicit tracer — so this asks the document, not
    /// `self.expr`. A page of definitions is plottable too: `f(x) = x² + 1` has a
    /// curve of its own.
    fn can_plot(&self) -> bool {
        self.expr.is_some() || !self.equations.is_empty() || !self.definitions.is_empty()
    }

    /// Keep the axis choice coherent with the document.
    ///
    /// Called after every re-parse. A choice that is still valid is kept — both
    /// axes are the user's to make, and `p² + q² = 1` with `q` vertical must stay
    /// that way while they type. A variable that no longer exists cannot stay on
    /// an axis, and a document with two named variables should get both axes
    /// filled without anyone having to choose: `x`/`y` first, then the common
    /// alternative pairs, so `i² + j² = 1` plots straight away.
    ///
    /// A variable that is *not* a natural axis letter stays a **parameter** and
    /// gets a control, rather than being promoted to the vertical axis. That is
    /// the difference between `a·x = 1` (a family of vertical lines, `a` on a
    /// slider) and `a = 1` (one line) — and it is why `l = 1 assumed` is gone.
    /// Any variable can still be put on an axis by hand, which is what makes
    /// `p² + q² = 1` work.
    fn choose_axes(&mut self) {
        let has = |variable: &SymbolId| self.free_vars.contains(variable);
        let pick = |names: &[&str], avoid: Option<SymbolId>| {
            names
                .iter()
                .map(|name| SymbolId::intern(name))
                .find(|candidate| has(candidate) && Some(*candidate) != avoid)
        };
        if !self.x_axis.is_some_and(|axis| has(&axis)) {
            self.x_axis = pick(&["x", "i", "t", "u", "s"], None)
                .or_else(|| self.free_vars.first().copied());
        }
        let vertical = pick(&["y", "j", "v"], self.x_axis);
        // Only a *named* second axis is guessed. `a` in `a·x = 1` is a parameter,
        // and silently plotting it as an axis hid the control that belongs there.
        // A vertical axis the user picked survives as long as it is still a free
        // variable and not the horizontal one; rewriting it on every parse lost
        // that choice on the next keystroke.
        if !self
            .y_axis
            .is_some_and(|axis| has(&axis) && Some(axis) != self.x_axis)
        {
            self.y_axis = vertical;
        }
    }

    // ---------------------------------------------------------------- editing

    /// Note that the session has changed, so it is written after the debounce.
    ///
    /// An edit is not the only thing `session()` holds: the palette, the plot
    /// window, the axes and the parameter values are saved too, and a change that
    /// did not ask for a save was simply lost on quit.
    pub fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    /// Record the document before an edit, so it can be taken back.
    ///
    /// Every path that mutates the field calls this first: the keyboard handler,
    /// the palette, paste, and loading a history entry.
    pub fn note_edit(&mut self, structural: bool) {
        self.undo.checkpoint(&self.field, structural);
        self.mark_dirty();
    }

    /// Undo one step.
    pub fn undo_edit(&mut self) {
        if self.undo.undo(&mut self.field) {
            self.recompute();
            self.mark_dirty();
            self.status = match self.undo.undo_depth() {
                0 => String::from("undone"),
                left => format!("undone ({left} more)"),
            };
        } else {
            self.status = String::from("nothing to undo");
        }
    }

    /// Redo one step.
    pub fn redo_edit(&mut self) {
        if self.undo.redo(&mut self.field) {
            self.recompute();
            self.mark_dirty();
            self.status = match self.undo.redo_depth() {
                0 => String::from("redone"),
                left => format!("redone ({left} more)"),
            };
        } else {
            self.status = String::from("nothing to redo");
        }
    }

    /// The document as LaTeX, on the clipboard.
    fn copy_latex(&mut self, ctx: &egui::Context) {
        let latex = self.field.to_latex();
        if latex.is_empty() {
            self.status = String::from("nothing to copy");
            return;
        }
        ctx.copy_text(latex.clone());
        self.status = String::from("document LaTeX copied");
    }

    /// Paste LaTeX in.
    ///
    /// One expression goes in *at the caret*, because that is what a text editor
    /// does and the field can absorb it. A whole document replaces the page: a
    /// pasted system is a new question, not an insertion.
    fn paste_latex(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.note_edit(true);
        let lines = paste_lines(text);
        if lines.len() > 1 {
            match MathField::from_latex_lines(&lines) {
                Ok(field) => {
                    self.field = field;
                    self.status = format!("pasted {} lines", lines.len());
                    self.recompute();
                }
                Err(why) => self.status = format!("could not paste: {why}"),
            }
            return;
        }
        match self.field.apply_snippet(text) {
            Ok(()) => {
                self.status = String::from("pasted");
                self.recompute();
            }
            Err(why) => {
                // Not a snippet: try it as a whole formula, which is what a
                // pasted line from elsewhere usually is.
                match MathField::from_latex(text) {
                    Ok(field) => {
                        self.field = field;
                        self.status = String::from("pasted a new document");
                        self.recompute();
                    }
                    Err(_) => self.status = format!("could not paste: {why}"),
                }
            }
        }
    }

    /// Put a piece of LaTeX back into the editor, replacing the document.
    ///
    /// This is what the history panel does with a click, and what a paste of a
    /// whole formula does when it is not a snippet.
    pub fn load_latex(&mut self, latex: &str) {
        let lines = paste_lines(latex);
        let field = if lines.len() > 1 {
            MathField::from_latex_lines(&lines)
        } else {
            MathField::from_latex(latex)
        };
        match field {
            Ok(field) => {
                self.note_edit(true);
                self.field = field;
                self.recompute();
                self.status = String::from("loaded into the editor");
            }
            Err(why) => self.status = format!("cannot load that: {why}"),
        }
    }

    /// Shortcuts that belong to the document rather than to a panel.
    ///
    /// They are taken out of the event queue here, before any panel draws, so a
    /// focused editor cannot swallow `Ctrl+Z` on its way past.
    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        let mut undo = false;
        let mut redo = false;
        let mut copy = false;
        let mut paste: Option<String> = None;
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Paste(text) if !text.is_empty() => {
                    paste = Some(text.clone());
                    false
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command => {
                    match key {
                        egui::Key::Z if modifiers.shift => redo = true,
                        egui::Key::Z => undo = true,
                        egui::Key::Y => redo = true,
                        egui::Key::C => copy = true,
                        _ => return true,
                    }
                    false
                }
                _ => true,
            });
        });
        if undo {
            self.undo_edit();
        }
        if redo {
            self.redo_edit();
        }
        if copy {
            self.copy_latex(ui.ctx());
        }
        if let Some(text) = paste {
            self.paste_latex(&text);
        }
    }

    // ------------------------------------------------------------- persistence

    /// The current state, as something that can be written to disk.
    fn session(&self) -> persist::Session {
        persist::Session {
            document: self.field.systems_latex(),
            palette: self.palette.name.to_string(),
            dark: self.palette.is_dark(),
            accent: [
                self.palette.accent.r(),
                self.palette.accent.g(),
                self.palette.accent.b(),
            ],
            x_range: (self.x_min, self.x_max),
            axes: (
                self.x_axis.map(|axis| axis.as_str().to_string()),
                self.y_axis.map(|axis| axis.as_str().to_string()),
            ),
            parameters: self
                .parameters
                .iter()
                .map(|parameter| (parameter.name.as_str().to_string(), parameter.value))
                .collect(),
        }
    }

    /// Install a saved session.
    fn apply_session(&mut self, session: &persist::Session) -> bool {
        let Ok(field) = MathField::from_systems(&session.document) else {
            return false;
        };
        self.field = field;
        // A session written before palettes existed has a light/dark flag and one
        // accent; it opens in the closest whole scheme.
        self.palette = theme::by_name(&session.palette).unwrap_or_else(|| {
            theme::nearest(
                session.dark,
                egui::Color32::from_rgb(session.accent[0], session.accent[1], session.accent[2]),
            )
        });
        self.x_min = session.x_range.0;
        self.x_max = session.x_range.1;
        self.x_axis = session.axes.0.as_deref().map(SymbolId::intern);
        self.y_axis = session.axes.1.as_deref().map(SymbolId::intern);
        // The theme needs an egui context, which `new` does not have.
        self.theme_pending = true;
        self.recompute();
        // Parameter values are keyed by name and the recompute above has just
        // reset them to the default, so the saved ones go back on afterwards.
        for (name, value) in &session.parameters {
            let name = SymbolId::intern(name);
            if let Some(parameter) = self.parameters.iter_mut().find(|p| p.name == name) {
                parameter.value = *value;
            }
        }
        // `recompute` auto-fits the view to the document, which would throw away
        // the window that was saved. Put it back and redraw without fitting.
        self.x_min = session.x_range.0;
        self.x_max = session.x_range.1;
        crate::panels::build_curve_fitting(self, false);
        true
    }

    /// Load the last session, unless this run was told not to.
    fn restore_session(&mut self) {
        if !self.sessions || sessions_disabled() {
            return;
        }
        let Some(session) = persist::load() else {
            return;
        };
        self.load_session(&session);
    }

    /// Install a session that was already read, reporting a refusal.
    ///
    /// `apply_session` refuses a document the field cannot build; staying silent
    /// looked exactly like a run with no saved session.
    fn load_session(&mut self, session: &persist::Session) {
        if self.apply_session(session) {
            self.status = String::from("restored the last session");
        } else {
            self.status = String::from("the saved session could not be read");
        }
    }

    /// Write the session now.
    fn save_session(&mut self) {
        if !self.sessions || sessions_disabled() {
            self.dirty_since = None;
            return;
        }
        if let Err(why) = persist::save(&self.session()) {
            self.status = why;
        }
        self.dirty_since = None;
    }

    /// Save a moment after the last change, so an unexpected close loses at most
    /// that moment. Debounced rather than written per keystroke: this is a file
    /// on a disk, not a buffer.
    fn autosave(&mut self, ui: &egui::Ui) {
        if !self.sessions {
            return;
        }
        let Some(since) = self.dirty_since else {
            return;
        };
        let debounce = std::time::Duration::from_millis(1200);
        if since.elapsed() < debounce {
            ui.ctx().request_repaint_after(debounce);
            return;
        }
        self.save_session();
    }

    fn start_simplify(&mut self) {
        self.start_job(JobKind::Simplify);
    }

    fn start_integrate(&mut self) {
        self.start_job(JobKind::Integrate);
    }

    /// Refuse to send a half-typed field as if it were a document.
    ///
    /// Returns true when the caller must not submit anything. A job reads the
    /// whole document, and an open argument slot is not one yet.
    fn refuse_half_typed(&mut self) -> bool {
        if self.is_typing() {
            self.status = String::from("nothing to send: the document is still being typed");
            true
        } else {
            false
        }
    }

    fn start_solve(&mut self) {
        if self.refuse_half_typed() {
            return;
        }
        // A page of equations is not always a system to solve together. `a = 2b`
        // and `b = 3` state two values; once they are substituted the rest of the
        // page is *one* equation. Without this, `x² + y² = a` with `a` pinned by
        // the page is a four-unknown nonlinear system, and the backend declines —
        // which is what it used to show for an entirely ordinary exercise.
        if self.equations.len() > 1 {
            let assignments = self.document_assignments();
            let mut equations: Vec<Expr> = Vec::new();
            for (left, right) in &self.equations {
                // The line that stated a value has done its job.
                if states_a_value(&assignments, left, right.as_ref()) {
                    continue;
                }
                let left = substitute_all(left, &assignments);
                let right = right
                    .as_ref()
                    .map(|right| substitute_all(right, &assignments));
                if let Some(right) = &right
                    && left == *right
                {
                    // `1 = 1`: the substitution consumed this line, and asking the
                    // solver about it would answer "every number".
                    continue;
                }
                equations.push(match right {
                    Some(right) => canlab_core::add(vec![
                        left,
                        canlab_core::mul(vec![canlab_core::int(-1), right]),
                    ]),
                    None => left,
                });
            }

            if equations.is_empty() {
                if assignments.is_empty() {
                    self.status = String::from("nothing to solve: the page states no value");
                } else {
                    self.solve = SolveState::System { assignments };
                    self.status = String::from("solved");
                }
                return;
            }

            if equations.len() == 1 {
                let equation = equations.pop().expect("length checked");
                let var = self
                    .free_vars
                    .iter()
                    .copied()
                    .find(|symbol| !assignments.iter().any(|(seen, _)| seen == symbol))
                    .or_else(|| self.free_vars.first().copied())
                    .unwrap_or_else(|| SymbolId::intern("x"));
                let label = String::from("Solve the equation");
                let generation = self.generation;
                if self.jobs.submit(Job {
                    label,
                    op: JobOp::Solve(equation, var),
                    generation,
                }) {
                    self.status = String::from("Solve the equation…");
                } else {
                    self.status = String::from("solver worker unavailable");
                }
                return;
            }

            let label = format!("Solve {} equations", equations.len());
            let generation = self.generation;
            let status = format!("{label}…");
            if self.jobs.submit(Job {
                label,
                op: JobOp::SolveSystem(equations, self.free_vars.clone()),
                generation,
            }) {
                self.status = status;
            } else {
                self.status = String::from("solver worker unavailable");
            }
            return;
        }
        self.start_job(JobKind::Solve);
    }

    fn start_job(&mut self, kind: JobKind) {
        if self.refuse_half_typed() {
            return;
        }
        // A comparison is a different request from an equation. It leaves the
        // editor as both sides plus the comparator, never as `left − right`,
        // because that fold makes `<` and `>` the same expression.
        if kind == JobKind::Solve {
            match self.field.build_relation_with(&self.definitions) {
                Ok(Some(relation)) => {
                    let Some(var) = self.free_vars.first().copied() else {
                        self.status = String::from("no variable to solve for");
                        return;
                    };
                    let label = format!("Solve the inequality for {}", var.as_str());
                    let status = format!("{label}…");
                    let generation = self.generation;
                    if self.jobs.submit(Job {
                        label,
                        op: JobOp::SolveRelation(relation, var),
                        generation,
                    }) {
                        self.status = status;
                    } else {
                        self.status = String::from("solver worker unavailable");
                    }
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    self.status = format!("nothing to send: {}", friendly_error(&error));
                    return;
                }
            }
        }
        // Send the field's own tree. Re-parsing `to_latex()` here would be a
        // second, subtly different interpretation of what the user built.
        //
        // The definitions are part of that interpretation: the panel and the
        // variable list were built with them, so `y = f(2)` must leave here as
        // the substituted equation rather than as an unresolved `f·2`.
        //
        // Solve wants an equation `left − right = 0`, not the bare left side, so
        // the relation is folded in before the expression leaves the editor.
        let equation = self
            .field
            .build_equations_with(&self.definitions)
            .ok()
            .and_then(|equations| {
                equations
                    .into_iter()
                    .find(|(_, right)| right.is_some())
                    .map(|(left, right)| {
                        canlab_core::add(vec![
                            left,
                            canlab_core::mul(vec![canlab_core::int(-1), right.expect("checked")]),
                        ])
                    })
            });
        let expr = match kind {
            JobKind::Solve => match equation
                .or_else(|| self.field.build_expr_with(&self.definitions).ok())
            {
                Some(expr) => expr,
                None => {
                    let reason = self
                        .field
                        .build_expr_with(&self.definitions)
                        .err()
                        .map(|error| friendly_error(&error))
                        .unwrap_or_else(|| String::from("no equation to solve"));
                    self.status = format!("nothing to send: {reason}");
                    return;
                }
            },
            _ => match self.field.build_expr_with(&self.definitions) {
                Ok(expr) => expr,
                Err(error) => {
                    self.status = format!("nothing to send: {}", friendly_error(&error));
                    return;
                }
            },
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
            // The worker has already used this to decide whether to retire the
            // backend; the panel only shows the answer.
            fatal: _,
        } = result;
        if generation != self.generation {
            self.status = String::from("discarded a stale solver result");
            return;
        }
        match outcome {
            JobOutcome::Value(expr) => {
                self.result_latex = to_latex(&expr);
                self.result_plain = node(&expr).to_string();
                self.approximation = approximation_of(&expr);
                self.status = format!("{label}: done");
                self.push_history(&label, self.result_latex.clone());
            }
            JobOutcome::Solutions(solutions) => {
                let Some(var) = var else {
                    self.status = String::from("solver returned solutions without a variable");
                    return;
                };
                let name = to_latex(&canlab_core::sym(&var.as_str()));
                self.result_latex = match &solutions {
                    canlab_solver::Solutions::None => String::from("\\varnothing"),
                    canlab_solver::Solutions::All => String::from("\\mathbb{R}"),
                    canlab_solver::Solutions::Inequality { .. } => {
                        format!("{name} \\in {}", solutions.to_latex())
                    }
                    _ => solution_lines(&solutions, var).join(",\\quad "),
                };
                self.result_plain = solution_lines(&solutions, var).join(", ");
                self.approximation = match &solutions {
                    canlab_solver::Solutions::Values(values) if values.len() == 1 => {
                        approximation_of(&values[0])
                    }
                    _ => None,
                };
                self.status = format!("{label}: {}", solution_count(&solutions));
                self.solve = SolveState::Solved { var, solutions };
                self.push_history(&label, self.result_latex.clone());
            }
            JobOutcome::Assignments(assignments) => {
                // A system answers per variable, in one list. An empty list is a
                // contradiction — no point satisfies every equation — which is a
                // real answer and must read like "no solutions", not like the
                // solver had nothing to say.
                if assignments.is_empty() {
                    self.result_latex = String::from("\\varnothing");
                    self.result_plain = String::from("no solutions");
                    self.status = format!("{label}: no solutions");
                } else {
                    let parts: Vec<String> = assignments
                        .iter()
                        .map(|(variable, value)| {
                            let name = to_latex(&canlab_core::sym(&variable.as_str()));
                            format!("{name} = {}", to_latex(value))
                        })
                        .collect();
                    self.result_latex = parts.join(",\\quad ");
                    self.result_plain = parts.join(", ");
                    self.status = format!("{label}: {} variable(s)", assignments.len());
                }
                self.solve = SolveState::System { assignments };
                self.push_history(&label, self.result_latex.clone());
            }
            JobOutcome::Declined(why) => {
                self.status = format!("{label}: declined");
                if kind.is_solve() {
                    self.solve = SolveState::Declined { label, why };
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

impl CasApp {
    /// One frame of the whole interface.
    ///
    /// The only entry point on purpose: Bevy calls it from an egui pass, and a
    /// test calls it with a `Ui` it built itself, so there is no second path
    /// through the drawing code to drift from the first.
    fn draw(&mut self, ui: &mut egui::Ui) {
        // Paint a few frames unconditionally before settling into egui's
        // on-demand mode: a window that has never been presented stays blank
        // until some event makes egui repaint.
        if self.paints < 4 {
            self.paints += 1;
            ui.ctx().request_repaint();
        }

        // A restored palette could not be applied in `new`, which has no context.
        if self.theme_pending {
            self.theme_pending = false;
            theme::apply(ui.ctx(), self.palette);
        }

        // Document-level shortcuts are claimed before any panel draws, so a
        // focused editor cannot swallow them on their way past.
        self.shortcuts(ui);
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

        self.autosave(ui);

        if self.jobs.in_flight() > 0 {
            ui.ctx().request_repaint();
        }
    }
}

/// Choose one palette: the look, and the fact that the session changed.
///
/// The palette is part of `session()`, so changing it without asking for a save
/// meant a scheme chosen in Settings was gone at the next start.
fn choose_palette(app: &mut CasApp, palette: &'static theme::Palette) {
    app.palette = palette;
    app.mark_dirty();
}

/// The Settings menu: appearance, in one place.
///
/// Deliberately small — a settings page that drifts into a second control panel
/// is a failure mode of its own. A *palette* is the whole appearance: surfaces,
/// text, accent and the plot's curve colours, so there is nothing else to set.
fn settings(app: &mut CasApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("PALETTE")
            .small()
            .strong()
            .color(crate::theme::weak()),
    );
    ui.label(
        egui::RichText::new("A scheme, not a sticker: surfaces, text and the plot's colours.")
            .small()
            .color(crate::theme::weak()),
    );
    ui.add_space(4.0);

    // Each button is painted *in* the palette it offers, so the choice is made
    // against the real thing rather than against a name.
    egui::Grid::new("palette-grid")
        .num_columns(2)
        .spacing(egui::vec2(6.0, 6.0))
        .show(ui, |ui| {
            for (index, palette) in theme::PALETTES.iter().enumerate() {
                let selected = app.palette.name == palette.name;
                let mut button = egui::Button::new(
                    egui::RichText::new(palette.name)
                        .color(palette.text)
                        .size(13.0),
                )
                .fill(palette.panel)
                .min_size(egui::vec2(150.0, 30.0));
                button = button.stroke(egui::Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected { palette.accent } else { palette.line },
                ));
                let response = ui.add(button);
                let response = response.on_hover_text(format!(
                    "{} · {}",
                    palette.name,
                    palette.mode.label()
                ));
                if response.clicked() {
                    choose_palette(app, palette);
                    theme::apply(ui.ctx(), palette);
                }
                // Two palettes per row.
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });

    ui.add_space(4.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("ACCENT")
                .small()
                .strong()
                .color(crate::theme::weak()),
        );
        let accent = app.palette.accent;
        let (dot, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 6.0, accent);
        for colour in app.palette.curves.iter().skip(1) {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, *colour);
        }
    });
    ui.label(
        egui::RichText::new("The plot cycles through these six colours.")
            .small()
            .color(crate::theme::weak()),
    );
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
                        .color(crate::theme::weak()),
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
                    ui.menu_button("Settings", |ui| settings(app, ui));
                    ui.label(
                        egui::RichText::new("drag the seams · split from a panel menu")
                            .small()
                            .color(crate::theme::weak()),
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
                            .color(crate::theme::weak()),
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

/// `CANLAB_SESSION=none` turns saving off, which is what a screenshot run or a
/// throwaway session wants.
fn sessions_disabled() -> bool {
    std::env::var("CANLAB_SESSION").as_deref() == Ok("none")
}

/// Split pasted text into document lines.
///
/// Three conventions arrive from the outside world: newlines, `\\` between rows
/// of a `cases` block, and the `\begin{cases}` / `\end{cases}` wrapper itself. All
/// three mean "these are separate lines", and none of them is worth preserving as
/// text inside a line.
fn paste_lines(text: &str) -> Vec<String> {
    text.replace("\\begin{cases}", " ")
        .replace("\\end{cases}", " ")
        .split("\\\\")
        .flat_map(|chunk| chunk.split('\n'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Did this line state one of the values in `assignments`?
///
/// Only a line whose one side is the bare symbol counts: `a = 2b` stated `a`, but
/// `a + 1 = 3` is an equation about `a` and stays in the system.
fn states_a_value(
    assignments: &[(SymbolId, Expr)],
    left: &Expr,
    right: Option<&Expr>,
) -> bool {
    // `s = e` states a value when `s` is one of the resolved symbols *and* the
    // page says the same thing: `a = 2b` stated `a = 6`, while `x² + y² = a` only
    // mentions `a` and stays an equation to solve.
    let states = |symbol: SymbolId, value: &Expr| {
        assignments
            .iter()
            .find(|(seen, _)| *seen == symbol)
            .is_some_and(|(_, stated)| substitute_all(value, assignments) == *stated)
    };
    let Some(right) = right else {
        return false;
    };
    match (left.as_symbol(), right.as_symbol()) {
        (Some(symbol), _) => match right.as_symbol() {
            // `s = s`: both sides are the symbol, which states nothing.
            Some(other) if other == symbol => false,
            _ => states(symbol, right),
        },
        (None, Some(symbol)) => states(symbol, left),
        (None, None) => false,
    }
}

/// Replace every symbol in `known` by its value, in order.
fn substitute_all(expression: &Expr, known: &[(SymbolId, Expr)]) -> Expr {
    known
        .iter()
        .fold(expression.clone(), |current, (symbol, value)| {
            canlab_solver::system::substitute(&current, *symbol, value)
        })
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

/// The names that mean another coordinate rather than a constant.
///
/// `z` is the third spatial coordinate and `w` the fourth; a, b, k and t are
/// constants in the notation this tool is used with. The list is a convention,
/// not a proof - which is why a parameter always shows its value on a slider, so
/// an invented value is visible rather than implied.
fn is_coordinate_name(symbol: SymbolId) -> bool {
    matches!(symbol.as_str().as_str(), "z" | "w")
}

/// The decimal beside an exact answer.
///
/// None when the answer is already a whole number, and when it has a free
/// variable — a parametric answer has no single value and inventing one by
/// evaluating at 1 would be a lie with a decimal point.
fn approximation_of(expression: &Expr) -> Option<String> {
    if canlab_core::approx::is_exact_decimal(expression) {
        return None;
    }
    canlab_core::approx::decimal_of(expression, 3).map(|decimal| format!("≈ {decimal}"))
}

/// One line per answer, in the notation the panels use.
///
/// A set of values prints as `x = a` per value, because that is how it is read
/// aloud. A family keeps its parameter (`x = π/6 + 2kπ`), an inequality is
/// membership (`x ∈ (-1, 3)`), and the two degenerate answers are words rather
/// than symbols — `\varnothing` in a monospace row is a puzzle, not an answer.
fn solution_lines(solutions: &canlab_solver::Solutions, var: SymbolId) -> Vec<String> {
    use canlab_solver::Solutions;
    let name = to_latex(&canlab_core::sym(&var.as_str()));
    match solutions {
        Solutions::Values(values) => values
            .iter()
            .map(|value| format!("{name} = {}", to_latex(value)))
            .collect(),
        Solutions::Family { .. } => vec![format!("{name} = {}", solutions.to_latex())],
        Solutions::Inequality { .. } => vec![format!("{name} \\in {}", solutions.to_latex())],
        // `sin x = ½` is two progressions; each is a line of its own, because
        // `x = π/6 + 2kπ, x = 5π/6 + 2kπ` is how it is read aloud.
        Solutions::Union(_) => vec![format!("{name} = {}", solutions.to_latex())],
        Solutions::None => vec![String::from("no solutions")],
        Solutions::All => vec![String::from("every number")],
    }
}

/// How many answers, in words, for the status bar.
fn solution_count(solutions: &canlab_solver::Solutions) -> String {
    use canlab_solver::Solutions;
    match solutions {
        Solutions::Values(values) => match values.len() {
            0 => String::from("no solutions"),
            1 => String::from("1 solution"),
            n => format!("{n} solutions"),
        },
        Solutions::Family { .. } => String::from("a family of solutions"),
        Solutions::Inequality { .. } => String::from("an interval"),
        Solutions::Union(members) => format!("{} families", members.len()),
        Solutions::None => String::from("no solutions"),
        Solutions::All => String::from("every number"),
    }
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
        ToExprError::Definition => String::from("a definition is a name, not a value"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paint one complete frame headlessly.
    ///
    /// This is the closest a test can get to "the window comes up": eframe's
    /// frame object cannot be built outside eframe, but everything the interface
    /// actually does lives in `CasApp::draw`, and a panic in there is exactly
    /// what produces a blank window.
    fn draw_a_frame(app: &mut CasApp, ctx: &egui::Context) {
        frame_with(app, ctx, Vec::new());
    }

    /// Paint one frame with `events` delivered to it.
    fn frame_with(app: &mut CasApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            app.draw(ui);
        });
        // A real backend uploads these; epaint panics if they are dropped
        // unhandled, which is the test harness's job and not a UI bug.
        output.textures_delta.clear();
    }

    /// A key press with modifiers.
    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn ctrl_z_and_ctrl_shift_z_work_through_a_real_frame() {
        // The undo *wiring* — claiming the event before any panel draws — is not
        // exercised by calling `undo_edit` directly.
        let ctx = egui::Context::default();
        let mut app = CasApp::new();
        app.field = MathField::new();
        app.recompute();
        app.note_edit(false);
        app.field.type_str("x");
        app.recompute();
        assert_eq!(app.field.to_latex(), "x");

        frame_with(&mut app, &ctx, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.field.to_latex(), "", "Ctrl+Z did not undo");

        frame_with(
            &mut app,
            &ctx,
            vec![key(
                egui::Key::Z,
                egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            )],
        );
        assert_eq!(app.field.to_latex(), "x", "Ctrl+Shift+Z did not redo");

        frame_with(&mut app, &ctx, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.field.to_latex(), "x");
    }

    #[test]
    fn ctrl_v_pastes_latex_at_the_caret() {
        let ctx = egui::Context::default();
        let mut app = CasApp::new();
        app.field = MathField::new();
        app.recompute();

        frame_with(
            &mut app,
            &ctx,
            vec![egui::Event::Paste(String::from("x^2+1"))],
        );
        assert_eq!(app.field.to_latex(), "x^2+1");
        assert!(app.expr.is_some(), "{:?}", app.error);
    }

    #[test]
    fn the_root_layout_follows_a_window_that_is_reported_too_big() {
        // The first frame of a Bevy window arrives before the scale factor is
        // known, so `content_rect` says 1770×1140 for a 1180×760 window. An
        // `Area` keeps the size it was born with, so that first frame used to
        // leave the dock three columns wide inside a window two-thirds that
        // size, with the variables panel off the right edge.
        let ctx = egui::Context::default();
        let mut app = CasApp::new();
        let pass = |app: &mut CasApp, width: f32, height: f32| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            let input = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            // The rectangle must come from *inside* the pass: a call made after
            // it would return the default and the assertion would pass for the
            // wrong reason.
            let mut laid_out = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(input, |ui| {
                laid_out = crate::draw_root(app, ui.ctx());
            });
            output.textures_delta.clear();
            laid_out
        };
        let big = pass(&mut app, 1770.0, 1140.0);
        assert!((big.width() - 1770.0).abs() < 0.5, "the first pass: {big:?}");
        let laid_out = pass(&mut app, 1180.0, 760.0);
        assert!(
            (laid_out.width() - 1180.0).abs() < 0.5 && (laid_out.height() - 760.0).abs() < 0.5,
            "the interface is laid out in {laid_out:?}, not the window"
        );
    }

    #[test]
    fn a_whole_frame_paints_without_panicking() {
        let ctx = egui::Context::default();
        let mut app = CasApp::new();
        // Three frames: the first builds the font atlas and the layout, the
        // second re-enters with last frame's state, the third is steady state.
        for _ in 0..3 {
            draw_a_frame(&mut app, &ctx);
        }
    }

    #[test]
    fn a_frame_with_a_system_and_a_ghost_bracket_paints_too() {
        let ctx = egui::Context::default();
        let mut app = CasApp::new();
        let mut field = MathField::new();
        field.type_str("(x+1");
        field.new_line();
        field.type_str("y=");
        field.apply_snippet(r"\bar{x}").unwrap();
        app.field = field;
        app.recompute();
        for _ in 0..2 {
            draw_a_frame(&mut app, &ctx);
        }
    }

    #[test]
    fn a_system_solves_every_variable_not_just_the_first() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x=1"),
            String::from("y=x"),
        ])
        .expect("a two-line system");
        app.recompute();
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::System { assignments } => {
                assert_eq!(assignments.len(), 2, "both variables are pinned down");
            }
            other => panic!("expected a system answer, got {other:?}"),
        }
    }

    #[test]
    fn a_three_unknown_system_solves_every_variable() {
        // Three unknowns is the smallest system that exercises the native
        // backend's Gaussian elimination rather than the two-variable shortcut.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x+y+z=6"),
            String::from("x-y=0"),
            String::from("y-z=0"),
        ])
        .expect("a three-line system");
        app.recompute();
        assert_eq!(app.equations.len(), 3);
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::System { assignments } => {
                assert_eq!(assignments.len(), 3, "{assignments:?}");
                for (variable, value) in assignments {
                    assert_eq!(
                        value.to_string(),
                        "2",
                        "{} should be 2",
                        variable.as_str()
                    );
                }
            }
            other => panic!("expected a system answer, got {other:?}"),
        }
    }

    #[test]
    fn a_contradictory_system_says_no_solutions() {
        // `x+y=1` and `x+y=2` cannot both hold. The solver's empty assignment
        // list is a real answer, and the result card must say so.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x+y=1"),
            String::from("x+y=2"),
        ])
        .expect("a contradictory system");
        app.recompute();
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::System { assignments } => {
                assert!(assignments.is_empty(), "{assignments:?}");
            }
            other => panic!("expected an empty system answer, got {other:?}"),
        }
        assert_eq!(app.result_latex, "\\varnothing");
        assert_eq!(app.result_plain, "no solutions");
        assert!(
            app.status.contains("no solutions"),
            "status: {}",
            app.status
        );
    }

    #[test]
    fn solving_a_linear_equation_reaches_the_variables_panel() {
        // The whole path: document -> editor -> worker thread -> panel state.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("2x+6=0").expect("a linear equation");
        app.recompute();
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if matches!(app.solve, SolveState::Solved { .. } | SolveState::Failed(_)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::Solved {
                solutions: canlab_solver::Solutions::Values(values),
                ..
            } => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].to_string(), "-3");
            }
            other => panic!("expected a solution, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_symbol_that_is_not_a_coordinate_is_a_parameter() {
        // `x² + y² = a` is a circle with a radius to turn, not a surface. The
        // structure is identical to `x² + y² = z`, so the difference is the name
        // and nothing else — which is why a parameter shows its value on a slider.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2+y^2=a").expect("a circle with a radius");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.parameters.len(), 1, "note: {}", app.plot_note);
        assert_eq!(app.parameters[0].name.as_str().to_string(), "a");
        assert!(!app.curves.is_empty(), "note: {}", app.plot_note);
        assert!(
            app.third_coordinates.is_empty(),
            "a is a knob, not a coordinate"
        );
    }

    #[test]
    fn a_system_may_state_its_own_parameter_and_still_plot() {
        // { x² + y² = s, s = 1 } is a circle. The second line is what makes the
        // first plottable; treating s as a third coordinate would say "surface"
        // about a system that has just finished saying what s is.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=s"),
            String::from("s=1"),
        ])
        .expect("a system that defines its own parameter");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert!(!app.curves.is_empty(), "plot note: {}", app.plot_note);
        assert_eq!(app.parameters.len(), 1, "note: {}", app.plot_note);
        assert_eq!(app.parameters[0].name.as_str().to_string(), "s");
        assert_eq!(app.parameters[0].value, 1.0);
        assert!(app.third_coordinates.is_empty(), "s is assigned, not a coordinate");
    }
    #[test]
    fn a_fractional_answer_shows_a_decimal_beside_it() {
        // 2x = 1 is x = 1/2, and a reader wants 0.5 without doing the
        // division. An integer answer gets no second form, because it needs none.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("2*x=1").expect("a line");
        app.recompute();
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(app.approximation.as_deref(), Some("≈ 0.5"));

        let mut whole = CasApp::new();
        whole.field = MathField::from_latex("2*x=4").expect("a line");
        whole.recompute();
        whole.start_solve();
        for _ in 0..400 {
            whole.poll_jobs(&ctx);
            if !matches!(whole.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(whole.approximation, None, "2 is already a decimal");
    }
    #[test]
    fn a_two_variable_equation_answers_in_the_chosen_variable() {
        // `x² + y² = 1` used to be declined as "two variables". It has two
        // answers in `x` with `y` as a parameter, and saying so is the whole
        // point of the parametric path — this is what autofill will insert.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2+y^2=1").expect("a circle");
        app.recompute();
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::Solved {
                solutions: canlab_solver::Solutions::Values(values),
                ..
            } => {
                assert_eq!(values.len(), 2, "a circle has two branches in x");
                let latex = app.result_latex.clone();
                assert!(latex.contains("sqrt"), "the result should carry a root: {latex}");
                assert!(
                    !latex.contains("\\frac{1}{4}"),
                    "the square factor should have been taken out: {latex}"
                );
            }
            other => panic!("expected two branches, got {other:?}"),
        }
    }

    #[test]
    fn the_plot_follows_the_document_without_a_button() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2+y^2=1").expect("a circle");
        app.recompute();
        assert_eq!(app.curves.len(), 1, "note: {}", app.plot_note);

        // Nothing to plot any more: the curves go away with the document.
        app.field = MathField::new();
        app.recompute();
        assert!(app.curves.is_empty());
    }

    #[test]
    fn any_variable_can_be_an_axis() {
        // Nothing about the maths knows what the letters are called: `i² + j² = 1`
        // is the same circle as `x² + y² = 1` and must plot without renaming.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("i^2+j^2=1").expect("a circle in i and j");
        app.recompute();
        assert_eq!(app.free_vars.len(), 2);
        assert_eq!(
            app.x_axis.map(|v| v.as_str().to_string()),
            Some(String::from("i"))
        );
        assert_eq!(
            app.y_axis.map(|v| v.as_str().to_string()),
            Some(String::from("j"))
        );
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1, "status: {}", app.plot_note);
    }

    #[test]
    fn swapping_the_axes_still_plots() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex("i^2+j^2=1").expect("a circle");
        app.recompute();
        app.x_axis = Some(SymbolId::intern("j"));
        app.y_axis = Some(SymbolId::intern("i"));
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1, "status: {}", app.plot_note);
    }

    #[test]
    fn a_hand_picked_vertical_axis_survives_a_recompute() {
        // `recompute` runs on every keystroke, so an axis guess that overwrote
        // the vertical one would undo the choice as soon as the user typed.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("p^2+q^2=1").expect("a circle");
        app.recompute();
        app.x_axis = Some(SymbolId::intern("q"));
        app.y_axis = Some(SymbolId::intern("p"));
        app.recompute();
        assert_eq!(
            app.x_axis.map(|v| v.as_str().to_string()),
            Some(String::from("q"))
        );
        assert_eq!(
            app.y_axis.map(|v| v.as_str().to_string()),
            Some(String::from("p"))
        );
    }

    #[test]
    fn an_equation_with_extra_variables_plots_them_as_parameters() {
        // `a` and `b` are not on an axis, so they are parameters. They start at
        // 1 — but the value is a control in the plot overlay, and the caption
        // says what it is actually set to rather than "assumed".
        let mut app = CasApp::new();
        app.field = MathField::from_latex("a*x+b*y=1").expect("an equation in four letters");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1, "plot note: {}", app.plot_note);
        let names: Vec<String> = app
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str().to_string())
            .collect();
        assert_eq!(names, vec![String::from("a"), String::from("b")]);
        assert!(app.parameters.iter().all(|parameter| parameter.value == 1.0));
        assert!(app.plot_note.contains("a = 1"), "plot note: {}", app.plot_note);
    }

    #[test]
    fn a_third_coordinate_does_not_plot_a_slice() {
        // `x² + y² = z` is a surface. It used to plot the circle at `z = 1`,
        // which is a slice of it: the picture looked like the graph of the
        // equation, and the only clue was a `z = 1.00` chip in the overlay.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2+y^2=z").expect("a surface");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert!(app.curves.is_empty(), "note: {}", app.plot_note);
        assert!(
            app.parameters.is_empty(),
            "z is a coordinate, not a knob: {:?}",
            app.parameters
        );
        assert!(app.plot_note.contains('z'), "note: {}", app.plot_note);
        assert!(
            app.plot_note.contains("third coordinate"),
            "the note must say why there is no picture: {}",
            app.plot_note
        );
    }

    #[test]
    fn a_radius_squared_equation_keeps_its_slider() {
        // The guard against over-refusing: `r` is inside a power, so it is a
        // coefficient and the radius belongs on a slider. This is the form the
        // circle is actually written in, and it must keep working.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x^2+y^2=r^2").expect("a circle");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.parameters.len(), 1, "note: {}", app.plot_note);
        assert_eq!(app.parameters[0].name.as_str().to_string(), "r");
        assert!(!app.curves.is_empty(), "note: {}", app.plot_note);
    }

    #[test]
    fn a_line_with_slope_and_intercept_keeps_both_sliders() {
        // `m` is a term of the right-hand side but not a side of its own, so it
        // stays a parameter. If this ever refused, the fix for the surface case
        // would have cost every line with a slider.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("y=k*x+m").expect("a line");
        app.recompute();
        crate::panels::build_curve(&mut app);
        let mut names: Vec<String> = app
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec![String::from("k"), String::from("m")]);
        assert!(!app.curves.is_empty(), "note: {}", app.plot_note);
    }

    #[test]
    fn a_parameter_value_changes_the_picture() {
        // `a*x = 1` is a vertical line at 1/a. Setting `a` to 2 must move it,
        // which is the whole point of the control.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("a*x=1").expect("a line through the origin");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.parameters.len(), 1, "note: {}", app.plot_note);
        let before: Vec<[f64; 2]> = app.curves.iter().flat_map(|c| c.segments.clone()).flatten().collect();
        assert!(!before.is_empty());

        app.parameters[0].value = 2.0;
        crate::panels::build_curve_fitting(&mut app, false);
        let after: Vec<[f64; 2]> = app.curves.iter().flat_map(|c| c.segments.clone()).flatten().collect();
        let mean_x = |points: &[[f64; 2]]| points.iter().map(|p| p[0]).sum::<f64>() / points.len() as f64;
        assert!(
            (mean_x(&before) - 1.0).abs() < 0.2,
            "a = 1 puts the line at x = 1: {}",
            mean_x(&before)
        );
        assert!(
            (mean_x(&after) - 0.5).abs() < 0.2,
            "a = 2 puts the line at x = 0.5: {}",
            mean_x(&after)
        );
    }

    #[test]
    fn a_system_reports_its_intersection_as_a_point() {
        // `x² + y² = 1` with `y = x` meets at (±1/√2, ±1/√2). The plot used to
        // draw a ring of near-zero samples there; now it draws the coordinates.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=1"),
            String::from("y=x"),
        ])
        .expect("a system");
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.intersections.len(), 2, "note: {}", app.plot_note);
        let root = std::f64::consts::FRAC_1_SQRT_2;
        for point in &app.intersections {
            assert!((point.x.abs() - root).abs() < 1e-3, "{point:?}");
            assert!((point.y - point.x).abs() < 1e-6, "on the line: {point:?}");
        }
    }

    #[test]
    fn a_definition_is_plotted_as_its_own_curve() {
        // `f(x) = x²` on its own is a page that defines a function; it should
        // draw that function rather than nothing at all.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("f(x) = x^2").expect("a definition");
        app.recompute();
        assert!(app.expr.is_none(), "a definition is not an expression");
        assert_eq!(app.definitions.len(), 1);
        // The document being only definitions must not switch the plot off: that
        // is what `can_plot` looks at.
        assert!(
            !app.curves.is_empty(),
            "recompute should have drawn it: {}",
            app.plot_note
        );
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1, "plot note: {}", app.plot_note);
        assert!(app.plot_note.contains("definition"), "note: {}", app.plot_note);
        // The curve really is x²: at x = 2 it is near y = 4.
        let points: Vec<[f64; 2]> = app.curves[0].segments.iter().flatten().copied().collect();
        let near_two = points
            .iter()
            .min_by(|a, b| (a[0] - 2.0).abs().total_cmp(&(b[0] - 2.0).abs()))
            .copied()
            .expect("a sample near x = 2");
        assert!((near_two[1] - 4.0).abs() < 0.2, "{near_two:?}");
    }

    #[test]
    fn a_document_of_definitions_shows_them_as_the_result() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("f(x) = x^2+1"),
            String::from("g(x) = f(x)"),
        ])
        .expect("two definitions");
        app.recompute();
        assert_eq!(app.definitions.len(), 2, "{:?}", app.definition_problems);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(
            app.result_latex.contains("f(x) ="),
            "result: {}",
            app.result_latex
        );
        assert_eq!(app.free_vars.len(), 1, "both take x");
    }

    #[test]
    fn a_definition_can_be_used_in_an_equation() {
        // The reader resolves the call, so the equation plotted is the expanded
        // one — and the definition still draws its own curve.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("f(x) = x^2").expect("a definition");
        app.field.new_line();
        app.field.type_str("y=f(2)");
        app.recompute();
        assert_eq!(app.definitions.len(), 1, "{:?}", app.definition_problems);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(!app.equations.is_empty(), "the equation line was read");
        crate::panels::build_curve(&mut app);
        assert!(app.curves.len() >= 2, "note: {}", app.plot_note);
    }

    #[test]
    fn a_definition_is_resolved_before_the_solver_is_asked() {
        // The panel and the variable list are built with the definitions, so the
        // job must be too: `y = f(2)` is `y = 5`, not `y - 2f = 0`.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("f(x) = x^2+1"),
            String::from("y = f(2)"),
        ])
        .expect("a definition and an equation");
        app.recompute();
        assert_eq!(app.equations.len(), 1, "{:?}", app.error);
        app.start_solve();

        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.solve {
            SolveState::Solved {
                solutions: canlab_solver::Solutions::Values(values),
                ..
            } => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].to_string(), "5", "y = f(2) is y = 5");
            }
            other => panic!("expected y = 5, got {other:?}"),
        }
    }

    #[test]
    fn a_definition_of_two_variables_is_not_a_curve() {
        // `g(a, b) = a·b` is a surface. It defines a name — `g(2, 3)` works — but
        // it must not make `a` and `b` look like free variables of the page.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("g(a,b) = a*b").expect("a definition");
        app.recompute();
        assert_eq!(app.definitions.len(), 1, "{:?}", app.definition_problems);
        assert!(app.free_vars.is_empty(), "{:?}", app.free_vars);
        crate::panels::build_curve(&mut app);
        assert!(app.curves.is_empty());
        assert_eq!(app.plot_note, "nothing to plot", "{}", app.plot_note);
    }

    #[test]
    fn a_definition_of_one_variable_is_a_curve_of_that_variable() {
        // `h(t) = t²` plots against `t`, not only against `x`.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("h(t) = t^2").expect("a definition");
        app.recompute();
        assert_eq!(
            app.x_axis.map(|axis| axis.as_str().to_string()),
            Some(String::from("t"))
        );
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1, "note: {}", app.plot_note);
    }

    #[test]
    fn enter_after_a_definition_starts_a_new_question() {
        // A definition is not an equation of a system, so the `cases` brace must
        // not wrap it together with the equations that follow.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("f(x) = x^2").expect("a definition");
        app.recompute();
        assert!(app.field.is_definition_row(app.field.root()));

        crate::panels::press_enter(&mut app, false);
        app.field.type_str("x+y=3");
        app.recompute();
        assert_eq!(app.field.system_count(), 2, "a new question, not a new line");
        assert_eq!(app.definitions.len(), 1);
        assert_eq!(app.equations.len(), 1, "{:?}", app.error);
        let latex = app.field.to_latex();
        assert!(
            !latex.contains("\\begin{cases} f"),
            "the definition is braced: {latex}"
        );
    }

    #[test]
    fn enter_inside_a_system_adds_an_equation() {
        // The other half of the rule: an equation is followed by another equation
        // of the *same* system.
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x=1").expect("an equation");
        app.recompute();
        crate::panels::press_enter(&mut app, false);
        app.field.type_str("y=2");
        app.recompute();
        assert_eq!(app.field.system_count(), 1);
        assert_eq!(app.field.line_count(), 2);
        assert_eq!(app.equations.len(), 2);
    }

    #[test]
    fn undo_takes_back_a_typed_change() {
        let mut app = CasApp::new();
        app.field = MathField::new();
        app.recompute();
        app.note_edit(false);
        app.field.type_str("x");
        app.recompute();
        assert_eq!(app.field.to_latex(), "x");
        app.undo_edit();
        assert_eq!(app.field.to_latex(), "");
        app.redo_edit();
        assert_eq!(app.field.to_latex(), "x");
    }

    #[test]
    fn a_session_round_trips_through_the_app() {
        // The document, the palette and the plot window survive a save and a
        // load — without touching the disk, so it cannot clobber a real session.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=4"),
            String::from("y=x"),
        ])
        .expect("a system");
        app.palette = theme::by_name("Solarized Light").expect("a palette");
        app.recompute();
        app.x_min = -3.5;
        app.x_max = 3.5;
        app.refresh_parameters();

        let session = app.session();
        let text = session.to_text();
        let read = persist::Session::from_text(&text).expect("readable");
        assert_eq!(read, session);

        let mut restored = CasApp::new();
        assert!(restored.apply_session(&read));
        assert_eq!(restored.field.line_count(), 2);
        assert_eq!(restored.palette.name, "Solarized Light");
        assert_eq!((restored.x_min, restored.x_max), (-3.5, 3.5));
    }

    #[test]
    fn a_session_from_before_palettes_still_opens() {
        // Version 1 files carry `theme` and `accent`. They must open in a whole
        // scheme rather than in egui's default look.
        let text = "canlab 1\ntheme light\naccent 201 100 0\nsystem\nline x+1\n";
        let session = persist::Session::from_text(text).expect("readable");
        let mut app = CasApp::new();
        assert!(app.apply_session(&session));
        assert!(!app.palette.is_dark(), "{}", app.palette.name);
        assert_eq!(app.field.to_latex(), "x+1");
    }

    #[test]
    fn a_saved_non_standard_axis_is_restored() {
        // `p` and `q` are not natural axis letters, so only the saved choice can
        // put them there; `recompute` must keep that choice.
        let session = persist::Session {
            document: vec![vec![String::from("p^2+q^2=1")]],
            axes: (Some(String::from("q")), Some(String::from("p"))),
            ..persist::Session::default()
        };
        let mut app = CasApp::new();
        assert!(app.apply_session(&session));
        assert_eq!(
            app.x_axis.map(|v| v.as_str().to_string()),
            Some(String::from("q"))
        );
        assert_eq!(
            app.y_axis.map(|v| v.as_str().to_string()),
            Some(String::from("p"))
        );
    }

    #[test]
    fn a_palette_choice_and_a_parameter_drag_ask_for_a_save() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex("a*x=1").expect("a line");
        app.recompute();
        assert_eq!(app.parameters.len(), 1);

        app.dirty_since = None;
        app.parameters[0].value = 2.0;
        crate::panels::plot_changed(&mut app);
        assert!(
            app.dirty_since.is_some(),
            "a parameter drag is part of the session"
        );

        app.dirty_since = None;
        choose_palette(&mut app, theme::by_name("Nord").expect("a palette"));
        assert!(
            app.dirty_since.is_some(),
            "a palette choice is part of the session"
        );
    }

    #[test]
    fn the_session_reflects_a_palette_choice_and_a_parameter_value() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex("a*x=1").expect("a line");
        app.recompute();
        app.parameters[0].value = 2.5;
        crate::panels::plot_changed(&mut app);
        choose_palette(&mut app, theme::by_name("Nord").expect("a palette"));

        let session = app.session();
        assert_eq!(session.palette, "Nord");
        assert_eq!(
            session.parameters,
            vec![(String::from("a"), 2.5)],
            "the parameter value travels with the session"
        );
    }

    #[test]
    fn a_session_that_cannot_be_read_says_so() {
        let mut app = CasApp::new();
        let broken = persist::Session {
            document: vec![vec![String::from("\\frac{")]],
            ..persist::Session::default()
        };
        assert!(!app.apply_session(&broken), "the document cannot be built");
        app.load_session(&broken);
        assert!(
            app.status.contains("could not be read"),
            "status: {}",
            app.status
        );
    }

    #[test]
    fn a_system_plots_one_curve_per_equation() {
        // `x+y=3` is not a function of x, so this is the implicit path: each
        // line of the system becomes the set of points where it holds.
        let mut app = CasApp::new();
        let mut field = MathField::new();
        field.type_str("x+y=3");
        field.new_line();
        field.type_str("2x-y=0");
        app.field = field;
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert!(app.curves.len() >= 2, "status: {}", app.plot_note);
        assert!(
            app.plot_note.contains("intersection"),
            "note: {}",
            app.plot_note
        );
        assert!(app.curves.iter().all(|curve| !curve.segments.is_empty()));
    }

    #[test]
    fn several_systems_all_reach_the_plot_but_do_not_intersect() {
        // Built from LaTeX rather than typed: after `^` the script absorbs
        // everything until the cursor is moved out, exactly as in Abitti, so a
        // typed `x^2+y^2=4` would be `x^{2+y^{2=4}}`.
        //
        // The two lines are in **different systems**, so they are not
        // simultaneous: `y = x` does not meet the circle of the other block, and
        // pairing them would draw a crossing that answers neither question. This
        // test used to assert the opposite, which is how the bug survived.
        let mut app = CasApp::new();
        let mut field = MathField::from_latex("x^2+y^2=4").expect("a circle");
        field.new_system();
        field.type_str("y=x");
        app.field = field;
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert!(app.curves.len() >= 2, "status: {}", app.plot_note);
        assert!(
            app.intersections.is_empty(),
            "curves from different systems were paired: {} intersections",
            app.intersections.len()
        );
    }

    #[test]
    fn a_system_built_from_latex_plots_every_line() {
        let mut app = CasApp::new();
        let field = MathField::from_latex_lines(&[
            String::from("x+y=3"),
            String::from("2x-y=0"),
        ])
        .expect("two lines");
        app.field = field;
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert!(app.curves.len() >= 2, "status: {}", app.plot_note);
        assert!(
            app.plot_note.contains("intersection"),
            "note: {}",
            app.plot_note
        );
    }

    #[test]
    fn a_plain_function_still_plots_as_a_function() {
        let mut app = CasApp::new();
        let mut field = MathField::new();
        field.type_str("x^2");
        app.field = field;
        app.recompute();
        crate::panels::build_curve(&mut app);
        assert_eq!(app.curves.len(), 1);
        assert!(!app.curves[0].segments.is_empty());
    }

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
    fn a_half_typed_equation_is_not_sent_to_the_solver() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex("x=1").expect("an equation");
        app.recompute();
        assert_eq!(app.equations.len(), 1);

        // `/` opens an empty fraction: a legal editing state, not a document.
        app.field.type_str("/");
        app.recompute();
        assert!(app.is_typing());
        assert!(
            app.equations.is_empty(),
            "the equations still describe the previous complete parse"
        );
        app.start_solve();
        assert!(
            app.status.starts_with("nothing to send"),
            "status: {}",
            app.status
        );
        assert_eq!(app.jobs.in_flight(), 0, "no job could have been queued");
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

    #[test]
    fn an_inequality_typed_into_the_field_is_answered() {
        // The whole trip: keys into the field, the relation reader, the job, the
        // sign chart, the result panel. The dispatch was compile-checked only
        // before this test, and a compile is not an answer.
        let mut app = CasApp::new();
        app.field = MathField::new();
        app.field.type_str("|x-1|<2");
        app.recompute();
        solve_and_wait(&mut app);
        match &app.solve {
            SolveState::Solved { solutions, .. } => {
                let latex = solutions.to_latex();
                assert!(
                    latex.contains("-1") && latex.contains("3"),
                    "|x-1| < 2 is the interval (-1, 3), got {latex}"
                );
            }
            other => panic!("the field's inequality was not answered: {other:?}"),
        }
    }

    /// Press Solve and wait for the background answer.
    fn solve_and_wait(app: &mut CasApp) {
        app.start_solve();
        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_jobs(&ctx);
            if !matches!(app.solve, SolveState::Idle) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the solve never answered: {}", app.status);
    }

    #[test]
    fn a_circle_pinned_by_two_assignments_is_solved_not_declined() {
        // The exercise from the screenshot: a circle whose radius the page states,
        // through one intermediate line so the substitution has to be in order.
        // Before this, the page was sent as a four-unknown nonlinear system, the
        // backend declined, and the result panel showed the document's own
        // description back as if that were the answer.
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=a"),
            String::from("a=2b"),
            String::from("b=3"),
        ])
        .expect("a three-line document");
        app.recompute();
        solve_and_wait(&mut app);
        match &app.solve {
            SolveState::Solved { var, solutions } => {
                assert_eq!(var.as_str(), "x");
                let latex = solutions.to_latex();
                assert!(
                    latex.contains("6") && latex.contains("y^{2}"),
                    "the circle has radius √6 and the answer is in x and y: {latex}"
                );
                // Two branches, and the constant is the substituted `a = 2·3`.
                assert_eq!(solutions.to_latex().matches("sqrt").count(), 2, "{latex}");
            }
            other => panic!("expected the parametric answer, got {other:?}"),
        }
    }

    #[test]
    fn the_page_states_its_own_parameters_in_dependency_order() {
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=a"),
            String::from("a=2b"),
            String::from("b=3"),
        ])
        .expect("a three-line document");
        app.recompute();
        let stated: Vec<(String, String)> = app
            .document_assignments()
            .into_iter()
            .map(|(symbol, value)| (symbol.as_str().to_string(), to_latex(&value)))
            .collect();
        // `a = 2b` is only a number once `b` is one, so both are resolved and `a`
        // is 6 rather than the `2b` that was typed.
        assert_eq!(
            stated,
            vec![
                (String::from("b"), String::from("3")),
                (String::from("a"), String::from("6")),
            ]
        );
        // And the plot's parameter list takes the page's value over the default.
        let a = app
            .parameters
            .iter()
            .find(|parameter| parameter.name.as_str() == "a")
            .expect("a is still a parameter");
        assert_eq!(a.value, 6.0);
    }

    #[test]
    fn an_equation_is_not_an_assignment_of_the_symbol_on_its_side() {
        // `x² + y² = a` mentions `a` but states nothing about it: the value side
        // mentions an axis, so `a` stays a parameter with a slider. Reading it as
        // `a = x² + y²` would turn the circle into a curve of `a`.
        let mut app = CasApp::new();
        app.field =
            MathField::from_latex_lines(&[String::from("x^2+y^2=a")]).expect("one line");
        app.recompute();
        assert!(
            app.document_assignments().is_empty(),
            "{:?}",
            app.document_assignments()
        );
        let a = app
            .parameters
            .iter()
            .find(|parameter| parameter.name.as_str() == "a")
            .expect("a is a parameter");
        assert_eq!(a.value, 1.0);
    }

    #[test]
    fn a_line_that_merely_mentions_a_stated_symbol_is_still_an_equation() {
        // `b = 3` states `b`; `x² + y² = b` is about the circle. The first line of
        // the pair must not be swallowed as "the line that stated b".
        let mut app = CasApp::new();
        app.field = MathField::from_latex_lines(&[
            String::from("x^2+y^2=b"),
            String::from("b=3"),
        ])
        .expect("two lines");
        app.recompute();
        solve_and_wait(&mut app);
        match &app.solve {
            SolveState::Solved { solutions, .. } => {
                let latex = solutions.to_latex();
                assert!(latex.contains("3"), "{latex}");
            }
            other => panic!("expected the circle in x, got {other:?}"),
        }
    }
}
