//! Function definitions: `f(x) = x² + 1`, and then `f(3)` or `f'(x)`.
//!
//! A definition is not algebra — it is a *name* for an expression — so it lives
//! beside the reader rather than inside the expression tree. The reader is handed
//! the table and resolves calls as it goes:
//!
//! * `f(3)` substitutes the argument into the body;
//! * `f'(x)` differentiates the body first, then substitutes — so `f'` is a
//!   function in its own right, and `f''` is that done twice;
//! * a name with the wrong number of arguments is an error, not a product.
//!
//! Recursion is refused when the definition is made: a name may not mention
//! itself. This is a calculator, not a programming language.

use std::collections::BTreeMap;

use canlab_core::{Expr, ExprNode, SymbolId};

use crate::tree::{Node, Row};

/// One defined function.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// The parameter names, in order.
    pub params: Vec<SymbolId>,
    /// The body, in terms of those parameters.
    pub body: Expr,
}

/// The functions a document has defined.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definitions {
    functions: BTreeMap<String, Function>,
}

/// Why a definition was refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DefineError {
    /// The body mentions the name being defined.
    Recursive(String),
    /// The same name with a different body.
    Duplicate(String),
    /// The right-hand side is not an expression this reader can build.
    Unreadable(String),
}

impl std::fmt::Display for DefineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefineError::Recursive(name) => write!(f, "{name} cannot be defined in terms of itself"),
            DefineError::Duplicate(name) => write!(f, "{name} is already defined differently"),
            DefineError::Unreadable(why) => write!(f, "the body cannot be read: {why}"),
        }
    }
}

impl Definitions {
    /// Define `name(params) = body`, or explain why not.
    pub fn define(
        &mut self,
        name: &str,
        params: Vec<SymbolId>,
        body: Expr,
    ) -> Result<(), DefineError> {
        if params.iter().any(|parameter| parameter.as_str() == name) {
            return Err(DefineError::Recursive(name.to_string()));
        }
        if mentions(&body, name) {
            return Err(DefineError::Recursive(name.to_string()));
        }
        if let Some(existing) = self.functions.get(name) {
            let same = existing.params == params && existing.body == body;
            return if same {
                Ok(())
            } else {
                Err(DefineError::Duplicate(name.to_string()))
            };
        }
        self.functions
            .insert(name.to_string(), Function { params, body });
        Ok(())
    }

    /// Look a name up.
    pub fn get(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    /// How many functions are defined.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// True when nothing is defined.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// The names, for a UI that wants to list them.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }

    /// The definitions themselves, in name order, for a UI that wants to show them.
    pub fn functions(&self) -> impl Iterator<Item = (&str, &Function)> {
        self.functions
            .iter()
            .map(|(name, function)| (name.as_str(), function))
    }

    /// Forget a definition, returning it. Used when one turns out to refer to
    /// itself and therefore defines nothing.
    pub fn remove(&mut self, name: &str) -> Option<Function> {
        self.functions.remove(name)
    }
}

/// A line that *looks* like a definition: `f(x) = …`.
///
/// Recognition is purely syntactic, and deliberately narrow: the left side must be
/// one name applied to a bracketed list of one-letter parameters. Anything else is
/// an equation, so `x² + y² = 1` and `a(x+1) = 0` keep being equations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The function's name.
    pub name: String,
    /// Its parameter names, in order.
    pub params: Vec<SymbolId>,
    /// The right-hand side, as it was typed.
    pub body: Row,
}

impl Candidate {
    /// Read `row` as a definition, if it is one.
    pub fn of(row: &Row) -> Option<Candidate> {
        let equal = row
            .iter()
            .position(|node| matches!(node, Node::Text(text) if text == "="))?;
        let left: Vec<&Node> = row[..equal]
            .iter()
            .filter(|node| !matches!(node, Node::Text(text) if text.trim().is_empty()))
            .collect();
        let [Node::Text(name), Node::Group { body, .. }] = left[..] else {
            return None;
        };
        if name.is_empty() || !name.chars().all(char::is_alphabetic) {
            return None;
        }
        let params = parameters(body)?;
        Some(Candidate {
            name: name.clone(),
            params,
            body: row[equal + 1..].to_vec(),
        })
    }
}

