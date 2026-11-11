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
pub(crate) fn parse(src: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(src)?;
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
}

impl Delim {
    fn open(self) -> &'static str {
        match self {
            Delim::Paren => "(",
            Delim::Bracket => "[",
            Delim::Brace => "{",
        }
    }

    fn close(self) -> &'static str {
        match self {
            Delim::Paren => ")",
            Delim::Bracket => "]",
            Delim::Brace => "}",
        }
    }

    fn matches_close(self, kind: &TokenKind) -> bool {
        matches!(
            (self, kind),
            (Delim::Paren, TokenKind::RParen)
                | (Delim::Bracket, TokenKind::RBracket)
                | (Delim::Brace, TokenKind::RBrace)
        )
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(src: &str) -> Result<Self, ParseError> {
        Ok(Parser { tokens: tokenize(src)?, index: 0 })
    }

    // -- token access -------------------------------------------------------

    /// The current token. Always valid: the token list ends with `Eof` and
    /// [`Parser::advance`] never moves past it.
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
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
                // A subscript has no expression form: `Expr` has no indexed
                // symbol, and dropping it made `v_1` and `v_2` both mean `v`.
                // The base of a logarithm is the one subscript that carries a
                // value, and `parse_function` reads it before this loop runs.
                Err(ParseError::new(SUBSCRIPT_REFUSAL, op_pos))
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

    // -- prefix and atoms ---------------------------------------------------

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
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
            "cdot" | "times" | "div" => Err(ParseError::new(
                format!("unexpected `\\{name}`; expected an expression"),
                pos,
            )),
            _ if is_function(name) => self.parse_function(name, pos),
            _ if is_greek(name) => Ok(ExprNode::sym(SymbolId::intern(name))),
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
        if open != close {
            return Err(ParseError::new(
                format!(
                    "mismatched delimiters: `\\left{}` closed by `\\right{}`",
                    open.open(),
                    close.close()
                ),
                right_pos,
            ));
        }
        Ok(inner)
    }

    fn take_open_delimiter(&mut self, who: &str) -> Result<Delim, ParseError> {
        let delim = match &self.peek().kind {
            TokenKind::LParen => Some(Delim::Paren),
            TokenKind::LBracket => Some(Delim::Bracket),
            _ => None,
        };
        match delim {
            Some(delim) => {
                self.advance();
                Ok(delim)
            }
            None => Err(ParseError::new(
                format!(
                    "expected `(` or `[` after `{who}` (found {})",
                    self.peek().kind.describe()
                ),
                self.peek().pos,
            )),
        }
    }

    fn take_close_delimiter(&mut self, who: &str) -> Result<Delim, ParseError> {
        let delim = match &self.peek().kind {
            TokenKind::RParen => Some(Delim::Paren),
            TokenKind::RBracket => Some(Delim::Bracket),
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
                    self.peek().kind.describe()
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
    name == "right"
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
    fn a_subscript_is_refused_before_it_can_swallow_a_power() {
        // `x_1^2` used to parse as `x^2`: the subscript was consumed and thrown
        // away, so the power silently attached to a different expression than
        // the one written. It is refused at the `_` now.
        let err = parse("x_1^2").expect_err("a subscript has no expression form");
        assert_eq!(err.position, 1);
        assert_eq!(err.message, SUBSCRIPT_REFUSAL);
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
}
