//! The editable field: a tree plus a cursor, and the structural operations.

use crate::cursor::{Cursor, PathStep, Selection};
use crate::template::{Template, TemplateError};
use crate::tree::{is_word_char, AccentKind, Delim, Node, Row, Slot};

/// What `Esc` did.
///
/// Abitti binds `Esc` on the math field to "close the whole formula editor and
/// commit", not to a MathQuill notion of leaving a structure (see
/// `docs/mathquill-interaction.md` §2). A model has no editor to close, so
/// [`MathField::escape`] does the structural half — jump out to the end of the
/// root — and reports whether there was anything to leave. The caller decides
/// whether that also closes the editor. **[INFERRED]**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EscapeOutcome {
    /// The cursor was nested and has been moved out.
    LeftStructure,
    /// The cursor was already at root level.
    AtRoot,
}

/// A MathQuill-style math field: an editable tree and a cursor inside it.
///
/// All operations are pure state transitions and are unit-tested; there is no
/// rendering, no measuring and no input-device handling in this crate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MathField {
    root: Row,
    cursor: Cursor,
    selection: Option<Selection>,
    /// A `\name` accumulation in progress. Observed: `\` opens a modal command
    /// block that is committed by the next non-letter, and an unterminated
    /// invalid name stays as raw text (`\alphax`).
    command: Option<String>,
}

impl Default for MathField {
    fn default() -> Self {
        MathField::new()
    }
}

impl MathField {
    /// An empty field with the cursor in the root.
    pub fn new() -> MathField {
        MathField {
            root: Vec::new(),
            cursor: Cursor::root(),
            selection: None,
            command: None,
        }
    }

    /// Build a field from an existing tree, cursor at the start.
    pub fn from_row(root: Row) -> MathField {
        let mut f = MathField::new();
        f.root = root;
        f
    }

    // ---------------------------------------------------------------- accessors

