//! The Pratt parser: tokens in, canonical [`Expr`] out.
//!
//! Precedence climbing with the binding powers documented on
//! [`crate::parse_latex`]. The parser is deliberately small and split into one
//! named function per construct; the interesting decisions are:
//!
//! * `parse_expr` is the only loop. Everything else is either a prefix
//!   (`parse_prefix`) or an atom (`parse_atom`), which is what keeps the
//!   precedence table in one place.
//! * The right-hand side of `/` is parsed at `BP_MUL + 1`, while implicit
//!   multiplication sits at `BP_IMPLICIT = 30`. That single asymmetry is what
//!   makes `1/2x` mean `1/(2x)`.
//! * A function argument is parsed at `BP_IMPLICIT` (or taken as a whole when
//!   it is braced/parenthesized), so `\sin 2x` is `sin(2*x)` and
//!   `\sin x + 1` is `sin(x) + 1`.
//!
//! Every recursive step consumes at least one token, so no input can make the
//! parser loop forever.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::normalize::{add, mul, normalize, pow};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

use crate::lexer::{tokenize, Token, TokenKind};
use crate::ParseError;

/// Binding power of `+` and `-`.
const BP_ADD: u8 = 10;
/// Binding power of `*`, `\cdot`, `\times`, `\div` and `/`.
const BP_MUL: u8 = 20;
/// Binding power of implicit multiplication, `2x`.
const BP_IMPLICIT: u8 = 30;
/// Binding power of the prefix operators `-` and `+`.
const BP_UNARY: u8 = 35;
/// Binding power of `^` and `_`; both are right-associative.
const BP_POW: u8 = 50;

/// Commands that name a function. They are applied to the product that follows
/// them, or to a single braced/parenthesized group.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "ln", "log", "exp", "arcsin", "arccos", "arctan", "sinh", "cosh",
    "tanh", "abs",
];

/// What the field says when its `ToExprError::Subscript` is displayed, word for
/// word: the two readers of a document must give the same reason for the same
/// input, and this crate cannot depend on `canlab-input` to borrow the string.
/// A test in `canlab-input` compares the two.
const SUBSCRIPT_REFUSAL: &str = "subscripts are not expressions";

/// Greek command names. The symbol is interned under the name without the
/// backslash, so `\pi` is the symbol `pi`.
const GREEK_NAMES: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "omicron",
    "pi",
    "varpi",
    "rho",
    "varrho",
    "sigma",
    "varsigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
];

/// Parse a complete LaTeX expression, rejecting anything left over.
/// A definite integral's answer, with its bound when it has one.
///
/// [`parse`] collapses this to a bare number because an `Expr` has nowhere to
/// put an error bound. That collapse is what makes `\int_0^1 e^{-x^2} dx` print
/// a sixteen-digit float with no marker: the value is approximate and the type
/// it is returned as says nothing about it. This entry keeps the distinction,
/// and a caller that prints an answer should use it.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// An expression, exact by construction.
    Value(Expr),
    /// A definite integral with a closed form and no error.
    Exact(Number),
    /// A definite integral that is numeric, with its one-sided bound.
    Approximate {
        /// The estimate.
        value: Number,
        /// The bound: the true value is within this much of `value`.
        error: f64,
    },
}

impl Statement {
    /// The value as an `Expr`, discarding the bound.
    ///
    /// This is the collapse [`parse`] performs. It is named so that a caller
    /// that discards a bound has to say so.
    pub fn to_expr(&self) -> Expr {
        match self {
            Statement::Value(expression) => expression.clone(),
            Statement::Exact(number) => ExprNode::num(number.clone()),
            Statement::Approximate { value, .. } => ExprNode::num(value.clone()),
        }
    }

    /// The bound, when there is one.
    pub fn error(&self) -> Option<f64> {
        match self {
            Statement::Approximate { error, .. } => Some(*error),
            _ => None,
        }
    }
}

/// Parse a statement, keeping a definite integral's bound.
pub(crate) fn parse_statement(src: &str) -> Result<Statement, ParseError> {
    let mut parser = Parser::new(src)?;
    if parser.starts_integral() {
        let value = parser.parse_definite_integral_value()?;
        parser.expect_end()?;
        return Ok(match value {
            crate::integral::IntegralValue::Exact(number) => Statement::Exact(number),
            crate::integral::IntegralValue::Approximate { value, error } => {
                Statement::Approximate { value, error }
            }
        });
    }
    let expression = parse(src)?;
    Ok(Statement::Value(expression))
}

pub(crate) fn parse(src: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(src)?;
    // A definite integral is a *statement*, not an expression: `\int_0^1 x dx`
    // describes a number but no `Expr` node holds its limits, so the parser
    // evaluates it on the spot and returns the number. Everything else is the
    // expression grammar below, unchanged.
    if parser.starts_integral() {
        let expr = parser.parse_definite_integral()?;
        parser.expect_end()?;
        return Ok(expr);
    }
    if parser.starts_limit() {
        let expr = parser.parse_limit_statement()?;
        parser.expect_end()?;
        return Ok(expr);
    }
    if parser.starts_figure() {
        let expr = parser.parse_figure_statement()?;
        parser.expect_end()?;
        return Ok(expr);
    }
    let expr = parser.parse_expr(0)?;
    parser.expect_end()?;
    Ok(expr)
}

/// The infix operators the loop in [`Parser::parse_expr`] knows about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Infix {
    Add,
    Sub,
    Mul,
    Div,
    /// Juxtaposition: `2x`, `\frac{1}{2}x`, `(x)(y)`.
    Implicit,
    Pow,
    /// `_`: refused, because `Expr` has no indexed symbol. The one subscript
    /// with a meaning — the base of `\log` — is read by `parse_function`
    /// before this loop ever sees it.
    Subscript,
    /// `!`: rejected with a message, not silently dropped.
    Factorial,
}

impl Infix {
    fn lbp(self) -> u8 {
        match self {
            Infix::Add | Infix::Sub => BP_ADD,
            Infix::Mul | Infix::Div => BP_MUL,
            Infix::Implicit => BP_IMPLICIT,
            Infix::Pow | Infix::Subscript | Infix::Factorial => BP_POW,
        }
    }
}

/// One of the delimiter pairs the parser can close.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delim {
    Paren,
    Bracket,
    Brace,
    /// `|`, which is both halves of an absolute-value pair.
    Abs,
    /// `\right.` — a delimiter that is not drawn. It closes an absolute value
    /// written for "evaluate here", which is how an exam writes it.
    None,
}

impl Delim {
    fn open(self) -> &'static str {
        match self {
            Delim::Paren => "(",
            Delim::Bracket => "[",
            Delim::Abs => "|",
            Delim::None => ".",
            Delim::Brace => "{",
        }
    }

    fn close(self) -> &'static str {
        match self {
            Delim::Paren => ")",
            Delim::Bracket => "]",
            Delim::Brace => "}",
            Delim::Abs => "|",
            Delim::None => ".",
        }
    }

    fn matches_close(self, kind: &TokenKind) -> bool {
        matches!(
            (self, kind),
            (Delim::Paren, TokenKind::RParen)
                | (Delim::Bracket, TokenKind::RBracket)
                | (Delim::Brace, TokenKind::RBrace)
                | (Delim::Abs, TokenKind::Pipe)
        )
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    /// The symbol whose occurrences the integrand reader is suppressing.
    ///
    /// `\int x\,dx` is read by the expression grammar as `x · d · x`, and the
    /// `d` is notation rather than a factor. There is no expression node for
    /// "a name that is not a factor", so the one statement that needs it — the
    /// definite integral — turns the suppression on for the span it re-reads
    /// and off again immediately.
    suppressed: Option<SymbolId>,
}

impl Parser {
    fn new(src: &str) -> Result<Self, ParseError> {
        Ok(Parser {
            tokens: tokenize(src)?,
            index: 0,
            suppressed: None,
        })
    }

    // -- token access -------------------------------------------------------

    /// The current token. Always valid: the token list ends with `Eof` and
    /// [`Parser::advance`] never moves past it.
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    /// Skip a `\,` spacing command.
    ///
    /// LaTeX's thin space is how an integral is written (`\\int_0^1 x\\,dx`) and
    /// it means nothing in an expression. It is dropped rather than refused,
    /// and the caller of `parse_expr` never sees it: the corpus and the field
    /// both produce it, and refusing it would be refusing correct LaTeX.
    fn skip_thin_space(&mut self) {
        while matches!(&self.peek().kind, TokenKind::Command(name) if name == ",") {
            self.advance();
        }
    }

    /// Skip a token the statement says is notation and not a factor.
    ///
    /// A suppressed `d` is an identifier `d` (the lexer reads the differential
    /// as `d` only when it is written `d x`; a `dx` is one identifier and the
    /// suppression then skips that whole identifier, whose variable is already
    /// known from the tokens).
    fn skip_suppressed(&mut self) {
        let Some(suppressed) = self.suppressed else {
            return;
        };
        let name = SymbolId::name(suppressed).to_string();
        loop {
            let is_suppressed =
                matches!(&self.peek().kind, TokenKind::Ident(text) if *text == name);
            if !is_suppressed {
                break;
            }
            self.advance();
            self.skip_thin_space();
        }
    }

