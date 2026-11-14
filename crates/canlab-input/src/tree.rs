//! The editable tree.
//!
//! The shape mirrors MathQuill's DOM: a [`Row`] is a horizontal sequence (a
//! "block"), and every construct owns one or more rows as its arguments. An empty
//! `Row` is the empty argument slot that `\frac{}{}` needs and that
//! [`canlab_core::Expr`] cannot express.

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

/// Which half of a bracket pair has not been typed yet.
///
/// MathQuill draws a bracket pair as soon as *either* half is typed, and the half
/// you have not typed is a **ghost**: it is visible, it is part of the LaTeX, and
/// it disappears into a real delimiter the moment you type it yourself. The source
/// says it plainly — a bracket is "first typed as one-sided bracket with matching
/// 'ghost' bracket at far end of current block, until you type an opposing one"
/// (`src/commands/math/commands.js`). See `docs/mathquill-interaction.md` §2.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum Ghost {
    /// Both delimiters are real: `\left(x\right)` typed in full.
    #[default]
    None,
    /// The opening delimiter is a ghost — it appeared because a stray `)` was
    /// typed and swallowed the run to its left.
    Open,
    /// The closing delimiter is a ghost — it appeared because `(` was typed.
    Close,
}

impl Ghost {
    /// True when the opening delimiter is drawn as a ghost.
    pub fn is_open(self) -> bool {
        matches!(self, Ghost::Open)
    }

    /// True when the closing delimiter is drawn as a ghost.
    pub fn is_close(self) -> bool {
        matches!(self, Ghost::Close)
    }
}

/// A one-argument decoration drawn above or below its body.
///
/// The set is not invented: each of these was typed into the live Abitti editor
/// and watched (`docs/mathquill-interaction.md` §4.3). `\ddot`, `\widehat`,
/// `\widetilde`, `\overbrace` and `\underbrace` are *not* commands there — they
/// come out as `\text{ddot}` and friends — so they are deliberately absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AccentKind {
    /// `\overrightarrow{…}`
    Vector,
    /// `\vec{…}` — the same arrow, spelled the short way.
    Vec,
    /// `\overleftarrow{…}`
    OverLeftArrow,
    /// `\overline{…}` — a rule across the body.
    Overline,
    /// `\bar{…}` — the same bar, in practice.
    Bar,
    /// `\hat{…}`
    Hat,
    /// `\tilde{…}`
    Tilde,
    /// `\dot{…}`
    Dot,
    /// `\underline{…}` — a rule under the body.
    Underline,
    /// `\underrightarrow{…}`
    UnderRightArrow,
}

impl AccentKind {
    /// The LaTeX command name, without the argument.
    pub fn command(self) -> &'static str {
        match self {
            AccentKind::Vector => "\\overrightarrow",
            AccentKind::Vec => "\\vec",
            AccentKind::OverLeftArrow => "\\overleftarrow",
            AccentKind::Overline => "\\overline",
            AccentKind::Bar => "\\bar",
            AccentKind::Hat => "\\hat",
            AccentKind::Tilde => "\\tilde",
            AccentKind::Dot => "\\dot",
            AccentKind::Underline => "\\underline",
            AccentKind::UnderRightArrow => "\\underrightarrow",
        }
    }

    /// The command a `\name` stands for, if it is an accent at all.
    pub fn from_command(name: &str) -> Option<AccentKind> {
        Some(match name {
            "overrightarrow" => AccentKind::Vector,
            "vec" => AccentKind::Vec,
            "overleftarrow" => AccentKind::OverLeftArrow,
            "overline" => AccentKind::Overline,
            "bar" => AccentKind::Bar,
            "hat" => AccentKind::Hat,
            "tilde" => AccentKind::Tilde,
            "dot" => AccentKind::Dot,
            "underline" => AccentKind::Underline,
            "underrightarrow" => AccentKind::UnderRightArrow,
            _ => return None,
        })
    }

    /// The glyph drawn above the body, if this accent is a glyph rather than a
    /// rule. `None` means "draw a rule", and which side it goes on is
    /// [`AccentKind::is_below`].
    pub fn mark(self) -> Option<char> {
        match self {
            AccentKind::Vector | AccentKind::Vec | AccentKind::UnderRightArrow => Some('→'),
            AccentKind::OverLeftArrow => Some('←'),
            AccentKind::Hat => Some('^'),
            AccentKind::Tilde => Some('~'),
            AccentKind::Dot => Some('·'),
            AccentKind::Overline | AccentKind::Bar | AccentKind::Underline => None,
        }
    }

    /// True when the decoration hangs under the body, as in `\underline`.
    pub fn is_below(self) -> bool {
        matches!(self, AccentKind::Underline | AccentKind::UnderRightArrow)
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
    /// `\left(…\right)` and friends, with either half possibly still a ghost.
    Group {
        delim: Delim,
        body: Row,
        ghost: Ghost,
    },
    /// A vector arrow, overline or underarrow.
    Accent { kind: AccentKind, body: Row },
    /// Pre-rendered LaTeX that the field holds as one unit.
    ///
    /// A definite integral's differential and a limit's approach are notation,
    /// not expressions: `dx` is two letters and one symbol, and `x \to 0` is
    /// three tokens that mean nothing on their own. The field's structural
    /// model has no node for either, and giving it one would mean a cursor rule,
    /// a template and a slot for a construct that is never edited character by
    /// character. A `Raw` is the smallest thing that lets the field *hold* such
    /// a statement: it emits its text verbatim and refuses to become an
    /// expression, because the parser is what reads it.
    Raw(String),
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

    /// A bracket pair with both delimiters real.
    pub fn group(delim: Delim, body: Row) -> Node {
        Node::Group {
            delim,
            body,
            ghost: Ghost::None,
        }
    }

    /// A bracket pair with one half still a ghost.
    pub fn ghost_group(delim: Delim, body: Row, ghost: Ghost) -> Node {
        Node::Group { delim, body, ghost }
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
            Node::Raw(_) => Vec::new(),
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
            (Node::Raw(_), _) => None,
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
            (Node::Raw(_), _) => None,
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
