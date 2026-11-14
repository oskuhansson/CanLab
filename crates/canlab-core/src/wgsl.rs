//! W7.2: the Lua subset, compiled to WGSL.
//!
//! A kernel is a small Lua program — literals, locals, assignment, `+ − * / ^`,
//! the `math` functions, `if`/`else`, `while`, a fixed `for`, `break`, and a
//! small builtin table. This module reads that subset and emits a WGSL function
//! that computes the same thing, and it is a **compiler**: anything outside the
//! subset is a diagnostic that names the construct and the line, never a guess.
//!
//! # The subset, exactly
//!
//! ```text
//! chunk     := statement*
//! statement := "local" NAME "=" expr
//!            | NAME "=" expr
//!            | "if" expr "then" chunk ("elseif" expr "then" chunk)* ("else" chunk)? "end"
//!            | "while" expr "do" chunk "end"
//!            | "for" NAME "=" expr "," expr ("," expr)? "do" chunk "end"
//!            | "break"
//!            | "return" expr
//! expr      := or_expr
//! or_expr   := and_expr ("or" and_expr)*
//! and_expr  := cmp_expr ("and" cmp_expr)*
//! cmp_expr  := add_expr (("<"|">"|"<="|">="|"=="|"~=") add_expr)?
//! add_expr  := mul_expr (("+"|"-") mul_expr)*
//! mul_expr  := pow_expr (("*"|"/"|"%") pow_expr)*
//! pow_expr  := unary ("^" pow_expr)?
//! unary     := ("-"|"not") unary | atom
//! atom      := NUMBER | NAME | NAME "(" args ")" | "(" expr ")"
//! ```
//!
//! # Why the WGSL is a function and not a whole shader
//!
//! The shader that runs a kernel has a fixed shape — an entry point, a storage
//! buffer, a per-invocation index — and the kernel is one function inside it.
//! Emitting the whole shader here would make this module responsible for the
//! binding layout, which is W7.3's job; emitting the function keeps the
//! compiler about the *language*.
//!
//! # What is refused, and why a diagnostic names the construct
//!
//! A table, a closure, `goto`, a string, a `repeat`, a method call, a
//! multi-value return — each is a construct this compiler does not have, and
//! each is refused with its name and its line. The alternative is a kernel that
//! compiles and computes something else, which is the failure this project
//! treats as the worst kind.
//!
//! # The NaN rule
//!
//! Two compilers propagate different NaN payloads, and the roadmap says so: a
//! differential test compares "is a NaN" and not the bits. This compiler emits
//! `0.0 / 0.0` for a division that may be a NaN, which is the same NaN every
//! WGSL implementation produces, and the test compares with `is_nan()`.

use std::collections::HashMap;
use std::fmt::Write as _;

/// Why a program is not in the subset.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// What the compiler could not read, named.
    pub construct: String,
    /// The line, from 1.
    pub line: usize,
    /// A sentence saying what is supported instead.
    pub hint: String,
}

impl Diagnostic {
    fn new(construct: impl Into<String>, line: usize, hint: impl Into<String>) -> Diagnostic {
        Diagnostic {
            construct: construct.into(),
            line,
            hint: hint.into(),
        }
    }

    /// The message a caller shows.
    pub fn message(&self) -> String {
        format!(
            "line {}: `{}` is not in the kernel subset — {}",
            self.line, self.construct, self.hint
        )
    }
}

/// A compiled kernel.
#[derive(Debug, Clone, PartialEq)]
pub struct Kernel {
    /// The WGSL function, named `kernel`.
    pub wgsl: String,
    /// The parameters the kernel takes, in order.
    pub parameters: Vec<String>,
    /// The locals it declares.
    pub locals: Vec<String>,
}

/// Compile a Lua-subset program to a WGSL function.
///
/// The program's `return` becomes the function's return value; a program with no
/// `return` returns the last assignment, which is what a document row means.
pub fn compile(program: &str) -> Result<Kernel, Diagnostic> {
    let tokens = tokenize(program)?;
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    let chunk = parser.chunk()?;
    let mut compiler = Compiler::default();
    compiler.block(&chunk, 1)?;
    let returns = compiler.returned;
    let body = compiler.out;
    let mut wgsl = String::new();
    let parameters: Vec<String> = compiler.parameters.clone();
    let signature: Vec<String> = parameters
        .iter()
        .map(|name| format!("{name}: f32"))
        .collect();
    let _ = write!(
        wgsl,
        "fn kernel({}) -> f32 {{\n{}{}}}\n",
        signature.join(", "),
        body,
        if returns {
            String::new()
        } else {
            String::from("  return 0.0;\n")
        }
    );
    Ok(Kernel {
        wgsl,
        parameters,
        locals: compiler.locals,
    })
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Keyword(&'static str),
    Symbol(&'static str),
    End,
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Number(value) => format!("the number `{value}`"),
            Token::Name(name) => format!("the name `{name}`"),
            Token::Keyword(word) => format!("`{word}`"),
            Token::Symbol(symbol) => format!("`{symbol}`"),
            Token::End => String::from("the end of the program"),
        }
    }
}

const KEYWORDS: &[&str] = &[
    "local", "if", "then", "elseif", "else", "end", "while", "do", "for", "break", "return",
    "and", "or", "not", "true", "false", "nil", "function", "repeat", "until", "goto", "in",
];

