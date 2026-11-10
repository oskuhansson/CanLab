//! The editable field: a tree plus a cursor, and the structural operations.

use crate::cursor::{Cursor, PathStep, Selection};
use crate::template::{Template, TemplateError};
use crate::tree::{is_word_char, AccentKind, Delim, Ghost, Node, Row, Slot};

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
    /// A document is a list of **systems**, and a system is a list of lines.
    ///
    /// One line is an expression. Several lines in one system are equations that
    /// hold together: they are edited as one block, rendered stacked with a
    /// brace, and printed as `\begin{cases}…\end{cases}`. Several systems in one
    /// document are separate questions on the same page, which is what a student
    /// with four sub-tasks actually needs.
    systems: Vec<Vec<Row>>,
    /// Which system the cursor is in.
    system: usize,
    /// Which line of that system the cursor is in. Cursor paths are relative to
    /// that row.
    line: usize,
    cursor: Cursor,
    selection: Option<Selection>,
    /// The fixed end of a shift-selection, while the cursor is the moving end.
    anchor: Option<Cursor>,
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
            systems: vec![vec![Vec::new()]],
            system: 0,
            line: 0,
            cursor: Cursor::root(),
            selection: None,
            anchor: None,
            command: None,
        }
    }

    /// Build a field from an existing tree, cursor at the start.
    pub fn from_row(root: Row) -> MathField {
        MathField {
            systems: vec![vec![root]],
            ..MathField::new()
        }
    }

    /// Build a field from several rows: one equation per line.
    pub fn from_lines(lines: Vec<Row>) -> MathField {
        let lines = if lines.is_empty() {
            vec![Vec::new()]
        } else {
            lines
        };
        MathField {
            systems: vec![lines],
            ..MathField::new()
        }
    }

    // ---------------------------------------------------------------- accessors

    /// The row the cursor is in.
    pub fn root(&self) -> &Row {
        &self.systems[self.system][self.line]
    }

    fn root_mut(&mut self) -> &mut Row {
        &mut self.systems[self.system][self.line]
    }

    /// The lines of the system the cursor is in, top to bottom.
    pub fn lines(&self) -> &[Row] {
        &self.systems[self.system]
    }

    /// Every system in the document, in order.
    pub fn systems(&self) -> &[Vec<Row>] {
        &self.systems
    }

    /// Which system the cursor is in.
    pub fn system(&self) -> usize {
        self.system
    }

    /// How many systems the document holds (always at least one).
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// True when the document holds more than one system.
    pub fn is_multi_system(&self) -> bool {
        self.systems.len() > 1
    }

    /// Which line the cursor is in.
    pub fn line(&self) -> usize {
        self.line
    }

    /// How many equations the current system holds (always at least one).
    pub fn line_count(&self) -> usize {
        self.systems[self.system].len()
    }

    /// True when the current system holds more than one equation.
    pub fn is_system(&self) -> bool {
        self.systems[self.system].len() > 1
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

    /// True when any line contains an empty argument — the field is
    /// syntactically fine and algebraically incomplete (`\frac{1}{ }`).
    pub fn is_incomplete(&self) -> bool {
        self.systems()
            .iter()
            .flatten()
            .any(|row| crate::tree::row_has_empty_slot(row))
    }

    /// True when the field holds nothing.
    pub fn is_blank(&self) -> bool {
        self.systems
            .iter()
            .all(|lines| lines.iter().all(Vec::is_empty))
            && self.command.is_none()
    }

    /// Discard everything.
    pub fn clear(&mut self) {
        *self = MathField::new();
    }

    // ------------------------------------------------------------ path plumbing

    fn row_at(&self, path: &[PathStep]) -> &Row {
        row_at_ro(self.root(), path)
    }

    fn row_at_mut(&mut self, path: &[PathStep]) -> &mut Row {
        row_at_mut(self.root_mut(), path)
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

    /// Every cursor position in the current line, in document order.
    ///
    /// This *is* the navigation model: MathQuill's Left/Right walk the flattened
    /// depth-first order of slots, which is why leaving a fraction is
    /// indistinguishable from walking past it. Verified against the live editor
    /// for `\frac{1}{2}` and `x^2`.
    pub fn positions(&self) -> Vec<Cursor> {
        let mut out = Vec::new();
        collect_positions(self.root(), &mut Vec::new(), &mut out);
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
        self.anchor = None;
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
        self.anchor = None;
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
        self.anchor = None;
        if let Some(c) = self.vertical_target(true) {
            self.cursor = c;
            return true;
        }
        // At the top of a line, `↑` crosses into the equation above.
        if self.cursor.path.is_empty() && self.line > 0 {
            self.line -= 1;
            let end = row_caret_len(self.root());
            self.cursor = Cursor::at(Vec::new(), end);
            return true;
        }
        // And at the top of a system, into the previous system's last line.
        if self.cursor.path.is_empty() && self.line == 0 && self.system > 0 {
            self.system -= 1;
            self.line = self.systems[self.system].len() - 1;
            let end = row_caret_len(self.root());
            self.cursor = Cursor::at(Vec::new(), end);
            return true;
        }
        false
    }

    /// Move down one visual row. See [`MathField::move_up`].
    pub fn move_down(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        self.anchor = None;
        if let Some(c) = self.vertical_target(false) {
            self.cursor = c;
            return true;
        }
        if self.cursor.path.is_empty() && self.line + 1 < self.systems[self.system].len() {
            self.line += 1;
            self.cursor = Cursor::root();
            return true;
        }
        // The last line of a system leads into the next system.
        if self.cursor.path.is_empty()
            && self.line + 1 == self.systems[self.system].len()
            && self.system + 1 < self.systems.len()
        {
            self.system += 1;
            self.line = 0;
            self.cursor = Cursor::root();
            return true;
        }
        false
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

    /// Select the whole current line (`Ctrl-A`).
    pub fn select_all(&mut self) {
        self.finish_command();
        let end = row_caret_len(self.root());
        self.selection = Some(Selection {
            start: Cursor::root(),
            end: Cursor::at(Vec::new(), end),
        });
        self.cursor = Cursor::at(Vec::new(), end);
        // Shift-arrow after `Ctrl-A` shrinks the selection from the end, which is
        // what an anchored selection means.
        self.anchor = Some(Cursor::root());
    }

    /// Drop the selection without touching the tree.
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.anchor = None;
    }

    /// Select from the current position one step left.
    ///
    /// This is `Shift-Left`: the first call fixes an anchor, later calls keep it
    /// and move the free end. Plain movement clears both, so the anchor always
    /// means "shift was held".
    pub fn extend_left(&mut self) {
        self.extend(|field| {
            field.move_left();
        });
    }

    /// Select one step right (`Shift-Right`). See [`MathField::extend_left`].
    pub fn extend_right(&mut self) {
        self.extend(|field| {
            field.move_right();
        });
    }

    /// Select one visual row up (`Shift-Up`). See [`MathField::extend_left`].
    pub fn extend_up(&mut self) {
        self.extend(|field| {
            field.move_up();
        });
    }

    /// Select one visual row down (`Shift-Down`). See [`MathField::extend_left`].
    pub fn extend_down(&mut self) {
        self.extend(|field| {
            field.move_down();
        });
    }

    fn extend(&mut self, step: impl FnOnce(&mut Self)) {
        let anchor = self.anchor.clone().unwrap_or_else(|| self.cursor.clone());
        step(self);
        self.anchor = Some(anchor.clone());
        // A selection is a range inside one row; crossing into another slot
        // leaves it empty rather than pretending (the model has no cross-row
        // selection, and `delete_selection` assumes one row).
        self.selection = selection_between(&anchor, &self.cursor);
        if self.selection.is_none() {
            self.anchor = None;
        }
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

    /// Move the cursor to another equation, cursor at its start.
    ///
    /// Returns `false` when there is no such line, or when it is already the
    /// active one, so a click handler can treat it as "nothing changed".
    pub fn set_position(&mut self, system: usize, line: usize) -> bool {
        let Some(lines) = self.systems.get(system) else {
            return false;
        };
        if line >= lines.len() || (system == self.system && line == self.line) {
            return false;
        }
        self.finish_command();
        self.selection = None;
        self.anchor = None;
        self.system = system;
        self.line = line;
        self.cursor = Cursor::root();
        true
    }

    /// Move to another equation of the system, cursor at its start.
    ///
    /// Returns `false` when there is no such line, or when it is already the
    /// active one, so a click handler can treat it as "nothing changed".
    pub fn set_line(&mut self, line: usize) -> bool {
        if line >= self.systems[self.system].len() || line == self.line {
            return false;
        }
        self.finish_command();
        self.selection = None;
        self.line = line;
        self.cursor = Cursor::root();
        true
    }

    fn path_exists(&self, cursor: &Cursor) -> bool {
        let mut row = self.root();
        for step in &cursor.path {
            let Some(node) = row.get(step.index) else {
                return false;
            };
            let Some(next) = node.slot(step.slot) else {
                return false;
            };
            row = next;
        }
        cursor.offset <= row_caret_len(row)
    }

    fn delete_selection(&mut self) -> Row {
        match self.selection.take() {
            None => Vec::new(),
            Some(sel) => {
                let start = sel.start.offset;
                let end = sel.end.offset;
                let path = sel.start.path;
                let row = std::mem::take(self.row_at_mut(&path));
                let (left, rest) = split_row(row, start);
                let (taken, right) = split_row(rest, end.saturating_sub(start));
                let mut joined = left;
                joined.extend(right);
                *self.row_at_mut(&path) = joined;
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
                // `|` is both halves of an absolute-value pair: the second one
                // confirms the first.
                if self.close_delim(Delim::Abs) {
                    true
                } else {
                    self.open_delim(Delim::Abs)
                }
            }
            ')' | ']' | '}' => {
                let delim = match c {
                    ')' => Delim::Paren,
                    ']' => Delim::Bracket,
                    _ => Delim::Brace,
                };
                if self.selection.is_none() && self.close_delim(delim) {
                    true
                } else {
                    self.wrap_to_the_left(delim)
                }
            }
            '=' => {
                self.drop_selection();
                self.insert_equals();
                true
            }
            '~' => {
                // Typing `~` gives the relation, not a tilde character
                // (observed in the live editor).
                self.insert_sym("\\sim");
                true
            }
            // The characters MathQuill rewrites on the way in. Everything else
            // that is not a letter or an operator stays a literal character —
            // `∞`, `α`, `°` and `‰` do *not* become commands, which is easy to
            // guess wrong. [OBSERVED]
            '\u{00bd}' => {
                // ½
                self.plain_structure(Node::Frac {
                    num: vec![Node::text("1")],
                    den: vec![Node::text("2")],
                });
                true
            }
            '\u{00b2}' => {
                // ²
                self.plain_structure(Node::Sup(vec![Node::text("2")]));
                true
            }
            '\u{00b3}' => {
                // ³
                self.plain_structure(Node::Sup(vec![Node::text("3")]));
                true
            }
            '\u{00d7}' => {
                // ×
                self.insert_sym("\\times");
                true
            }
            '\u{03c0}' => {
                // π
                self.insert_sym("\\pi");
                true
            }
            '\u{2264}' => {
                // ≤
                self.insert_sym("\\le");
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
        let (mut left, right) = self.split_here();
        match left.last_mut() {
            Some(Node::Text(s)) if s.chars().last().is_some_and(|l| is_word_char(l) && is_word_char(c)) => {
                s.push(c);
            }
            _ => left.push(Node::Text(c.to_string())),
        }
        left.extend(right);
        self.write_row(left);
        self.cursor.offset = at + 1;
        self.auto_operator_name();
    }

    /// Split the cursor's row at the cursor, leaving it empty on return.
    fn split_here(&mut self) -> (Row, Row) {
        let row = std::mem::take(self.current_row_mut());
        split_row(row, self.cursor.offset)
    }

    fn write_row(&mut self, row: Row) {
        *self.current_row_mut() = row;
    }

    /// The index of the node that ends exactly at the cursor, if any.
    fn node_ending_at_cursor(&self) -> Option<usize> {
        let mut offset = 0;
        for (index, node) in self.current_row().iter().enumerate() {
            offset += node_caret_len(node);
            if offset == self.cursor.offset {
                return Some(index);
            }
        }
        None
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
        let Some(index) = self.node_ending_at_cursor() else {
            return;
        };
        let Some(name) = ({
            match self.current_row().get(index) {
                Some(Node::Text(s)) => NAMES
                    .iter()
                    .find(|n| s.ends_with(**n))
                    .map(|n| (*n).to_string()),
                _ => None,
            }
        }) else {
            return;
        };
        let prefix = {
            match self.current_row().get(index) {
                Some(Node::Text(s)) => {
                    let keep = s.chars().count() - name.chars().count();
                    s.chars().take(keep).collect::<String>()
                }
                _ => return,
            }
        };
        let (mut left, right) = self.split_here();
        // `index` is the last node of `left` by construction.
        left.pop();
        if !prefix.is_empty() {
            left.push(Node::Text(prefix));
        }
        left.push(Node::sym(format!("\\{name}")));
        left.extend(right);
        self.write_row(left);
        // The name's characters became one node, so the caret loses the
        // difference: `sin` (3) → one `\sin`, cursor still at the end.
        self.cursor.offset = at - name.chars().count() + 1;
    }

    /// `=` after `<` or `>` rewrites the pair as `\le` / `\ge`. Observed:
    /// `2<=3` → `2\le3`, `x<=y` → `x\le y`; but `!=` and `=>` stay literal.
    fn insert_equals(&mut self) {
        let at = self.cursor.offset;
        let replacement = match self.node_ending_at_cursor() {
            Some(index) => match self.current_row().get(index) {
                Some(Node::Text(s)) if s.ends_with('<') => Some("\\le"),
                Some(Node::Text(s)) if s.ends_with('>') => Some("\\ge"),
                _ => None,
            },
            None => None,
        };
        let Some(sym) = replacement else {
            self.insert_char_literal('=');
            return;
        };
        let (mut left, right) = self.split_here();
        let mut emptied = false;
        if let Some(Node::Text(s)) = left.last_mut() {
            s.pop();
            emptied = s.is_empty();
        }
        if emptied {
            left.pop();
        }
        left.push(Node::sym(sym));
        left.extend(right);
        self.write_row(left);
        // The `<` was one caret position, the `\le` is one too.
        self.cursor.offset = at;
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
        let (left, right) = self.split_here();
        let mut row = left;
        row.push(Node::text("\\"));
        row.extend(right);
        self.write_row(row);
        self.cursor.offset += 1;
        self.command = Some(String::new());
    }

    /// The node the in-progress command lives in: the text run that ends at the
    /// cursor.
    fn command_node_index(&self) -> Option<usize> {
        if self.command.is_some() {
            self.node_ending_at_cursor()
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
        // The run grew by one character, so the caret moves with it.
        self.cursor.offset += 1;
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
        let len = node_caret_len(&self.current_row()[i]);
        self.current_row_mut().remove(i);
        self.cursor.offset -= len;

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
            // `\` followed directly by a space is an escaped space, not an empty
            // command name. [OBSERVED] (`\ ` → `\ `)
            "" => vec![Node::text(" ")],
            "sqrt" => vec![Node::sqrt(Vec::new())],
            "frac" => vec![Node::Frac { num: Vec::new(), den: Vec::new() }],
            "binom" => vec![Node::Binom { top: Vec::new(), bottom: Vec::new() }],
            // Narrow integrals: observed, `\int` on its own is exactly `\int`,
            // and pressing `_` lands a script *inside* the operator's own
            // subscript. The palette's `\int_{X}^{X}` is the way to get the full
            // form, so the command itself stays a plain token. [OBSERVED] /
            // deliberately simplified.
            "int" | "sum" | "lim" | "prod" => vec![Node::sym(format!("\\{name}"))],
            n if AccentKind::from_command(n).is_some() => vec![Node::Accent {
                kind: AccentKind::from_command(n).expect("checked"),
                body: Vec::new(),
            }],
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
        let take = match self.node_ending_at_cursor() {
            Some(index) => matches!(
                self.current_row().get(index),
                Some(Node::Text(s)) if s.chars().next().is_some_and(|c| c.is_alphanumeric())
            ),
            None => false,
        };
        if !take {
            return Vec::new();
        }
        let (mut left, right) = self.split_here();
        let node = left.pop().expect("the node ends at the cursor");
        let len = node_caret_len(&node);
        left.extend(right);
        self.write_row(left);
        self.cursor.offset = at - len;
        vec![node]
    }

    /// Insert a template from the palette (or the keyboard), landing the cursor in
    /// its first empty slot.
    pub fn insert_template(&mut self, t: Template) -> Result<(), TemplateError> {
        self.finish_command();

        // `^` twice enters the existing superscript rather than nesting a new one.
        // **[INFERRED]**
        if matches!(t, Template::Power | Template::Subscript) && self.selection.is_none() {
            if let Some(index) = self.node_ending_at_cursor() {
                let exists = matches!(
                    (t, self.current_row().get(index)),
                    (Template::Power, Some(Node::Sup(_)))
                        | (Template::Subscript, Some(Node::Sub(_)))
                );
                if exists {
                    self.cursor = self.cursor.clone().pushed(index, t.content_slot().unwrap());
                    let end = {
                        let row = self.current_row();
                        row_caret_len(row)
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
        // A bracket typed into an empty field arrives with a ghost closing half —
        // the pair is drawn, but only the half you typed is real. Wrapping a
        // selection hardens both immediately: the content already delimits it.
        let ghost = if content.is_empty() {
            Ghost::Close
        } else {
            Ghost::None
        };

        let mut nodes = match t {
            Template::Frac => vec![Node::Frac { num: Vec::new(), den: Vec::new() }],
            Template::Binom => vec![Node::Binom { top: Vec::new(), bottom: Vec::new() }],
            Template::Sqrt => vec![Node::sqrt(Vec::new())],
            Template::NthRoot => vec![Node::Sqrt { index: Some(Vec::new()), body: Vec::new() }],
            Template::Power => vec![Node::Sup(Vec::new())],
            Template::Subscript => vec![Node::Sub(Vec::new())],
            Template::Paren => vec![Node::ghost_group(Delim::Paren, Vec::new(), ghost)],
            Template::Bracket => vec![Node::ghost_group(Delim::Bracket, Vec::new(), ghost)],
            Template::Abs => vec![Node::ghost_group(Delim::Abs, Vec::new(), ghost)],
            Template::Brace => vec![Node::ghost_group(Delim::Brace, Vec::new(), ghost)],
            Template::Vector => vec![Node::Accent { kind: AccentKind::Vector, body: Vec::new() }],
            Template::Overline => vec![Node::Accent { kind: AccentKind::Overline, body: Vec::new() }],
            Template::Underline => {
                vec![Node::Accent { kind: AccentKind::UnderRightArrow, body: Vec::new() }]
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

    /// Apply a palette entry straight from its Abitti `insert` string.
    ///
    /// The palette is data — every key carries the LaTeX-ish command Abitti's own
    /// editor executes — so the field reads the same string instead of keeping a
    /// second table of special cases. See [`crate::snippet`].
    pub fn apply_snippet(&mut self, snippet: &str) -> Result<(), crate::SnippetError> {
        let nodes = crate::snippet::parse(snippet)?;
        self.finish_command();
        self.drop_selection();
        self.insert_constructed(nodes, None);
        Ok(())
    }

    /// Build a field from LaTeX, for *display*: a solver result, a history entry.
    ///
    /// This is the palette's snippet language with the extra conventions the
    /// printer uses (`\,`, `\quad`, `\operatorname{…}`), which is enough to read
    /// anything [`canlab_tex::to_latex`] writes. It is not a general LaTeX
    /// parser: anything outside that subset is refused rather than guessed at.
    pub fn from_latex(latex: &str) -> Result<MathField, crate::SnippetError> {
        let row = crate::snippet::parse(latex)?;
        Ok(MathField::from_row(row))
    }

    /// Build a field from several lines of LaTeX.
    pub fn from_latex_lines(lines: &[String]) -> Result<MathField, crate::SnippetError> {
        let rows: Result<Vec<Row>, crate::SnippetError> =
            lines.iter().map(|line| crate::snippet::parse(line)).collect();
        Ok(MathField::from_lines(rows?))
    }

    /// Insert already-built nodes at the cursor and place the cursor in the first
    /// empty slot of the inserted structures.
    fn insert_constructed(&mut self, nodes: Vec<Node>, content_slot: Option<Slot>) {
        let at = self.cursor.offset;
        let base_path = self.cursor.path.clone();
        let count = nodes.len();
        let added: usize = nodes.iter().map(node_caret_len).sum();
        let (left, right) = self.split_here();
        let first = left.len();
        let mut row = left;
        row.extend(nodes);
        row.extend(right);
        self.write_row(row);
        self.cursor = template_target(self, &base_path, first, count, content_slot, at + added);
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

    /// Confirm the bracket the cursor is at the end of.
    ///
    /// Two things happen at once, and both were observed: a ghost delimiter
    /// becomes real, and the cursor steps outside the pair. Returns `false` when
    /// there is no matching bracket to close, which is the case that falls through
    /// to [`MathField::wrap_to_the_left`].
    fn close_delim(&mut self, delim: Delim) -> bool {
        let Some(last) = self.cursor.path.last().copied() else {
            return false;
        };
        if last.slot != Slot::Body {
            return false;
        }
        let parent = self.cursor.path[..self.cursor.path.len() - 1].to_vec();
        let (matches, len) = match self.row_at(&parent).get(last.index) {
            Some(Node::Group { delim: d, body, .. }) => (*d == delim, row_caret_len(body)),
            _ => return false,
        };
        if !matches || self.cursor.offset != len {
            return false;
        }
        if let Some(Node::Group { ghost, .. }) = self.row_at_mut(&parent).get_mut(last.index) {
            *ghost = Ghost::None;
        }
        self.cursor = Cursor::at(parent, last.index + 1);
        true
    }

    /// A closer with nothing to close: it swallows everything to its left in the
    /// block.
    ///
    /// Not "the trailing run" — the live editor wraps the whole block, so `1+2)`
    /// is `\left(1+2\right)`, not `1+\left(2\right)`, and `()]` is
    /// `\left[\left(\right)\right]`. MathQuill's brackets are "first typed as
    /// one-sided bracket with matching ghost bracket at far end of current
    /// block", and the far end of the block is where the content starts.
    /// [OBSERVED]
    fn wrap_to_the_left(&mut self, delim: Delim) -> bool {
        let taken = if self.selection.is_some() {
            self.delete_selection()
        } else {
            let (left, right) = self.split_here();
            self.write_row(right);
            self.cursor.offset = 0;
            left
        };
        self.insert_constructed(vec![Node::ghost_group(delim, taken, Ghost::Open)], None);
        true
    }

    /// Insert a finished node at the cursor: the cursor ends up after it.
    fn plain_structure(&mut self, node: Node) {
        self.finish_command();
        self.drop_selection();
        self.insert_constructed(vec![node], None);
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
        // At the very start of an empty line: fold the line away again.
        if self.cursor.path.is_empty() && self.cursor.offset == 0 && self.remove_empty_line() {
            return;
        }
        let at = self.cursor.offset;
        if at > 0 {
            let (mut left, right) = self.split_here();
            match left.last_mut() {
                Some(Node::Text(s)) if s.chars().count() > 1 => {
                    s.pop();
                }
                Some(_) => {
                    left.pop();
                }
                None => {}
            }
            left.extend(right);
            self.write_row(left);
            self.cursor.offset = at - 1;
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
        let len = node_caret_len(&self.current_row()[i]);
        if empty {
            self.command = None;
            self.current_row_mut().remove(i);
            self.cursor.offset -= len;
        } else {
            let raw = format!("\\{}", self.command.as_ref().expect("checked"));
            if let Node::Text(s) = &mut self.current_row_mut()[i] {
                *s = raw;
            }
            self.cursor.offset -= 1;
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
            let before = caret_before(self.row_at(&parent_path), last.index);
            self.cursor = Cursor::at(parent_path, before);
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
        let offset = {
            let row = self.row_at_mut(&parent_path);
            let before = caret_before(row, last.index);
            let mut at = last.index;
            for n in contents {
                row.insert(at, n);
                at += 1;
            }
            let junction_carets: usize = row[last.index..last.index + junction]
                .iter()
                .map(node_caret_len)
                .sum();
            before + junction_carets
        };
        self.cursor = Cursor::at(parent_path, offset);
    }

    /// Forward delete: one character, or one whole node.
    pub fn delete_forward(&mut self) {
        self.finish_command();
        if self.selection.is_some() {
            self.drop_selection();
            return;
        }
        let (left, mut right) = self.split_here();
        match right.first_mut() {
            Some(Node::Text(s)) if s.chars().count() > 1 => {
                s.remove(0);
            }
            Some(_) => {
                right.remove(0);
            }
            None => {}
        }
        let mut row = left;
        row.extend(right);
        self.write_row(row);
    }

    // -------------------------------------------------------------- hand-off keys

    /// `Esc`: leave every structure and report whether there was one.
    pub fn escape(&mut self) -> EscapeOutcome {
        self.finish_command();
        let was_nested = self.cursor.is_nested();
        self.selection = None;
        let end = row_caret_len(self.root());
        self.cursor = Cursor::at(Vec::new(), end);
        if was_nested {
            EscapeOutcome::LeftStructure
        } else {
            EscapeOutcome::AtRoot
        }
    }

    /// `Enter`: the next equation of the same system.
    ///
    /// Abitti's `Enter` commits the formula into an inline editor; this field *is*
    /// the editor, so the same key does the thing a student means by it: it starts
    /// a new line, and the lines are read together as one system.
    pub fn new_line(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        self.systems[self.system].insert(self.line + 1, Vec::new());
        self.line += 1;
        self.cursor = Cursor::root();
        true
    }

    /// `Enter` in an inline editor: hand back the LaTeX and empty the field.
    ///
    /// Kept because it is the observed Abitti behaviour for a field embedded in a
    /// document, and it is the honest definition of "commit". The GUI uses
    /// [`MathField::new_line`] instead.
    pub fn commit(&mut self) -> String {
        self.finish_command();
        let latex = self.to_latex();
        *self = MathField::new();
        latex
    }

    /// Drop a line that is empty, when the cursor is on it and it is not the only
    /// one. Used by Backspace so a system can be shortened again.
    fn remove_empty_line(&mut self) -> bool {
        if !self.root().is_empty() {
            return false;
        }
        // Another line in this system: fold it away.
        if self.systems[self.system].len() > 1 && self.line > 0 {
            self.systems[self.system].remove(self.line);
            self.line -= 1;
            let end = row_caret_len(self.root());
            self.cursor = Cursor::at(Vec::new(), end);
            return true;
        }
        // A whole empty system folds into the one above it.
        if self.systems.len() > 1 && self.line == 0 && self.systems[self.system].len() == 1 {
            self.systems.remove(self.system);
            self.system -= 1;
            self.line = self.systems[self.system].len() - 1;
            let end = row_caret_len(self.root());
            self.cursor = Cursor::at(Vec::new(), end);
            return true;
        }
        false
    }

    /// Start a new, separate system below the current one.
    ///
    /// A page of sub-tasks is several systems, not one long one: they are solved
    /// and plotted apart, so the document keeps them apart.
    pub fn new_system(&mut self) -> bool {
        self.finish_command();
        self.selection = None;
        self.anchor = None;
        self.systems.push(vec![Vec::new()]);
        self.system = self.systems.len() - 1;
        self.line = 0;
        self.cursor = Cursor::root();
        true
    }

    // ------------------------------------------------------------------- output

    /// Abitti-flavoured LaTeX for the whole document. See [`crate::latex`].
    ///
    /// One system of one line is just that line. A system of several lines is a
    /// `cases` environment, which is Abitti's own vocabulary for a system, and
    /// several systems are those blocks separated by `\quad`.
    pub fn to_latex(&self) -> String {
        if self.systems.len() == 1 && self.systems[0].len() == 1 {
            return crate::latex::emit(&self.systems[0][0]);
        }
        let blocks: Vec<String> = self.systems.iter().map(|lines| block(lines)).collect();
        blocks.join("\\quad ")
    }

    /// Convert the current line to a `canlab-core` expression, or explain why not.
    ///
    /// A system has no single expression; use [`MathField::build_lines`] for one.
    pub fn build_expr(&self) -> Result<crate::Expr, crate::ToExprError> {
        if self.systems.len() != 1 || self.systems[0].len() != 1 {
            return Err(crate::ToExprError::System);
        }
        crate::expr::build(&self.systems[0][0])
    }

    /// Convert every line to an expression: the equations of the document.
    ///
    /// Fails with the first line's error, so the caller can point at it.
    pub fn build_lines(&self) -> Result<Vec<crate::Expr>, crate::ToExprError> {
        self.systems
            .iter()
            .flatten()
            .map(|row| crate::expr::build(row))
            .collect()
    }

    /// Read every line as an *equation*: the two sides of its `=`.
    ///
    /// A system is written `x+y=3`, so each line has a relation in it, which
    /// [`MathField::build_expr`] refuses by design — a relation is not a value.
    /// The right-hand side is `None` for a line with no `=`, which is legal while
    /// someone is still typing.
    pub fn build_equations(
        &self,
    ) -> Result<Vec<(crate::Expr, Option<crate::Expr>)>, crate::ToExprError> {
        self.systems
            .iter()
            .flatten()
            .filter(|row| row_caret_len(row) > 0)
            .map(|row| equation(row))
            .collect()
    }

    /// The equations of one system, so a caller can plot or solve them apart.
    pub fn build_system_equations(
        &self,
        system: usize,
    ) -> Result<Vec<(crate::Expr, Option<crate::Expr>)>, crate::ToExprError> {
        let Some(lines) = self.systems.get(system) else {
            return Err(crate::ToExprError::EmptySlot);
        };
        lines
            .iter()
            .filter(|row| row_caret_len(row) > 0)
            .map(|row| equation(row))
            .collect()
    }

    /// Convert to a `canlab-core` expression when the field is complete enough.
    ///
    /// Returns `None` for a half-typed field (`\frac{1}{ }`), for a base-less
    /// superscript, and for constructs with no algebraic meaning (subscripts,
    /// binomials, relations).
    pub fn to_expr(&self) -> Option<crate::Expr> {
        self.build_expr().ok()
    }

    /// The same expression printed by `canlab-tex`, for callers that want the CAS's
    /// own rendering rather than the field's.
    pub fn to_expr_latex(&self) -> Option<String> {
        self.to_expr().map(|e| canlab_tex::to_latex(&e))
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
    let mut offset = 0;
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
        let len = node_caret_len(node);
        if !followed_by_script {
            // One position *between* every pair of characters — a text run is
            // not one indivisible atom. This is the difference between "the
            // caret is between nodes" and what the live editor does.
            for step in 1..=len {
                out.push(Cursor::at(path.clone(), offset + step));
            }
        }
        offset += len;
    }
}

/// How many caret positions a node contributes: one per character for a text
/// run, one for a structure.
fn node_caret_len(node: &Node) -> usize {
    match node {
        Node::Text(s) => s.chars().count(),
        _ => 1,
    }
}

/// How many caret positions a row has (its offsets run `0..=row_caret_len`).
pub fn row_caret_len(row: &Row) -> usize {
    row.iter().map(node_caret_len).sum()
}

/// One system as LaTeX: the line itself, or a `cases` block when it has several.
fn block(lines: &[Row]) -> String {
    if lines.len() == 1 {
        return crate::latex::emit(&lines[0]);
    }
    let rows: Vec<String> = lines.iter().map(crate::latex::emit).collect();
    format!("\\begin{{cases}} {} \\end{{cases}}", rows.join(" \\\\ "))
}

/// One line as an equation: everything before its `=` and everything after.
fn equation(row: &Row) -> Result<(crate::Expr, Option<crate::Expr>), crate::ToExprError> {
    let mut lhs: Vec<Node> = Vec::new();
    let mut rhs: Option<Vec<Node>> = None;
    for node in row {
        match (&mut rhs, node) {
            (None, Node::Text(text)) if text == "=" => rhs = Some(Vec::new()),
            (None, node) => lhs.push(node.clone()),
            (Some(side), node) => side.push(node.clone()),
        }
    }
    let lhs = crate::expr::build(&lhs)?;
    let rhs = match rhs {
        Some(side) => Some(crate::expr::build(&side)?),
        None => None,
    };
    Ok((lhs, rhs))
}

/// The caret offset immediately before node `index` of `row`.
fn caret_before(row: &Row, index: usize) -> usize {
    row[..index.min(row.len())].iter().map(node_caret_len).sum()
}

/// The selection between two cursors, or `None` when they are in different rows
/// or on the same spot.
fn selection_between(a: &Cursor, b: &Cursor) -> Option<Selection> {
    if a.path != b.path || a.offset == b.offset {
        return None;
    }
    let (start, end) = if a.offset < b.offset {
        (a.offset, b.offset)
    } else {
        (b.offset, a.offset)
    };
    Some(Selection {
        start: Cursor::at(a.path.clone(), start),
        end: Cursor::at(a.path.clone(), end),
    })
}

/// Split a row at a caret offset.
///
/// The split can fall *inside* a text run, which is the whole point: `12|3`
/// becomes `["12"]` and `["3"]`.
fn split_row(row: Row, at: usize) -> (Row, Row) {
    let mut left: Row = Vec::new();
    let mut right: Row = Vec::new();
    let mut offset = 0;
    let mut splitting = true;
    for node in row {
        let len = node_caret_len(&node);
        if splitting && offset + len <= at {
            left.push(node);
            offset += len;
            continue;
        }
        if splitting && offset < at {
            // The offset lands inside this node; only text can be cut.
            if let Node::Text(s) = &node {
                let chars: Vec<char> = s.chars().collect();
                let cut = (at - offset).min(chars.len());
                let (head, tail) = chars.split_at(cut);
                let head: String = head.iter().collect();
                let tail: String = tail.iter().collect();
                if !head.is_empty() {
                    left.push(Node::Text(head));
                }
                if !tail.is_empty() {
                    right.push(Node::Text(tail));
                }
                offset += len;
                splitting = false;
                continue;
            }
            left.push(node);
            offset += len;
            continue;
        }
        splitting = false;
        right.push(node);
        offset += len;
    }
    (left, right)
}

/// Where the cursor goes after inserting `count` nodes at `at`.
///
/// The rule, which reproduces every palette row that was measured: **the first
/// still-empty slot of the inserted structure, in document order; failing that,
/// the end of the content slot.**
fn template_target(
    field: &MathField,
    base_path: &[PathStep],
    first: usize,
    count: usize,
    content_slot: Option<Slot>,
    caret_after: usize,
) -> Cursor {
    let row = field.row_at(base_path);
    for k in 0..count {
        let node = &row[first + k];
        for slot in node.slots() {
            if node.slot(slot).is_some_and(|r| r.is_empty()) {
                let mut path = base_path.to_vec();
                path.push(PathStep::new(first + k, slot));
                return Cursor::at(path, 0);
            }
        }
    }
    if let Some(slot) = content_slot {
        let node = &row[first];
        if let Some(r) = node.slot(slot) {
            let mut path = base_path.to_vec();
            path.push(PathStep::new(first, slot));
            return Cursor::at(path, row_caret_len(r));
        }
    }
    Cursor::at(base_path.to_vec(), caret_after)
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
