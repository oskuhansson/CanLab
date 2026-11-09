//! The pure layout model: a field becomes a [`DisplayList`], and nothing here
//! knows what a window is.
//!
//! # Baseline rows
//!
//! Every row is laid out as a **baseline row**: a horizontal sequence of boxes
//! that share one baseline. A box reports how far it reaches above that baseline
//! (its *ascent*) and below it (its *descent*), and the row takes the maximum of
//! each. Nothing else about vertical placement is global. That single rule
//! explains the two behaviours people notice first: `\frac{1}{2}+x` keeps the
//! `+x` on the same baseline as the fraction, and a row that contains a tall
//! fraction grows both upwards and downwards rather than pushing its text down.
//!
//! The measurements are relative to the box's own baseline, with `y` growing
//! downward, so a box's top is `-ascent` and its bottom is `descent`.
//!
//! # A fraction is a box, not a line
//!
//! A fraction measures its numerator and denominator rows independently, makes
//! itself `max(num.width, den.width) + 2 * pad` wide, and reports one ascent
//! reaching the top of the numerator and one descent reaching the bottom of the
//! denominator. It is then just another box in its parent row. Every construct
//! here follows that pattern — root, group, binomial, accent — which is why
//! nesting works without a layout pass that knows what it is nesting. There is
//! no "fraction line" concept anywhere above this paragraph; the rule is one
//! `Item::Rule` inside the fraction's own body.
//!
//! # Superscripts are siblings
//!
//! [`cas_input`] stores `x^2` as two siblings, `Node::Text("x")` followed by
//! `Node::Sup(...)`, because a decoration may exist with no base at all (typing
//! `^` on an empty field). A row therefore places a script as a box whose
//! baseline is *shifted* by `±0.45 · size` rather than aligned to the row
//! baseline, and a script with no preceding box is still a valid, placeable
//! item. Adjacent scripts are stacked in one column so `x_1^2` reads as a pair
//! of scripts on the `x` rather than `x_1{}^2`.
//!
//! # Carets are an ordered list
//!
//! [`DisplayList::carets`] contains exactly one rect per
//! [`MathField::positions`] entry, in that same depth-first order. The widget
//! zips the two lists by index and never recomputes a cursor. The layout walk
//! mirrors `cas_input`'s own position collector, including its one quirk: there
//! is no cursor position *between* a base and its script, so no caret is emitted
//! there either. Keeping that order is the contract the interactive layer rests
//! on, and it is why a click can be turned into a structural cursor at all.

use cas_input::{AccentKind, Delim, MathField, Node, Row};

// --------------------------------------------------------------------- geometry

/// A point in display-list coordinates.
///
/// `x` grows to the right and **`y` grows downward**, matching the screen
/// convention every painting API uses. A position *above* the baseline therefore
/// has a smaller `y`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos2 {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate; grows downward.
    pub y: f32,
}

impl Pos2 {
    /// The origin.
    pub const ZERO: Pos2 = Pos2 { x: 0.0, y: 0.0 };

    /// Construct a point.
    pub const fn new(x: f32, y: f32) -> Pos2 {
        Pos2 { x, y }
    }
}

/// A size, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// Width.
    pub x: f32,
    /// Height.
    pub y: f32,
}

impl Vec2 {
    /// Construct a size.
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
}

/// An axis-aligned rectangle, given by its top-left corner and its size.
///
/// `y` grows downward, so `y` is the **top** edge and `y + h` the bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width; always non-negative for rects this crate produces.
    pub w: f32,
    /// Height; always non-negative for rects this crate produces.
    pub h: f32,
}

impl Rect {
    /// Construct a rectangle from its top-left corner and its size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// The right edge, `x + w`.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge, `y + h`.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// The horizontal centre.
    pub fn center_x(&self) -> f32 {
        self.x + self.w * 0.5
    }
}

// ----------------------------------------------------------------- measurement

/// Metrics for one run of text, in pixels.
///
/// `ascent + descent` is the height of the text's box; the baseline sits between
/// them. An implementation is free to approximate (the egui one uses 80 %/20 %
/// of the laid-out height, which is good enough for horizontal rules and script
/// placement), but it must be *consistent*: the same metrics are used to place
/// the text and to size the structures around it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// Advance width of the run.
    pub width: f32,
    /// Distance from the baseline to the top of the text's box.
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the text's box.
    pub descent: f32,
}