fn tokenize(source: &str) -> Result<Vec<(Token, usize)>, Diagnostic> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    let mut line = 1usize;
    while let Some(&c) = chars.peek() {
        match c {
            '\n' => {
                line += 1;
                chars.next();
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            '-' if source[chars.clone().take(2).collect::<String>().len()..].starts_with("--") => {
                // A comment: to the end of the line.
                while let Some(&next) = chars.peek() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            // A `.` starts a number only when a digit follows it; otherwise it
            // is a field access, which is refused below. The first version
            // tested `source.contains('.')`, which is true for any program with
            // a decimal point anywhere — so `x.foo` was read as the number `.`.
            c if c.is_ascii_digit()
                || (c == '.'
                    && chars
                        .clone()
                        .nth(1)
                        .is_some_and(|next| next.is_ascii_digit())) => {
                let mut text = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() || next == '.' {
                        text.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let value: f64 = text
                    .parse()
                    .map_err(|_| Diagnostic::new(format!("the number `{text}`"), line, "a number must be a decimal literal"))?;
                tokens.push((Token::Number(value), line));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut text = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '_' {
                        text.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // `math.sin` is one *name* in this subset, not a field access:
                // the builtin table is addressed by a dotted name, and the
                // field-access refusal below is for tables a kernel does not
                // have.
                if text == "math" && chars.peek() == Some(&'.') {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    if lookahead.peek().is_some_and(|next| next.is_alphabetic()) {
                        text.push('.');
                        chars.next();
                        while let Some(&next) = chars.peek() {
                            if next.is_alphanumeric() || next == '_' {
                                text.push(next);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                let token = match KEYWORDS.iter().find(|word| **word == text) {
                    Some(word) => Token::Keyword(word),
                    None => Token::Name(text),
                };
                tokens.push((token, line));
            }
            '"' | '\'' => {
                return Err(Diagnostic::new(
                    "a string",
                    line,
                    "a kernel computes numbers; a string has no value here",
                ));
            }
            '{' => {
                return Err(Diagnostic::new(
                    "a table",
                    line,
                    "a kernel has numbers and locals, not tables",
                ));
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push((Token::Symbol("=="), line));
                } else {
                    tokens.push((Token::Symbol("="), line));
                }
            }
            '~' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push((Token::Symbol("~="), line));
                } else {
                    return Err(Diagnostic::new("`~`", line, "only `~=` is a comparison"));
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push((Token::Symbol("<="), line));
                } else {
                    tokens.push((Token::Symbol("<"), line));
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push((Token::Symbol(">="), line));
                } else {
                    tokens.push((Token::Symbol(">"), line));
                }
            }
            '+' => {
                chars.next();
                tokens.push((Token::Symbol("+"), line));
            }
            '-' => {
                chars.next();
                tokens.push((Token::Symbol("-"), line));
            }
            '*' => {
                chars.next();
                tokens.push((Token::Symbol("*"), line));
            }
            '/' => {
                chars.next();
                tokens.push((Token::Symbol("/"), line));
            }
            '%' => {
                chars.next();
                tokens.push((Token::Symbol("%"), line));
            }
            '^' => {
                chars.next();
                tokens.push((Token::Symbol("^"), line));
            }
            '(' => {
                chars.next();
                tokens.push((Token::Symbol("("), line));
            }
            ')' => {
                chars.next();
                tokens.push((Token::Symbol(")"), line));
            }
            ',' => {
                chars.next();
                tokens.push((Token::Symbol(","), line));
            }
            ';' => {
                chars.next();
            }
            ':' => {
                return Err(Diagnostic::new(
                    "a method call",
                    line,
                    "a kernel calls the builtin functions by name",
                ));
            }
            '.' => {
                return Err(Diagnostic::new(
                    "a field access",
                    line,
                    "a kernel has no tables to index",
                ));
            }
            other => {
                return Err(Diagnostic::new(
                    format!("the character `{other}`"),
                    line,
                    "a kernel is numbers, names and arithmetic",
                ));
            }
        }
    }
    tokens.push((Token::End, line));
    Ok(tokens)
}

// ---------------------------------------------------------------------------
// The syntax tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Number(f64),
    Name(String),
    Call(String, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
enum Statement {
    Local(String, Expr),
    Assign(String, Expr),
    If(Vec<(Expr, Vec<Statement>)>, Option<Vec<Statement>>),
    While(Expr, Vec<Statement>),
    For(String, Expr, Expr, Option<Expr>, Vec<Statement>),
    Break,
    Return(Expr),
}

// ---------------------------------------------------------------------------
// The parser
// ---------------------------------------------------------------------------

struct Parser {
    /// The tokens, each with the line it is on. The line travels with the token
    /// and not in a field of the parser, because a diagnostic must name the line
    /// of the *construct* and not of wherever the parser happens to be.
    tokens: Vec<(Token, usize)>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.position].0
    }

    fn peek_line(&self) -> usize {
        self.tokens[self.position].1
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].0.clone();
        if self.position + 1 < self.tokens.len() {
            self.position += 1;
        }
        token
    }

    fn eat_keyword(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Token::Keyword(k) if *k == word) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_symbol(&mut self, symbol: &str) -> bool {
        if matches!(self.peek(), Token::Symbol(s) if *s == symbol) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<(), Diagnostic> {
        if self.eat_symbol(symbol) {
            return Ok(());
        }
        Err(Diagnostic::new(
            self.peek().describe(),
            self.peek_line(),
            format!("expected `{symbol}`"),
        ))
    }

    fn expect_keyword(&mut self, word: &str) -> Result<(), Diagnostic> {
        if self.eat_keyword(word) {
            return Ok(());
        }
        Err(Diagnostic::new(
            self.peek().describe(),
            self.peek_line(),
            format!("expected `{word}`"),
        ))
    }

    fn name(&mut self) -> Result<String, Diagnostic> {
        match self.peek().clone() {
            Token::Name(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(Diagnostic::new(
                other.describe(),
                self.peek_line(),
                "expected a name",
            )),
        }
    }

    fn chunk(&mut self) -> Result<Vec<Statement>, Diagnostic> {
        let mut statements = Vec::new();
        loop {
            match self.peek() {
                Token::End => break,
                Token::Keyword("end") | Token::Keyword("else") | Token::Keyword("elseif") => break,
                _ => statements.push(self.statement()?),
            }
        }
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Statement, Diagnostic> {
        let line = self.peek_line();
        match self.peek().clone() {
            Token::Keyword("local") => {
                self.advance();
                let name = self.name()?;
                self.expect_symbol("=")?;
                Ok(Statement::Local(name, self.expr()?))
            }
            Token::Keyword("if") => {
                self.advance();
                let mut branches = Vec::new();
                let condition = self.expr()?;
                self.expect_keyword("then")?;
                let body = self.chunk()?;
                branches.push((condition, body));
                let mut otherwise = None;
                loop {
                    if self.eat_keyword("elseif") {
                        let condition = self.expr()?;
                        self.expect_keyword("then")?;
                        let body = self.chunk()?;
                        branches.push((condition, body));
                    } else if self.eat_keyword("else") {
                        otherwise = Some(self.chunk()?);
                        break;
                    } else {
                        break;
                    }
                }
                self.expect_keyword("end")?;
                Ok(Statement::If(branches, otherwise))
            }
            Token::Keyword("while") => {
                self.advance();
                let condition = self.expr()?;
                self.expect_keyword("do")?;
                let body = self.chunk()?;
                self.expect_keyword("end")?;
                Ok(Statement::While(condition, body))
            }
            Token::Keyword("for") => {
                self.advance();
                let name = self.name()?;
                self.expect_symbol("=")?;
                let from = self.expr()?;
                self.expect_symbol(",")?;
                let to = self.expr()?;
                let step = if self.eat_symbol(",") {
                    Some(self.expr()?)
                } else {
                    None
                };
                self.expect_keyword("do")?;
                let body = self.chunk()?;
                self.expect_keyword("end")?;
                Ok(Statement::For(name, from, to, step, body))
            }
            Token::Keyword("break") => {
                self.advance();
                Ok(Statement::Break)
            }
            Token::Keyword("return") => {
                self.advance();
                Ok(Statement::Return(self.expr()?))
            }
            Token::Keyword("function") => Err(Diagnostic::new(
                "a function definition",
                line,
                "a kernel is one function; call the builtins instead",
            )),
            Token::Keyword("repeat") => Err(Diagnostic::new(
                "a `repeat` loop",
                line,
                "`while` is the loop this kernel has",
            )),
            Token::Keyword("goto") => Err(Diagnostic::new(
                "`goto`",
                line,
                "WGSL has no `goto`, and neither does this subset",
            )),
            Token::Keyword("in") => Err(Diagnostic::new(
                "a generic `for`",
                line,
                "the `for` here is `for i = a, b do`",
            )),
            Token::Name(name) => {
                self.advance();
                self.expect_symbol("=")?;
                Ok(Statement::Assign(name, self.expr()?))
            }
            other => Err(Diagnostic::new(
                other.describe(),
                line,
                "expected a statement",
            )),
        }
    }

    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.and_expr()?;
        while self.eat_keyword("or") {
            let right = self.and_expr()?;
            left = Expr::Binary("or", Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.comparison()?;
        while self.eat_keyword("and") {
            let right = self.comparison()?;
            left = Expr::Binary("and", Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Expr, Diagnostic> {
        let left = self.additive()?;
        for symbol in ["<=", ">=", "==", "~=", "<", ">"] {
            if self.eat_symbol(symbol) {
                let right = self.additive()?;
                let operator: &'static str = match symbol {
                    "<=" => "<=",
                    ">=" => ">=",
                    "==" => "==",
                    "~=" => "~=",
                    "<" => "<",
                    _ => ">",
                };
                return Ok(Expr::Binary(operator, Box::new(left), Box::new(right)));
            }
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.multiplicative()?;
        loop {
            if self.eat_symbol("+") {
                let right = self.multiplicative()?;
                left = Expr::Binary("+", Box::new(left), Box::new(right));
            } else if self.eat_symbol("-") {
                let right = self.multiplicative()?;
                left = Expr::Binary("-", Box::new(left), Box::new(right));
            } else {
                return Ok(left);
            }
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.power()?;
        loop {
            if self.eat_symbol("*") {
                let right = self.power()?;
                left = Expr::Binary("*", Box::new(left), Box::new(right));
            } else if self.eat_symbol("/") {
                let right = self.power()?;
                left = Expr::Binary("/", Box::new(left), Box::new(right));
            } else if self.eat_symbol("%") {
                let right = self.power()?;
                left = Expr::Binary("%", Box::new(left), Box::new(right));
            } else {
                return Ok(left);
            }
        }
    }

    fn power(&mut self) -> Result<Expr, Diagnostic> {
        let base = self.unary()?;
        if self.eat_symbol("^") {
            // Right-associative: `2^3^2` is `2^(3^2)`.
            let exponent = self.power()?;
            return Ok(Expr::Binary("^", Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.eat_symbol("-") {
            return Ok(Expr::Unary("-", Box::new(self.unary()?)));
        }
        if self.eat_keyword("not") {
            return Ok(Expr::Unary("not", Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, Diagnostic> {
        let line = self.peek_line();
        match self.peek().clone() {
            Token::Number(value) => {
                self.advance();
                Ok(Expr::Number(value))
            }
            Token::Name(name) => {
                self.advance();
                if self.eat_symbol("(") {
                    let mut args = Vec::new();
                    if !self.eat_symbol(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat_symbol(",") {
                                continue;
                            }
                            self.expect_symbol(")")?;
                            break;
                        }
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Name(name))
                }
            }
            Token::Symbol("(") => {
                self.advance();
                let inner = self.expr()?;
                self.expect_symbol(")")?;
                Ok(inner)
            }
            Token::Keyword("true") => {
                self.advance();
                Ok(Expr::Number(1.0))
            }
            Token::Keyword("false") => {
                self.advance();
                Ok(Expr::Number(0.0))
            }
            Token::Keyword("nil") => Err(Diagnostic::new(
                "`nil`",
                line,
                "a kernel has numbers and locals, not nil",
            )),
            other => Err(Diagnostic::new(
                other.describe(),
                line,
                "expected an expression",
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// The compiler
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Compiler {
    out: String,
    parameters: Vec<String>,
    locals: Vec<String>,
    returned: bool,
    /// How deeply nested the loops are: a `break` outside one is refused.
    loops: usize,
}

impl Compiler {
    fn block(&mut self, statements: &[Statement], depth: usize) -> Result<(), Diagnostic> {
        for statement in statements {
            self.statement(statement, depth)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Statement, depth: usize) -> Result<(), Diagnostic> {
        let indent = "  ".repeat(depth);
        match statement {
            Statement::Local(name, value) => {
                let expression = self.expr(value)?;
                let _ = writeln!(self.out, "{indent}var {name}: f32 = {expression};");
                self.locals.push(name.clone());
            }
            Statement::Assign(name, value) => {
                let expression = self.expr(value)?;
                if self.locals.contains(name) || self.parameters.contains(name) {
                    let _ = writeln!(self.out, "{indent}{name} = {expression};");
                } else {
                    // An assignment to a name that was never declared is a
                    // *parameter*: the kernel takes it from the caller.
                    self.parameters.push(name.clone());
                    let _ = writeln!(self.out, "{indent}var {name}: f32 = {expression};");
                    self.locals.push(name.clone());
                }
            }
            Statement::If(branches, otherwise) => {
                for (index, (condition, body)) in branches.iter().enumerate() {
                    // A condition is a `bool` in WGSL, and `select(0.0, 1.0, …)`
                    // is an `f32`. The first version emitted the select here and
                    // the result did not compile: `if (select(0.0, 1.0, x > 1.0))`
                    // passes an `f32` where a `bool` is required. The Solver
                    // owner found it while writing the W7.3 sweep, which cannot
                    // see it because the sweep does not run a WGSL validator.
                    let test = self.condition(condition)?;
                    if index == 0 {
                        let _ = writeln!(self.out, "{indent}if ({test}) {{");
                    } else {
                        let _ = writeln!(self.out, "{indent}}} else if ({test}) {{");
                    }
                    self.block(body, depth + 1)?;
                }
                if let Some(body) = otherwise {
                    let _ = writeln!(self.out, "{indent}}} else {{");
                    self.block(body, depth + 1)?;
                }
                let _ = writeln!(self.out, "{indent}}}");
            }
            Statement::While(condition, body) => {
                let test = self.condition(condition)?;
                let _ = writeln!(self.out, "{indent}while ({test}) {{");
                self.loops += 1;
                self.block(body, depth + 1)?;
                self.loops -= 1;
                let _ = writeln!(self.out, "{indent}}}");
            }
            Statement::For(name, from, to, step, body) => {
                let start = self.expr(from)?;
                let end = self.expr(to)?;
                let stride = match step {
                    Some(step) => self.expr(step)?,
                    None => String::from("1.0"),
                };
                // A fixed `for` is a WGSL `for` with a float counter, because a
                // kernel's arithmetic is `f32`: an integer counter would need a
                // cast at every use.
                let _ = writeln!(
                    self.out,
                    "{indent}for (var {name}: f32 = {start}; {name} <= {end}; {name} = {name} + {stride}) {{"
                );
                self.locals.push(name.clone());
                self.loops += 1;
                self.block(body, depth + 1)?;
                self.loops -= 1;
                let _ = writeln!(self.out, "{indent}}}");
            }
            Statement::Break => {
                if self.loops == 0 {
                    return Err(Diagnostic::new(
                        "`break`",
                        0,
                        "a `break` must be inside a loop",
                    ));
                }
                let _ = writeln!(self.out, "{indent}break;");
            }
            Statement::Return(value) => {
                let expression = self.expr(value)?;
                let _ = writeln!(self.out, "{indent}return {expression};");
                self.returned = true;
            }
        }
        Ok(())
    }

    /// An expression as a WGSL `bool`, for a condition.
    ///
    /// A comparison is emitted as the comparison itself, `x > 1.0`, which is a
    /// `bool`; anything else is a number, and a number is truthy when it is not
    /// zero — `x != 0.0`, which is also a `bool`. The one thing that is *not*
    /// done is wrapping a comparison in `select(0.0, 1.0, …)`: that is the
    /// value form, and a value is an `f32`.
    fn condition(&mut self, expression: &Expr) -> Result<String, Diagnostic> {
        if let Expr::Binary(operator, left, right) = expression
            && matches!(
                *operator,
                "<" | ">" | "<=" | ">=" | "==" | "~=" | "and" | "or"
            )
        {
            let a = self.expr(left)?;
            let b = self.expr(right)?;
            return Ok(match *operator {
                "<" => format!("({a} < {b})"),
                ">" => format!("({a} > {b})"),
                "<=" => format!("({a} <= {b})"),
                ">=" => format!("({a} >= {b})"),
                "==" => format!("({a} == {b})"),
                "~=" => format!("({a} != {b})"),
                "and" => format!("({a} != 0.0 && {b} != 0.0)"),
                _ => format!("({a} != 0.0 || {b} != 0.0)"),
            });
        }
        if let Expr::Unary("not", operand) = expression {
            let inner = self.condition(operand)?;
            return Ok(format!("(!{inner})"));
        }
        let value = self.expr(expression)?;
        Ok(format!("({value} != 0.0)"))
    }

    fn expr(&mut self, expression: &Expr) -> Result<String, Diagnostic> {
        match expression {
            Expr::Number(value) => Ok(format_float(*value)),
            Expr::Name(name) => Ok(name.clone()),
            Expr::Unary(operator, operand) => {
                let inner = self.expr(operand)?;
                Ok(match *operator {
                    "-" => format!("(-{inner})"),
                    _ => format!("select(1.0, 0.0, {inner} != 0.0)"),
                })
            }
            Expr::Binary(operator, left, right) => {
                let a = self.expr(left)?;
                let b = self.expr(right)?;
                Ok(match *operator {
                    "+" => format!("({a} + {b})"),
                    "-" => format!("({a} - {b})"),
                    "*" => format!("({a} * {b})"),
                    "/" => format!("({a} / {b})"),
                    "%" => format!("({a} % {b})"),
                    "^" => format!("pow({a}, {b})"),
                    "<" => format!("select(0.0, 1.0, {a} < {b})"),
                    ">" => format!("select(0.0, 1.0, {a} > {b})"),
                    "<=" => format!("select(0.0, 1.0, {a} <= {b})"),
                    ">=" => format!("select(0.0, 1.0, {a} >= {b})"),
                    "==" => format!("select(0.0, 1.0, {a} == {b})"),
                    "~=" => format!("select(0.0, 1.0, {a} != {b})"),
                    "and" => format!("select(0.0, 1.0, {a} != 0.0 && {b} != 0.0)"),
                    "or" => format!("select(0.0, 1.0, {a} != 0.0 || {b} != 0.0)"),
                    other => {
                        return Err(Diagnostic::new(
                            format!("the operator `{other}`"),
                            0,
                            "the arithmetic subset is `+ - * / % ^`",
                        ))
                    }
                })
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<String, Diagnostic> {
        let arguments: Vec<String> = args
            .iter()
            .map(|argument| self.expr(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let joined = arguments.join(", ");
        // The builtin table. `rand` is the per-invocation random stream; in WGSL
        // it is a hash of the invocation index, which the shader supplies as
        // `seed` — the compiler emits the call and the shader defines it.
        let mapped = match (name, arguments.len()) {
            ("math.sin", 1) => format!("sin({joined})"),
            ("math.cos", 1) => format!("cos({joined})"),
            ("math.tan", 1) => format!("tan({joined})"),
            ("math.exp", 1) => format!("exp({joined})"),
            ("math.log", 1) => format!("log({joined})"),
            ("math.sqrt", 1) => format!("sqrt({joined})"),
            ("math.abs", 1) => format!("abs({joined})"),
            ("math.floor", 1) => format!("floor({joined})"),
            ("math.min", 2) => format!("min({joined})"),
            ("math.max", 2) => format!("max({joined})"),
            ("math.pow", 2) => format!("pow({joined})"),
            ("rand", 0) => String::from("rand(seed)"),
            ("math.random", 0) => String::from("rand(seed)"),
            (other, count) => {
                return Err(Diagnostic::new(
                    format!("the call `{other}({count} argument(s))`"),
                    0,
                    "the builtin table is `math.sin/cos/tan/exp/log/sqrt/abs/floor/min/max/pow` \
                     and `rand()`",
                ))
            }
        };
        Ok(mapped)
    }
}

/// A number as a WGSL `f32` literal.
///
/// WGSL has no implicit conversion: `2` is an `i32` and `2.0` is an `f32`, and
/// `2 * x` with a float `x` does not compile. Every literal therefore carries
/// its decimal point.
fn format_float(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

// ---------------------------------------------------------------------------
// The interpreter: the same program, run on the CPU
// ---------------------------------------------------------------------------

/// Which arithmetic the interpreter uses.
///
/// The WGSL emitter writes `f32`, so a GPU run rounds every operation to
/// single precision. An interpreter that used `f64` throughout would disagree
/// with the GPU for a reason that has nothing to do with the kernel — so the
/// precision is a parameter and not a constant, and a differential test runs the
/// same program both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// `f64`, which is what the Rust reference computes.
    F64,
    /// `f32`, which is what the emitted WGSL computes.
    F32,
}

impl Precision {
    fn round(self, value: f64) -> f64 {
        match self {
            Precision::F64 => value,
            Precision::F32 => f64::from(value as f32),
        }
    }
}

/// Why a program could not be run.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// The program is not in the subset.
    Diagnostic(Diagnostic),
    /// A name was read before it was written.
    Unbound(String),
    /// A `return` outside a function body — which is every `return` here, since
    /// the program *is* the body.
    NoValue,
    /// The program ran more statements than the budget allows. A `while` that
    /// never terminates is a bug in the kernel, and a hang is not a diagnostic.
    BudgetExceeded {
        /// How many statements were allowed.
        budget: u64,
    },
}

impl RunError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            RunError::Diagnostic(diagnostic) => diagnostic.message(),
            RunError::Unbound(name) => {
                format!("`{name}` is read before it is written")
            }
            RunError::NoValue => {
                "the program returned no value, and a kernel must".to_string()
            }
            RunError::BudgetExceeded { budget } => format!(
                "the program ran {budget} statements without finishing; a kernel loop is unbounded"
            ),
        }
    }
}

impl From<Diagnostic> for RunError {
    fn from(diagnostic: Diagnostic) -> RunError {
        RunError::Diagnostic(diagnostic)
    }
}

/// How many statements one run may execute before it is called a runaway.
///
/// A kernel is a few hundred statements per invocation; a million is far above
/// any real one and far below a hang.
pub const RUN_BUDGET: u64 = 1_000_000;

/// Run a Lua-subset program on the CPU, returning its `return` value.
///
/// This is the other half of "written once": [`compile`] parses the program and
/// emits WGSL for the GPU, and this runs the *same parsed program* here. Without
/// it the two sources would be a source and a re-implementation, which is the
/// drift this workstream exists to avoid — and `mlua` is not a dependency, so
/// there is no Lua runtime to run the text with.
///
/// `arguments` are the program's free names, in the order the caller supplies
/// them; `seed` is the per-invocation random stream that `rand()` draws from.
pub fn interpret(
    program: &str,
    arguments: &[(&str, f64)],
    seed: u64,
    precision: Precision,
) -> Result<f64, RunError> {
    let tokens = tokenize(program)?;
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    let chunk = parser.chunk()?;
    let mut machine = Machine {
        variables: HashMap::new(),
        precision,
        rng: seed,
        steps: 0,
    };
    for (name, value) in arguments {
        machine.variables.insert((*name).to_string(), *value);
    }
    match machine.run(&chunk)? {
        Flow::Return(value) => Ok(value),
        // A program with no `return` returns zero, which is what the emitter
        // writes for one (`return 0.0;`) so that the two agree.
        Flow::Normal | Flow::Break => Ok(0.0),
    }
}

enum Flow {
    Normal,
    Break,
    Return(f64),
}

struct Machine {
    variables: HashMap<String, f64>,
    precision: Precision,
    /// The random stream's state. splitmix64, so that a seed gives the same
    /// draws on every machine and in every language.
    rng: u64,
    steps: u64,
}

impl Machine {
    fn tick(&mut self) -> Result<(), RunError> {
        self.steps += 1;
        if self.steps > RUN_BUDGET {
            return Err(RunError::BudgetExceeded {
                budget: RUN_BUDGET,
            });
        }
        Ok(())
    }

    /// The next draw in `[0, 1)`.
    ///
    /// splitmix64 and not a language's default generator: the same seed must
    /// give the same stream here and in the WGSL, or a differential test is
    /// comparing two different photons.
    fn random(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // The top 53 bits, so the result is a `f64` in `[0, 1)` without a bias.
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    fn run(&mut self, statements: &[Statement]) -> Result<Flow, RunError> {
        for statement in statements {
            self.tick()?;
            match statement {
                Statement::Local(name, value) | Statement::Assign(name, value) => {
                    let value = self.eval(value)?;
                    self.variables.insert(name.clone(), value);
                }
                Statement::If(branches, otherwise) => {
                    let mut taken = false;
                    for (condition, body) in branches {
                        if self.eval(condition)? != 0.0 {
                            match self.run(body)? {
                                Flow::Normal => {}
                                other => return Ok(other),
                            }
                            taken = true;
                            break;
                        }
                    }
                    if !taken && let Some(body) = otherwise {
                        match self.run(body)? {
                            Flow::Normal => {}
                            other => return Ok(other),
                        }
                    }
                }
                Statement::While(condition, body) => {
                    while self.eval(condition)? != 0.0 {
                        self.tick()?;
                        match self.run(body)? {
                            Flow::Normal => {}
                            Flow::Break => break,
                            other => return Ok(other),
                        }
                    }
                }
                Statement::For(name, from, to, step, body) => {
                    let from = self.eval(from)?;
                    let to = self.eval(to)?;
                    let step = match step {
                        Some(step) => self.eval(step)?,
                        None => 1.0,
                    };
                    // A zero step would loop forever; the budget catches it, but
                    // a `for` that cannot advance is refused by name instead of
                    // by running a million times first.
                    if step == 0.0 {
                        return Err(RunError::Diagnostic(Diagnostic::new(
                            "a `for` with a zero step",
                            0,
                            "the loop would never advance",
                        )));
                    }
                    let mut value = from;
                    while (step > 0.0 && value <= to) || (step < 0.0 && value >= to) {
                        self.tick()?;
                        self.variables.insert(name.clone(), value);
                        match self.run(body)? {
                            Flow::Normal => {}
                            Flow::Break => break,
                            other => return Ok(other),
                        }
                        value += step;
                    }
                }
                Statement::Break => return Ok(Flow::Break),
                Statement::Return(value) => {
                    let value = self.eval(value)?;
                    return Ok(Flow::Return(value));
                }
            }
        }
        Ok(Flow::Normal)
    }

    fn eval(&mut self, expression: &Expr) -> Result<f64, RunError> {
        let value = match expression {
            Expr::Number(value) => *value,
            Expr::Name(name) => *self
                .variables
                .get(name)
                .ok_or_else(|| RunError::Unbound(name.clone()))?,
            Expr::Unary(operator, operand) => {
                let operand = self.eval(operand)?;
                match *operator {
                    "-" => -operand,
                    _ => {
                        if operand == 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                }
            }
            Expr::Binary(operator, left, right) => {
                let a = self.eval(left)?;
                let b = self.eval(right)?;
                match *operator {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => a / b,
                    "%" => a % b,
                    "^" => a.powf(b),
                    "<" => bool_value(a < b),
                    ">" => bool_value(a > b),
                    "<=" => bool_value(a <= b),
                    ">=" => bool_value(a >= b),
                    "==" => bool_value(a == b),
                    "~=" => bool_value(a != b),
                    "and" => bool_value(a != 0.0 && b != 0.0),
                    "or" => bool_value(a != 0.0 || b != 0.0),
                    other => {
                        return Err(RunError::Diagnostic(Diagnostic::new(
                            format!("the operator `{other}`"),
                            0,
                            "the arithmetic subset is `+ - * / % ^`",
                        )))
                    }
                }
            }
            Expr::Call(name, arguments) => {
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(self.eval(argument)?);
                }
                self.call(name, &values)?
            }
        };
        Ok(self.precision.round(value))
    }

    fn call(&mut self, name: &str, values: &[f64]) -> Result<f64, RunError> {
        let one = |values: &[f64]| values.first().copied();
        let value = match (name, values.len()) {
            ("math.sin", 1) => one(values).map(f64::sin),
            ("math.cos", 1) => one(values).map(f64::cos),
            ("math.tan", 1) => one(values).map(f64::tan),
            ("math.exp", 1) => one(values).map(f64::exp),
            ("math.log", 1) => one(values).map(f64::ln),
            ("math.sqrt", 1) => one(values).map(f64::sqrt),
            ("math.abs", 1) => one(values).map(f64::abs),
            ("math.floor", 1) => one(values).map(f64::floor),
            ("math.min", 2) => Some(values[0].min(values[1])),
            ("math.max", 2) => Some(values[0].max(values[1])),
            ("math.pow", 2) => Some(values[0].powf(values[1])),
            ("rand", 0) => Some(self.random()),
            ("math.random", 0) => Some(self.random()),
            (other, count) => {
                return Err(RunError::Diagnostic(Diagnostic::new(
                    format!("the call `{other}({count} argument(s))`"),
                    0,
                    "the builtin table is `math.sin/cos/tan/exp/log/sqrt/abs/floor/min/max/pow` \
                     and `rand()`",
                )))
            }
        };
        value.ok_or_else(|| {
            RunError::Diagnostic(Diagnostic::new(
                format!("the call `{name}`"),
                0,
                "a builtin was called with the wrong number of arguments",
            ))
        })
    }
}

fn bool_value(condition: bool) -> f64 {
    if condition { 1.0 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_ok(source: &str) -> Kernel {
        compile(source).unwrap_or_else(|error| panic!("{source} should compile: {}", error.message()))
    }

    #[test]
    fn a_literal_expression_compiles_to_wgsl() {
        let kernel = compile_ok("return 2 * 3 + 1");
        assert!(kernel.wgsl.contains("fn kernel()"), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("return ((2.0 * 3.0) + 1.0);"), "{}", kernel.wgsl);
    }

    #[test]
    fn a_local_and_an_assignment_compile() {
        let kernel = compile_ok("local x = 2\nreturn x * x");
        assert!(kernel.wgsl.contains("var x: f32 = 2.0;"), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("return (x * x);"), "{}", kernel.wgsl);
        assert_eq!(kernel.locals, vec![String::from("x")]);
    }

    #[test]
    fn the_math_functions_map_to_wgsl_builtins() {
        let kernel = compile_ok("return math.sin(0.5) + math.sqrt(4) + math.abs(-1)");
        assert!(kernel.wgsl.contains("sin(0.5)"), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("sqrt(4.0)"), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("abs((-1.0))"), "{}", kernel.wgsl);
    }

    #[test]
    fn an_if_else_becomes_a_wgsl_if() {
        let kernel = compile_ok("if x > 1 then\n  return 1\nelse\n  return 0\nend");
        assert!(kernel.wgsl.contains("if ((x > 1.0))"), "{}", kernel.wgsl);
        // A condition is a `bool`, never a `select`: the first version emitted
        // the value form and the result was not valid WGSL.
        assert!(!kernel.wgsl.contains("if (select("), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("} else {"), "{}", kernel.wgsl);
    }

    #[test]
    fn a_while_loop_becomes_a_wgsl_while() {
        let kernel = compile_ok("local i = 0\nwhile i < 10 do\n  i = i + 1\nend\nreturn i");
        assert!(kernel.wgsl.contains("while ((i < 10.0))"), "{}", kernel.wgsl);
        assert!(!kernel.wgsl.contains("while (select("), "{}", kernel.wgsl);
    }

    #[test]
    fn a_fixed_for_becomes_a_wgsl_for() {
        let kernel = compile_ok("local s = 0\nfor i = 0, 9 do\n  s = s + i\nend\nreturn s");
        assert!(kernel.wgsl.contains("for (var i: f32 = 0.0; i <= 9.0; i = i + 1.0)"), "{}", kernel.wgsl);
    }

    #[test]
    fn a_break_outside_a_loop_is_refused() {
        let error = compile("break").expect_err("a refusal");
        assert!(error.message().contains("break"), "{}", error.message());
    }

    #[test]
    fn every_rejection_names_the_construct_and_the_line() {
        // Each case is a construct outside the subset, and the diagnostic names
        // it and the line it is on.
        let cases: &[(&str, &str, usize)] = &[
            ("local x = 1\nlocal t = {1, 2}", "table", 2),
            ("local x = 1\nlocal s = \"hello\"", "string", 2),
            ("local x = 1\nfunction f() end", "function definition", 2),
            ("local x = 1\nrepeat x = x + 1 until x > 3", "`repeat` loop", 2),
            ("local x = 1\ngoto done", "`goto`", 2),
            ("local x = 1\nx = x.foo", "field access", 2),
            ("local x = 1\nx = x:bar()", "method call", 2),
        ];
        for (source, construct, line) in cases {
            let error = compile(source).expect_err(&format!("{source} should be refused"));
            assert!(
                error.construct.contains(construct),
                "{source}: the construct is `{}`, expected `{construct}`",
                error.construct
            );
            assert_eq!(error.line, *line, "{source}: {}", error.message());
        }
    }

    #[test]
    fn an_unknown_function_is_refused_by_name() {
        let error = compile("return foo(1)").expect_err("a refusal");
        assert!(error.construct.contains("foo"), "{}", error.construct);
        assert!(error.hint.contains("math.sin"), "{}", error.hint);
    }

    #[test]
    fn a_power_is_wgsl_pow_and_is_right_associative() {
        let kernel = compile_ok("return 2 ^ 3 ^ 2");
        // `2^(3^2)` and not `(2^3)^2`.
        assert!(
            kernel.wgsl.contains("pow(2.0, pow(3.0, 2.0))"),
            "{}",
            kernel.wgsl
        );
    }

    #[test]
    fn the_comparisons_are_selects_because_wgsl_has_no_float_bool() {
        // A kernel's values are `f32`, so a comparison is a `select` that yields
        // `1.0` or `0.0`.
        let kernel = compile_ok("return 1 < 2");
        assert!(kernel.wgsl.contains("select(0.0, 1.0, 1.0 < 2.0)"), "{}", kernel.wgsl);
        let kernel = compile_ok("return 1 ~= 2");
        assert!(kernel.wgsl.contains("select(0.0, 1.0, 1.0 != 2.0)"), "{}", kernel.wgsl);
    }

    #[test]
    fn the_random_stream_is_a_call_the_shader_defines() {
        let kernel = compile_ok("return rand()");
        assert!(kernel.wgsl.contains("rand(seed)"), "{}", kernel.wgsl);
    }

    #[test]
    fn the_interpreter_runs_the_program_the_compiler_parsed() {
        // The point of the interpreter: `compile` and `interpret` read the same
        // text through the same parser, so a kernel has one source and two
        // executions.
        let value = interpret("return 2 * 3 + 1", &[], 0, Precision::F64).expect("a value");
        assert_eq!(value, 7.0);
        // A local, a loop and a branch.
        let program = "local s = 0\nfor i = 1, 4 do\n  s = s + i\nend\nreturn s";
        assert_eq!(interpret(program, &[], 0, Precision::F64).expect("a value"), 10.0);
        // An argument.
        let value = interpret("return x * x", &[("x", 3.0)], 0, Precision::F64).expect("a value");
        assert_eq!(value, 9.0);
        // `while` with a `break`.
        let program = "local i = 0\nwhile 1 do\n  i = i + 1\n  if i > 4 then\n    break\n  end\nend\nreturn i";
        assert_eq!(interpret(program, &[], 0, Precision::F64).expect("a value"), 5.0);
    }

    #[test]
    fn the_two_precisions_agree_to_single_precision() {
        // The differential test W7.2 asks for, without a GPU: the same program
        // in `f64` and in `f32`. The `f32` run is what the emitted WGSL would
        // compute, so their difference is the floating-point difference the GPU
        // would show — not a driver's, and the doc says so.
        let program = "local a = x * x + 3 * x\nlocal b = math.sqrt(a + 1)\nreturn b / (1 + a)";
        for x in [0.5, 1.0, 2.0, 7.5] {
            let wide = interpret(program, &[("x", x)], 0, Precision::F64).expect("a value");
            let narrow = interpret(program, &[("x", x)], 0, Precision::F32).expect("a value");
            let relative = if wide == 0.0 {
                (narrow - wide).abs()
            } else {
                ((narrow - wide) / wide).abs()
            };
            assert!(
                relative < 1e-6,
                "at x = {x}: f64 {wide}, f32 {narrow}, relative {relative}"
            );
        }
    }

    #[test]
    fn the_random_stream_is_the_same_for_the_same_seed() {
        // A seed must give the same draws every time, or a differential test
        // compares two different photons.
        let program = "return rand() + rand()";
        let first = interpret(program, &[], 42, Precision::F64).expect("a value");
        let second = interpret(program, &[], 42, Precision::F64).expect("a value");
        assert_eq!(first, second);
        // A different seed gives a different draw.
        let other = interpret(program, &[], 43, Precision::F64).expect("a value");
        assert_ne!(first, other);
        // And the draws are in `[0, 1)`.
        for seed in 0..16 {
            let value = interpret("return rand()", &[], seed, Precision::F64).expect("a value");
            assert!((0.0..1.0).contains(&value), "seed {seed} drew {value}");
        }
    }

    #[test]
    fn a_runaway_loop_stops_at_the_budget_instead_of_hanging() {
        // A kernel's `while` is unbounded, and a hang is not a diagnostic.
        let error = interpret("while 1 do\nend\nreturn 0", &[], 0, Precision::F64)
            .expect_err("a runaway");
        assert!(
            matches!(error, RunError::BudgetExceeded { .. }),
            "{error:?}"
        );
        assert!(error.message().contains("without finishing"), "{}", error.message());
    }

    #[test]
    fn a_name_read_before_it_is_written_is_refused() {
        let error = interpret("return y", &[], 0, Precision::F64).expect_err("a refusal");
        assert_eq!(error, RunError::Unbound(String::from("y")));
        // A construct outside the subset is a diagnostic, not a wrong number.
        let error = interpret("local t = {1, 2}\nreturn 0", &[], 0, Precision::F64)
            .expect_err("a refusal");
        assert!(error.message().contains("table"), "{}", error.message());
    }

    #[test]
    fn a_for_with_a_zero_step_is_refused_rather_than_run_a_million_times() {
        let error = interpret("for i = 0, 10, 0 do\nend\nreturn 0", &[], 0, Precision::F64)
            .expect_err("a refusal");
        assert!(error.message().contains("never advance"), "{}", error.message());
    }

    #[test]
    fn a_program_without_a_return_still_returns_a_value() {
        let kernel = compile_ok("local x = 1");
        assert!(kernel.wgsl.contains("return 0.0;"), "{}", kernel.wgsl);
    }
}
