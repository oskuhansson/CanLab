//! The expression tree.

use std::fmt;
use std::sync::Arc;

use crate::number::Number;
use crate::symbol::SymbolId;

/// Structural sharing: cloning an `Expr` is a refcount bump.
pub type Expr = Arc<ExprNode>;

pub type FuncId = SymbolId;

/// Canonical-form rules, applied by [`crate::normalize`] and relied on
/// everywhere else:
///
/// * `Add`/`Mul` are flattened, never empty, never length 1.
/// * `Mul` may start with a `Num` coefficient; the remaining factors are sorted.
/// * `Fn` is only used for named functions; `\sqrt` becomes `Pow(_, 1/2)`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExprNode {
    Num(Number),
    Sym(SymbolId),
    Add(Vec<Expr>),
    Mul(Vec<Expr>),
    Pow(Expr, Expr),
    Fn(FuncId, Vec<Expr>),
    Hold(Box<Hold>),
}

/// Constructs we refuse to evaluate but can still print and compare.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Hold {
    Integral { body: Expr, var: SymbolId },
}

impl ExprNode {
    /// `ExprNode::int(n)` — shorthand for a small integer literal.
    pub fn int(value: i64) -> Expr {
        int(value)
    }

    pub fn num(n: Number) -> Expr {
        Arc::new(ExprNode::Num(n))
    }

    pub fn sym(s: SymbolId) -> Expr {
        Arc::new(ExprNode::Sym(s))
    }

    pub fn add_raw(terms: Vec<Expr>) -> Expr {
        Arc::new(ExprNode::Add(terms))
    }

    pub fn mul_raw(factors: Vec<Expr>) -> Expr {
        Arc::new(ExprNode::Mul(factors))
    }

    pub fn pow_raw(base: Expr, exp: Expr) -> Expr {
        Arc::new(ExprNode::Pow(base, exp))
    }

    pub fn fn_raw(name: FuncId, args: Vec<Expr>) -> Expr {
        Arc::new(ExprNode::Fn(name, args))
    }

    pub fn hold(h: Hold) -> Expr {
        Arc::new(ExprNode::Hold(Box::new(h)))
    }

    /// Split a term into its numeric coefficient and the rest.
    ///
    /// `2*x` -> `(2, x)`, `-x` -> `(-1, x)`, `5` -> `(5, 1)`.
    pub(crate) fn split_coeff(&self) -> (Number, Expr) {
        match self {
            ExprNode::Num(n) => (n.clone(), ExprNode::num(Number::one())),
            ExprNode::Mul(factors) if !factors.is_empty() => match &*factors[0] {
                ExprNode::Num(n) => {
                    let rest = &factors[1..];
                    let base = match rest.len() {
                        0 => ExprNode::num(Number::one()),
                        1 => rest[0].clone(),
                        _ => ExprNode::mul_raw(rest.to_vec()),
                    };
                    (n.clone(), base)
                }
                _ => (Number::one(), Arc::new(self.clone())),
            },
            _ => (Number::one(), Arc::new(self.clone())),
        }
    }

    pub fn is_zero(&self) -> bool {
        matches!(self, ExprNode::Num(n) if n.is_zero())
    }

    pub fn is_one(&self) -> bool {
        matches!(self, ExprNode::Num(n) if n.is_one())
    }