impl Metrics {
    /// The full height of the text's box.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// The font, abstracted to the one operation layout needs.
///
/// Layout never asks for a glyph's shape, only for its box; that is why the whole
/// engine is testable with a fake font and why the egui adapter is short.
pub trait Measure {
    /// Metrics for `text` rendered at `size` px.
    fn measure(&self, text: &str, size: f32) -> Metrics;
}

// ---------------------------------------------------------------------- style

/// The knobs the layout reads. All distances are in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Size of the root row's text.
    pub base_size: f32,
    /// Scripts are this fraction of their base size …
    pub script_scale: f32,
    /// … but never smaller than this.
    pub min_script_size: f32,
    /// Vertical gap between stacked rows and between a rule and its content.
    pub line_gap: f32,
    /// Thickness of fraction bars, radical overlines and accent rules.
    pub rule_thickness: f32,
    /// Padding around structures: fraction rule overhang, radical gap, group
    /// breathing room, empty-slot margin.
    pub pad: f32,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            base_size: 22.0,
            script_scale: 0.7,
            min_script_size: 10.0,
            line_gap: 2.0,
            rule_thickness: 1.0,
            pad: 2.0,
        }
    }
}

impl Style {
    /// The size a script of a base rendered at `base` gets.
    pub fn script_size(&self, base: f32) -> f32 {
        (base * self.script_scale).max(self.min_script_size)
    }

    /// How far a superscript rises (or a subscript sinks) above/below its base.
    pub fn script_raise(&self, base: f32) -> f32 {
        base * 0.45
    }

    /// Height of the math axis above the baseline: the line a fraction bar and a
    /// delimiter's centre sit on. A quarter of the base size approximates the
    /// x-height of most fonts.
    pub fn axis(&self, size: f32) -> f32 {
        size * 0.25
    }

    /// Side length of the box drawn for an empty slot.
    pub fn empty_box(&self, size: f32) -> f32 {
        size * 0.6
    }
}

// ---------------------------------------------------------------- display list

/// One drawable thing.
///
/// Coordinates are absolute display-list pixels: `(0, 0)` is the top-left of
/// [`DisplayList::size`] and the first baseline is at [`DisplayList::baseline`].
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A run of text whose **baseline-left** corner is `pos`.
    Text {
        /// Baseline-left position.
        pos: Pos2,
        /// The characters to draw.
        text: String,
        /// Font size in pixels.
        size: f32,
        /// Whether the run should be slanted (variables are; digits and
        /// operators are not).
        italic: bool,
    },
    /// A filled horizontal bar: a fraction rule, a radical overline or an
    /// accent rule. Given as a rect so it stays crisp at any thickness.
    Rule {
        /// The bar's rectangle; `h` is the rule thickness.
        rect: Rect,
    },
    /// A single glyph scaled to the height of the structure it wraps: `( ) [ ]
    /// { } | ‖` and, in this crate, the radical sign.
    Delim {
        /// Baseline-left position of the stretched glyph.
        pos: Pos2,
        /// The glyph.
        ch: char,
        /// The stretched font size.
        size: f32,
    },
    /// A box marking an empty slot. A slot with no content is a legal, clickable
    /// object in the input model, so it has to be visible.
    Placeholder {
        /// The box to outline.
        rect: Rect,
    },
    /// A caret line. One of these is emitted per
    /// [`MathField::positions`] entry, in the same order; a renderer normally
    /// draws only the entry that matches the field's current cursor.
    Caret {
        /// The caret's rectangle. Its width is zero: the line has no advance, and
        /// a renderer is expected to give it a stroke width when painting.
        rect: Rect,
    },
}

impl Item {
    /// Move this item by `(dx, dy)`.
    fn shift(&mut self, dx: f32, dy: f32) {
        match self {
            Item::Text { pos, .. } | Item::Delim { pos, .. } => {
                pos.x += dx;
                pos.y += dy;
            }
            Item::Rule { rect } | Item::Placeholder { rect } | Item::Caret { rect } => {
                rect.x += dx;
                rect.y += dy;
            }
        }
    }
}

/// A laid-out field: everything needed to paint it, with no font in sight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    /// Total size of the content, excluding any widget padding the caller adds.
    pub size: Vec2,
    /// The drawable items, in painting order. Includes every caret as an
    /// [`Item::Caret`] so the list is self-describing; a normal renderer skips
    /// those and draws the current one from [`DisplayList::carets`] instead.
    pub items: Vec<Item>,
    /// One caret rect per position, in the same order as
    /// [`MathField::positions`]. This is a contract: the caller zips this vector
    /// with that one by index.
    pub carets: Vec<Rect>,
    /// The y coordinate of the root row's baseline.
    pub baseline: f32,
}

impl DisplayList {
    /// The number of caret positions laid out.
    pub fn caret_count(&self) -> usize {
        self.carets.len()
    }
}

// ------------------------------------------------------------------ internals

/// The strut: a lowercase letter, measured to give an empty row a real height.
const STRUT: &str = "x";

