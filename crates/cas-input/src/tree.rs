//! The editable tree.
//!
//! The shape mirrors MathQuill's DOM: a [`Row`] is a horizontal sequence (a
//! "block"), and every construct owns one or more rows as its arguments. An empty
//! `Row` is the empty argument slot that `\frac{}{}` needs and that
//! [`cas_core::Expr`] cannot express.

/// A horizontal sequence of nodes — a MathQuill block.
pub type Row = Vec<Node>;

/// Which argument of a construct a [`Row`] is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Slot {
    /// Numerator of a fraction or binomial.
    Num,
    /// Denominator of a fraction or binomial.
    Den,
    /// Optional index of a root: the `n` in `\sqrt[n]{x}`.
    Index,
    /// Radicand, bracket content, accent content.
    Body,
    /// Superscript argument of [`Node::Sup`].
    Sup,
    /// Subscript argument of [`Node::Sub`].
    Sub,
}

/// A scalable delimiter pair. Always emitted as `\left…\right`, which is what
/// MathQuill writes whether you typed the delimiter or used the palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Delim {
    Paren,
    Bracket,
    Abs,
    Brace,
}

impl Delim {
    /// The opening half, as LaTeX.
    pub fn open(self) -> &'static str {
        match self {
            Delim::Paren => "\\left(",
            Delim::Bracket => "\\left[",
            Delim::Abs => "\\left|",
            Delim::Brace => "\\left\\{",
        }
    }

    /// The closing half, as LaTeX.
    pub fn close(self) -> &'static str {
        match self {
            Delim::Paren => "\\right)",
            Delim::Bracket => "\\right]",
            Delim::Abs => "\\right|",
            Delim::Brace => "\\right\\}",
        }
    }

    /// The delimiter a typed character would open, if any.
    pub fn opening(c: char) -> Option<Delim> {
        match c {
            '(' => Some(Delim::Paren),
            '[' => Some(Delim::Bracket),
            '|' => Some(Delim::Abs),
            '{' => Some(Delim::Brace),
            _ => None,
        }
    }

    /// Whether `c` is this delimiter's closing character.
    pub fn is_closed_by(self, c: char) -> bool {
        matches!(
            (self, c),
            (Delim::Paren, ')') | (Delim::Bracket, ']') | (Delim::Abs, '|') | (Delim::Brace, '}')
        )
    }
}

/// A one-argument decoration drawn above or below its body.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AccentKind {
    /// `\overrightarrow{…}`
    Vector,
    /// `\overline{…}`
    Overline,
    /// `\underrightarrow{…}`
    Underline,
}

impl AccentKind {
    /// The LaTeX command name, without the argument.
    pub fn command(self) -> &'static str {
        match self {
            AccentKind::Vector => "\\overrightarrow",
            AccentKind::Overline => "\\overline",
            AccentKind::Underline => "\\underrightarrow",
        }
    }
}

/// One node of the editable tree.
///
/// `Sup`/`Sub` are *decorations on the preceding sibling*, not containers with a
/// base of their own. That is not a shortcut: MathQuill stores `x^2` as two
/// siblings and lets `^` on an empty field produce `^{ }` with no base at all
/// (observed in the live editor), so the base-less case has to be representable.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum Node {
    /// A run of plain characters. Word characters (`a`–`z`, `0`–`9`, `.`) run
    /// together; every operator character gets its own node, which is what makes
    /// `/` able to take "the trailing character run" unambiguously.
    Text(String),
    /// A LaTeX command token, emitted verbatim: `\alpha`, `\le`, `\cdot`, `\sin`.
    Sym(String),
    /// `\frac{num}{den}`
    Frac { num: Row, den: Row },
    /// `\binom{top}{bottom}`
    Binom { top: Row, bottom: Row },
    /// `\sqrt{body}` or `\sqrt[index]{body}`. `index: None` means the optional
    /// index does not exist at all — it is not an empty slot and the cursor cannot
    /// reach it (`\sqrt{y}`, not `\sqrt[]{y}`).
    Sqrt { index: Option<Row>, body: Row },
    /// A superscript attached to the preceding sibling, if any.
    Sup(Row),
    /// A subscript attached to the preceding sibling, if any.
    Sub(Row),
    /// `\left(…\right)` and friends.
    Group { delim: Delim, body: Row },
    /// A vector arrow, overline or underarrow.
    Accent { kind: AccentKind, body: Row },
}

