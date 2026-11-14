//! The palette's `insert` language → editable tree nodes.
//!
//! Abitti's palette is data: every key carries an `insert` string, and the
//! formula editor runs it as a MathQuill command (`mq[fn ?? 'cmd'](action)` in
//! `src/latexCommands.ts`). `cmd` parses anything starting with `\` as LaTeX and
//! types anything else keystroke by keystroke, which is why clicking a button can
//! either build a surface (`a/b` → a real fraction) or add a token (`\alpha`).
//!
//! Our [`crate::tree`] is close enough to LaTeX that the same strings can be read
//! as a tiny language instead of a table of special cases:
//!
//! * `X` is the cursor marker the palette data uses — it becomes an empty slot;
//! * `\frac{X}{X}`, `\sqrt[X]{X}`, `\binom{X}{X}` build the structures;
//! * `\left(…\right)` and friends build a bracket pair;
//! * `\int_{X}^{X}` is a symbol followed by its own limit decorations;
//! * `^` / `_` before their argument attach a script, which is how `²` (`^2`)
//!   and `xₙ` (`x_X`) arrive;
//! * any other `\name` is a command token, exactly what typing it would give.
//!
//! Anything the model genuinely lacks — tables (`\begin{cases}`), a real
//! `\overleftarrow` — is refused rather than silently pasted as text, so the
//! caller can say "that one is not modelled" instead of putting raw LaTeX in the
//! field.

use crate::tree::{AccentKind, Delim, Ghost, Node, Row};

/// Why a palette snippet could not be read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SnippetError {
    /// A construct this editor does not model.
    Unsupported(String),
    /// A snippet that does not parse: an unclosed argument, a missing `\right`.
    Malformed(String),
}

impl std::fmt::Display for SnippetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnippetError::Unsupported(what) => write!(f, "{what} is not modelled"),
            SnippetError::Malformed(what) => write!(f, "malformed snippet: {what}"),
        }
    }
}

impl std::error::Error for SnippetError {}

/// Whether a line is a calculus statement, which the field holds as one unit.
///
/// `\int_a^b f\,dx`, `\lim_{x\to a} f`, `\vol_a^b y\,dx` and `\area_a^b f, g`
/// are notation, not expressions: a differential is not a product and an
/// approach is not an argument. The field's structural model has no node for
/// them, so a line that starts with one of these commands is held as a single
/// [`Node::Raw`] and read by `canlab-parse`, which is the one LaTeX grammar that
/// knows them. Anything else is read structurally, exactly as before.
pub fn is_calculus_statement(line: &str) -> bool {
    let trimmed = line.trim_start();
    let starts_with_command = ["\\int", "\\lim", "\\vol", "\\area"].iter().any(|command| {
        trimmed.starts_with(command)
            && !trimmed[command.len()..].starts_with(|c: char| c.is_ascii_alphabetic())
    });
    // An ODE line — `y' = -k*y` or `y(0) = 1` — is a statement too, and for the
    // same reason: no `Expr` node holds a derivative or an initial condition, so
    // the row is held whole and `canlab-ode` reads it. The apostrophe is the
    // marker; a line with an `X` slot is still being typed and stays structural.
    if !starts_with_command && is_ode_line(trimmed) {
        return !trimmed.contains('X');
    }
    // A line still being *typed* is not a statement. The palette inserts
    // `\int_{X}^{X}`, whose `X`s are empty slots the cursor lands in, and a row
    // with an empty slot is a construct under construction — the structural
    // reader owns it until the slots are filled. Only a line with no `X` left is
    // a finished statement, and only then does the parser read it.
    starts_with_command && !trimmed.contains('X')
}

/// Whether a line is an ODE statement: `y' = …` or `y(0) = …`.
///
/// A derivative line is `name` followed by `'` and an `=`; an initial condition is
/// `name(…)` and an `=`. Both are held whole, because an `Expr` has no node for
/// either — the same reason `\int` is held whole.
fn is_ode_line(line: &str) -> bool {
    let Some((left, _right)) = line.split_once('=') else {
        return false;
    };
    let left = left.trim();
    // `y'` — a name, an apostrophe, nothing else.
    if let Some(name) = left.strip_suffix('\'') {
        let name = name.trim();
        return !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    }
    // `y(0)` — a name and a parenthesised argument.
    if let Some(open) = left.find('(')
        && left.ends_with(')')
    {
        let name = left[..open].trim();
        return !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && left[open + 1..left.len() - 1].trim().parse::<f64>().is_ok();
    }
    false
}

