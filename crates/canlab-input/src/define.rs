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
        // A call inside a body is left alone: definitions cannot call each
        // other yet, and pretending otherwise would need an evaluator here.
        ExprNode::Fn(_, _) | ExprNode::Hold(_) => body.clone(),
    }
}