impl Node {
    /// Convenience constructor for a plain-character run.
    pub fn text(s: impl Into<String>) -> Node {
        Node::Text(s.into())
    }

    /// Convenience constructor for a command token.
    pub fn sym(s: impl Into<String>) -> Node {
        Node::Sym(s.into())
    }

    /// A root with no optional index.
    pub fn sqrt(body: Row) -> Node {
        Node::Sqrt { index: None, body }
    }

    /// The argument rows of this node, in document (left-to-right) order.
    ///
    /// The order matters: it is the order in which Left/Right visit them, and it
    /// is why `\nthroot` puts the cursor in the index while `\sqrt` puts it in the
    /// body.
    pub fn slots(&self) -> Vec<Slot> {
        match self {
            Node::Text(_) | Node::Sym(_) => Vec::new(),
            Node::Frac { .. } | Node::Binom { .. } => vec![Slot::Num, Slot::Den],
            Node::Sqrt { index, .. } => match index {
                Some(_) => vec![Slot::Index, Slot::Body],
                None => vec![Slot::Body],
            },
            Node::Sup(_) => vec![Slot::Sup],
            Node::Sub(_) => vec![Slot::Sub],
            Node::Group { .. } | Node::Accent { .. } => vec![Slot::Body],
        }
    }

    /// Borrow one argument row.
    pub fn slot(&self, slot: Slot) -> Option<&Row> {
        match (self, slot) {
            (Node::Frac { num, .. }, Slot::Num) => Some(num),
            (Node::Frac { den, .. }, Slot::Den) => Some(den),
            (Node::Binom { top, .. }, Slot::Num) => Some(top),
            (Node::Binom { bottom, .. }, Slot::Den) => Some(bottom),
            (Node::Sqrt { index, .. }, Slot::Index) => index.as_ref(),
            (Node::Sqrt { body, .. }, Slot::Body) => Some(body),
            (Node::Sup(row), Slot::Sup) => Some(row),
            (Node::Sub(row), Slot::Sub) => Some(row),
            (Node::Group { body, .. }, Slot::Body) => Some(body),
            (Node::Accent { body, .. }, Slot::Body) => Some(body),
            _ => None,
        }
    }

    /// Mutably borrow one argument row.
    pub fn slot_mut(&mut self, slot: Slot) -> Option<&mut Row> {
        match (self, slot) {
            (Node::Frac { num, .. }, Slot::Num) => Some(num),
            (Node::Frac { den, .. }, Slot::Den) => Some(den),
            (Node::Binom { top, .. }, Slot::Num) => Some(top),
            (Node::Binom { bottom, .. }, Slot::Den) => Some(bottom),
            (Node::Sqrt { index, .. }, Slot::Index) => index.as_mut(),
            (Node::Sqrt { body, .. }, Slot::Body) => Some(body),
            (Node::Sup(row), Slot::Sup) => Some(row),
            (Node::Sub(row), Slot::Sub) => Some(row),
            (Node::Group { body, .. }, Slot::Body) => Some(body),
            (Node::Accent { body, .. }, Slot::Body) => Some(body),
            _ => None,
        }
    }

    /// True for a leaf that holds characters rather than structure.
    pub fn is_leaf(&self) -> bool {
        matches!(self, Node::Text(_) | Node::Sym(_))
    }

    /// True when this node has an argument that is still empty anywhere inside it.
    ///
    /// This is the predicate behind "`to_expr` refuses a half-typed field": a
    /// fraction with an empty numerator is perfectly good LaTeX and perfectly bad
    /// algebra.
    pub fn has_empty_slot(&self) -> bool {
        self.slots()
            .into_iter()
            .any(|s| self.slot(s).is_some_and(row_has_empty_slot))
    }
}

/// True when a row is blank, or contains any node with an empty argument.
pub fn row_has_empty_slot(row: &Row) -> bool {
    row.is_empty() || row.iter().any(Node::has_empty_slot)
}

/// True when `c` may extend a run of plain characters rather than starting a new
/// node. Kept deliberately narrow: operators must stay separate so that `/` can
/// take "the trailing character run" and stop at `+` (observed: `1+2` → `1+\frac{2}{}`,
/// while `xy` → `\frac{xy}{}`).
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '.'
}