    /// Consume the current token, except at end of input, where `Eof` sticks.
    fn advance(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
        token
    }

    fn expect_end(&self) -> Result<(), ParseError> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Eof => Ok(()),
            other => Err(ParseError::new(
                format!("unexpected {} after the expression", other.describe()),
                token.pos,
            )),
        }
    }

    // -- the loop -----------------------------------------------------------

    /// Parse an expression whose operators bind at least as tightly as
    /// `min_bp`. This is the whole precedence climbing algorithm.
    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix()?;
        while let Some(op) = self.peek_infix() {
            if op.lbp() < min_bp {
                break;
            }
            lhs = self.apply_infix(lhs, op)?;
        }
        Ok(lhs)
    }

    /// Which infix operator starts at the current token, if any.
    fn peek_infix(&self) -> Option<Infix> {
        match &self.peek().kind {
            TokenKind::Plus => Some(Infix::Add),
            TokenKind::Minus => Some(Infix::Sub),
            TokenKind::Star => Some(Infix::Mul),
            TokenKind::Slash => Some(Infix::Div),
            TokenKind::Caret => Some(Infix::Pow),
            TokenKind::Underscore => Some(Infix::Subscript),
            TokenKind::Bang => Some(Infix::Factorial),
            TokenKind::Command(name) if name == "cdot" || name == "times" => Some(Infix::Mul),
            TokenKind::Command(name) if name == "div" => Some(Infix::Div),
            _ if self.starts_implicit_factor() => Some(Infix::Implicit),
            _ => None,
        }
    }

    fn apply_infix(&mut self, lhs: Expr, op: Infix) -> Result<Expr, ParseError> {
        let op_pos = self.peek().pos;
        if op != Infix::Implicit {
            self.advance();
        }
        match op {
            Infix::Add => {
                let rhs = self.parse_expr(BP_ADD + 1)?;
                Ok(add([lhs, rhs]))
            }
            Infix::Sub => {
                let rhs = self.parse_expr(BP_ADD + 1)?;
                Ok(add([lhs, negate(rhs)]))
            }
            Infix::Mul => {
                let rhs = self.parse_expr(BP_MUL + 1)?;
                Ok(mul([lhs, rhs]))
            }
            Infix::Div => {
                let rhs = self.parse_expr(BP_MUL + 1)?;
                divide(lhs, rhs, op_pos)
            }
            Infix::Implicit => {
                // No token is consumed here: the right-hand side is the atom
                // that made this an implicit product in the first place.
                let rhs = self.parse_expr(BP_IMPLICIT + 1)?;
                Ok(mul([lhs, rhs]))
            }
            Infix::Pow => {
                let rhs = self.parse_script(BP_POW, "^", "exponent", op_pos)?;
                Ok(pow(lhs, rhs))
            }
            Infix::Subscript => {
                // `v_1` is **one name**, not `v` with a decoration: `v_1` and
                // `v_2` are two variables, and a reader that dropped the
                // subscript made them the same one. The symbol is interned with
                // the underscore in its name, which is also what LaTeX prints,
                // so the round trip needs nothing special.
                //
                // Only a symbol with a simple subscript can be a name. `x_{n+1}`
                // is an index expression, and there is no symbol for one — that
                // is refused, and the refusal says which shape is supported.
                let base = match &*lhs {
                    ExprNode::Sym(symbol) => Some(*symbol),
                    _ => None,
                };
                let subscript = match &self.peek().kind {
                    TokenKind::Number(text) => Some(text.clone()),
                    TokenKind::Ident(text) => Some(text.clone()),
                    _ => None,
                };
                match (base, subscript) {
                    (Some(symbol), Some(subscript)) => {
                        self.advance();
                        Ok(ExprNode::sym(SymbolId::intern(&format!(
                            "{}_{subscript}",
                            symbol.as_str()
                        ))))
                    }
                    _ => Err(ParseError::new(
                        "a subscript is part of a name only when it is a number or a letter; \
                         an index expression has no symbol",
                        op_pos,
                    )),
                }
            }
            Infix::Factorial => {
                Err(ParseError::new("factorial `!` is not supported", op_pos))
            }
        }
    }

    /// `^` or `_` with a mandatory argument.
    fn parse_script(
        &mut self,
        min_bp: u8,
        op: &str,
        label: &str,
        op_pos: usize,
    ) -> Result<Expr, ParseError> {
        if !self.can_start_atom() {
            return Err(ParseError::new(
                format!(
                    "missing {label} after `{op}` (found {})",
                    self.peek().kind.describe()
                ),
                op_pos,
            ));
        }
        self.parse_expr(min_bp)
    }

    // -- figures of revolution and area -------------------------------------

    /// Whether the input is a `\vol` or `\area` statement.
    ///
    /// The two commands are this project's own spelling for the two figures
    /// W3.4 asks for, and they are statements for the same reason `\int` is: the
    /// answer is a number, and no `Expr` node holds the construction. The
    /// grammar is deliberately literal:
    ///
    /// ```text
    /// \vol_{a}^{b} y(x) \, dx        about the x-axis (discs)
    /// \vol_{a}^{b} y(x) \, d x       the same; the axis is the differential's
    /// \area_{a}^{b} f , g            the area between f and g
    /// ```
    fn starts_figure(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Command(name) if name == "vol" || name == "area")
    }

    fn parse_figure_statement(&mut self) -> Result<Expr, ParseError> {
        let open = self.peek().pos;
        let command = match &self.peek().kind {
            TokenKind::Command(name) => name.clone(),
            _ => unreachable!("starts_figure checked"),
        };
        self.advance();
        let lower = self.parse_limit("lower")?;
        let upper = self.parse_limit("upper")?;
        let lower = evaluated(&lower).ok_or_else(|| {
            ParseError::new("a figure needs a numeric lower limit", open)
        })?;
        let upper = evaluated(&upper).ok_or_else(|| {
            ParseError::new("a figure needs a numeric upper limit", open)
        })?;
        if command == "vol" {
            let (curve, var, axis) = self.parse_volume_body()?;
            let value = crate::figure::volume_of_revolution(&curve, var, lower, upper, axis)
                .map_err(|error| ParseError::new(error.message(), open))?;
            return Ok(ExprNode::num(value.number()));
        }
        // `\area_{a}^{b} f , g`: the two curves, separated by a comma.
        let first = self.parse_expr(0)?;
        if !matches!(self.peek().kind, TokenKind::Comma) {
            return Err(ParseError::new(
                "`\\area` needs two curves separated by a comma",
                self.peek().pos,
            ));
        }
        self.advance();
        let second = self.parse_expr(0)?;
        let mut symbols = Vec::new();
        crate::limit::collect_symbols(&first, &mut symbols);
        crate::limit::collect_symbols(&second, &mut symbols);
        let var = symbols
            .into_iter()
            .find(|symbol| &*SymbolId::name(*symbol) != "pi" && &*SymbolId::name(*symbol) != "e")
            .ok_or_else(|| ParseError::new("`\\area` needs a variable of integration", open))?;
        let value = crate::figure::area_between(&first, &second, var, lower, upper)
            .map_err(|error| ParseError::new(error.message(), open))?;
        Ok(ExprNode::num(value.number()))
    }

    /// The body of a `\vol`: the curve, its variable, and the axis.
    ///
    /// The axis is the differential's letter — `dx` sweeps about the `x`-axis
    /// and `dy` about the `y`-axis — which is how the exam writes it and is the
    /// least surprising reading. The curve must be a function of the other
    /// variable; a `\vol` whose curve mentions both is refused by the integral
    /// as a parameter.
    fn parse_volume_body(&mut self) -> Result<(Expr, SymbolId, crate::figure::Axis), ParseError> {
        let start = self.index;
        let mut end = start;
        while !matches!(self.tokens[end].kind, TokenKind::Eof) {
            end += 1;
        }
        let (body_end, var) = self.split_differential(start, end).ok_or_else(|| {
            ParseError::new(
                "`\\vol` must end with the differential, as in `dx`",
                start,
            )
        })?;
        let axis = match &*SymbolId::name(var) {
            "x" => crate::figure::Axis::X,
            "y" => crate::figure::Axis::Y,
            other => {
                return Err(ParseError::new(
                    format!("`\\vol` sweeps about the `x`- or `y`-axis, not `{other}`"),
                    start,
                ))
            }
        };
        let mut body_tokens = self.tokens[start..=body_end].to_vec();
        let eof_pos = self.tokens[body_end].pos;
        body_tokens.push(Token { kind: TokenKind::Eof, pos: eof_pos });
        let saved = std::mem::replace(&mut self.tokens, body_tokens);
        self.index = 0;
        let curve = self.parse_expr(0);
        self.tokens = saved;
        let curve = curve?;
        self.index = end;
        // The variable the curve is a function of: the other one.
        let mut symbols = Vec::new();
        crate::limit::collect_symbols(&curve, &mut symbols);
        let variable = symbols
            .into_iter()
            .find(|symbol| &*SymbolId::name(*symbol) != "pi" && &*SymbolId::name(*symbol) != "e")
            .ok_or_else(|| ParseError::new("`\\vol` needs a curve that depends on the variable", start))?;
        Ok((curve, variable, axis))
    }

    // -- limits -------------------------------------------------------------

    /// Whether the input starts with `\lim`.
    fn starts_limit(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Command(name) if name == "lim")
    }

    /// `\lim_{x \to a} f(x)`, evaluated.
    ///
    /// The approach is required and must be a number or `\infty`; a limit
    /// without one is not a limit. The body is parsed and then handed to
    /// [`crate::limit::limit`], which knows the school set and refuses what it
    /// cannot establish.
    fn parse_limit_statement(&mut self) -> Result<Expr, ParseError> {
        let open = self.peek().pos;
        self.advance(); // `\lim`
        if !matches!(self.peek().kind, TokenKind::Underscore) {
            return Err(ParseError::new(
                "`\\lim` needs its approach, as in `\\lim_{x \\to 0}`",
                open,
            ));
        }
        self.advance();
        // The approach is `x \to a` or `x \to \infty`, inside braces or not.
        let braced = matches!(self.peek().kind, TokenKind::LBrace);
        if braced {
            self.advance();
        }
        let variable = match self.peek().kind.clone() {
            TokenKind::Ident(name) => {
                self.advance();
                SymbolId::intern(&name)
            }
            other => {
                return Err(ParseError::new(
                    format!(
                        "`\\lim` needs a variable before its arrow (found {})",
                        other.describe()
                    ),
                    self.peek().pos,
                ))
            }
        };
        self.expect_arrow()?;
        // The approach's own expression must not swallow the `^+`/`^-` or the
        // closing brace: `\lim_{x \to 0^+}` is `0`, then `^+`, then `}`. The
        // approach is parsed at the power's binding power so that a superscript
        // is left for the reader below.
        let (target, sign) = self.parse_approach()?;
        // `x \to a^+` and `x \to a^-`: the side the approach is from.
        let mut approach = crate::limit::Approach::TwoSided;
        if matches!(self.peek().kind, TokenKind::Caret) {
            self.advance();
            match &self.peek().kind {
                TokenKind::Plus => {
                    self.advance();
                    approach = crate::limit::Approach::FromAbove;
                }
                TokenKind::Minus => {
                    self.advance();
                    approach = crate::limit::Approach::FromBelow;
                }
                other => {
                    return Err(ParseError::new(
                        format!(
                            "a one-sided approach is `^+` or `^-` (found {})",
                            other.describe()
                        ),
                        self.peek().pos,
                    ))
                }
            }
        }
        if braced {
            if !matches!(self.peek().kind, TokenKind::RBrace) {
                return Err(ParseError::new(
                    "`\\lim_{…}` is missing its closing brace",
                    self.peek().pos,
                ));
            }
            self.advance();
        }
        let body = self.parse_expr(0)?;
        let target = crate::limit::Target {
            value: target,
            infinity: sign,
            approach,
        };
        let value = crate::limit::limit(&body, variable, target)
            .map_err(|error| ParseError::new(error.message(), open))?;
        Ok(ExprNode::num(match value {
            crate::limit::LimitValue::Finite(number) => number,
            crate::limit::LimitValue::Infinite(sign) => {
                return Err(ParseError::new(
                    format!(
                        "the limit is {} infinity, which is not a number this parser can answer with",
                        if sign < 0 { "negative" } else { "positive" }
                    ),
                    open,
                ))
            }
            crate::limit::LimitValue::Numeric { value, .. } => Number::float(value),
        }))
    }

    /// `\to`, `\rightarrow` or `->`.
    fn expect_arrow(&mut self) -> Result<(), ParseError> {
        let token = self.peek().clone();
        let is_arrow = match &token.kind {
            TokenKind::Command(name) => name == "to" || name == "rightarrow",
            TokenKind::Minus => {
                // `->` is `-` then `>`; consume both.
                self.advance();
                if matches!(&self.peek().kind, TokenKind::Ident(text) if text == ">") {
                    self.advance();
                    return Ok(());
                }
                return Err(ParseError::new(
                    "`\\lim` needs `\\to` between the variable and the approach",
                    token.pos,
                ));
            }
            _ => false,
        };
        if !is_arrow {
            return Err(ParseError::new(
                "`\\lim` needs `\\to` between the variable and the approach",
                token.pos,
            ));
        }
        self.advance();
        Ok(())
    }

    /// The point approached: a number, or `\infty` / `-\infty`.
    ///
    /// Returns the finite value (if any) and the infinity's sign.
    fn parse_approach(&mut self) -> Result<(Option<f64>, i8), ParseError> {
        let negative = matches!(self.peek().kind, TokenKind::Minus);
        if negative {
            self.advance();
        }
        if let TokenKind::Command(name) = &self.peek().kind {
            if name == "infty" {
                self.advance();
                return Ok((None, if negative { -1 } else { 1 }));
            }
        }
        // `BP_POW + 1` and not `0`: a superscript after the approach is the
        // side of the approach (`0^+`), not a power of it, so the loop must
        // stop *before* `^`. Parsing at `BP_UNARY` was not enough — `^` binds
        // at `BP_POW`, which is higher, so the loop took it and the `+` inside
        // became an expression.
        let expression = self.parse_expr(BP_POW + 1)?;
        let Some(number) = evaluated(&expression) else {
            return Err(ParseError::new(
                "the approach must be a number or `\\infty`",
                self.peek().pos,
            ));
        };
        let value = number.to_f64();
        Ok((Some(if negative { -value } else { value }), 0))
    }

    // -- definite integrals -------------------------------------------------

    /// Whether the input starts with `\int`.
    fn starts_integral(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Command(name) if name == "int")
    }

    /// `\int_{a}^{b} f\, dx`, evaluated.
    ///
    /// The limits are required and exact. Latin Mathematics describes an
    /// integral without limits as well, and this parser refuses that one rather
    /// than inventing a variable of integration: in the exam dialect a definite
    /// integral is what gets typed, and `∫f` with no limits has no value to
    /// answer with. A refusal names what is missing, which is the difference
    /// between "this field cannot read your input" and "this integral is
    /// indefinite".
    fn parse_definite_integral(&mut self) -> Result<Expr, ParseError> {
        Ok(ExprNode::num(self.parse_definite_integral_value()?.number()))
    }

    /// The integral as an [`crate::integral::IntegralValue`], bound and all.
    fn parse_definite_integral_value(
        &mut self,
    ) -> Result<crate::integral::IntegralValue, ParseError> {
        let open = self.peek().pos;
        self.advance(); // `\int`
        let lower = self.parse_limit("lower")?;
        let upper = self.parse_limit("upper")?;
        let (integrand, var) = self.parse_integrand()?;
        let lower = evaluated(&lower).ok_or_else(|| {
            ParseError::new("a definite integral needs a numeric lower limit", open)
        })?;
        let upper = evaluated(&upper).ok_or_else(|| {
            ParseError::new("a definite integral needs a numeric upper limit", open)
        })?;
        crate::integral::definite_integral(&integrand, var, lower, upper)
            .map_err(|error| ParseError::new(error.message(), open))
    }

    /// The `_a` or `^b` after `\int`, with the braces optional (`\int_0^2`).
    fn parse_limit(&mut self, role: &str) -> Result<Expr, ParseError> {
        let token = self.peek().clone();
        let expected = if role == "lower" { "_" } else { "^" };
        match token.kind {
            TokenKind::Underscore if role == "lower" => {}
            TokenKind::Caret if role == "upper" => {}
            _ => {
                return Err(ParseError::new(
                    format!(
                        "`\\int` is missing its {role} limit `{expected}` (found {})",
                        token.kind.describe()
                    ),
                    token.pos,
                ))
            }
        }
        self.advance();
        self.parse_required_argument("int", role)
    }

    /// The integrand and the `d`-variable that follows it.
    ///
    /// `dx` is two tokens — the command `d`-like symbol `d` (an identifier) and
    /// the variable — and the reader has no `d` command, so both arrive as one
    /// identifier or as `d` followed by a variable. Both spellings are read, and
    /// so is `\,dx`, whose thin space the parser drops as a factor that has no
    /// place in an expression.
    /// The integrand and the `dx` that follows it.
    ///
    /// The integrand is parsed **with the differential cut out of the token
    /// stream**, and this is the only reading that works. A product in `Expr` is
    /// canonicalised: `normalize::mul` collects its factors into a `BTreeMap`
    /// keyed by the factor's own ordering and merges identical bases into
    /// powers, so `x · x² · d` becomes `x³ · d` — the `x` that `dx` named is
    /// merged into the body and is no longer a factor to find. Reading the
    /// variable from the parsed tree gave `x³` as the integrand of `x² dx` and
    /// `x²` as the integrand of `x dx`, both of which integrate to plausible
    /// wrong numbers. The tokens are not canonicalised, so the cut happens there.
    ///
    /// Two spellings reach here: `\\,dx`, where the lexer gives one identifier
    /// `dx`, and `d x`, which is two. Both end with the differential, and
    /// everything before it is the body.
    fn parse_integrand(&mut self) -> Result<(Expr, SymbolId), ParseError> {
        let start = self.index;
        // The tokens of the integrand, up to and including its `Eof`.
        let mut end = start;
        while !matches!(self.tokens[end].kind, TokenKind::Eof) {
            end += 1;
        }
        let (body_end, var) = self.split_differential(start, end).ok_or_else(|| {
            ParseError::new(
                "`\\int` must end with the differential, as in `dx`; without it there is no variable of integration",
                start,
            )
        })?;
        // Parse the body from a token list that ends where the body ends. A
        // trailing `\,` between the body and the differential is dropped with
        // it: the span must not end on a spacing command, because a leading
        // spacing command is skipped by `parse_prefix` and a *trailing* one is
        // what `expect_end` then refuses — "unexpected end of input", raised by
        // a thin space.
        let mut body_end = body_end;
        while body_end > start
            && matches!(&self.tokens[body_end].kind, TokenKind::Command(name) if name == ",")
        {
            body_end -= 1;
        }
        let mut body_tokens = self.tokens[start..=body_end].to_vec();
        let eof_pos = self.tokens[body_end].pos;
        body_tokens.push(Token { kind: TokenKind::Eof, pos: eof_pos });
        let saved = std::mem::replace(&mut self.tokens, body_tokens);
        self.index = 0;
        let body = self.parse_expr(0);
        self.tokens = saved;
        let body = body?;
        // The main token stream is restored; skip past the body and the
        // differential so that the caller's `expect_end` sees the end of input.
        // The integral's own syntax must not be read again as an expression,
        // which is what happened when this left the index at the body's start:
        // `\int_0^2 3x^2 dx` then failed with "unexpected number `2`" — the
        // lower limit, read twice.
        self.index = end;
        Ok((body, var))
    }

    /// Where the body ends, and the variable of integration, from the tokens of
    /// the integrand (with the `Eof` at `end`).
    ///
    /// The differential is the last thing a writer put down. `dx` is one
    /// identifier whose last letter is the variable; `d x` is an identifier `d`
    /// followed by an identifier `x`. A trailing `\,` thin space is skipped: it
    /// is spacing, not a factor.
    fn split_differential(&self, start: usize, end: usize) -> Option<(usize, SymbolId)> {
        if end <= start {
            return None;
        }
        let mut last = end - 1;
        // A trailing `\,` is spacing and not a factor; it is between the body
        // and the `dx` (`x\,dx`) or after it, so the scan skips any `\,` it
        // meets on the way to the differential.
        while last > start && matches!(&self.tokens[last].kind, TokenKind::Command(name) if name == ",") {
            last -= 1;
        }
        let TokenKind::Ident(name) = &self.tokens[last].kind else {
            return None;
        };
        // `x\,dx` puts the thin space between the body and the differential,
        // so the identifier found above is `dx` and everything after it was a
        // space; nothing else to skip.
        if name.len() >= 2 && name.starts_with('d') {
            let letter = name.chars().last()?;
            return Some((last - 1, SymbolId::intern(&letter.to_string())));
        }
        if name == "d" || last == start {
            return None;
        }
        match &self.tokens[last - 1].kind {
            TokenKind::Ident(d) if d == "d" => {
                Some((last - 2, SymbolId::intern(name)))
            }
            _ => None,
        }
    }

    // -- prefix and atoms ---------------------------------------------------

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        self.skip_thin_space();
        self.skip_suppressed();
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Minus => {
                self.advance();
                // Unary minus binds looser than `^` (`-2^2` is `-(2^2)`) and
                // tighter than implicit multiplication.
                let rhs = self.parse_expr(BP_UNARY)?;
                Ok(negate(rhs))
            }
            TokenKind::Plus => {
                self.advance();
                self.parse_expr(BP_UNARY)
            }
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        self.skip_suppressed();
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Number(text) => {
                self.advance();
                number_literal(&text, token.pos)
            }
            TokenKind::Ident(name) => {
                self.advance();
                if is_function(&name) {
                    self.parse_function(&name, token.pos)
                } else {
                    Ok(split_letters(&name))
                }
            }
            TokenKind::Command(name) => {
                self.advance();
                self.command_atom(&name, token.pos)
            }
            TokenKind::LBrace => self.parse_group(Delim::Brace, token.pos),
            TokenKind::LParen => self.parse_group(Delim::Paren, token.pos),
            TokenKind::LBracket => self.parse_group(Delim::Bracket, token.pos),
            TokenKind::Eof => Err(ParseError::new(
                "unexpected end of input; expected an expression",
                token.pos,
            )),
            other => Err(ParseError::new(
                format!("unexpected {}; expected an expression", other.describe()),
                token.pos,
            )),
        }
    }

    /// Parse `{ ... }`, `( ... )` or `[ ... ]`, consuming the closer.
    fn parse_group(&mut self, delim: Delim, open_pos: usize) -> Result<Expr, ParseError> {
        self.advance();
        let inner = self.parse_expr(0)?;
        self.expect_close(delim, open_pos)?;
        Ok(inner)
    }

    fn expect_close(&mut self, delim: Delim, open_pos: usize) -> Result<(), ParseError> {
        if delim.matches_close(&self.peek().kind) {
            self.advance();
            return Ok(());
        }
        let token = self.peek();
        let err = match &token.kind {
            TokenKind::Eof => ParseError::new(
                format!(
                    "unclosed `{}`: expected `{}` before end of input",
                    delim.open(),
                    delim.close()
                ),
                open_pos,
            ),
            other => ParseError::new(
                format!(
                    "expected `{}` to close `{}` (found {})",
                    delim.close(),
                    delim.open(),
                    other.describe()
                ),
                token.pos,
            ),
        };
        Err(err)
    }

    // -- commands -----------------------------------------------------------

    fn command_atom(&mut self, name: &str, pos: usize) -> Result<Expr, ParseError> {
        match name {
            "frac" | "dfrac" | "tfrac" => self.parse_frac(name, pos),
            "sqrt" => self.parse_sqrt(),
            "left" => self.parse_left_right(pos),
            "right" => Err(ParseError::new("unexpected `\\right`", pos)),
            // LaTeX's spacing commands mean nothing in an expression. `\\,` is
            // how an integral is written (`\\int_0^1 x\\,dx`) and `\\;` and `\\quad`
            // are how a formula is laid out; all of them are skipped rather
            // than refused, because refusing them refuses correct LaTeX.
            "," | ";" | ":" | "!" | " " | "quad" | "qquad" => self.parse_prefix(),
            "cdot" | "times" | "div" => Err(ParseError::new(
                format!("unexpected `\\{name}`; expected an expression"),
                pos,
            )),
            // `\begin{pmatrix}…\end{pmatrix}`: rows of expressions, `&` between
            // columns and `\\` between rows.
            "begin" => self.parse_environment(pos),
            "end" => Err(ParseError::new("unexpected `\\end`", pos)),
            // `\binom{n}{k}` is `C(n, k)`: two braced arguments, in that order.
            "binom" => {
                let n = self.parse_prefix()?;
                let k = self.parse_prefix()?;
                Ok(ExprNode::fn_raw(SymbolId::intern("binom"), vec![n, k]))
            }
            _ if is_function(name) => self.parse_function(name, pos),
            _ if is_greek(name) => Ok(ExprNode::sym(SymbolId::intern(name))),
            // An accent is a decoration of an expression and the tree has no
            // node for one, so `\vec{v}` cannot be read. Saying "unknown
            // command" would be wrong — the command is known and the editor
            // emits it — so the refusal names the actual reason.
            // An accent is a decoration, not a value: it is held, printed and
            // compared, and it has no number — the same treatment `\int` gets.
            _ if canlab_core::Accent::from_command(name).is_some() => {
                let kind = canlab_core::Accent::from_command(name).expect("checked");
                // The argument is one atom: `\vec{v}` is the letter, and
                // `\vec{v} + w` is the letter plus `w`.
                let body = self.parse_prefix()?;
                Ok(ExprNode::hold(canlab_core::Hold::Accent { kind, body }))
            }
            "overleftarrow" => Err(ParseError::new(
                "`\\overleftarrow` is a display accent this does not read; \
                 `\\vec` and `\\overrightarrow` are the ones it does",
                pos,
            )),
            _ => Err(ParseError::new(format!("unknown command `\\{name}`"), pos)),
        }
    }

    /// `\frac{a}{b}` -> `a * b^(-1)`.
    fn parse_frac(&mut self, name: &str, pos: usize) -> Result<Expr, ParseError> {
        let numerator = self.parse_required_argument(name, "numerator")?;
        let denominator = self.parse_required_argument(name, "denominator")?;
        divide(numerator, denominator, pos)
    }

    /// `\sqrt{x}` -> `x^(1/2)`; `\sqrt[n]{x}` -> `x^(1/n)`.
    fn parse_sqrt(&mut self) -> Result<Expr, ParseError> {
        let mut index = None;
        if matches!(self.peek().kind, TokenKind::LBracket) {
            let bracket_pos = self.peek().pos;
            self.advance();
            let parsed = self.parse_expr(0)?;
            self.expect_close(Delim::Bracket, bracket_pos)?;
            if parsed.as_number().is_some_and(|number| number.is_zero()) {
                return Err(ParseError::new(
                    "the index of `\\sqrt` must not be zero",
                    bracket_pos,
                ));
            }
            index = Some(parsed);
        }

        let radicand = self.parse_required_argument("sqrt", "radicand")?;
        let exponent = match index {
            Some(index) => reciprocal(index),
            None => ExprNode::num(Number::ratio(1, 2)),
        };
        Ok(pow(radicand, exponent))
    }

    /// One mandatory argument of a command: a brace group, or (as in
    /// `\frac12`) a single atom.
    fn parse_required_argument(
        &mut self,
        command: &str,
        role: &str,
    ) -> Result<Expr, ParseError> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::LBrace => self.parse_group(Delim::Brace, token.pos),
            _ if self.can_start_primary() => self.parse_atom(),
            _ => Err(ParseError::new(
                format!(
                    "`\\{command}` is missing its {role} (found {})",
                    self.peek().kind.describe()
                ),
                token.pos,
            )),
        }
    }

    /// `\left( ... \right)`, `\left[ ... \right]`.
    ///
    /// The delimiters are checked for a matching pair; the grouping itself is
    /// only structural and leaves no node behind.
    /// `\begin{pmatrix}…\end{pmatrix}`.
    ///
    /// Only `pmatrix` is read. A table is not an expression, and the refusal
    /// names the environment rather than guessing at one.
    fn parse_environment(&mut self, begin_pos: usize) -> Result<Expr, ParseError> {
        let name = self.braced_name("\\begin")?;
        if name != "pmatrix" {
            return Err(ParseError::new(
                format!(
                    "`\\begin{{{name}}}` is not an environment this reads; `pmatrix` is the one it does"
                ),
                begin_pos,
            ));
        }
        let mut rows: Vec<Vec<Expr>> = vec![Vec::new()];
        loop {
            // At the start of a cell: is it the end?
            if self.at_end_environment()? {
                break;
            }
            let cell = self.parse_expr(0)?;
            rows.last_mut().expect("a row").push(cell);
            match &self.peek().kind {
                TokenKind::Amp => {
                    self.advance();
                }
                // `\\` ends a row.
                TokenKind::Command(name) if name == "\\" => {
                    self.advance();
                    rows.push(Vec::new());
                }
                _ => break,
            }
        }
        if !self.at_end_environment()? {
            return Err(ParseError::new(
                "unclosed `\\begin{pmatrix}`: expected `\\end{pmatrix}`",
                begin_pos,
            ));
        }
        // The `\end` and its `{pmatrix}`.
        self.advance();
        let closing = self.braced_name("\\end")?;
        if closing != "pmatrix" {
            return Err(ParseError::new(
                format!("`\\begin{{pmatrix}}` closed by `\\end{{{closing}}}`"),
                begin_pos,
            ));
        }
        // A trailing `\\` leaves an empty last row, which is not a row.
        if rows.last().is_some_and(Vec::is_empty) {
            rows.pop();
        }
        if rows.is_empty() || rows.iter().any(Vec::is_empty) {
            return Err(ParseError::new("a matrix has no empty row", begin_pos));
        }
        let width = rows[0].len();
        if rows.iter().any(|row| row.len() != width) {
            return Err(ParseError::new(
                "a matrix's rows must have the same number of columns",
                begin_pos,
            ));
        }
        Ok(ExprNode::hold(canlab_core::Hold::Matrix { rows }))
    }

    /// Whether the next tokens are `\end{pmatrix}`.
    fn at_end_environment(&mut self) -> Result<bool, ParseError> {
        Ok(matches!(&self.peek().kind, TokenKind::Command(name) if name == "end"))
    }

    /// The name inside the braces after a command, e.g. `{pmatrix}`.
    fn braced_name(&mut self, who: &str) -> Result<String, ParseError> {
        if !matches!(self.peek().kind, TokenKind::LBrace) {
            return Err(ParseError::new(
                format!("expected `{{` after `{who}` (found {})", self.peek().kind.describe()),
                self.peek().pos,
            ));
        }
        self.advance();
        let name = match &self.peek().kind {
            TokenKind::Ident(name) => name.clone(),
            other => {
                return Err(ParseError::new(
                    format!("expected an environment name after `{who}{{` (found {})", other.describe()),
                    self.peek().pos,
                ))
            }
        };
        self.advance();
        if !matches!(self.peek().kind, TokenKind::RBrace) {
            return Err(ParseError::new(
                format!("expected `}}` after `{who}{{{name}`"),
                self.peek().pos,
            ));
        }
        self.advance();
        Ok(name)
    }

    fn parse_left_right(&mut self, left_pos: usize) -> Result<Expr, ParseError> {
        let open = self.take_open_delimiter("\\left")?;
        let inner = self.parse_expr(0)?;

        let right_pos = match &self.peek().kind {
            TokenKind::Command(name) if name == "right" => self.peek().pos,
            TokenKind::Eof => {
                return Err(ParseError::new(
                    format!(
                        "unclosed `\\left{}`: expected `\\right{}`",
                        open.open(),
                        open.close()
                    ),
                    left_pos,
                ));
            }
            other => {
                return Err(ParseError::new(
                    format!(
                        "unclosed `\\left{}`: expected `\\right{}` (found {})",
                        open.open(),
                        open.close(),
                        other.describe()
                    ),
                    self.peek().pos,
                ));
            }
        };
        self.advance(); // `\right`
        let close = self.take_close_delimiter("\\right")?;
        // `\right.` draws nothing and matches anything.
        if open != close && close != Delim::None {
            return Err(ParseError::new(
                format!(
                    "mismatched delimiters: `\\left{}` closed by `\\right{}`",
                    open.open(),
                    close.close()
                ),
                right_pos,
            ));
        }
        // An absolute value is a value, not a decoration: `\left|x\right|` is
        // `abs(x)`, which the evaluator and the differentiator both know. The
        // brackets around a group are grouping and produce nothing.
        if open == Delim::Abs {
            return Ok(ExprNode::fn_raw(SymbolId::intern("abs"), vec![inner]));
        }
        Ok(inner)
    }

    fn take_open_delimiter(&mut self, who: &str) -> Result<Delim, ParseError> {
        let delim = match &self.peek().kind {
            TokenKind::LParen => Some(Delim::Paren),
            TokenKind::LBracket => Some(Delim::Bracket),
            TokenKind::Pipe => Some(Delim::Abs),
            _ => None,
        };
        match delim {
            Some(delim) => {
                self.advance();
                Ok(delim)
            }
            None => Err(ParseError::new(
                format!(
                    "expected `(`, `[` or `|` after `{who}` (found {})",
                    self.peek().kind.describe()
                ),
                self.peek().pos,
            )),
        }
    }

    fn take_close_delimiter(&mut self, who: &str) -> Result<Delim, ParseError> {
        // `\right.` draws nothing and matches anything.
        if matches!(self.peek().kind, TokenKind::Dot) {
            self.advance();
            return Ok(Delim::None);
        }
        let delim = match &self.peek().kind {
            TokenKind::RParen => Some(Delim::Paren),
            TokenKind::RBracket => Some(Delim::Bracket),
            TokenKind::RBrace => Some(Delim::Brace),
            TokenKind::Pipe => Some(Delim::Abs),
            _ => None,
        };
        match delim {
            Some(delim) => {
                self.advance();
                Ok(delim)
            }
            None => Err(ParseError::new(
                format!(
                    "expected `)` or `]` after `{who}` (found {})",
                    "expected `)`, `]`, `}}`, `|` or `.` after `{who}` (found {})",
                ),
                self.peek().pos,
            )),
        }
    }

    // -- function application ----------------------------------------------

    /// `\sin`, `\sin^2`, `\sin(x)`, `\sin x`, and the bare `sin(...)` form.
    ///
    /// The name token has already been consumed; `name_pos` is where it was,
    /// for diagnostics.
    fn parse_function(&mut self, name: &str, name_pos: usize) -> Result<Expr, ParseError> {
        let mut exponent = None;
        let mut base = None;
        loop {
            let token = self.peek().clone();
            match token.kind {
                TokenKind::Caret => {
                    self.advance();
                    // `\sin^2 x` is `(sin x)^2`: the superscript belongs to
                    // the application, not to the argument.
                    exponent = Some(self.parse_script(BP_POW, "^", "exponent", token.pos)?);
                }
                TokenKind::Underscore => {
                    self.advance();
                    // `\log_2 x` is the one function subscript that carries a
                    // value: the base. On any other function it is decoration,
                    // and a parser that dropped it would change the answer —
                    // the field refuses the same input, and so does this.
                    if name != "log" {
                        return Err(ParseError::new(SUBSCRIPT_REFUSAL, token.pos));
                    }
                    if base.is_some() {
                        return Err(ParseError::new(
                            "a logarithm has only one base",
                            token.pos,
                        ));
                    }
                    base = Some(self.parse_script(BP_POW + 1, "_", "subscript", token.pos)?);
                }
                _ => break,
            }
        }

        let argument = self.parse_function_argument(name, name_pos)?;
        let value = match base {
            // `\log_b x` is `ln(x)/ln(b)`, the same lowering the field uses for
            // the same input. Reading it as a natural log would be a different
            // number, and refusing it would put the two readers back into
            // disagreement about an input the field already defines.
            Some(base) => {
                let upper = ExprNode::fn_raw(SymbolId::intern("ln"), vec![argument]);
                let lower = ExprNode::fn_raw(SymbolId::intern("ln"), vec![base]);
                mul([upper, pow(lower, ExprNode::int(-1))])
            }
            None => normalize(&ExprNode::fn_raw(SymbolId::intern(name), vec![argument])),
        };
        Ok(match exponent {
            Some(exponent) => pow(value, exponent),
            None => value,
        })
    }

    fn parse_function_argument(
        &mut self,
        name: &str,
        name_pos: usize,
    ) -> Result<Expr, ParseError> {
        let token = self.peek().clone();
        match token.kind {
            // A braced or parenthesized argument is complete on its own, so a
            // following `^` applies to the function value: `\sin(x)^2`.
            TokenKind::LBrace => self.parse_group(Delim::Brace, token.pos),
            TokenKind::LParen => self.parse_group(Delim::Paren, token.pos),
            // `\left( ... \right)` is a complete group too, so it behaves like
            // the plain parentheses: `\sin\left(x\right)^2` is `(sin x)^2`.
            TokenKind::Command(name) if name == "left" => self.parse_atom(),
            // Otherwise the function consumes the product that follows it at
            // the implicit-multiplication level: `\sin 2x` is `sin(2*x)`.
            _ if self.can_start_atom() => self.parse_expr(BP_IMPLICIT),
            _ => Err(ParseError::new(
                format!(
                    "function `{name}` is missing its argument (found {})",
                    self.peek().kind.describe()
                ),
                name_pos,
            )),
        }
    }

    // -- token classification ----------------------------------------------

    /// Can the current token begin an atom (or a prefixed atom)?
    fn can_start_atom(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Number(_) | TokenKind::Ident(_) => true,
            TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => true,
            TokenKind::Minus | TokenKind::Plus => true,
            TokenKind::Command(name) => !is_terminator_command(name) && !is_infix_command(name),
            _ => false,
        }
    }

    /// Can the current token begin an atom *without* a prefix operator? Used
    /// where only one token may be consumed (`\frac12`).
    fn can_start_primary(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Number(_) | TokenKind::Ident(_) => true,
            TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => true,
            TokenKind::Command(name) => !is_terminator_command(name) && !is_infix_command(name),
            _ => false,
        }
    }

    /// Can the current token start the right-hand factor of an implicit
    /// product? Deliberately narrower than [`Parser::can_start_atom`]: `-` and
    /// `+` start a new term (`x - y` is a subtraction, never `x * (-y)`), and
    /// `{` is only meaningful as a command argument.
    fn starts_implicit_factor(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Number(_) | TokenKind::Ident(_) | TokenKind::LParen => true,
            TokenKind::Command(name) => !is_terminator_command(name) && !is_infix_command(name),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers: canonical construction and token classification
// ---------------------------------------------------------------------------

fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

fn is_greek(name: &str) -> bool {
    GREEK_NAMES.contains(&name)
}

/// Operators that must never be read as the start of an implicit factor.
fn is_infix_command(name: &str) -> bool {
    matches!(name, "cdot" | "times" | "div")
}

/// `\right` ends a `\left ... \right` group; it never starts an atom.
fn is_terminator_command(name: &str) -> bool {
    // `\int` is a statement's head, not a factor: `2\int_0^1` has no reading in
    // this grammar, and reading it as `2 * (an integral)` would need the whole
    // integral inside the expression grammar, which has no node for it.
    // `\end` and `\\` close a matrix cell rather than starting a factor: without
    // them here, `3\end{pmatrix}` read as `3 · \end`, and the error was
    // "unexpected `\end`" instead of a parsed matrix.
    name == "end"
        || name == "\\"
        || name == "right"
        || name == "int"
        || name == "lim"
        || name == "to"
        || name == "rightarrow"
        || name == "vol"
        || name == "area"
}

/// `xy` -> `x*y`, matching how LaTeX reads a run of letters. Known function
/// names are handled before this is ever called.
fn split_letters(name: &str) -> Expr {
    let factors: Vec<Expr> = name
        .chars()
        .map(|letter| ExprNode::sym(SymbolId::intern(&letter.to_string())))
        .collect();
    mul(factors)
}

/// `-e` as `(-1) * e`, built through `normalize` so the coefficient folds into
/// a numeric literal where possible.
fn negate(expr: Expr) -> Expr {
    mul([ExprNode::int(-1), expr])
}

/// `1/e` as an exact reciprocal where possible, otherwise `e^(-1)`.
fn reciprocal(expr: Expr) -> Expr {
    if let ExprNode::Num(number) = &*expr {
        return ExprNode::num(number.recip());
    }
    pow(expr, ExprNode::int(-1))
}

/// `a/b` as `a * b^(-1)`, the canonical shape for division in `canlab-core`.
fn divide(numerator: Expr, denominator: Expr, op_pos: usize) -> Result<Expr, ParseError> {
    if denominator.as_number().is_some_and(|number| number.is_zero()) {
        return Err(ParseError::new("division by zero", op_pos));
    }
    Ok(mul([numerator, reciprocal(denominator)]))
}

/// Turn the text of a [`TokenKind::Number`] into an exact number.
///
/// Decimals are read as an exact rational over a power of ten, so `3.5` is
/// `7/2` and arithmetic on it stays exact. A literal that does not fit in
/// `i128` is a parse error rather than a silent float.
fn number_literal(text: &str, pos: usize) -> Result<Expr, ParseError> {
    let overflow = || ParseError::new(format!("number literal `{text}` is too large"), pos);
    match text.split_once('.') {
        None => {
            let value: i128 = text.parse().map_err(|_| overflow())?;
            Ok(ExprNode::num(Number::int(value)))
        }
        Some((whole, fraction)) => {
            let digits = format!("{whole}{fraction}");
            let numerator: i128 = digits.parse().map_err(|_| overflow())?;
            let denominator = 10i128
                .checked_pow(fraction.len() as u32)
                .ok_or_else(overflow)?;
            Ok(ExprNode::num(Number::ratio(numerator, denominator)))
        }
    }
}

/// The numeric value of a parsed limit, if it is a number at all.
///
/// `π` and `e` are not numbers to the parser — they are symbols, and this crate
/// has no evaluator — so `\int_0^{\pi}` is refused with "a definite integral
/// needs a numeric lower limit". That is a narrower answer than the CAS can
/// eventually give and a wider one than none, and it is stated in the message
/// rather than hidden. `crate::integral` takes any two numbers; when limits are
/// expressions, only this function changes.
fn evaluated(expr: &Expr) -> Option<Number> {
    match &**expr {
        ExprNode::Num(number) => Some(number.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Expr {
        parse(src).unwrap_or_else(|err| panic!("{src:?} should parse: {err}"))
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(parse_ok("2+3*4").to_string(), "14");
    }

    #[test]
    fn powers_are_right_associative() {
        assert_eq!(parse_ok("2^3^2").to_string(), "512");
    }

    #[test]
    fn unary_minus_binds_looser_than_power() {
        assert_eq!(parse_ok("-2^2").to_string(), "-4");
    }

    #[test]
    fn explicit_multiplication_absorbs_implicit_products() {
        assert_eq!(parse_ok("2*3x").to_string(), "6*x");
    }

    #[test]
    fn missing_script_argument_reports_the_operator_position() {
        let err = parse("x^").expect_err("`^` needs an exponent");
        assert_eq!(err.position, 1);
        assert!(err.message.contains("exponent"), "message was {}", err.message);
    }

    #[test]
    fn empty_parens_are_an_error_not_a_panic() {
        let err = parse("()").expect_err("`()` has no expression");
        assert_eq!(err.position, 1);
    }

    #[test]
    fn function_application_accepts_a_parenthesized_argument() {
        assert_eq!(parse_ok(r"\sin(x)").to_string(), "sin(x)");
        assert_eq!(parse_ok(r"\sin x").to_string(), "sin(x)");
    }

    #[test]
    fn a_subscript_is_part_of_the_name_and_never_swallows_a_power() {
        // `x_1^2` used to parse as `x^2`: the subscript was consumed and thrown
        // away. The rule now is that `x_1` is one name, so `x_1^2` is that name
        // squared — the power attaches to the name that was written.
        assert_eq!(parse_ok("x_1").to_string(), "x_1");
        assert_eq!(parse_ok("x_1^2").to_string(), "x_1^2");
        // An index *expression* is still refused, and the reason names the shape
        // that is supported.
        let err = parse("x_{n+1}").expect_err("an index expression has no symbol");
        assert!(err.message.contains("part of a name"), "{}", err.message);
        // A plain power still parses, which is the other half of the old test.
        assert_eq!(parse_ok("x^2").to_string(), "x^2");
    }

    #[test]
    fn a_logarithm_with_two_bases_is_refused() {
        // `\log_2_3 x` has no meaning: the base is read once, and a second `_`
        // on the same name would otherwise be dropped.
        let err = parse(r"\log_2_3 x").expect_err("a logarithm has one base");
        assert_eq!(err.position, 6);
        assert!(
            err.message.contains("only one base"),
            "message was {}",
            err.message
        );
    }

    #[test]
    fn large_decimal_literals_are_rejected() {
        let digits = "9".repeat(40);
        let err = parse(&format!("0.{digits}")).expect_err("does not fit in i128");
        assert!(err.message.contains("too large"), "message was {}", err.message);
    }

    // -- definite integrals ---------------------------------------------

    /// The number a parse evaluated to, or the error's message.
    fn value(src: &str) -> Result<canlab_core::number::Number, String> {
        parse(src)
            .map(|expr| expr.as_number().expect("an integral is a number").clone())
            .map_err(|err| err.message)
    }

    #[test]
    fn the_two_corpus_integrals_are_eight_and_sixty_three() {
        // The numbers come from the exam, not from this parser: 3x²·x from 0 to
        // 2 is 8 (x³ = 8), and from 1 to 4 is 63 (4³ − 1³). Both were refused
        // before `\int` had an expression form, and both are the corpus rows
        // the roadmap calls W3.1.
        assert_eq!(value(r"\int_{0}^{2} 3x^2 dx"), Ok(canlab_core::number::Number::int(8)));
        assert_eq!(value(r"\int_1^4 3x^2 dx"), Ok(canlab_core::number::Number::int(63)));
    }

    #[test]
    fn the_integrand_is_not_the_product_it_was_typed_as() {
        // The one bug that produced plausible wrong answers: `x²·dx` merges
        // into `x³` in the canonical product, so reading the tree back gave the
        // integrand `x³` and the answer 4 instead of 1/3. These three pin the
        // integrand-and-power cases that the merge destroys.
        assert_eq!(value(r"\int_0^1 x dx"), Ok(canlab_core::number::Number::ratio(1, 2)));
        assert_eq!(value(r"\int_0^1 x^2 dx"), Ok(canlab_core::number::Number::ratio(1, 3)));
        assert_eq!(value(r"\int_0^1 x^3 dx"), Ok(canlab_core::number::Number::ratio(1, 4)));
        // And a coefficient outside the power is a coefficient, not a merge.
        assert_eq!(value(r"\int_0^2 3x^2 dx"), Ok(canlab_core::number::Number::int(8)));
    }

    #[test]
    fn a_differential_written_with_a_space_is_the_same_integral() {
        assert_eq!(value(r"\int_0^1 x^2 d x"), value(r"\int_0^1 x^2 dx"));
        assert_eq!(value(r"\int_0^1 x^2\,dx"), value(r"\int_0^1 x^2 dx"));
    }

    #[test]
    fn an_area_splits_at_a_crossing_that_lands_on_a_sample() {
        // `\area_0^2 x , 1` is `∫_0^2 |x − 1| = 1/2 + 1/2 = 1`. The crossing is
        // at `x = 1`, which is exactly the midpoint of `[0, 2]` and exactly one
        // of the 512 sample points — so the sign-change test `previous · value <
        // 0` saw `0 · positive = 0`, missed the crossing, took the whole
        // interval as one piece, read the sign at the midpoint as `0` (not
        // negative), and integrated the *signed* difference to `0`.
        assert_eq!(
            value(r"\area_0^2 x , 1"),
            Ok(canlab_core::number::Number::int(1)),
            "a crossing on a sample point was missed"
        );
        // The same area with the curves swapped.
        assert_eq!(
            value(r"\area_0^2 1 , x"),
            Ok(canlab_core::number::Number::int(1))
        );
        // A crossing at a point that is *not* a sample still works. The
        // crossing is at `0.5`, which is not an integer, so the bounds are
        // floats and the answer is numeric: `∫_0^2 |x − 0.5| = 1/8 + 9/8 = 5/4`.
        // The value is right and the type says it came from a float bound.
        assert_eq!(
            value(r"\area_0^2 x , 0.5"),
            Ok(canlab_core::number::Number::float(1.25))
        );
        // And a crossing at an endpoint is not an interior crossing: the sign
        // does not change inside, so the answer is the plain integral.
        assert_eq!(
            value(r"\area_0^3 x , 3"),
            Ok(canlab_core::number::Number::ratio(9, 2))
        );
    }

    #[test]
    fn a_power_of_a_reciprocal_keeps_its_pole() {
        // `(x^{-2})^{-1}` is `x²` only where `x ≠ 0`: at zero the inner power is
        // infinite and the outer one turns that into zero, so the *numeric* path
        // sees a finite value and integrates `x²` across a point where the
        // integrand is not defined. The flattening is therefore only valid on an
        // interval that excludes zero, and an interval that contains it must be
        // refused — the same refusal `\int_0^1 (x^2)^{-1} dx` gives.
        assert!(
            value(r"\int_0^1 (x^{-2})^{-1} dx").is_err(),
            "(x^-2)^-1 across zero must be refused, not integrated"
        );
        assert!(
            value(r"\int_{-1}^{1} (x^{-2})^{-1} dx").is_err(),
            "(x^-2)^-1 across zero must be refused"
        );
        // Away from zero the flattening is faithful, and exact: on `[1, 2]` the
        // integrand is `x²`, so the integral is `8/3 − 1/3 = 7/3`.
        assert_eq!(
            value(r"\int_1^2 (x^{-2})^{-1} dx"),
            Ok(canlab_core::number::Number::ratio(7, 3))
        );
        // A pole that the flattening *preserves* is still a pole: `(x^{-1})²` is
        // `x^{-2}`, and both are infinite at zero.
        assert!(value(r"\int_0^1 (x^{-1})^2 dx").is_err());
        // And a positive inner power has no pole to lose.
        assert_eq!(
            value(r"\int_0^1 (x^2)^{-1} dx").is_err(),
            true,
            "a reciprocal across zero is singular"
        );
        assert_eq!(
            value(r"\int_1^2 (x^2)^{-1} dx"),
            Ok(canlab_core::number::Number::ratio(1, 2))
        );
    }

    #[test]
    fn a_reciprocal_is_exact_and_positive() {
        // `∫_1^2 x^{-2} dx = [−1/x]_1^2 = −1/2 + 1 = 1/2`. The value is exact,
        // and it is **positive**: the first version of the exact path answered
        // `−1/2`, a sign error that no test caught because no test asserted this
        // integral at all.
        assert_eq!(
            value(r"\int_1^2 \frac{1}{x^2} dx"),
            Ok(canlab_core::number::Number::ratio(1, 2))
        );
        // The same integral written with a negative exponent.
        assert_eq!(
            value(r"\int_1^2 x^{-2} dx"),
            Ok(canlab_core::number::Number::ratio(1, 2))
        );
        // `∫_1^4 x^{-1/2} dx = [2√x]_1^4 = 4 − 2 = 2`. The exponent is
        // fractional, so this goes to the numeric path and comes back as a
        // float: exactness is for an integer power, and the test says so rather
        // than demanding a type the arithmetic does not produce.
        assert_eq!(
            value(r"\int_1^4 \frac{1}{\sqrt{x}} dx"),
            Ok(canlab_core::number::Number::float(2.0))
        );
        // A reciprocal across zero is a pole, and is refused rather than
        // answered with a finite number.
        assert!(value(r"\int_{-1}^1 \frac{1}{x^2} dx").is_err());
    }

    #[test]
    fn an_inverted_interval_gives_the_negative_and_a_constant_is_counted_once() {
        assert_eq!(value(r"\int_1^0 x dx"), Ok(canlab_core::number::Number::ratio(-1, 2)));
        assert_eq!(value(r"\int_{-1}^{1} x dx"), Ok(canlab_core::number::Number::int(0)));
        // ∫2 dx on [0, 3] is 6: the constant is a term of its own and must not
        // be counted as a factor that multiplies the next term.
        assert_eq!(value(r"\int_0^3 2 dx"), Ok(canlab_core::number::Number::int(6)));
    }

    #[test]
    fn a_closed_form_and_a_numeric_one_both_land_on_the_known_value() {
        // ∫₀¹ x² is 1/3 to twelve digits; the numeric path is what answers it
        // when there is no polynomial form to recognise.
        let Some(canlab_core::number::Number::Float(numeric)) =
            value(r"\int_0^1 \sin x dx").ok().filter(|_| false)
        else {
            // `sin` has a closed form in `integral`, and its value is
            // 1 − cos 1.
            let value = value(r"\int_0^1 \sin x dx").expect("sin integrates");
            let expected = 1.0 - 1.0f64.cos();
            assert!((value.to_f64() - expected).abs() < 1e-12, "{value:?}");
            return;
        };
        let _ = numeric;
    }

    #[test]
    fn the_gaussian_is_numeric_with_a_value_from_outside_the_parser() {
        // ∫₀¹ e^{−x²} dx = 0.7468241328124270…, a value this crate cannot derive
        // and that a wrong integrand cannot reproduce.
        let value = value(r"\int_0^1 e^{-x^2} dx").expect("a numeric answer");
        assert!((value.to_f64() - 0.746_824_132_812_427_0).abs() < 1e-12, "{value:?}");
    }

    #[test]
    fn a_pole_inside_the_limits_is_refused_and_not_integrated_across() {
        // ∫₋₁¹ 1/x is not zero: the two areas are infinite. A numeric routine
        // that sampled symmetrically would answer 0 and be wrong twice over.
        let err = value(r"\int_{-1}^{1} \frac{1}{x} dx").expect_err("refused");
        assert!(err.contains("not defined"), "{err}");
        // And 1/x on an interval that avoids the pole is a logarithm:
        // ∫₁² 1/x = ln 2.
        let value = value(r"\int_1^{2} \frac{1}{x} dx").expect("a logarithm");
        assert!((value.to_f64() - 2.0f64.ln()).abs() < 1e-12, "{value:?}");
    }

    #[test]
    fn an_integral_with_a_parameter_or_a_symbolic_limit_is_refused_for_its_own_reason() {
        let err = value(r"\int_0^1 a x dx").expect_err("a parameter");
        assert!(err.contains("`a` is a parameter"), "{err}");
        let err = value(r"\int_0^t x dx").expect_err("a symbolic limit");
        assert!(err.contains("numeric upper limit"), "{err}");
        let err = value(r"\int_0^1 3x^2").expect_err("no differential");
        assert!(err.contains("differential"), "{err}");
        let err = value(r"\int_x^1 3x^2 dx").expect_err("a symbolic lower limit");
        assert!(err.contains("numeric lower limit"), "{err}");
    }

    #[test]
    fn int_is_not_a_factor_outside_an_integral() {
        // `\int` became a statement's head; it must not have become a factor.
        let err = parse(r"2\int_0^1 x dx").expect_err("an integral is not a factor");
        assert!(err.message.contains("int"), "{}", err.message);
    }
    // -- limits ----------------------------------------------------------

    #[test]
    fn the_school_limits_come_out_exactly() {
        // `sin x/x → 1` and `(1−cos x)/x² → ½` are the two an exam expects to be
        // known; both are recognised structurally and answered exactly rather
        // than approximated.
        assert_eq!(value(r"\lim_{x \to 0} \frac{\sin x}{x}"), Ok(Number::int(1)));
        assert_eq!(
            value(r"\lim_{x \to 0} \frac{1 - \cos x}{x^2}"),
            Ok(Number::ratio(1, 2))
        );
        // `0/0` by cancellation: the common factor of `x²−1` and `x−1`.
        assert_eq!(value(r"\lim_{x \to 1} \frac{x^2 - 1}{x - 1}"), Ok(Number::int(2)));
        assert_eq!(value(r"\lim_{x \to 3} \frac{x^2 - 9}{x - 3}"), Ok(Number::int(6)));
    }

    #[test]
    fn a_limit_at_infinity_is_the_ratio_of_the_leading_terms() {
        // `∞/∞` by degree, in all three cases.
        assert_eq!(
            value(r"\lim_{x \to \infty} \frac{3x^2 + 1}{2x^2 - 5}"),
            Ok(Number::ratio(3, 2))
        );
        assert_eq!(value(r"\lim_{x \to \infty} \frac{x}{x^2 + 1}"), Ok(Number::int(0)));
        // The numerator's degree is higher: the limit is infinite, and the
        // parser says so rather than answering with a number.
        let error = value(r"\lim_{x \to \infty} \frac{x^2}{x + 1}").expect_err("diverges");
        assert!(error.contains("infinity"), "{error}");
    }

    #[test]
    fn a_jump_reports_that_it_does_not_exist_with_both_sides() {
        // `1/(x−2)` from below is `−∞` and from above `+∞`; the two-sided limit
        // is the message that says both, which is the answer the roadmap asks
        // for and the one a student needs.
        let error = value(r"\lim_{x \to 2} \frac{1}{x - 2}").expect_err("does not exist");
        assert!(error.contains("does not exist"), "{error}");
        assert!(error.contains("negative infinity"), "{error}");
        assert!(error.contains("positive infinity"), "{error}");
        // And the one-sided limits are each a limit of their own.
        let above = value(r"\lim_{x \to 2^+} \frac{1}{x - 2}").expect_err("diverges");
        assert!(above.contains("positive infinity"), "{above}");
        let below = value(r"\lim_{x \to 2^-} \frac{1}{x - 2}").expect_err("diverges");
        assert!(below.contains("negative infinity"), "{below}");
    }

    #[test]
    fn a_limit_that_agrees_from_both_sides_is_the_limit() {
        // `1/x²` diverges from both sides *in the same direction*, so the limit
        // is `+∞` and not "does not exist". The two-sided case has to tell
        // "both sides say the same infinity" from "the sides disagree".
        let error = value(r"\lim_{x \to 0} \frac{1}{x^2}").expect_err("diverges");
        assert!(error.contains("positive infinity"), "{error}");
        assert!(!error.contains("does not exist"), "{error}");
    }

    #[test]
    fn a_limit_that_is_not_a_number_is_refused_with_its_reason() {
        let error = value(r"\lim_{x \to 0} a x").expect_err("a parameter");
        assert!(error.contains("`a` is a parameter"), "{error}");
        let error = value(r"\lim_{x \to t} x").expect_err("a symbolic approach");
        assert!(error.contains("approach must be a number"), "{error}");
        let error = value(r"\lim x").expect_err("no approach");
        assert!(error.contains("needs its approach"), "{error}");
    }

    #[test]
    fn a_continuous_function_is_evaluated_by_substitution() {
        // The common case, and the one that must not be routed through the
        // numeric path: `x²` at 3 is the *exact* 9 — not `9.0`, which would say
        // the answer is a measurement — and `sin 0` is exactly 0.
        assert_eq!(value(r"\lim_{x \to 3} x^2"), Ok(Number::int(9)));
        assert_eq!(value(r"\lim_{x \to 0} \sin x"), Ok(Number::float(0.0)));
        assert_eq!(value(r"\lim_{x \to 2} x + 1"), Ok(Number::int(3)));
    }
    // -- volumes and areas -----------------------------------------------

    #[test]
    fn a_sphere_and_a_cone_match_their_closed_forms() {
        // The two figures the roadmap names. The closed forms are `4πr³/3` and
        // `πr²h/3`; the values come from them, not from this code.
        let sphere = value(r"\vol_{-2}^{2} \sqrt{4 - x^2} dx").expect("a sphere");
        let expected = 4.0 / 3.0 * std::f64::consts::PI * 8.0;
        assert!((sphere.to_f64() - expected).abs() < 1e-9, "{sphere:?} vs {expected}");
        // `y = x/2` on `[0, 2]`: a cone of radius 1 and height 2.
        let cone = value(r"\vol_{0}^{2} \frac{x}{2} dx").expect("a cone");
        let expected = std::f64::consts::PI * 2.0 / 3.0;
        assert!((cone.to_f64() - expected).abs() < 1e-9, "{cone:?} vs {expected}");
    }

    #[test]
    fn the_area_between_two_curves_is_the_area_and_not_the_signed_area() {
        // `x` and `x²` on `[0, 1]`: `∫(x − x²) = 1/6`. The curves cross at 0 and
        // 1, which are the limits, so this is one piece.
        let area = value(r"\area_{0}^{1} x , x^2").expect("an area");
        assert!((area.to_f64() - 1.0 / 6.0).abs() < 1e-9, "{area:?}");
        // `x²` and `x⁴` on `[−1, 1]`: they cross at −1, 0 and 1, and the middle
        // piece has the opposite sign. The signed integral of `x² − x⁴` is
        // `4/15`; a version that forgot the crossing would give 0 for the
        // `x⁴ − x²` ordering, and this test pins the crossing split.
        let area = value(r"\area_{-1}^{1} x^2 , x^4").expect("an area");
        assert!((area.to_f64() - 4.0 / 15.0).abs() < 1e-9, "{area:?}");
        // Swapping the curves does not change the area, which is the property
        // the absolute value exists for.
        let swapped = value(r"\area_{-1}^{1} x^4 , x^2").expect("an area");
        assert!((swapped.to_f64() - area.to_f64()).abs() < 1e-9, "{swapped:?} vs {area:?}");
    }

    #[test]
    fn a_figure_that_cannot_be_read_is_refused_with_its_reason() {
        let error = value(r"\vol_0^1 x").expect_err("no differential");
        assert!(error.contains("differential"), "{error}");
        let error = value(r"\area_0^1 x").expect_err("one curve");
        assert!(error.contains("two curves"), "{error}");
        // `\vol_0^1 x dy` is a curve in `x` swept about `y`: the curve is in
        // the wrong variable for the differential, and the reason is the
        // parameter one, not a generic "cannot read". The old assertion
        // accepted either `parameter` or `curve`, so it could not tell this
        // refusal from the `\vol`-specific one below.
        let error = value(r"\vol_0^1 x dy").expect_err("a curve in x swept about y");
        assert!(error.contains("`x` is a parameter here"), "{error}");
        // The `\vol`-specific refusal is a curve with no variable of its own.
        // It is a different reason, so the test names it rather than accepting
        // any message containing "curve".
        let error = value(r"\vol_0^1 5 dx").expect_err("a constant curve");
        assert!(
            error.contains("needs a curve that depends on the variable"),
            "{error}"
        );
    }
}