/// The parameters of a definition: one-letter names separated by commas.
///
/// `f(x)` has one and `g(a,b)` has two. A run of two letters is *not* two
/// parameters — `f(xy)` is a product of `x` and `y`, i.e. an equation — so it
/// refuses rather than guessing.
fn parameters(body: &Row) -> Option<Vec<SymbolId>> {
    let mut names: Vec<String> = Vec::new();
    let mut current = String::new();
    for node in body {
        match node {
            Node::Text(text) if text.trim().is_empty() => {}
            Node::Text(text) if text == "," => {
                if current.is_empty() {
                    return None;
                }
                names.push(std::mem::take(&mut current));
            }
            Node::Text(text) => {
                if !current.is_empty() || !text.chars().all(char::is_alphabetic) {
                    return None;
                }
                current.push_str(text);
            }
            _ => return None,
        }
    }
    if current.is_empty() {
        // `f()` names a constant, and a trailing comma is still being typed.
        return None;
    }
    names.push(current);

    let mut params = Vec::new();
    for name in names {
        let mut characters = name.chars();
        let first = characters.next()?;
        if characters.next().is_some() {
            return None;
        }
        params.push(SymbolId::intern(&first.to_string()));
    }
    Some(params)
}

/// Read the whole document's definitions.
///
/// Definitions are **document-wide and order-independent**: a line may use a
/// function defined below it. Order-independence is not achieved by retrying
/// until something sticks — that publishes a *wrong* body first and cannot take
/// it back. Instead the lines are ordered by their dependencies, so a body is
/// only ever built once every name it applies is known.
///
/// Two properties are load-bearing here:
///
/// * one broken definition must not take the others with it, so a body that
///   cannot be read is reported and skipped rather than blanking the table;
/// * a cycle is refused. `f(x) = f(x) + 1` and `f(x) = g(x), g(x) = f(x)` are not
///   definitions of anything, and inventing an answer for them would be worse
///   than saying so.
pub fn harvest(candidates: &[Candidate]) -> (Definitions, Vec<(usize, DefineError)>) {
    let mut definitions = Definitions::default();
    let mut problems: Vec<(usize, DefineError)> = Vec::new();

    let names: Vec<&str> = candidates.iter().map(|candidate| candidate.name.as_str()).collect();
    let dependencies: Vec<Vec<usize>> = candidates
        .iter()
        .map(|candidate| calls(&candidate.body, &names))
        .collect();

    // Depth-first topological order. A candidate reached while still in progress
    // is part of a cycle; everything that depends on it is unobtainable too.
    let mut order: Vec<usize> = Vec::new();
    let mut state = vec![Visit::Unseen; candidates.len()];
    for index in 0..candidates.len() {
        visit(index, &dependencies, &mut state, &mut order);
    }

    // A definition that could not be made leaves its dependents unbuildable. They
    // must be *skipped*, not built against a table that lacks the name: a missing
    // name reads as a product, so the dependent would be quietly wrong rather
    // than absent.
    let mut failed = vec![false; candidates.len()];
    for index in order {
        let candidate = &candidates[index];
        let broken = dependencies[index].iter().any(|dependency| failed[*dependency]);
        if broken {
            problems.push((
                index,
                DefineError::Unreadable(String::from(
                    "it uses a definition that cannot be made",
                )),
            ));
            failed[index] = true;
            continue;
        }
        let body = match crate::expr::build_with(&candidate.body, &definitions) {
            Ok(body) => body,
            Err(error) => {
                problems.push((index, DefineError::Unreadable(error.to_string())));
                failed[index] = true;
                continue;
            }
        };
        if let Err(error) = definitions.define(&candidate.name, candidate.params.clone(), body) {
            problems.push((index, error));
            failed[index] = true;
        }
    }

    // Whatever the ordering could not place: a cycle, or something downstream of
    // one. Only the names actually *in* the cycle are described as recursive.
    for (index, candidate) in candidates.iter().enumerate() {
        if state[index] == Visit::Ordered {
            continue;
        }
        let error = if reaches_itself(index, &dependencies) {
            DefineError::Recursive(candidate.name.clone())
        } else {
            DefineError::Unreadable(String::from(
                "it uses a definition that cannot be made",
            ))
        };
        problems.push((index, error));
    }

    // Belt and braces: a body that mentions its own name defines nothing, however
    // it got there.
    let self_referential: Vec<(usize, DefineError)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let function = definitions.get(&candidate.name)?;
            mentions(&function.body, &candidate.name)
                .then(|| (index, DefineError::Recursive(candidate.name.clone())))
        })
        .collect();
    for (index, error) in self_referential {
        definitions.remove(&candidates[index].name);
        problems.push((index, error));
    }

    problems.sort_by_key(|(index, _)| *index);
    problems.dedup_by_key(|(index, _)| *index);
    (definitions, problems)
}