    pub fn root(&self) -> &Row {
        &self.root
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// The active selection, if any.
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// True while a `\name` is being accumulated (`\` has been typed).
    pub fn pending_command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    /// True when the field contains an empty argument anywhere — the field is
    /// syntactically fine and algebraically incomplete (`\frac{1}{ }`).
    pub fn is_incomplete(&self) -> bool {
        crate::tree::row_has_empty_slot(&self.root)
    }

    /// True when the field holds nothing.
    pub fn is_blank(&self) -> bool {
        self.root.is_empty() && self.command.is_none()
    }

    /// Discard everything.
    pub fn clear(&mut self) {
        *self = MathField::new();
    }

    // ------------------------------------------------------------ path plumbing

    fn row_at(&self, path: &[PathStep]) -> &Row {
        row_at_ro(&self.root, path)
    }

    fn row_at_mut(&mut self, path: &[PathStep]) -> &mut Row {
        row_at_mut(&mut self.root, path)
    }

    fn current_row(&self) -> &Row {
        let path = self.cursor.path.clone();
        self.row_at(&path)
    }

    fn current_row_mut(&mut self) -> &mut Row {
        let path = self.cursor.path.clone();
        self.row_at_mut(&path)
    }

    // ------------------------------------------------------------- the position list

    /// Every cursor position in document order.
    ///
    /// This *is* the navigation model: MathQuill's Left/Right walk the flattened
    /// depth-first order of slots, which is why leaving a fraction is
    /// indistinguishable from walking past it. Verified against the live editor
    /// for `\frac{1}{2}` and `x^2`.
    pub fn positions(&self) -> Vec<Cursor> {
        let mut out = Vec::new();
        collect_positions(&self.root, &mut Vec::new(), &mut out);
        out
    }

    fn current_index(&self) -> Option<usize> {
        self.positions().iter().position(|c| *c == self.cursor)
    }

    // ------------------------------------------------------------------ movement

    /// Move one position left in the flattened order.
    pub fn move_left(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        match self.current_index() {
            Some(0) | None => false,
            Some(i) => {
                self.cursor = self.positions()[i - 1].clone();
                true
            }
        }
    }

    /// Move one position right in the flattened order.
    pub fn move_right(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        match self.current_index() {
            None => false,
            Some(i) => {
                let ps = self.positions();
                if i + 1 < ps.len() {
                    self.cursor = ps[i + 1].clone();
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Move up one visual row.
    ///
    /// **This is the one place where the model deliberately approximates.** In
    /// Abitti `↑`/`↓` are *geometric*: the cursor keeps its x pixel and snaps to
    /// the nearest position in the slot above. `\frac{123456}{7}` with the cursor
    /// at the denominator's left edge lands at `num[2]`, because the denominator
    /// is centred under a much wider numerator. With no layout there is no x
    /// pixel, so we clamp the offset instead. Everything else about the observed
    /// behaviour (numerator ⇄ denominator, nothing above the numerator) is
    /// reproduced. Isolated here on purpose.
    pub fn move_up(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        match self.vertical_target(true) {
            Some(c) => {
                self.cursor = c;
                true
            }
            None => false,
        }
    }

    /// Move down one visual row. See [`MathField::move_up`].
    pub fn move_down(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        match self.vertical_target(false) {
            Some(c) => {
                self.cursor = c;
                true
            }
            None => false,
        }
    }

    /// The sibling slot directly above (`up == true`) or below, if there is one.
    fn vertical_target(&self, up: bool) -> Option<Cursor> {
        let last = *self.cursor.path.last()?;
        let sibling = match (last.slot, up) {
            (Slot::Den, true) => Slot::Num,
            (Slot::Num, false) => Slot::Den,
            (Slot::Index, false) => Slot::Body,
            (Slot::Body, true) => Slot::Index,
            _ => return None,
        };
        let mut path = self.cursor.path.clone();
        path.pop();
        let node = &self.row_at(&path)[last.index];
        let target = node.slot(sibling)?;
        let offset = self.cursor.offset.min(target.len());
        let mut step_path = path;
        step_path.push(PathStep::new(last.index, sibling));
        Some(Cursor::at(step_path, offset))
    }

    // ------------------------------------------------------------------ selection

    /// Select the entire field (`Ctrl-A`).
    pub fn select_all(&mut self) {
        self.finish_command();
        self.selection = Some(Selection {
            start: Cursor::root(),
            end: Cursor::at(Vec::new(), self.root.len()),
        });
        self.cursor = Cursor::at(Vec::new(), self.root.len());
    }

    /// Drop the selection without touching the tree.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Place the cursor at a position the caller obtained from
    /// [`MathField::positions`] or from its own hit-testing, clearing any
    /// selection.
    ///
    /// Returns `false` (leaving the field untouched) when the path does not exist
    /// in the current tree — which is what a renderer should expect after the tree
    /// changed under it.
    pub fn set_cursor(&mut self, cursor: Cursor) -> bool {
        self.finish_command();
        if !self.path_exists(&cursor) {
            return false;
        }
        self.selection = None;
        self.cursor = cursor;
        true
    }

    fn path_exists(&self, cursor: &Cursor) -> bool {
        let mut row = &self.root;
        for step in &cursor.path {
            let Some(node) = row.get(step.index) else {
                return false;
            };
            let Some(next) = node.slot(step.slot) else {
                return false;
            };
            row = next;
        }
        cursor.offset <= row.len()
    }

    fn delete_selection(&mut self) -> Row {
        match self.selection.take() {
            None => Vec::new(),
            Some(sel) => {
                let start = sel.start.offset;
                let end = sel.end.offset;
                let path = sel.start.path;
                let row = self.row_at_mut(&path);
                let taken: Row = row.drain(start..end).collect();
                self.cursor = Cursor::at(path, start);
                taken
            }
        }
    }

    fn drop_selection(&mut self) {
        let _ = self.delete_selection();
    }

    // -------------------------------------------------------------------- typing

    /// Type a whole string, one keystroke at a time. This is the test-friendly
    /// form of "the user pressed these keys".
    pub fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    /// Handle one printable keystroke. Returns `false` only for characters the
    /// model does not bind at all.
    pub fn insert_char(&mut self, c: char) -> bool {
        // A `\name` accumulation swallows the next character if it is a letter.
        if self.command.is_some() {
            if c.is_ascii_alphabetic() {
                self.push_command_char(c);
                return true;
            }
            self.finish_command();
            // The terminating space is consumed, not inserted: typing
            // `\sqrt ` then `x` gives `\sqrt{x}`, not `\sqrt{\ x}`. [OBSERVED]
            if c == ' ' {
                return true;
            }
        }

        match c {
            '\\' => {
                self.drop_selection();
                self.begin_command();
                true
            }
            '/' => {
                self.slash();
                true
            }
            '^' => {
                self.power();
                true
            }
            '_' => {
                self.subscript();
                true
            }
            '*' => {
                self.insert_sym("\\cdot");
                true
            }
            '(' => self.open_delim(Delim::Paren),
            '[' => self.open_delim(Delim::Bracket),
            '{' => self.open_delim(Delim::Brace),
            '|' => {
                // `|` is both halves of an absolute-value pair.
                if self.closes_innermost(Delim::Abs) {
                    self.close_innermost();
                    true
                } else {
                    self.open_delim(Delim::Abs)
                }
            }
            ')' | ']' | '}' => {
                self.drop_selection();
                self.close_or_literal(c)
            }
            '=' => {
                self.drop_selection();
                self.insert_equals();
                true
            }
            c if c.is_control() => false,
            c => {
                self.drop_selection();
                self.insert_char_literal(c);
                true
            }
        }
    }

    /// Insert one plain character, extending the trailing word run when it can.
    ///
    /// Word characters run together (`xy` is one node, which is what lets `/` take
    /// all of it); operators always get their own node (`1+2` has three, which is
    /// what makes `/` take only the `2`). Both behaviours observed.
    fn insert_char_literal(&mut self, c: char) {
        let at = self.cursor.offset;
        let row = self.current_row_mut();
        if at > 0 {
            if let Node::Text(s) = &mut row[at - 1] {
                if let Some(last) = s.chars().last() {
                    if is_word_char(last) && is_word_char(c) {
                        s.push(c);
                        self.cursor.offset = at;
                        self.auto_operator_name();
                        return;
                    }
                }
            }
        }
        row.insert(at, Node::Text(c.to_string()));
        self.cursor.offset = at + 1;
        self.auto_operator_name();
    }

    /// Split a trailing operator name out of a word run.
    ///
    /// Observed: `sin` becomes `\sin` as soon as the letters match, even mid-word
    /// (`sinx` → `\sin x`, `arcsin` → `\arcsin`), while `pi` and `sqrt` do not.
    fn auto_operator_name(&mut self) {
        const NAMES: &[&str] = &[
            "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "sin", "cos", "tan", "cot", "sec",
            "csc", "ln", "log", "exp", "max", "min", "lim", "det", "gcd",
        ];
        let at = self.cursor.offset;
        let Some(name) = ({
            let row = self.current_row();
            match at.checked_sub(1).and_then(|i| row.get(i)) {
                Some(Node::Text(s)) => {
                    let mut found = None;
                    for n in NAMES {
                        if s.ends_with(n) {
                            found = Some((*n).to_string());
                            break;
                        }
                    }
                    found
                }
                _ => None,
            }
        }) else {
            return;
        };
        let keep_len = {
            let row = self.current_row();
            match &row[at - 1] {
                Node::Text(s) => s.len() - name.len(),
                _ => return,
            }
        };
        let row = self.current_row_mut();
        let prefix = match &row[at - 1] {
            Node::Text(s) => s[..keep_len].to_string(),
            _ => return,
        };
        let replaced = vec![Node::sym(format!("\\{name}"))];
        if prefix.is_empty() {
            row.splice(at - 1..at, replaced);
            self.cursor.offset = at;
        } else {
            if let Node::Text(s) = &mut row[at - 1] {
                *s = prefix;
            }
            row.splice(at..at, replaced);
            self.cursor.offset = at + 1;
        }
    }

    /// `=` after `<` or `>` rewrites the pair as `\le` / `\ge`. Observed:
    /// `2<=3` → `2\le3`, `x<=y` → `x\le y`; but `!=` and `=>` stay literal.
    fn insert_equals(&mut self) {
        let at = self.cursor.offset;
        let replacement = {
            let row = self.current_row();
            match at.checked_sub(1).and_then(|i| row.get(i)) {
                Some(Node::Text(s)) if s.ends_with('<') => Some("\\le"),
                Some(Node::Text(s)) if s.ends_with('>') => Some("\\ge"),
                _ => None,
            }
        };
        let Some(sym) = replacement else {
            self.insert_char_literal('=');
            return;
        };
        let row = self.current_row_mut();
        let keep_len = match &row[at - 1] {
            Node::Text(s) => s.len() - 1,
            _ => 0,
        };
        if keep_len == 0 {
            row[at - 1] = Node::sym(sym);
        } else {
            if let Node::Text(s) = &mut row[at - 1] {
                s.truncate(keep_len);
            }
            row.insert(at, Node::sym(sym));
            self.cursor.offset = at + 1;
        }
    }

    /// Insert a command token, replacing any selection.
    pub fn insert_sym(&mut self, sym: &str) {
        self.finish_command();
        self.drop_selection();
        let at = self.cursor.offset;
        self.current_row_mut().insert(at, Node::sym(sym));
        self.cursor.offset = at + 1;
    }

    // -------------------------------------------------------- LaTeX command entry

    fn begin_command(&mut self) {
        let at = self.cursor.offset;
        self.current_row_mut().insert(at, Node::text("\\"));
        self.cursor.offset = at + 1;
        self.command = Some(String::new());
    }

    fn command_node_index(&self) -> Option<usize> {
        if self.command.is_some() {
            self.cursor.offset.checked_sub(1)
        } else {
            None
        }
    }

    fn push_command_char(&mut self, c: char) {
        let Some(i) = self.command_node_index() else {
            return;
        };
        let name = self.command.get_or_insert_with(String::new);
        name.push(c);
        let raw = format!("\\{name}");
        if let Node::Text(s) = &mut self.current_row_mut()[i] {
            *s = raw;
        }
    }

    /// Commit an in-progress `\name`.
    ///
    /// Three outcomes, all observed in the live editor:
    /// * a known structure command (`\sqrt`, `\frac`, `\binom`, `\int`, `\sum`,
    ///   `\lim`) inserts that structure and puts the cursor in its first empty slot;
    /// * a known symbol command (`\alpha`, `\le`, …) inserts a [`Node::Sym`];
    /// * anything else becomes upright text (`\alp` + space → `\text{alp}`).
    ///
    /// The symbol list is a subset of MathQuill's; extend it as needed. **The
    /// "unknown name becomes `\text{…}`" rule is the one guess in this function** —
    /// it is observed for `\alp ` but not for every possible name.
    fn finish_command(&mut self) {
        let Some(i) = self.command_node_index() else {
            self.command = None;
            return;
        };
        let Some(name) = self.command.take() else {
            return;
        };
        self.current_row_mut().remove(i);
        self.cursor.offset = i;

        const GREEK: &[&str] = &[
            "alpha", "beta", "gamma", "delta", "epsilon", "varepsilon", "zeta", "eta", "theta",
            "vartheta", "iota", "kappa", "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau",
            "upsilon", "phi", "varphi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda",
            "Xi", "Pi", "Sigma", "Upsilon", "Phi", "Psi", "Omega",
        ];
        const RELATIONS: &[&str] = &[
            "le", "leq", "ge", "geq", "ne", "neq", "approx", "equiv", "sim", "propto", "in",
            "notin", "subset", "cup", "cap", "setminus", "forall", "exists", "infty", "partial",
            "rightarrow", "Rightarrow", "Leftrightarrow", "leftrightarrow", "cdot", "times", "pm",
            "mid", "parallel", "angle", "perp", "nabla", "ldots", "circ", "uparrow", "downarrow",
            "nearrow", "searrow", "varnothing", "neg", "wedge", "vee", "not",
        ];
        const FUNCTIONS: &[&str] = &[
            "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
            "tanh", "ln", "log", "exp", "max", "min", "det", "gcd",
        ];

        let nodes: Vec<Node> = match name.as_str() {
            "sqrt" => vec![Node::sqrt(Vec::new())],
            "frac" => vec![Node::Frac { num: Vec::new(), den: Vec::new() }],
            "binom" => vec![Node::Binom { top: Vec::new(), bottom: Vec::new() }],
            "int" => vec![
                Node::sym("\\int"),
                Node::Sub(Vec::new()),
                Node::Sup(Vec::new()),
            ],
            "sum" => vec![
                Node::sym("\\sum"),
                Node::Sub(Vec::new()),
                Node::Sup(Vec::new()),
            ],
            "lim" => vec![Node::sym("\\lim"), Node::Sub(Vec::new())],
            n if GREEK.contains(&n) || RELATIONS.contains(&n) || FUNCTIONS.contains(&n) => {
                vec![Node::sym(format!("\\{n}"))]
            }
            n => vec![Node::sym(format!("\\text{{{n}}}"))],
        };
        let slot = match name.as_str() {
            "frac" | "binom" => Some(Slot::Num),
            "sqrt" => Some(Slot::Body),
            _ => None,
        };
        self.insert_constructed(nodes, slot);
    }

    // --------------------------------------------------------------- structures

    /// `^`: attach a superscript to the node on the left (or start a base-less
    /// one when there is nothing there, which MathQuill allows).
    pub fn power(&mut self) {
        let _ = self.insert_template(Template::Power);
    }

    /// `_`: attach a subscript, same rules as [`MathField::power`].
    pub fn subscript(&mut self) {
        let _ = self.insert_template(Template::Subscript);
    }

    /// `/`: the smart fraction.
    ///
    /// Takes the selection, else the trailing run of word characters in the
    /// current row, and makes it the numerator. The cursor then lands in the first
    /// empty slot of the result, which is the denominator when the numerator got
    /// content and the numerator when it did not. Both outcomes observed.
    pub fn slash(&mut self) {
        self.finish_command();
        let leading = self.take_leading_run();
        self.insert_constructed(
            vec![Node::Frac { num: leading, den: Vec::new() }],
            Some(Slot::Num),
        );
    }

    /// The selection, or the trailing word run at the cursor.
    ///
    /// The "trailing run" rule is why `xy/` gives `\frac{xy}{ }` but `1+2/` gives
    /// `1+\frac{2}{ }`. What happens when the node to the left is a *command*
    /// (`\alpha/`) was not observed; this model leaves it alone. **[INFERRED]**
    fn take_leading_run(&mut self) -> Row {
        if self.selection.is_some() {
            return self.delete_selection();
        }
        let at = self.cursor.offset;
        if at == 0 {
            return Vec::new();
        }
        let row = self.current_row_mut();
        let take = match &row[at - 1] {
            Node::Text(s) => s.chars().next().is_some_and(|c| c.is_alphanumeric()),
            _ => false,
        };
        if take {
            let node = row.remove(at - 1);
            self.cursor.offset = at - 1;
            vec![node]
        } else {
            Vec::new()
        }
    }

    /// Insert a template from the palette (or the keyboard), landing the cursor in
    /// its first empty slot.
    pub fn insert_template(&mut self, t: Template) -> Result<(), TemplateError> {
        self.finish_command();

        // `^` twice enters the existing superscript rather than nesting a new one.
        // **[INFERRED]**
        if matches!(t, Template::Power | Template::Subscript) && self.selection.is_none() {
            if let Some(prev) = self.cursor.offset.checked_sub(1) {
                let exists = {
                    let row = self.current_row();
                    matches!(
                        (t, row.get(prev)),
                        (Template::Power, Some(Node::Sup(_)))
                            | (Template::Subscript, Some(Node::Sub(_)))
                    )
                };
                if exists {
                    self.cursor = self.cursor.clone().pushed(prev, t.content_slot().unwrap());
                    let end = {
                        let row = self.current_row();
                        row.len()
                    };
                    self.cursor.offset = end;
                    return Ok(());
                }
            }
        }

        let content = if t.content_slot().is_some() {
            self.delete_selection()
        } else {
            self.drop_selection();
            Vec::new()
        };

        let mut nodes = match t {
            Template::Frac => vec![Node::Frac { num: Vec::new(), den: Vec::new() }],
            Template::Binom => vec![Node::Binom { top: Vec::new(), bottom: Vec::new() }],
            Template::Sqrt => vec![Node::sqrt(Vec::new())],
            Template::NthRoot => vec![Node::Sqrt { index: Some(Vec::new()), body: Vec::new() }],
            Template::Power => vec![Node::Sup(Vec::new())],
            Template::Subscript => vec![Node::Sub(Vec::new())],
            Template::Paren => vec![Node::Group { delim: Delim::Paren, body: Vec::new() }],
            Template::Bracket => vec![Node::Group { delim: Delim::Bracket, body: Vec::new() }],
            Template::Abs => vec![Node::Group { delim: Delim::Abs, body: Vec::new() }],
            Template::Brace => vec![Node::Group { delim: Delim::Brace, body: Vec::new() }],
            Template::Vector => vec![Node::Accent { kind: AccentKind::Vector, body: Vec::new() }],
            Template::Overline => vec![Node::Accent { kind: AccentKind::Overline, body: Vec::new() }],
            Template::Underline => {
                vec![Node::Accent { kind: AccentKind::Underline, body: Vec::new() }]
            }
            Template::Int => vec![Node::sym("\\int"), Node::Sub(Vec::new()), Node::Sup(Vec::new())],
            Template::Sum => vec![Node::sym("\\sum"), Node::Sub(Vec::new()), Node::Sup(Vec::new())],
            Template::Lim => vec![Node::sym("\\lim"), Node::Sub(Vec::new())],
            Template::Sin => vec![Node::sym("\\sin")],
            Template::Cos => vec![Node::sym("\\cos")],
            Template::Tan => vec![Node::sym("\\tan")],
        };

        if let Some(slot) = t.content_slot() {
            if !content.is_empty() {
                if let Some(row) = nodes[0].slot_mut(slot) {
                    *row = content;
                }
            }
        }

        self.insert_constructed(nodes, t.content_slot());
        Ok(())
    }

    /// Insert already-built nodes at the cursor and place the cursor in the first
    /// empty slot of the inserted structures.
    fn insert_constructed(&mut self, nodes: Vec<Node>, content_slot: Option<Slot>) {
        let at = self.cursor.offset;
        let base_path = self.cursor.path.clone();
        let count = nodes.len();
        {
            let row = self.row_at_mut(&base_path);
            for (k, n) in nodes.into_iter().enumerate() {
                row.insert(at + k, n);
            }
        }
        self.cursor = template_target(self, &base_path, at, count, content_slot);
    }

    fn open_delim(&mut self, delim: Delim) -> bool {
        let t = match delim {
            Delim::Paren => Template::Paren,
            Delim::Bracket => Template::Bracket,
            Delim::Abs => Template::Abs,
            Delim::Brace => Template::Brace,
        };
        let _ = self.insert_template(t);
        true
    }

    fn closes_innermost(&self, delim: Delim) -> bool {
        let Some(last) = self.cursor.path.last() else {
            return false;
        };
        if last.slot != Slot::Body {
            return false;
        }
        let path = &self.cursor.path[..self.cursor.path.len() - 1];
        matches!(self.row_at(path).get(last.index), Some(Node::Group { delim: d, .. }) if *d == delim)
    }

    fn close_innermost(&mut self) {
        if let Some(last) = self.cursor.path.last().copied() {
            let mut path = self.cursor.path.clone();
            path.pop();
            self.cursor = Cursor::at(path, last.index + 1);
        }
    }

    /// A closing delimiter leaves the enclosing group; anything else is literal.
    ///
    /// A stray `)` in Abitti builds a "ghost" bracket pair around the preceding
    /// run; that is not modelled here. **[INFERRED]**
    fn close_or_literal(&mut self, c: char) -> bool {
        let delimiter = match c {
            ')' => Some(Delim::Paren),
            ']' => Some(Delim::Bracket),
            '}' => Some(Delim::Brace),
            _ => None,
        };
        if let Some(d) = delimiter {
            if self.closes_innermost(d) {
                self.close_innermost();
                return true;
            }
        }
        self.insert_char_literal(c);
        true
    }

    // ------------------------------------------------------------------ deleting

    /// Backspace: delete a character, or dissolve the structure the cursor sits at
    /// the start of.
    ///
    /// The observed rules, in order:
    /// * a pending `\name` loses its last letter (and vanishes when empty);
    /// * a selection is replaced;
    /// * with a character to the left, delete it;
    /// * at the start of a slot, **unwrap** the construct and keep the cursor's
    ///   logical position: a fraction becomes `num ++ den` with the cursor at the
    ///   junction, a root or bracket becomes its body with the cursor at the start;
    /// * at the start of an *empty* superscript/subscript, drop the decoration;
    /// * at the start of a *non-empty* superscript/subscript, delete the base.
    pub fn backspace(&mut self) {
        if self.command.is_some() {
            self.backspace_command();
            return;
        }
        if self.selection.is_some() {
            self.drop_selection();
            return;
        }
        let at = self.cursor.offset;
        if at > 0 {
            let row = self.current_row_mut();
            let i = at - 1;
            let mut removed = false;
            if let Node::Text(s) = &mut row[i] {
                s.pop();
                if s.is_empty() {
                    removed = true;
                }
            } else {
                removed = true;
            }
            if removed {
                row.remove(i);
                self.cursor.offset = i;
            }
            return;
        }
        self.backspace_out_of_structure();
    }

    fn backspace_command(&mut self) {
        let Some(i) = self.command_node_index() else {
            self.command = None;
            return;
        };
        let empty = {
            let name = self.command.as_mut().expect("checked");
            name.pop();
            name.is_empty()
        };
        if empty {
            self.command = None;
            self.current_row_mut().remove(i);
            self.cursor.offset = i;
        } else {
            let raw = format!("\\{}", self.command.as_ref().expect("checked"));
            if let Node::Text(s) = &mut self.current_row_mut()[i] {
                *s = raw;
            }
        }
    }

    /// Backspace at offset 0 of a slot.
    fn backspace_out_of_structure(&mut self) {
        let Some(last) = self.cursor.path.last().copied() else {
            return;
        };
        let mut parent_path = self.cursor.path.clone();
        parent_path.pop();

        let slot_is_blank = {
            let path = self.cursor.path.clone();
            self.row_at(&path).is_empty()
        };

        // A non-empty superscript/subscript escalates: backspace deletes the base.
        if matches!(last.slot, Slot::Sup | Slot::Sub) && !slot_is_blank {
            self.cursor = Cursor::at(parent_path, last.index);
            self.backspace();
            return;
        }

        // A root's optional index is not unwrapped; backspace turns the root back
        // into a plain `\sqrt`. **[INFERRED]** — not observed.
        if last.slot == Slot::Index {
            if let Some(Node::Sqrt { index, .. }) = self.row_at_mut(&parent_path).get_mut(last.index)
            {
                *index = None;
            }
            self.cursor = Cursor::at(
                {
                    let mut p = parent_path;
                    p.push(PathStep::new(last.index, Slot::Body));
                    p
                },
                0,
            );
            return;
        }

        let node = {
            let row = self.row_at_mut(&parent_path);
            row.remove(last.index)
        };
        let (contents, junction) = unwrap_construct(&node, last.slot);
        {
            let row = self.row_at_mut(&parent_path);
            for (k, n) in contents.into_iter().enumerate() {
                row.insert(last.index + k, n);
            }
        }
        self.cursor = Cursor::at(parent_path, last.index + junction);
    }

    /// Forward delete: one character, or one whole node.
    pub fn delete_forward(&mut self) {
        self.finish_command();
        if self.selection.is_some() {
            self.drop_selection();
            return;
        }
        let at = self.cursor.offset;
        let row = self.current_row_mut();
        let Some(node) = row.get_mut(at) else {
            return;
        };
        let mut remove = true;
        if let Node::Text(s) = node {
            if !s.is_empty() {
                s.remove(0);
                remove = s.is_empty();
            }
        }
        if remove {
            row.remove(at);
        }
    }

    // -------------------------------------------------------------- hand-off keys

    /// `Esc`: leave every structure and report whether there was one.
    pub fn escape(&mut self) -> EscapeOutcome {
        self.finish_command();
        let was_nested = self.cursor.is_nested();
        self.selection = None;
        self.cursor = Cursor::at(Vec::new(), self.root.len());
        if was_nested {
            EscapeOutcome::LeftStructure
        } else {
            EscapeOutcome::AtRoot
        }
    }

    /// `Enter`: commit the formula and start a fresh field.
    ///
    /// Observed: Abitti inserts the current LaTeX into the outer editor, adds a
    /// line break, and opens a new empty formula editor. The model returns the
    /// committed LaTeX — including an incomplete `\frac{1}{ }`, because that is
    /// exactly what MathQuill hands over — and empties itself.
    pub fn enter(&mut self) -> String {
        self.finish_command();
        let latex = self.to_latex();
        *self = MathField::new();
        latex
    }

    // ------------------------------------------------------------------- output

    /// Abitti-flavoured LaTeX for the whole field. See [`crate::latex`].
    pub fn to_latex(&self) -> String {
        crate::latex::emit(&self.root)
    }

    /// Convert to a `cas-core` expression, or explain why not.
    pub fn build_expr(&self) -> Result<crate::Expr, crate::ToExprError> {
        crate::expr::build(&self.root)
    }

    /// Convert to a `cas-core` expression when the field is complete enough.
    ///
    /// Returns `None` for a half-typed field (`\frac{1}{ }`), for a base-less
    /// superscript, and for constructs with no algebraic meaning (subscripts,
    /// binomials, relations).
    pub fn to_expr(&self) -> Option<crate::Expr> {
        self.build_expr().ok()
    }

    /// The same expression printed by `cas-tex`, for callers that want the CAS's
    /// own rendering rather than the field's.
    pub fn to_expr_latex(&self) -> Option<String> {
        self.to_expr().map(|e| cas_tex::to_latex(&e))
    }
}

// --------------------------------------------------------------------- helpers

fn row_at_ro<'a>(row: &'a Row, path: &[PathStep]) -> &'a Row {
    match path.split_first() {
        None => row,
        Some((step, rest)) => {
            let node = &row[step.index];
            row_at_ro(node.slot(step.slot).expect("cursor path is valid"), rest)
        }
    }
}

fn row_at_mut<'a>(row: &'a mut Row, path: &[PathStep]) -> &'a mut Row {
    match path.split_first() {
        None => row,
        Some((step, rest)) => {
            let node = &mut row[step.index];
            row_at_mut(node.slot_mut(step.slot).expect("cursor path is valid"), rest)
        }
    }
}

fn collect_positions(row: &Row, path: &mut Vec<PathStep>, out: &mut Vec<Cursor>) {
    out.push(Cursor::at(path.clone(), 0));
    for (i, node) in row.iter().enumerate() {
        for slot in node.slots() {
            path.push(PathStep::new(i, slot));
            if let Some(sub) = node.slot(slot) {
                collect_positions(sub, path, out);
            }
            path.pop();
        }
        // There is no cursor position *between* an atom and its superscript: the
        // decoration belongs to the atom. Observed: `←` from the start of the sup
        // in `x^2` lands before the `x`, not between `x` and `^2`.
        let followed_by_script = matches!(row.get(i + 1), Some(Node::Sup(_)) | Some(Node::Sub(_)));
        if !followed_by_script {
            out.push(Cursor::at(path.clone(), i + 1));
        }
    }
}

/// Where the cursor goes after inserting `count` nodes at `at`.
///
/// The rule, which reproduces every palette row that was measured: **the first
/// still-empty slot of the inserted structure, in document order; failing that,
/// the end of the content slot.**
fn template_target(
    field: &MathField,
    base_path: &[PathStep],
    at: usize,
    count: usize,
    content_slot: Option<Slot>,
) -> Cursor {
    let row = field.row_at(base_path);
    for k in 0..count {
        let node = &row[at + k];
        for slot in node.slots() {
            if node.slot(slot).is_some_and(|r| r.is_empty()) {
                let mut path = base_path.to_vec();
                path.push(PathStep::new(at + k, slot));
                return Cursor::at(path, 0);
            }
        }
    }
    if let Some(slot) = content_slot {
        let node = &row[at];
        if let Some(r) = node.slot(slot) {
            let mut path = base_path.to_vec();
            path.push(PathStep::new(at, slot));
            return Cursor::at(path, r.len());
        }
    }
    Cursor::at(base_path.to_vec(), at + count)
}

/// Replace a construct with its contents when Backspace is pressed at the start of
/// one of its slots.
///
/// Returns the replacement nodes and the offset (relative to the splice point) at
/// which the cursor should sit — the *logical* position is preserved, which is why
/// unwrapping from a denominator leaves the cursor between the numerator's and the
/// denominator's contents.
fn unwrap_construct(node: &Node, slot: Slot) -> (Row, usize) {
    match node {
        Node::Frac { num, den } => {
            // Coming from the numerator means the cursor was at the very left of
            // the construct; coming from the denominator means it was at the
            // num/den junction. Both observed.
            let junction = if slot == Slot::Num { 0 } else { num.len() };
            let mut out = num.clone();
            out.extend(den.iter().cloned());
            (out, junction)
        }
        Node::Binom { top, bottom } => {
            let junction = if slot == Slot::Num { 0 } else { top.len() };
            let mut out = top.clone();
            out.extend(bottom.iter().cloned());
            (out, junction)
        }
        Node::Sqrt { body, .. } => (body.clone(), 0),
        Node::Group { body, .. } => (body.clone(), 0),
        Node::Accent { body, .. } => (body.clone(), 0),
        Node::Sup(row) | Node::Sub(row) => (row.clone(), 0),
        Node::Text(_) | Node::Sym(_) => (Vec::new(), 0),
    }
}