    pub fn as_number(&self) -> Option<&Number> {
        match self {
            ExprNode::Num(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<SymbolId> {
        match self {
            ExprNode::Sym(s) => Some(*s),
            _ => None,
        }
    }

    /// `Fn` name as a `String`; `None` for non-functions.
    pub fn func_name(&self) -> Option<String> {
        match self {
            ExprNode::Fn(f, _) => Some(f.as_str()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Plain-text rendering (debugging, CLI fallback)
// ---------------------------------------------------------------------------

const PREC_ADD: u8 = 1;
const PREC_MUL: u8 = 2;
const PREC_POW: u8 = 3;
const PREC_ATOM: u8 = 4;

fn node_prec(node: &ExprNode) -> u8 {
    match node {
        ExprNode::Add(_) => PREC_ADD,
        ExprNode::Mul(_) => PREC_MUL,
        ExprNode::Pow(..) => PREC_POW,
        // A rational prints as `a/b`, which binds like a division.
        ExprNode::Num(Number::Rat(..)) => PREC_MUL,
        _ => PREC_ATOM,
    }
}

fn write_node(node: &ExprNode, f: &mut fmt::Formatter<'_>, min_prec: u8) -> fmt::Result {
    let parens = node_prec(node) < min_prec;
    if parens {
        write!(f, "(")?;
    }

    match node {
        ExprNode::Num(n) => write!(f, "{n}")?,
        ExprNode::Sym(s) => write!(f, "{s}")?,
        ExprNode::Add(terms) => {
            for (i, term) in terms.iter().enumerate() {
                let (coeff, base) = term.split_coeff();
                let negative = coeff.is_negative();
                if i == 0 {
                    if negative {
                        write!(f, "-")?;
                    }
                } else if negative {
                    write!(f, " - ")?;
                } else {
                    write!(f, " + ")?;
                }
                let magnitude = if negative { coeff.neg() } else { coeff };
                if base.is_one() {
                    write!(f, "{magnitude}")?;
                } else if magnitude.is_one() {
                    write_node(&base, f, PREC_MUL)?;
                } else {
                    if node_prec(&ExprNode::Num(magnitude.clone())) > PREC_MUL {
                        write!(f, "{magnitude}")?;
                    } else {
                        write!(f, "({magnitude})")?;
                    }
                    write!(f, "*")?;
                    write_node(&base, f, PREC_MUL)?;
                }
            }
        }
        ExprNode::Mul(factors) => {
            for (i, factor) in factors.iter().enumerate() {
                if i > 0 {
                    write!(f, "*")?;
                }
                // The leading coefficient may be a bare rational; everything
                // after it must be atomic or parenthesized.
                let min = if i == 0 { PREC_ADD } else { PREC_POW };
                write_node(factor, f, min)?;
            }
        }
        ExprNode::Pow(base, exp) => {
            write_node(base, f, PREC_ATOM)?;
            write!(f, "^")?;
            write_node(exp, f, PREC_ATOM)?;
        }
        ExprNode::Fn(name, args) => {
            write!(f, "{name}(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_node(arg, f, PREC_ADD)?;
            }
            write!(f, ")")?;
        }
        ExprNode::Hold(hold) => match &**hold {
            Hold::Integral { body, var } => {
                write!(f, "Integral(")?;
                write_node(body, f, PREC_ADD)?;
                write!(f, ", {var})")?;
            }
        },
    }

    if parens {
        write!(f, ")")?;
    }
    Ok(())
}

impl fmt::Display for ExprNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_node(self, f, 0)
    }
}

/// Borrow an `Expr` as its `ExprNode` so callers can format it without
/// allocating. `Arc<T>` does not itself implement `Display`.
#[inline]
pub fn node(e: &Expr) -> &ExprNode {
    e
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

pub fn int(value: i64) -> Expr {
    ExprNode::num(Number::int(value as i128))
}

pub fn ratio(num: i64, den: i64) -> Expr {
    ExprNode::num(Number::ratio(num as i128, den as i128))
}

pub fn sym(name: &str) -> Expr {
    ExprNode::sym(SymbolId::intern(name))
}

/// `sum(iter)` — normalizing.
pub fn sum<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    crate::normalize::add(items)
}

/// `product(iter)` — normalizing.
pub fn product<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    crate::normalize::mul(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_order_is_preserved() {
        // Raw constructors do not sort; only `normalize` establishes order.
        let x = sym("x");
        let expr = ExprNode::add_raw(vec![ExprNode::mul_raw(vec![int(2), x]), int(1)]);
        assert_eq!(expr.to_string(), "2*x + 1");
    }

    #[test]
    fn nested_powers_get_parenthesized() {
        let x = sym("x");
        let e = ExprNode::pow_raw(
            ExprNode::pow_raw(x, int(2)),
            ExprNode::num(Number::ratio(1, 2)),
        );
        assert_eq!(e.to_string(), "(x^2)^(1/2)");
    }

    #[test]
    fn addition_inside_multiplication_gets_parenthesized() {
        let x = sym("x");
        let sum = ExprNode::add_raw(vec![x, int(1)]);
        let e = ExprNode::mul_raw(vec![int(2), sum]);
        assert_eq!(e.to_string(), "2*(x + 1)");
    }
}