/// A fragment under construction, in coordinates relative to its own baseline
/// (`y == 0`). Items above the baseline have negative `y`.
#[derive(Debug, Default)]
struct Frag {
    width: f32,
    ascent: f32,
    descent: f32,
    items: Vec<Item>,
    carets: Vec<Rect>,
}

impl Frag {
    fn new() -> Frag {
        Frag::default()
    }

    /// Append `child` positioned at `(dx, dy)` relative to this fragment's
    /// origin/baseline, growing this fragment's ascent and descent to fit it.
    fn place(&mut self, child: Frag, dx: f32, dy: f32) {
        self.ascent = self.ascent.max(child.ascent - dy);
        self.descent = self.descent.max(child.descent + dy);
        for mut item in child.items {
            item.shift(dx, dy);
            self.items.push(item);
        }
        for mut caret in child.carets {
            caret.x += dx;
            caret.y += dy;
            self.carets.push(caret);
        }
    }
}

/// A caret recorded during row assembly.
///
/// Carets at the row's own offsets cannot be turned into rects until the row's
/// final ascent is known, so they are kept as an x coordinate and resolved by
/// [`RowBuild::finish`]. Carets inside child rows are already rects.
#[derive(Debug)]
enum CaretSlot {
    /// An offset in the row being built.
    Local(f32),
    /// A caret inside a placed child row.
    Fixed(Rect),
}

/// A row being assembled. Mirrors [`Frag`], except that its carets are still
/// unresolved [`CaretSlot`]s; the order of the slots is the depth-first position
/// order of the row.
#[derive(Debug, Default)]
struct RowBuild {
    width: f32,
    ascent: f32,
    descent: f32,
    items: Vec<Item>,
    slots: Vec<CaretSlot>,
}

impl RowBuild {
    /// Place a child fragment at `(dx, dy)` relative to the row baseline.
    fn place(&mut self, child: Frag, dx: f32, dy: f32) {
        self.ascent = self.ascent.max(child.ascent - dy);
        self.descent = self.descent.max(child.descent + dy);
        for mut item in child.items {
            item.shift(dx, dy);
            self.items.push(item);
        }
        for mut caret in child.carets {
            caret.x += dx;
            caret.y += dy;
            self.slots.push(CaretSlot::Fixed(caret));
        }
    }

    /// Resolve local caret slots against the finished row's vertical extent.
    fn finish(self) -> Frag {
        let RowBuild {
            width,
            ascent,
            descent,
            items,
            slots,
        } = self;
        let carets = slots
            .into_iter()
            .map(|slot| match slot {
                CaretSlot::Local(x) => Rect::new(x, -ascent, 0.0, ascent + descent),
                CaretSlot::Fixed(rect) => rect,
            })
            .collect();
        Frag {
            width,
            ascent,
            descent,
            items,
            carets,
        }
    }
}

/// True for the two decoration nodes, which a row positions itself (they are not
/// standalone boxes at the row baseline).
fn is_script(node: &Node) -> bool {
    matches!(node, Node::Sup(_) | Node::Sub(_))
}

/// The two halves of a scaled delimiter pair.
fn delim_chars(delim: Delim) -> (char, char) {
    match delim {
        Delim::Paren => ('(', ')'),
        Delim::Bracket => ('[', ']'),
        Delim::Abs => ('|', '|'),
        Delim::Brace => ('{', '}'),
    }
}

/// Split a text run into italic and upright pieces.
///
/// Letters become italic (that is the math convention) and digits, dots and
/// operators stay upright. Splitting rather than picking one style for the whole
/// node matters because `cas-input` glues word characters together, so `2x` is a
/// single `Text` node.
fn text_runs(text: &str) -> Vec<(String, bool)> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    for ch in text.chars() {
        let italic = ch.is_alphabetic();
        match runs.last_mut() {
            Some((run, same)) if *same == italic => run.push(ch),
            _ => runs.push((ch.to_string(), italic)),
        }
    }
    runs
}