/// Read one palette `insert` string into the nodes it stands for.
pub fn parse(snippet: &str) -> Result<Row, SnippetError> {
    if is_calculus_statement(snippet) {
        return Ok(vec![crate::tree::Node::Raw(snippet.trim().to_string())]);
    }
    let mut parser = Parser {
        chars: snippet.chars().collect(),
        pos: 0,
    };
    let row = parser.row()?;
    if parser.pos < parser.chars.len() {
        return Err(SnippetError::Malformed(format!(
            "unexpected {:?}",
            parser.chars[parser.pos]
        )));
    }
    Ok(row)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn eat(&mut self, want: char) -> bool {
        if self.peek() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, want: char) -> Result<(), SnippetError> {
        if self.eat(want) {
            Ok(())
        } else {
            Err(SnippetError::Malformed(format!("expected {want:?}")))
        }
    }

    /// True when the input at the cursor is `\name` followed by a non-letter.
    fn at_command(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let mut i = self.pos + 1;
        for want in name.chars() {
            if self.chars.get(i) != Some(&want) {
                return false;
            }
            i += 1;
        }
        !self.chars.get(i).is_some_and(|c| c.is_alphabetic())
    }

    fn letters(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if !c.is_ascii_alphabetic() {
                break;
            }
            name.push(c);
            self.pos += 1;
        }
        name
    }

    /// A `{ … }` argument.
    fn braced(&mut self) -> Result<Row, SnippetError> {
        self.expect('{')?;
        let row = self.row()?;
        self.expect('}')?;
        Ok(row)
    }

    /// A whole row, stopping at any closer the caller owns.
    fn row(&mut self) -> Result<Row, SnippetError> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                '}' | ']' | ')' => break,
                ' ' | '\t' => {
                    self.pos += 1;
                }
                // A bare bracket is a group, exactly as typing one is. The
                // printer writes `\left(…\right)`, but a hand-written or
                // externally produced `(a+b)^2` has to read too.
                // `/` is the fraction key, as it is when typing: the run to its
                // left becomes the numerator and what follows the denominator.
                '/' => {
                    self.pos += 1;
                    let numerator = match out.pop() {
                        Some(node) => vec![node],
                        None => Vec::new(),
                    };
                    let denominator = self.script_argument()?;
                    out.push(Node::Frac {
                        num: numerator,
                        den: denominator,
                    });
                }
                '(' => {
                    self.pos += 1;
                    let body = self.row()?;
                    self.expect(')')?;
                    out.push(Node::ghost_group(Delim::Paren, body, Ghost::None));
                }
                '[' => {
                    self.pos += 1;
                    let body = self.row()?;
                    self.expect(']')?;
                    out.push(Node::ghost_group(Delim::Bracket, body, Ghost::None));
                }
                '\\' => {
                    if self.at_command("right") {
                        break;
                    }
                    self.command(&mut out)?;
                }
                // The palette's cursor marker: an empty slot, not a character.
                'X' => {
                    self.pos += 1;
                }
                '^' => {
                    self.pos += 1;
                    out.push(Node::Sup(self.script_argument()?));
                }
                '_' => {
                    self.pos += 1;
                    out.push(Node::Sub(self.script_argument()?));
                }
                '{' => {
                    let inner = self.braced()?;
                    out.extend(inner);
                }
                _ => self.text(&mut out),
            }
        }
        Ok(out)
    }

    /// The argument of `^` or `_`: a braced row, or a single character.
    fn script_argument(&mut self) -> Result<Row, SnippetError> {
        if self.peek() == Some('{') {
            return self.braced();
        }
        // A script absorbs the following *run*, not one character: `x^23` is
        // x²³ and `x^2y` is x^(2y), exactly as typing behaves.
        let mut run = String::new();
        // A sign belongs to the argument, not to what follows it. Typing `2/-4`
        // puts `-4` in the denominator and `x^-2` puts `-2` in the exponent,
        // because the cursor is already inside the slot when the sign is typed;
        // loading must not produce an empty slot with the sign left outside.
        let sign = match self.peek() {
            Some(c @ ('+' | '-')) => {
                self.pos += 1;
                Some(c)
            }
            _ => None,
        };
        while let Some(c) = self.peek() {
            if crate::tree::is_word_char(c) {
                run.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        let mut row = Vec::new();
        if let Some(sign) = sign {
            row.push(Node::text(sign.to_string()));
        }
        if !run.is_empty() && run != "X" {
            row.push(Node::text(run));
        }
        Ok(row)
    }

    /// A run of plain characters.
    ///
    /// Word characters (`2x`, `ab`, `1.5`) run together, exactly as they do when
    /// typed — the field's reader splits them back into factors. Operators get
    /// their own node, which is what lets `+`, `-` and `=` mean something to the
    /// reader instead of disappearing into a text blob.
    fn text(&mut self, out: &mut Row) {
        let mut run = String::new();
        while let Some(c) = self.peek() {
            if matches!(
                c,
                '\\' | '{' | '}' | '^' | '_' | 'X' | ']' | ')' | '(' | '[' | '/' | ' ' | '\t'
            ) {
                break;
            }
            if crate::tree::is_word_char(c) {
                run.push(c);
                self.pos += 1;
            } else {
                if !run.is_empty() {
                    out.push(Node::text(std::mem::take(&mut run)));
                }
                // `*` is the multiplication sign, as it is on the keyboard.
                if c == '*' {
                    out.push(Node::sym("\\cdot"));
                } else {
                    out.push(Node::text(c.to_string()));
                }
                self.pos += 1;
            }
        }
        if !run.is_empty() {
            out.push(Node::text(run));
        }
    }

    fn command(&mut self, out: &mut Row) -> Result<(), SnippetError> {
        self.pos += 1; // the backslash
        if matches!(self.peek(), Some(',') | Some(';') | Some(':') | Some('!') | Some('>')) {
            // `\,`, `\;`, `\:`, `\!`, `\>`: spacing, nothing to draw.
            self.pos += 1;
            return Ok(());
        }
        // An escaped literal, which is how the printer writes a character LaTeX
        // would otherwise eat: `\%`, `\#`, `\&`, `\$`, and `\ ` for a space. They
        // are text, not commands, and a printed document has to read back.
        if let Some(literal @ ('%' | '#' | '&' | '$' | ' ')) = self.peek() {
            self.pos += 1;
            out.push(Node::text(literal.to_string()));
            return Ok(());
        }
        let name = self.letters();
        match name.as_str() {
            "frac" => {
                let num = self.braced()?;
                let den = self.braced()?;
                out.push(Node::Frac { num, den });
            }
            "binom" => {
                let top = self.braced()?;
                let bottom = self.braced()?;
                out.push(Node::Binom { top, bottom });
            }
            "sqrt" => {
                let index = if self.eat('[') {
                    let index = self.row()?;
                    self.expect(']')?;
                    Some(index)
                } else {
                    None
                };
                let body = self.braced()?;
                out.push(Node::Sqrt { index, body });
            }
            "int" | "sum" | "prod" => {
                let mut lower = None;
                let mut upper = None;
                for _ in 0..2 {
                    if self.eat('_') {
                        lower = Some(self.braced()?);
                    } else if self.eat('^') {
                        upper = Some(self.braced()?);
                    } else {
                        break;
                    }
                }
                out.push(Node::sym(format!("\\{name}")));
                if let Some(lower) = lower {
                    out.push(Node::Sub(lower));
                }
                if let Some(upper) = upper {
                    out.push(Node::Sup(upper));
                }
            }
            "lim" => {
                out.push(Node::sym("\\lim"));
                if self.eat('_') {
                    out.push(Node::Sub(self.braced()?));
                }
            }
            "overrightarrow" | "underrightarrow" | "overline" | "bar" | "overleftarrow" => {
                let body = self.braced()?;
                let kind = AccentKind::from_command(name.as_str()).ok_or_else(|| {
                    SnippetError::Unsupported(format!("\\{name}"))
                })?;
                out.push(Node::Accent { kind, body });
            }
            "left" => out.push(self.bracket()?),
            "text" | "mathrm" => {
                let body = self.braced()?;
                out.push(Node::text(plain(&body)));
            }
            // Spacing commands carry no glyph; dropping them keeps the rendered
            // form readable without inventing space nodes. (`\,` and friends are
            // consumed before a name is read at all.)
            "quad" | "qquad" => {}
            // `\operatorname{tanh}` is a function name, not maths.
            "operatorname" => {
                let body = self.braced()?;
                out.push(Node::text(plain(&body)));
            }
            // A blackboard-bold letter is one token, not a command followed by a
            // character: `\mathbb{Z}` has to print as itself.
            "mathbb" | "mathcal" | "mathbf" => {
                let body = self.braced()?;
                out.push(Node::sym(format!("\\{name}{{{}}}", plain(&body))));
            }
            "begin" => {
                return Err(SnippetError::Unsupported(String::from(
                    "a table environment",
                )));
            }
            // `\not\equiv`, `\not\subset`: one token, or the renderer shows the
            // literal word "not".
            "not" => {
                let mut negated = String::from("\\not");
                if self.peek() == Some('\\') {
                    self.pos += 1;
                    negated.push('\\');
                    negated.push_str(&self.letters());
                }
                out.push(Node::sym(negated));
            }
            "" => {
                return Err(SnippetError::Malformed(String::from("lone backslash")));
            }
            _ => out.push(Node::sym(format!("\\{name}"))),
        }
        Ok(())
    }

    /// `\left( … \right)` and its three siblings.
    fn bracket(&mut self) -> Result<Node, SnippetError> {
        let delim = if self.eat('\\') {
            match self.bump() {
                Some('{') => Delim::Brace,
                Some('}') => Delim::Brace,
                other => {
                    return Err(SnippetError::Malformed(format!(
                        "unknown delimiter {other:?}"
                    )));
                }
            }
        } else {
            match self.bump() {
                Some('(') => Delim::Paren,
                Some('[') => Delim::Bracket,
                Some('|') => Delim::Abs,
                other => {
                    return Err(SnippetError::Malformed(format!(
                        "unknown delimiter {other:?}"
                    )));
                }
            }
        };
        let body = self.row()?;
        if !self.at_command("right") {
            return Err(SnippetError::Malformed(String::from("missing \\right")));
        }
        self.pos += 1; // backslash
        let _ = self.letters(); // "right"
        if self.eat('\\') {
            let _ = self.bump(); // `\}` / `\)`
        } else {
            let _ = self.bump(); // `)`, `]`, `|`
        }
        Ok(Node::ghost_group(delim, body, Ghost::None))
    }
}

/// The characters of a row, for arguments that are text rather than maths.
fn plain(row: &Row) -> String {
    let mut out = String::new();
    for node in row {
        match node {
            Node::Text(s) => out.push_str(s),
            Node::Sym(s) => out.push_str(s),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod ode_statement_tests {
    use super::*;

    #[test]
    fn a_derivative_line_is_a_statement_and_a_plain_equation_is_not() {
        // `y' = -k*y` has no `Expr` node, so the row is held whole for
        // `canlab-ode` to read — the same rule `\int` follows.
        assert!(is_calculus_statement("y' = -k*y"));
        assert!(is_calculus_statement("x'=y"));
        // An initial condition is the other half of the same document.
        assert!(is_calculus_statement("y(0) = 1"));
        assert!(is_calculus_statement("x(-1.5) = 2"));
        // A plain equation is an `Expr` and stays structural: the solver owns it.
        assert!(!is_calculus_statement("y = -k*y"));
        assert!(!is_calculus_statement("x^2 + y^2 = 1"));
        // A half-typed line with an empty slot stays structural too.
        assert!(!is_calculus_statement("y' = X"));
    }

    #[test]
    fn an_ode_line_is_held_whole_and_a_plain_one_is_parsed() {
        let row = parse("y' = -k*y").expect("a row");
        assert_eq!(row.len(), 1);
        assert!(matches!(row[0], crate::tree::Node::Raw(_)), "{:?}", row[0]);
        // `y = 2x` is an ordinary equation and becomes a tree.
        let row = parse("y = 2x").expect("a row");
        assert!(!matches!(row[0], crate::tree::Node::Raw(_)), "{:?}", row[0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_argument_becomes_a_real_fraction() {
        let row = parse(r"\frac{1}{2}").unwrap();
        assert_eq!(row.len(), 1);
        match &row[0] {
            Node::Frac { num, den } => {
                assert_eq!(num, &vec![Node::text("1")]);
                assert_eq!(den, &vec![Node::text("2")]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_x_marker_becomes_an_empty_slot() {
        let row = parse(r"\frac{X}{X}").unwrap();
        match &row[0] {
            Node::Frac { num, den } => {
                assert!(num.is_empty() && den.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_superscript_button_attaches_a_script() {
        let row = parse("^2").unwrap();
        assert_eq!(row, vec![Node::Sup(vec![Node::text("2")])]);
        let row = parse("x^{X}").unwrap();
        assert_eq!(row, vec![Node::text("x"), Node::Sup(Vec::new())]);
    }

    #[test]
    fn an_integral_carries_its_own_limits() {
        let row = parse(r"\int_{X}^{X}").unwrap();
        assert_eq!(row.len(), 3);
        assert_eq!(row[0], Node::sym("\\int"));
        assert!(matches!(row[1], Node::Sub(_)));
        assert!(matches!(row[2], Node::Sup(_)));
    }

    #[test]
    fn brackets_come_back_whole() {
        let row = parse(r"\left(X\right)").unwrap();
        match &row[0] {
            Node::Group { delim, body, ghost } => {
                assert_eq!(*delim, Delim::Paren);
                assert!(body.is_empty());
                assert_eq!(*ghost, Ghost::None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_finished_limit_is_held_as_one_statement() {
        // A limit's approach is notation, not an argument: `x \to \infty` is
        // three tokens that mean nothing to the expression grammar, and the
        // field has no node for them. A *finished* statement (no empty slot
        // left) is therefore held as one `Raw` and read by `canlab-parse`; the
        // palette's `\lim_{X}` is still structural while its slot is empty,
        // which `a_limit_reads_as_a_command_with_a_subscript` used to cover and
        // `an_integral_carries_its_own_limits` still does.
        let row = parse(r"\lim_{x\rightarrow\infty}").unwrap();
        assert_eq!(row, vec![Node::Raw(r"\lim_{x\rightarrow\infty}".to_string())]);
        // And it reads back as the same LaTeX, so the field can show what it
        // holds.
        assert_eq!(crate::latex::emit(&row), r"\lim_{x\rightarrow\infty}");
    }

    #[test]
    fn a_limit_being_typed_is_still_structural() {
        // The palette's `\lim_{X}`: the slot is empty, so the cursor can enter
        // it and the structural reader owns the line.
        let row = parse(r"\lim_{X}").unwrap();
        assert_eq!(row[0], Node::sym("\\lim"));
        assert!(matches!(row[1], Node::Sub(_)));
    }

    #[test]
    fn negated_relations_stay_one_token() {
        let row = parse(r"\not\equiv").unwrap();
        assert_eq!(row, vec![Node::sym("\\not\\equiv")]);
    }

    #[test]
    fn tables_are_refused_rather_than_flattened() {
        let error = parse(r"\begin{cases} X \\ X \end{cases}").unwrap_err();
        assert!(matches!(error, SnippetError::Unsupported(_)));
    }

    #[test]
    fn every_structural_palette_entry_is_modelled_or_refused_on_purpose() {
        const NOT_MODELLED: &[&str] = &["cases", "matrix", "array"];
        for button in canlab_parse::palette::STRUCTURES {
            let parsed = parse(button.insert);
            if NOT_MODELLED.contains(&button.label) {
                assert!(
                    matches!(parsed, Err(SnippetError::Unsupported(_))),
                    "`{}` should be refused, got {parsed:?}",
                    button.label
                );
            } else {
                assert!(parsed.is_ok(), "`{}` failed: {parsed:?}", button.label);
            }
        }
    }
}