/// Where a candidate is in the dependency walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Visit {
    /// Not reached yet.
    Unseen,
    /// Being visited: reaching it again is a cycle.
    InProgress,
    /// Placed at the end of the order.
    Ordered,
    /// Part of a cycle, or downstream of one.
    Cyclic,
}

/// Post-order depth-first visit: dependencies are pushed before their use.
fn visit(
    index: usize,
    dependencies: &[Vec<usize>],
    state: &mut [Visit],
    order: &mut Vec<usize>,
) -> bool {
    match state[index] {
        Visit::Ordered => return true,
        Visit::InProgress | Visit::Cyclic => {
            state[index] = Visit::Cyclic;
            return false;
        }
        Visit::Unseen => {}
    }
    state[index] = Visit::InProgress;
    let mut ok = true;
    for &dependency in &dependencies[index] {
        if dependency == index || !visit(dependency, dependencies, state, order) {
            ok = false;
        }
    }
    state[index] = if ok {
        Visit::Ordered
    } else {
        Visit::Cyclic
    };
    if ok {
        order.push(index);
    }
    ok
}

/// The candidates this body applies as a function: `f(…)` where `f` is defined
/// somewhere in the document.
///
/// Spaces between the name and the bracket are layout, not algebra: `f (x)` is a
/// call, so they are stepped over rather than breaking the pattern.
fn calls(body: &Row, names: &[&str]) -> Vec<usize> {
    let significant: Vec<&Node> = body
        .iter()
        .filter(|node| !matches!(node, Node::Text(text) if text.trim().is_empty()))
        .collect();
    let mut out: Vec<usize> = Vec::new();
    for window in significant.windows(2) {
        let [Node::Text(text), Node::Group { .. }] = &window[0..2] else {
            continue;
        };
        if let Some(index) = names.iter().position(|name| *name == text) {
            if !out.contains(&index) {
                out.push(index);
            }
        }
    }
    out
}

/// Is `index` part of a dependency cycle, rather than merely downstream of one?
fn reaches_itself(index: usize, dependencies: &[Vec<usize>]) -> bool {
    let mut stack = dependencies[index].clone();
    let mut seen = vec![false; dependencies.len()];
    while let Some(node) = stack.pop() {
        if node == index {
            return true;
        }
        if seen.get(node).copied().unwrap_or(true) {
            continue;
        }
        seen[node] = true;
        stack.extend(dependencies[node].iter().copied());
    }
    false
}

/// Does `expression` mention the symbol called `name`?
fn mentions(expression: &Expr, name: &str) -> bool {
    match &**expression {
        ExprNode::Num(_) => false,
        ExprNode::Sym(symbol) => symbol.as_str() == name,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            terms.iter().any(|term| mentions(term, name))
        }
        ExprNode::Pow(base, exponent) => mentions(base, name) || mentions(exponent, name),
        ExprNode::Fn(_, args) => args.iter().any(|argument| mentions(argument, name)),
        ExprNode::Hold(_) => false,
    }
}

/// Replace the parameters with the arguments.
///
/// Arity is checked by the caller, so a short argument list cannot panic here:
/// a missing argument is replaced by zero.
pub fn substitute(body: &Expr, params: &[SymbolId], args: &[Expr]) -> Expr {
    match &**body {
        ExprNode::Num(_) => body.clone(),
        ExprNode::Sym(symbol) => match params.iter().position(|parameter| parameter == symbol) {
            Some(index) => args.get(index).cloned().unwrap_or_else(|| ExprNode::int(0)),
            None => body.clone(),
        },
        ExprNode::Add(terms) => {
            let mapped: Vec<Expr> = terms
                .iter()
                .map(|term| substitute(term, params, args))
                .collect();
            canlab_core::add(mapped)
        }
        ExprNode::Mul(factors) => {
            let mapped: Vec<Expr> = factors
                .iter()
                .map(|factor| substitute(factor, params, args))
                .collect();
            canlab_core::mul(mapped)
        }
        ExprNode::Pow(base, exponent) => canlab_core::pow(
            substitute(base, params, args),
            substitute(exponent, params, args),
        ),
        // A *built-in* call in the body (`sin x`, `ln x`) has its arguments
        // substituted like anything else: `f(x) = x + sin x` called with 2 must
        // become `2 + sin 2`, not `2 + sin x`. A call to a *user* definition has
        // already been inlined by the reader, so only `Fn` and a held integral
        // reach this arm, and the integral is rightly opaque.
        ExprNode::Fn(name, call_args) => ExprNode::fn_raw(
            *name,
            call_args
                .iter()
                .map(|argument| substitute(argument, params, args))
                .collect(),
        ),
        ExprNode::Hold(_) => body.clone(),
    }
}