/// The display text for a `Node::Sym` command token.
///
/// [`cas_input`] stores commands as LaTeX (`\alpha`, `\sin`, `\text{alp}`),
/// which is the right storage format and the wrong thing to paint. The table is
/// deliberately small and total: an unknown command is shown as its own name
/// rather than disappearing, so nothing a user can type becomes invisible.
fn sym_display(sym: &str) -> (String, bool) {
    if let Some(inner) = sym.strip_prefix("\\text{")
        && let Some(inner) = inner.strip_suffix('}')
    {
        return (inner.to_owned(), false);
    }

    let mapped = match sym {
        // Lowercase Greek is italic, as in print.
        "\\alpha" => Some(("α", true)),
        "\\beta" => Some(("β", true)),
        "\\gamma" => Some(("γ", true)),
        "\\delta" => Some(("δ", true)),
        "\\epsilon" => Some(("ε", true)),
        "\\varepsilon" => Some(("ε", true)),
        "\\zeta" => Some(("ζ", true)),
        "\\eta" => Some(("η", true)),
        "\\theta" => Some(("θ", true)),
        "\\vartheta" => Some(("ϑ", true)),
        "\\iota" => Some(("ι", true)),
        "\\kappa" => Some(("κ", true)),
        "\\lambda" => Some(("λ", true)),
        "\\mu" => Some(("μ", true)),
        "\\nu" => Some(("ν", true)),
        "\\xi" => Some(("ξ", true)),
        "\\pi" => Some(("π", true)),
        "\\rho" => Some(("ρ", true)),
        "\\sigma" => Some(("σ", true)),
        "\\tau" => Some(("τ", true)),
        "\\upsilon" => Some(("υ", true)),
        "\\phi" => Some(("φ", true)),
        "\\varphi" => Some(("φ", true)),
        "\\chi" => Some(("χ", true)),
        "\\psi" => Some(("ψ", true)),
        "\\omega" => Some(("ω", true)),
        // Uppercase Greek is upright.
        "\\Gamma" => Some(("Γ", false)),
        "\\Delta" => Some(("Δ", false)),
        "\\Theta" => Some(("Θ", false)),
        "\\Lambda" => Some(("Λ", false)),
        "\\Xi" => Some(("Ξ", false)),
        "\\Pi" => Some(("Π", false)),
        "\\Sigma" => Some(("Σ", false)),
        "\\Upsilon" => Some(("Υ", false)),
        "\\Phi" => Some(("Φ", false)),
        "\\Psi" => Some(("Ψ", false)),
        "\\Omega" => Some(("Ω", false)),
        // Relations and operators.
        "\\le" | "\\leq" => Some(("≤", false)),
        "\\ge" | "\\geq" => Some(("≥", false)),
        "\\ne" | "\\neq" => Some(("≠", false)),
        "\\approx" => Some(("≈", false)),
        "\\equiv" => Some(("≡", false)),
        "\\sim" => Some(("∼", false)),
        "\\propto" => Some(("∝", false)),
        "\\pm" => Some(("±", false)),
        "\\times" => Some(("×", false)),
        "\\cdot" => Some(("·", false)),
        "\\infty" => Some(("∞", false)),
        "\\partial" => Some(("∂", false)),
        "\\nabla" => Some(("∇", false)),
        "\\sum" => Some(("∑", false)),
        "\\prod" => Some(("∏", false)),
        "\\int" => Some(("∫", false)),
        "\\rightarrow" => Some(("→", false)),
        "\\Rightarrow" => Some(("⇒", false)),
        "\\leftrightarrow" => Some(("↔", false)),
        "\\Leftrightarrow" => Some(("⇔", false)),
        "\\in" => Some(("∈", false)),
        "\\notin" => Some(("∉", false)),
        "\\subset" => Some(("⊂", false)),
        "\\cup" => Some(("∪", false)),
        "\\cap" => Some(("∩", false)),
        "\\setminus" => Some(("∖", false)),
        "\\forall" => Some(("∀", false)),
        "\\exists" => Some(("∃", false)),
        "\\angle" => Some(("∠", false)),
        "\\perp" => Some(("⊥", false)),
        "\\ldots" => Some(("…", false)),
        "\\circ" => Some(("∘", false)),
        "\\uparrow" => Some(("↑", false)),
        "\\downarrow" => Some(("↓", false)),
        "\\nearrow" => Some(("↗", false)),
        "\\searrow" => Some(("↘", false)),
        "\\varnothing" => Some(("∅", false)),
        "\\neg" => Some(("¬", false)),
        "\\wedge" => Some(("∧", false)),
        "\\vee" => Some(("∨", false)),
        // Function names stay upright; that is the whole point of `\sin`.
        "\\sin" => Some(("sin", false)),
        "\\cos" => Some(("cos", false)),
        "\\tan" => Some(("tan", false)),
        "\\cot" => Some(("cot", false)),
        "\\sec" => Some(("sec", false)),
        "\\csc" => Some(("csc", false)),
        "\\arcsin" => Some(("arcsin", false)),
        "\\arccos" => Some(("arccos", false)),
        "\\arctan" => Some(("arctan", false)),
        "\\sinh" => Some(("sinh", false)),
        "\\cosh" => Some(("cosh", false)),
        "\\tanh" => Some(("tanh", false)),
        "\\ln" => Some(("ln", false)),
        "\\log" => Some(("log", false)),
        "\\exp" => Some(("exp", false)),
        "\\max" => Some(("max", false)),
        "\\min" => Some(("min", false)),
        "\\det" => Some(("det", false)),
        "\\gcd" => Some(("gcd", false)),
        "\\lim" => Some(("lim", false)),
        _ => None,
    };

    match mapped {
        Some((text, italic)) => (text.to_owned(), italic),
        None => match sym.strip_prefix('\\') {
            Some(name) => (name.to_owned(), false),
            None => (sym.to_owned(), false),
        },
    }
}

