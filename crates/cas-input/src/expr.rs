//! Converting the field to `cas-core`.
//!
//! A math field is allowed to be *half typed*: `\frac{1}{ }` is a legitimate state
//! of the editor and has no algebraic meaning. This module is where that line is
//! drawn â€” every empty slot, dangling superscript and construct without an
//! expression equivalent is reported instead of guessed at.

use cas_core::expr::ExprNode;
use cas_core::{add, mul, pow, Expr, Number, SymbolId};

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
    /// rendering command (`\text{â€¦}`), an operator used as an operand.
    UnsupportedCommand(String),
    /// Something that is not a number, a variable or an operator.
    UnexpectedText(String),
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
        }
    }
}

impl std::error::Error for ToExprError {}

/// Translate a row, or explain why not.
pub fn build(row: &Row) -> Result<Expr, ToExprError> {
    // Spaces are layout, not algebra.
    let nodes: Vec<&Node> = row.iter().filter(|n| !is_space_node(n)).collect();
    if nodes.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    let mut terms: Vec<Expr> = Vec::new();
    let mut current: Row = Vec::new();
    let mut negative = false;
    for node in nodes {
        match node {
            Node::Text(s) if s == "+" => {
                terms.push(signed(&current, negative)?);
                current.clear();
                negative = false;
            }
            Node::Text(s) if s == "-" => {
                terms.push(signed(&current, negative)?);
                current.clear();
                negative = true;
            }
            _ => current.push(node.clone()),
        }
    }
    terms.push(signed(&current, negative)?);
    Ok(sum(terms))
}

fn is_space_node(node: &Node) -> bool {
    matches!(node, Node::Text(s) if s.trim().is_empty())
}

fn product(row: &Row) -> Result<Expr, ToExprError> {
    let nodes: Vec<&Node> = row.iter().filter(|n| !is_space_node(n)).collect();
    if nodes.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    let mut factors: Vec<Expr> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        // A function token consumes the next atom as its argument. MathQuill does
        // not group them (`\sin x` is two siblings), but `Fn(sin, [x])` is the
        // only useful reading for a CAS. **[INFERRED]**
        if let Node::Sym(name) = nodes[i] {
            if let Some(func) = function_of(name) {
                if i + 1 >= nodes.len() {
                    return Err(ToExprError::UnsupportedCommand(name.clone()));
                }
                let mut arg = atom(nodes[i + 1])?;
                let mut j = i + 2;
                while j < nodes.len() {
                    match nodes[j] {
                        Node::Sup(sub) => {
                            arg = pow(arg, build(sub)?);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                factors.push(ExprNode::fn_raw(SymbolId::intern(func), vec![arg]));
                i = j;
                if let Some(Node::Sym(s)) = nodes.get(i) {
                    if s == "\\cdot" {
                        i += 1;
                    }
                }
                continue;
            }
        }
        let mut e = atom(nodes[i])?;
        i += 1;
        while i < nodes.len() {
            match nodes[i] {
                Node::Sup(sub) => {
                    e = pow(e, build(sub)?);
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

fn signed(row: &Row, negative: bool) -> Result<Expr, ToExprError> {
    let e = product(row)?;
    if negative {
        Ok(mul([ExprNode::int(-1), e]))
    } else {
        Ok(e)
    }
}

fn sum(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => ExprNode::int(0),
        1 => terms.pop().expect("length checked"),
        _ => add(terms),
    }
}

fn atom(node: &Node) -> Result<Expr, ToExprError> {
    match node {
        Node::Text(s) => text_expr(s),
        Node::Sym(name) => sym_expr(name),
        Node::Frac { num, den } => {
            let n = build(num)?;
            let d = build(den)?;
            Ok(match (int_of(&n), int_of(&d)) {
                (Some(a), Some(b)) if b != 0 => ExprNode::num(Number::ratio(a, b)),
                _ => mul([n, pow(d, ExprNode::int(-1))]),
            })
        }
        Node::Sqrt { index: None, body } => Ok(pow(build(body)?, ExprNode::num(Number::ratio(1, 2)))),
        Node::Sqrt { index: Some(_), .. } => Err(ToExprError::UnsupportedConstruct("an indexed root")),
        Node::Group { body, .. } => build(body),
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

fn text_expr(s: &str) -> Result<Expr, ToExprError> {
    if s.is_empty() {
        return Err(ToExprError::EmptySlot);
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s
            .parse::<i128>()
            .map(|v| ExprNode::num(Number::int(v)))
            .map_err(|_| ToExprError::UnexpectedText(s.to_string()));
    }
    if s.contains('.') && s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return s
            .parse::<f64>()
            .map(|v| ExprNode::num(Number::float(v)))
            .map_err(|_| ToExprError::UnexpectedText(s.to_string()));
    }
    if s.chars().all(|c| c.is_alphabetic()) {
        let mut syms: Vec<Expr> = s
            .chars()
            .map(|c| ExprNode::sym(SymbolId::intern(&c.to_string())))
            .collect();
        return Ok(match syms.len() {
            1 => syms.pop().expect("length checked"),
            _ => mul(syms),
        });
    }
    Err(ToExprError::UnexpectedText(s.to_string()))
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
