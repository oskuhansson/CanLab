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
mod shot;
mod theme;
mod undo;

use std::time::Instant;

use canlab_core::{Expr, ExprNode, Hold, SymbolId, expr::node};
use canlab_input::{Definitions, MathField, Node, ToExprError};
use canlab_plot::{Curve, Intersection};
use canlab_tex::to_latex;
use eframe::egui;

use jobs::{Job, JobKind, JobOp, JobOutcome, JobResult, Jobs};
use layout::{Dock, PanelKind, Workspace};


fn main() -> eframe::Result<()> {
    // The GL renderer is only for screenshots; see the feature in Cargo.toml.
    // `cfg!` would compile both arms, and `Renderer::Glow` does not exist in a
    // wgpu-only build.
    #[cfg(feature = "glow")]
    let renderer = eframe::Renderer::Glow;
    #[cfg(not(feature = "glow"))]
    let renderer = eframe::Renderer::default();

    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
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
#[derive(Default, Debug)]
pub enum SolveState {
    #[default]
    Idle,
    Solved {
        var: SymbolId,
        values: Vec<Expr>,
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
    /// Values for the variables that are not on an axis.
    parameters: Vec<Parameter>,
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
    /// A pending screenshot, when `CANLAB_SHOT` asks for one.
    shot: shot::Shot,
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
            parameters: Vec::new(),
            intersections: Vec::new(),
            undo: undo::Undo::new(),
            dirty_since: None,
            // A test that restored the last session would depend on whichever
            // document the developer last had open — and would overwrite it.
            sessions: !cfg!(test),
            theme_pending: false,
            shot: shot::Shot::from_env(),
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
        if self.is_typing() {
            return;
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

    /// Recompute the parameter list: every free variable that is not on an axis.
    ///
    /// Values the user has set are kept; a new variable starts at 1, which is
    /// what it used to be forced to whether anyone wanted it or not.
    pub fn refresh_parameters(&mut self) {
        let axes: Vec<SymbolId> = [self.x_axis, self.y_axis]
            .into_iter()
            .flatten()
            .collect();
        let mut next: Vec<Parameter> = Vec::new();
        for variable in &self.free_vars {
            if axes.contains(variable) {
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
    /// Called after every re-parse. A variable that no longer exists cannot stay
    /// on an axis, and a document with two named variables should get both axes
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
        if self.y_axis != vertical {
            self.y_axis = vertical;
        }
    }

    // ---------------------------------------------------------------- editing

    /// Record the document before an edit, so it can be taken back.
    ///
    /// Every path that mutates the field calls this first: the keyboard handler,
    /// the palette, paste, and loading a history entry.
    pub fn note_edit(&mut self, structural: bool) {
        self.undo.checkpoint(&self.field, structural);
        self.dirty_since = Some(Instant::now());
    }

    /// Undo one step.
    pub fn undo_edit(&mut self) {
        if self.undo.undo(&mut self.field) {
            self.recompute();
            self.dirty_since = Some(Instant::now());
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
            self.dirty_since = Some(Instant::now());
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
        if self.apply_session(&session) {
            self.status = String::from("restored the last session");
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

    fn start_solve(&mut self) {
        // A document with several equations is a *system*: solve it as one, or
        // the answer would only cover the first line.
        if self.equations.len() > 1 {
            let equations: Vec<Expr> = self
                .equations
                .iter()
                .map(|(left, right)| match right {
                    Some(right) => canlab_core::add(vec![
                        left.clone(),
                        canlab_core::mul(vec![canlab_core::int(-1), right.clone()]),
                    ]),
                    None => left.clone(),
                })
                .collect();
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
        // Send the field's own tree. Re-parsing `to_latex()` here would be a
        // second, subtly different interpretation of what the user built.
        //
        // Solve wants an equation `left − right = 0`, not the bare left side, so
        // the relation is folded in before the expression leaves the editor.
        let equation = self.field.build_equations().ok().and_then(|equations| {
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
            JobKind::Solve => match equation.or_else(|| self.field.build_expr().ok()) {
                Some(expr) => expr,
                None => {
                    let reason = self
                        .field
                        .build_expr()
                        .err()
                        .map(|error| friendly_error(&error))
                        .unwrap_or_else(|| String::from("no equation to solve"));
                    self.status = format!("nothing to send: {reason}");
                    return;
                }
            },
            _ => match self.field.build_expr() {
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
            JobOutcome::Assignments(assignments) => {
                // A system answers per variable, in one list.
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

impl eframe::App for CasApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

impl CasApp {
    /// One frame of the whole interface.
    ///
    /// Split out of [`eframe::App::ui`] so a test can drive it: constructing a
    /// real `eframe::Frame` is not possible outside eframe, but a `Ui` is.
    fn draw(&mut self, ui: &mut egui::Ui) {
        // Paint a few frames unconditionally before settling into egui's
        // on-demand mode: a window that has never been presented stays blank
        // until some event makes egui repaint. The window size, meanwhile, is
        // decided at creation (`with_inner_size`) and left alone — creating it
        // maximised, or resizing it afterwards, leaves a stale framebuffer.
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

        if self.shot.pending() {
            self.shot.request(ui.ctx(), self.paints);
        }
        self.autosave(ui);

        if self.jobs.in_flight() > 0 {
            ui.ctx().request_repaint();
        }
    }
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
                    app.palette = palette;
                    theme::apply(ui.ctx(), palette);
                }
                // Three swatches under the name: surface, accent, first curve.
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

fn free_symbols(expr: &Expr, out: &mut Vec<SymbolId>) {    match &**expr {
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
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.draw(ui);
        });
        // A real backend uploads these; epaint panics if they are dropped
        // unhandled, which is the test harness's job and not a UI bug.
        output.textures_delta.clear();
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
            SolveState::Solved { values, .. } => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].to_string(), "-3");
            }
            other => panic!("expected a solution, got {other:?}"),
        }
    }

    #[test]
    fn a_two_variable_equation_is_declined_with_a_reason() {
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
            SolveState::Declined { why, .. } => {
                assert!(why.contains("one variable"), "why: {why}");
            }
            other => panic!("expected a decline, got {other:?}"),
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
    fn several_systems_all_reach_the_plot() {
        // Built from LaTeX rather than typed: after `^` the script absorbs
        // everything until the cursor is moved out, exactly as in Abitti, so a
        // typed `x^2+y^2=4` would be `x^{2+y^{2=4}}`.
        let mut app = CasApp::new();
        let mut field = MathField::from_latex("x^2+y^2=4").expect("a circle");
        field.new_system();
        field.type_str("y=x");
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