// ------------------------------------------------------------------- the engine

/// One layout pass. Holds the two inputs every method needs.
struct Layout<'a> {
    measure: &'a dyn Measure,
    style: &'a Style,
}

impl Layout<'_> {
    // -- rows ---------------------------------------------------------------

    /// Lay out a row: place every child box on one baseline and collect the
    /// row's carets in depth-first position order.
    fn row(&self, row: &Row, size: f32) -> Frag {
        let strut = self.measure.measure(STRUT, size);
        let mut build = RowBuild {
            width: 0.0,
            ascent: strut.ascent.max(0.0),
            descent: strut.descent.max(0.0),
            items: Vec::new(),
            slots: vec![CaretSlot::Local(0.0)],
        };

        if row.is_empty() {
            // An empty row is a legal, editable slot: draw the box that says so.
            let side = self.style.empty_box(size);
            let rect = Rect::new(0.0, -self.style.axis(size) - side * 0.5, side, side);
            build.items.push(Item::Placeholder { rect });
            build.width = side;
            return build.finish();
        }

        let mut i = 0;
        while i < row.len() {
            if is_script(&row[i]) {
                // A run of scripts is one column: `x_1^2` puts both after the `x`
                // at the same x, which is how the input model means them.
                let start = i;
                while i < row.len() && is_script(&row[i]) {
                    i += 1;
                }
                let script_size = self.style.script_size(size);
                let raise = self.style.script_raise(size);
                let x = build.width;
                let mut column_width: f32 = 0.0;
                for node in &row[start..i] {
                    let (frag, dy) = match node {
                        Node::Sup(script) => (self.row(script, script_size), -raise),
                        Node::Sub(script) => (self.row(script, script_size), raise),
                        // `is_script` guarantees the arms above; this one keeps
                        // the loop total without a panic path.
                        _ => (Frag::new(), 0.0),
                    };
                    column_width = column_width.max(frag.width);
                    build.place(frag, x, dy);
                }
                build.width = x + column_width;
                // A script run is maximal, so there is always a position after it.
                build.slots.push(CaretSlot::Local(build.width));
            } else {
                let x = build.width;
                let frag = self.node(&row[i], size);
                let width = frag.width;
                build.place(frag, x, 0.0);
                build.width = x + width;
                i += 1;
                // Mirror `cas_input::MathField::positions`: there is no position
                // between an atom and the script that decorates it.
                if !matches!(row.get(i), Some(Node::Sup(_)) | Some(Node::Sub(_))) {
                    build.slots.push(CaretSlot::Local(build.width));
                }
            }
        }

        build.finish()
    }

    // -- nodes --------------------------------------------------------------

    fn node(&self, node: &Node, size: f32) -> Frag {
        match node {
            Node::Text(text) => self.text(text, size),
            Node::Sym(sym) => self.sym(sym, size),
            Node::Frac { num, den } => self.frac(num, den, size),
            Node::Binom { top, bottom } => self.binom(top, bottom, size),
            Node::Sqrt { index, body } => self.sqrt(index.as_ref(), body, size),
            // Rows never route scripts through here (they place them as shifted
            // boxes), but a bare decoration is still a legal tree and must lay
            // out on its own.
            Node::Sup(script) => self.script(script, size, -1.0),
            Node::Sub(script) => self.script(script, size, 1.0),
            Node::Group { delim, body } => self.group(*delim, body, size),
            Node::Accent { kind, body } => self.accent(*kind, body, size),
        }
    }

    /// A bare script with no base: the base-less `^{2}` case.
    fn script(&self, row: &Row, size: f32, direction: f32) -> Frag {
        let child = self.row(row, self.style.script_size(size));
        let width = child.width;
        let mut out = Frag::new();
        out.width = width;
        out.place(child, 0.0, direction * self.style.script_raise(size));
        out
    }

    fn text(&self, text: &str, size: f32) -> Frag {
        let mut out = Frag::new();
        for (run, italic) in text_runs(text) {
            let metrics = self.measure.measure(&run, size);
            let pos = Pos2::new(out.width, 0.0);
            out.items.push(Item::Text {
                pos,
                text: run,
                size,
                italic,
            });
            out.width += metrics.width;
            out.ascent = out.ascent.max(metrics.ascent);
            out.descent = out.descent.max(metrics.descent);
        }
        if out.items.is_empty() {
            // A `Text("")` node is possible transiently while a `\command` is
            // being typed; give it the strut height rather than zero.
            let strut = self.measure.measure(STRUT, size);
            out.ascent = out.ascent.max(strut.ascent);
            out.descent = out.descent.max(strut.descent);
        }
        out
    }

    fn sym(&self, sym: &str, size: f32) -> Frag {
        let (text, italic) = sym_display(sym);
        let metrics = self.measure.measure(&text, size);
        let mut out = Frag::new();
        out.width = metrics.width;
        out.ascent = metrics.ascent.max(0.0);
        out.descent = metrics.descent.max(0.0);
        if !text.is_empty() {
            let pos = Pos2::ZERO;
            out.items.push(Item::Text {
                pos,
                text,
                size,
                italic,
            });
        }
        out
    }

    fn frac(&self, num: &Row, den: &Row, size: f32) -> Frag {
        let pad = self.style.pad;
        let thickness = self.style.rule_thickness;
        let axis = self.style.axis(size);

        let numerator = self.row(num, size);
        let denominator = self.row(den, size);

        let width = numerator.width.max(denominator.width) + 2.0 * pad;
        // The bar sits on the math axis; the numerator's baseline is `pad` above
        // it and the denominator's is `pad` below it.
        let rule_top = -axis - thickness * 0.5;
        let num_dx = (width - numerator.width) * 0.5;
        let num_dy = rule_top - pad - numerator.descent;
        let den_dx = (width - denominator.width) * 0.5;
        let den_dy = rule_top + thickness + pad + denominator.ascent;

        let mut out = Frag::new();
        out.width = width;
        out.ascent = axis + thickness * 0.5;
        out.descent = axis - thickness * 0.5;
        out.items.push(Item::Rule {
            rect: Rect::new(0.0, rule_top, width, thickness),
        });
        out.place(numerator, num_dx, num_dy);
        out.place(denominator, den_dx, den_dy);
        out
    }

    fn binom(&self, top: &Row, bottom: &Row, size: f32) -> Frag {
        let pad = self.style.pad;
        let gap = self.style.line_gap;
        let axis = self.style.axis(size);

        let upper = self.row(top, size);
        let lower = self.row(bottom, size);

        let stack_height = upper.ascent + upper.descent + gap + lower.ascent + lower.descent;
        let content_width = upper.width.max(lower.width);
        let stack_top = -axis - stack_height * 0.5;
        let upper_dy = stack_top + upper.ascent;
        let lower_dy = stack_top + upper.ascent + upper.descent + gap + lower.ascent;

        let (open, close) = delim_chars(Delim::Paren);
        let open_frag = self.stretch(open, stack_height + 2.0 * pad, size);
        let close_frag = self.stretch(close, stack_height + 2.0 * pad, size);
        let open_width = open_frag.width;
        let close_width = close_frag.width;

        let mut out = Frag::new();
        out.width = open_width + content_width + close_width;
        let upper_dx = open_width + (content_width - upper.width) * 0.5;
        let lower_dx = open_width + (content_width - lower.width) * 0.5;
        out.place(upper, upper_dx, upper_dy);
        out.place(lower, lower_dx, lower_dy);
        out.place(open_frag, 0.0, 0.0);
        out.place(close_frag, open_width + content_width, 0.0);
        out
    }

    fn sqrt(&self, index: Option<&Row>, body: &Row, size: f32) -> Frag {
        let pad = self.style.pad;
        let thickness = self.style.rule_thickness;

        let radicand = self.row(body, size);
        let body_width = radicand.width;
        let body_ascent = radicand.ascent;
        let body_descent = radicand.descent;

        // The radical glyph is stretched to the radicand's height plus the
        // overline's gap and thickness, so its top lands on the rule.
        let radical = self.radical(body_ascent + body_descent + pad + thickness, size);
        let radical_width = radical.width;

        let index_frag = index.map(|row| self.row(row, self.style.script_size(size)));
        let index_width = index_frag.as_ref().map_or(0.0, |frag| frag.width);

        let mut out = Frag::new();
        out.width = index_width + radical_width + body_width;
        out.ascent = body_ascent + pad + thickness;
        out.descent = body_descent;

        // Overline: it starts just inside the radical's right edge (no seam) and
        // ends exactly at the radicand's right edge.
        let rule_x = index_width + radical_width - thickness * 0.5;
        let rule_w = body_width + thickness * 0.5;
        let rule_y = -body_ascent - pad - thickness;
        out.items.push(Item::Rule {
            rect: Rect::new(rule_x, rule_y, rule_w, thickness),
        });

        if let Some(index_frag) = index_frag {
            // The index is small, raised, and to the left of the radical.
            out.place(index_frag, 0.0, -self.style.script_raise(size));
        }
        out.place(radical, index_width, body_descent);
        out.place(radicand, index_width + radical_width, 0.0);
        out
    }

    fn group(&self, delim: Delim, body: &Row, size: f32) -> Frag {
        let pad = self.style.pad;
        let radicand = self.row(body, size);
        let body_width = radicand.width;
        let height = radicand.ascent + radicand.descent + 2.0 * pad;

        let (open, close) = delim_chars(delim);
        let open_frag = self.stretch(open, height, size);
        let close_frag = self.stretch(close, height, size);
        let open_width = open_frag.width;
        let close_width = close_frag.width;

        let mut out = Frag::new();
        out.width = open_width + body_width + close_width;
        out.place(open_frag, 0.0, 0.0);
        out.place(radicand, open_width, 0.0);
        out.place(close_frag, open_width + body_width, 0.0);
        out
    }

    fn accent(&self, kind: AccentKind, body: &Row, size: f32) -> Frag {
        let pad = self.style.pad;
        let thickness = self.style.rule_thickness;
        let base = self.row(body, size);

        match kind {
            AccentKind::Overline => {
                let width = base.width.max(1.0);
                let y = -base.ascent - pad - thickness;
                let mut out = Frag::new();
                out.width = width;
                out.ascent = base.ascent + pad + thickness;
                out.descent = base.descent;
                out.items.push(Item::Rule {
                    rect: Rect::new(0.0, y, width, thickness),
                });
                out.place(base, 0.0, 0.0);
                out
            }
            AccentKind::Vector => {
                let arrow = self.measure.measure("→", size);
                let width = base.width.max(arrow.width);
                let dy = -base.ascent - pad - arrow.descent;
                let mut out = Frag::new();
                out.width = width;
                out.ascent = base.ascent + pad + arrow.height();
                out.descent = base.descent;
                let pos = Pos2::new((width - arrow.width) * 0.5, dy);
                out.items.push(Item::Text {
                    pos,
                    text: "→".to_owned(),
                    size,
                    italic: false,
                });
                out.place(base, 0.0, 0.0);
                out
            }
            AccentKind::Underline => {
                let arrow = self.measure.measure("→", size);
                let width = base.width.max(arrow.width);
                let dy = base.descent + pad + arrow.ascent;
                let mut out = Frag::new();
                out.width = width;
                out.ascent = base.ascent;
                out.descent = base.descent + pad + arrow.height();
                let pos = Pos2::new((width - arrow.width) * 0.5, dy);
                out.items.push(Item::Text {
                    pos,
                    text: "→".to_owned(),
                    size,
                    italic: false,
                });
                out.place(base, 0.0, 0.0);
                out
            }
        }
    }

    // -- glyphs -------------------------------------------------------------

    /// A single glyph whose baseline sits `dy` below this fragment's baseline.
    fn glyph(&self, text: &str, size: f32, dy: f32, italic: bool) -> Frag {
        let metrics = self.measure.measure(text, size);
        let mut out = Frag::new();
        out.width = metrics.width;
        out.ascent = (metrics.ascent - dy).max(0.0);
        out.descent = (metrics.descent + dy).max(0.0);
        let pos = Pos2::new(0.0, dy);
        out.items.push(Item::Text {
            pos,
            text: text.to_owned(),
            size,
            italic,
        });
        out
    }

    /// A delimiter glyph stretched to `height`, centred on the math axis.
    fn stretch(&self, ch: char, height: f32, base_size: f32) -> Frag {
        let mut buf = [0u8; 4];
        let text: &str = ch.encode_utf8(&mut buf);
        let base_metrics = self.measure.measure(text, base_size);
        let base_height = base_metrics.height().max(f32::EPSILON);
        // Never shrink a delimiter below its normal size.
        let size = base_size * (height / base_height).max(1.0);
        let metrics = self.measure.measure(text, size);
        // Centre the glyph's box on the math axis.
        let dy = -self.style.axis(base_size) + (metrics.ascent - metrics.descent) * 0.5;

        let mut out = Frag::new();
        out.width = metrics.width;
        out.ascent = (metrics.ascent - dy).max(0.0);
        out.descent = (metrics.descent + dy).max(0.0);
        let pos = Pos2::new(0.0, dy);
        out.items.push(Item::Delim { pos, ch, size });
        out
    }

    /// The radical sign, stretched so that its box bottom sits on the fragment's
    /// baseline (the caller places it at the radicand's bottom).
    fn radical(&self, height: f32, base_size: f32) -> Frag {
        let base_metrics = self.measure.measure("√", base_size);
        let base_height = base_metrics.height().max(f32::EPSILON);
        let size = base_size * (height / base_height).max(1.0);
        let metrics = self.measure.measure("√", size);
        self.glyph("√", size, -metrics.descent, false)
    }
}

