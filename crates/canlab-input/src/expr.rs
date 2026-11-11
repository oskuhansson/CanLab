//! Converting the field to `canlab-core`.
//!
//! A math field is allowed to be *half typed*: `\frac{1}{ }` is a legitimate state
//! of the editor and has no algebraic meaning. This module is where that line is
//! drawn — every empty slot, dangling superscript and construct without an
//! expression equivalent is reported instead of guessed at.

use canlab_core::expr::ExprNode;
use canlab_core::{add, mul, pow, Expr, Number, SymbolId};

use crate::tree::{Node, Row};

/// Why a field cannot be read as an expression.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ToExprError {
    /// The field is empty, or an argument slot is empty: `\frac{1}{ }`.
    EmptySlot,
    /// A superscript with nothing to superscript (the `^{x}` the palette can
    /// create on an empty field).
    DanglingSuperscript,
    /// A subscript has no expression equivalent here (`x_1`).
    Subscript,
    /// A construct this converter does not translate: `\binom`, `\overrightarrow`,
    /// an indexed root.
    UnsupportedConstruct(&'static str),
    /// A command token with no expression equivalent: a relation (`\le`), a
    /// rendering command (`\text{…}`), an operator used as an operand.
    UnsupportedCommand(String),
    /// Something that is not a number, a variable or an operator.
    UnexpectedText(String),
    /// The field holds several lines, i.e. a system, which is not one expression.
    System,
    /// A line that defines a function (`f(x) = x² + 1`) is not a value.
    Definition,
}

impl std::fmt::Display for ToExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToExprError::EmptySlot => write!(f, "the field has an empty slot"),
            ToExprError::DanglingSuperscript => write!(f, "superscript with no base"),
            ToExprError::Subscript => write!(f, "subscripts are not expressions"),
            ToExprError::UnsupportedConstruct(c) => write!(f, "{c} has no expression form"),
            ToExprError::UnsupportedCommand(c) => write!(f, "{c} has no expression form"),
            ToExprError::UnexpectedText(s) => write!(f, "cannot read {s:?} as an operand"),
            ToExprError::System => write!(f, "a system is not a single expression"),
            ToExprError::Definition => write!(f, "a definition is not a value"),
        }
    }
}

impl std::error::Error for ToExprError {}

/// Translate a row, or explain why not.
pub fn build(row: &Row) -> Result<Expr, ToExprError> {
    build_with(row, &crate::define::Definitions::default())
}

/// [`build`], with the document's function definitions available.
pub fn build_with(row: &Row, definitions: &crate::define::Definitions) -> Result<Expr, ToExprError> {
    // Spaces are layout, not algebra.
    let nodes: Vec<&Node> = row
        .iter()
        .filter(|n| !is_space_node(n) && !is_unit_marker(n))
        .collect();
    if nodes.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    let mut terms: Vec<Expr> = Vec::new();
    let mut current: Row = Vec::new();
    let mut negative = false;
    for node in nodes {
        match node {
            Node::Text(s) if s == "+" || s == "-" => {
                let sign_negative = s == "-";
                // A sign directly after a multiplication multiplies by a negative
                // number: `1·-6` is `1·(-6)`, not `1 - 6`. Toggling the term's sign
                // is the same thing, because a term is a product — and it keeps the
                // precedence right, since a term ends at `+` or `-`, not at `·`.
                //
                // Without this the sign was read as a *binary* one, so
                // `1·-6 + 2·-3` came out as `1 - 6 + 2 - 3`, i.e. `-6` instead of
                // `-12`. Nothing in the corpus put a sign after a multiplication,
                // which is why nothing caught it.
                let after_product = matches!(
                    current.last(),
                    Some(Node::Sym(name)) if name == "\\cdot"
                );
                if current.is_empty() || after_product {
                    // A sign with nothing before it is a *unary* sign for the
                    // term that follows: `-x` inside a superscript, or the `-1`
                    // in `x+-1`. Toggling is what makes `x--y` read as `x + y`.
                    negative ^= sign_negative;
                } else {
                    terms.push(signed(&current, negative, definitions)?);
                    current.clear();
                    negative = sign_negative;
                }
            }
            _ => current.push(node.clone()),
        }
    }
    terms.push(signed(&current, negative, definitions)?);
    Ok(sum(terms))
}

