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
    fn a_program_without_a_return_still_returns_a_value() {
        let kernel = compile_ok("local x = 1");
        assert!(kernel.wgsl.contains("return 0.0;"), "{}", kernel.wgsl);
    }
}