// ------------------------------------------------------------------ entry point

/// Lay out a whole field.
///
/// The returned [`DisplayList`] has its origin at the top-left; its `baseline`
/// is the root row's baseline, which a caller uses to align the field with
/// surrounding text.
///
/// ```
/// # use cas_input::MathField;
/// # use cas_mathview::{layout_field, Measure, Metrics, Style};
/// # struct Fixed;
/// # impl Measure for Fixed {
/// #     fn measure(&self, text: &str, size: f32) -> Metrics {
/// #         Metrics { width: 0.6 * size * text.chars().count() as f32,
/// #                   ascent: 0.8 * size, descent: 0.2 * size }
/// #     }
/// # }
/// let mut field = MathField::new();
/// field.type_str("x^2");
/// let list = layout_field(&field, &Fixed, &Style::default());
///
/// // `x^2` has four positions: before `x`, inside the script, after the script's
/// // text, and after the whole decoration.
/// assert_eq!(list.carets.len(), 4);
/// assert_eq!(list.carets.len(), field.positions().len());
/// ```
pub fn layout_field(field: &MathField, measure: &dyn Measure, style: &Style) -> DisplayList {
    let layout = Layout { measure, style };
    let Frag {
        width,
        ascent,
        descent,
        items,
        mut carets,
    } = layout.row(field.root(), style.base_size);

    let ascent = ascent.max(0.0);
    let descent = descent.max(0.0);

    // The fragment space has its baseline at y = 0; shift everything so the
    // content starts at the top-left, as a display list should.
    let mut items = items;
    for item in &mut items {
        item.shift(0.0, ascent);
    }
    for caret in &mut carets {
        caret.y += ascent;
        items.push(Item::Caret { rect: *caret });
    }

    DisplayList {
        size: Vec2::new(width, ascent + descent),
        items,
        carets,
        baseline: ascent,
    }
}