fn is_space_node(node: &Node) -> bool {
    matches!(node, Node::Text(s) if s.trim().is_empty())
}

/// Unit markers: `80°`, `5 %`, `3 ‰`.
///
/// They are part of the *notation* — they stay in the document's LaTeX and in
/// what gets printed — but they are not algebra, so the value reader steps over
/// them. No scaling is applied: `80°` reads as the number 80, and converting it
/// to radians is the caller's business, not the editor's.
pub fn is_unit_marker(node: &Node) -> bool {
    matches!(node, Node::Text(marker) if matches!(marker.as_str(), "°" | "%" | "‰"))
}

fn product(row: &Row, definitions: &crate::define::Definitions) -> Result<Expr, ToExprError> {
    let nodes: Vec<&Node> = row.iter().filter(|n| !is_space_node(n)).collect();
    if nodes.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    let mut factors: Vec<Expr> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        // A *defined* function: `f(3)`, `f'(x)`, `f''(x)`.
        //
        // The primes are their own text runs (an apostrophe is not a word
        // character), so they are counted before the argument list is read, and
        // the derivative is taken on the body before substituting: `f'` is a
        // function in its own right.
        if let Node::Text(text) = nodes[i]
            && let Some(function) = definitions.get(text)
        {
            let mut order = 0u32;
            let mut j = i + 1;
            while let Some(Node::Text(mark)) = nodes.get(j) {
                let primes = mark
                    .chars()
                    .filter(|character| *character == '\'' || *character == '\u{2032}')
                    .count();
                if primes == 0 {
                    break;
                }
                order += primes as u32;
                j += 1;
            }
            if let Some(Node::Group { body, .. }) = nodes.get(j) {
                let arguments = split_arguments(body, definitions)?;
                if arguments.len() != function.params.len() {
                    return Err(ToExprError::UnsupportedCommand(format!(
                        "{text} takes {} argument(s)",
                        function.params.len()
                    )));
                }
                let mut body = function.body.clone();
                if order > 0 {
                    let Some(variable) = function.params.first().copied() else {
                        return Err(ToExprError::UnsupportedCommand(text.clone()));
                    };
                    body = canlab_core::differentiate_n(&body, variable, order).ok_or_else(|| {
                        ToExprError::UnsupportedCommand(format!(
                            "the derivative of {text} (order {order})"
                        ))
                    })?;
                }
                factors.push(crate::define::substitute(
                    &body,
                    &function.params,
                    &arguments,
                ));
                i = j + 1;
                if let Some(Node::Sym(s)) = nodes.get(i)
                    && s == "\\cdot"
                {
                    i += 1;
                }
                continue;
            }
        }

        // A function token consumes the next atom as its argument. MathQuill does
        // not group them (`\sin x` is two siblings), but `Fn(sin, [x])` is the
        // only useful reading for a CAS. **[INFERRED]**
        if let Node::Sym(name) = nodes[i] {
            if let Some(func) = function_of(name) {
                // `\log_2(x)` — the subscript on a *logarithm* is its base, not
                // a decoration on a variable. Read as a ratio of natural logs,
                // which is what it means and what the evaluator can do.
                //
                // A base means nothing on any other function: reading
                // `\sin_2 x` as `ln x / ln 2` supplied a value the user did not
                // type, so that is refused with the same reason as any other
                // subscript. `canlab-parse` makes the same distinction.
                //
                // A script keeps absorbing what follows it, so `\log_2(10)` can
                // arrive with the argument *inside* the subscript. When it does,
                // the first item is the base and the rest is the argument.
                let mut next = i + 1;
                let mut base: Option<Expr> = None;
                let mut argument_in_subscript: Option<Expr> = None;
                if let Some(Node::Sub(base_row)) = nodes.get(next) {
                    if func != "log" {
                        return Err(ToExprError::Subscript);
                    }
                    if base_row.len() > 1 {
                        base = Some(build_with(&base_row[..1].to_vec(), definitions)?);
                        argument_in_subscript = Some(build_with(&base_row[1..].to_vec(), definitions)?);
                    } else {
                        base = Some(build_with(&base_row.to_vec(), definitions)?);
                    }
                    next += 1;
                }
                let from_subscript = argument_in_subscript.is_some();
                let mut arg = match argument_in_subscript {
                    Some(argument) => argument,
                    None => {
                        if next >= nodes.len() {
                            return Err(ToExprError::UnsupportedCommand(name.clone()));
                        }
                        atom(nodes[next], definitions)?
                    }
                };
                let mut j = if from_subscript { next } else { next + 1 };
                while j < nodes.len() {
                    match nodes[j] {
                        Node::Sup(sub) => {
                            arg = pow(arg, build_with(sub, definitions)?);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                let value = match base {
                    Some(base) => {
                        let upper = ExprNode::fn_raw(SymbolId::intern("ln"), vec![arg]);
                        let lower = ExprNode::fn_raw(SymbolId::intern("ln"), vec![base]);
                        mul([upper, pow(lower, ExprNode::int(-1))])
                    }
                    None => ExprNode::fn_raw(SymbolId::intern(func), vec![arg]),
                };
                factors.push(value);
                i = j;
                if let Some(Node::Sym(s)) = nodes.get(i) {
                    if s == "\\cdot" {
                        i += 1;
                    }
                }
                continue;
            }
        }
        let mut e = atom(nodes[i], definitions)?;
        i += 1;
        while i < nodes.len() {
            match nodes[i] {
                Node::Sup(sub) => {
                    e = pow(e, build_with(sub, definitions)?);
                    i += 1;
                }
                Node::Sub(_) => return Err(ToExprError::Subscript),
                _ => break,
            }
        }
        factors.push(e);
        if let Some(Node::Sym(s)) = nodes.get(i) {
            if s == "\\cdot" {
                i += 1;
            }
        }
    }
    match factors.len() {
        0 => Err(ToExprError::EmptySlot),
        1 => Ok(factors.pop().expect("length checked")),
        _ => Ok(mul(factors)),
    }
}

fn signed(
    row: &Row,
    negative: bool,
    definitions: &crate::define::Definitions,
) -> Result<Expr, ToExprError> {
    let e = product(row, definitions)?;
    if negative {
        Ok(mul([ExprNode::int(-1), e]))
    } else {
        Ok(e)
    }
}

/// Split a call's argument list on its commas.
fn split_arguments(
    row: &Row,
    definitions: &crate::define::Definitions,
) -> Result<Vec<Expr>, ToExprError> {
    let mut arguments: Vec<Expr> = Vec::new();
    let mut current: Row = Vec::new();
    for node in row {
        match node {
            Node::Text(text) if text == "," => {
                arguments.push(build_with(&current, definitions)?);
                current.clear();
            }
            _ => current.push(node.clone()),
        }
    }
    if !current.is_empty() || !arguments.is_empty() {
        arguments.push(build_with(&current, definitions)?);
    }
    Ok(arguments)
}
fn sum(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => ExprNode::int(0),
        1 => terms.pop().expect("length checked"),
        _ => add(terms),
    }
}

fn atom(node: &Node, definitions: &crate::define::Definitions) -> Result<Expr, ToExprError> {
    match node {
        Node::Text(s) => text_expr(s),
        Node::Sym(name) => sym_expr(name),
        Node::Frac { num, den } => {
            let n = build_with(num, definitions)?;
            let d = build_with(den, definitions)?;
            // Division by zero is not a value. Without this it becomes a NaN
            // constant, which then poisons everything downstream silently.
            if matches!(&*d, ExprNode::Num(number) if number.is_zero()) {
                return Err(ToExprError::UnexpectedText(String::from("1/0")));
            }
            Ok(match (int_of(&n), int_of(&d)) {
                (Some(a), Some(b)) if b != 0 => ExprNode::num(Number::ratio(a, b)),
                _ => mul([n, pow(d, ExprNode::int(-1))]),
            })
        }
        Node::Sqrt { index: None, body } => Ok(pow(
            build_with(body, definitions)?,
            ExprNode::num(Number::ratio(1, 2)),
        )),
        // `\sqrt[n]{x}` is `x^(1/n)` — a cube root is school mathematics, and
        // refusing it was a limitation, not a policy.
        Node::Sqrt {
            index: Some(index),
            body,
        } => Ok(pow(
            build_with(body, definitions)?,
            pow(build_with(index, definitions)?, ExprNode::int(-1)),
        )),
        Node::Group { body, .. } => build_with(body, definitions),
        Node::Accent { .. } => Err(ToExprError::UnsupportedConstruct("an accent")),
        Node::Binom { .. } => Err(ToExprError::UnsupportedConstruct("\\binom")),
        Node::Sup(_) => Err(ToExprError::DanglingSuperscript),
        Node::Sub(_) => Err(ToExprError::Subscript),
    }
}

fn int_of(e: &Expr) -> Option<i128> {
    match &**e {
        ExprNode::Num(Number::Int(v)) => Some(*v),
        _ => None,
    }
}

/// Read one run of plain characters as a product.
///
/// The field glues adjacent word characters into a single run — that is what
/// makes `1/2x` put the whole `2x` in the denominator — so `2x` arrives here as
/// one string even though it means two factors. The run is tokenised back into
/// a number and a chain of symbols, which is what a student means by `2x`, `3xy`
/// or `2.5x`.
fn text_expr(s: &str) -> Result<Expr, ToExprError> {
    if s.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    let mut factors: Vec<Expr> = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' {
            let mut number = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || d == '.' {
                    number.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            let value = if number.contains('.') {
                number.parse::<f64>().map(Number::float).ok()
            } else {
                number.parse::<i128>().map(Number::int).ok()
            };
            let Some(value) = value else {
                return Err(ToExprError::UnexpectedText(s.to_string()));
            };
            factors.push(ExprNode::num(value));
        } else if c.is_alphabetic() {
            chars.next();
            factors.push(ExprNode::sym(SymbolId::intern(&c.to_string())));
        } else {
            return Err(ToExprError::UnexpectedText(s.to_string()));
        }
    }
    match factors.len() {
        0 => Err(ToExprError::EmptySlot),
        1 => Ok(factors.pop().expect("length checked")),
        _ => Ok(mul(factors)),
    }
}

fn sym_expr(name: &str) -> Result<Expr, ToExprError> {
    const GREEK: &[&str] = &[
        "alpha", "beta", "gamma", "delta", "epsilon", "varepsilon", "zeta", "eta", "theta",
        "vartheta", "iota", "kappa", "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau",
        "upsilon", "phi", "varphi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda",
        "Xi", "Pi", "Sigma", "Upsilon", "Phi", "Psi", "Omega",
    ];
    let bare = name.strip_prefix('\\').unwrap_or(name);
    if GREEK.contains(&bare) {
        return Ok(ExprNode::sym(SymbolId::intern(bare)));
    }
    if function_of(name).is_some() {
        // A function token with no argument: `\sin` alone is not an expression.
        return Err(ToExprError::UnsupportedCommand(name.to_string()));
    }
    Err(ToExprError::UnsupportedCommand(name.to_string()))
}

fn function_of(name: &str) -> Option<&'static str> {
    const FUNCTIONS: &[&str] = &[
        "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
        "tanh", "ln", "log", "exp",
    ];
    let bare = name.strip_prefix('\\')?;
    FUNCTIONS.iter().find(|f| **f == bare).copied()
}