/// The index of the caret nearest to `at`, if one is within `band`.
///
/// This is the pure half of click-to-cursor: a widget converts the pointer into
/// display-list coordinates, calls this, and zips the returned index with
/// [`MathField::positions`]. Candidates are restricted to caret rects whose
/// vertical extent is within `band` of `at` — clicking a superscript should not
/// snap to a baseline caret half a line away — and are then ordered by
/// horizontal distance, with vertical distance as the tie-break (two scripts on
/// one base share an x).
pub fn nearest_caret(list: &DisplayList, at: Pos2, band: f32) -> Option<usize> {
    let band = band.max(0.0);
    list.carets
        .iter()
        .enumerate()
        .filter_map(|(index, caret)| {
            let vertical = if at.y < caret.y {
                caret.y - at.y
            } else if at.y > caret.bottom() {
                at.y - caret.bottom()
            } else {
                0.0
            };
            if vertical > band {
                return None;
            }
            let horizontal = (at.x - caret.center_x()).abs();
            Some((index, horizontal, vertical))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(index, _, _)| index)
}

#[cfg(test)]
mod tests {
    use super::{Metrics, Pos2, Rect, Style, text_runs};

    #[test]
    fn text_runs_split_letters_from_digits() {
        let runs = text_runs("2x+1");
        let joined: String = runs.iter().map(|(run, _)| run.as_str()).collect();
        assert_eq!(joined, "2x+1");
        assert_eq!(
            runs.len(),
            3,
            "\"2\", \"x\" and the merged operator run \"+1\""
        );
        assert!(!runs[0].1, "a digit is upright");
        assert!(runs[1].1, "a letter is italic");
        assert_eq!(runs[2].0, "+1");
        assert!(!runs[2].1, "operators and digits stay upright");
    }

    #[test]
    fn style_script_size_never_underflows_the_minimum() {
        let style = Style::default();
        assert_eq!(style.script_size(22.0), 15.4);
        // Two levels down the scale would put it below 10, so the floor wins.
        assert_eq!(style.script_size(10.0), 10.0);
    }

    #[test]
    fn geometry_helpers_agree_with_their_fields() {
        let metrics = Metrics {
            width: 10.0,
            ascent: 8.0,
            descent: 2.0,
        };
        assert_eq!(metrics.height(), 10.0);
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(rect.right(), 4.0);
        assert_eq!(rect.bottom(), 6.0);
        assert_eq!(rect.center_x(), 2.5);
        assert_eq!(Pos2::ZERO, Pos2::new(0.0, 0.0));
    }
}
