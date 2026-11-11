//! Expression-level algebra built on the polynomial: expand, collect, divide,
//! cancel and factor.
//!
//! These take and return [`Expr`] because that is what the rest of the project
//! speaks. They live here rather than in `canlab-core` so the dependency runs one
//! way: core does not know what a polynomial is.

use canlab_core::{Expr, ExprNode, Number, SymbolId};

use crate::Poly;

/// Why a factorisation was not possible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FactorError {
    /// Not a polynomial in one variable: a function call, a negative power, or
    /// more than one variable.
    NotAPolynomial,
    /// A polynomial that this crate cannot factor, such as a cubic with no
    /// rational root.
    NoFactors,
}

impl std::fmt::Display for FactorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactorError::NotAPolynomial => {
                write!(f, "only a polynomial in one variable can be factored")
            }
            FactorError::NoFactors => write!(f, "no rational factorisation was found"),
        }
    }
}

/// Every symbol in an expression, in first-seen order.
pub fn free_symbols(expression: &Expr) -> Vec<SymbolId> {
    fn walk(expression: &Expr, out: &mut Vec<SymbolId>) {
        match &**expression {
            ExprNode::Sym(symbol) => {
                if !out.contains(symbol) {
                    out.push(*symbol);
                }
            }
            ExprNode::Num(_) => {}
            ExprNode::Add(terms) | ExprNode::Mul(terms) => {
                for term in terms {
                    walk(term, out);
                }
            }
            ExprNode::Pow(base, exponent) => {
                walk(base, out);
                walk(exponent, out);
            }
            ExprNode::Fn(_, args) => {
                for argument in args {
                    walk(argument, out);
                }
            }
            ExprNode::Hold(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(expression, &mut out);
    out
}

/// Multiply out products of sums and collect like terms.
///
/// A polynomial in its own variables goes through [`Poly`], which already has a
/// canonical form. Anything else — a function, a negative power — is distributed
/// structurally instead, so `(x + 1)·sin x` becomes `x·sin x + sin x` and stays
/// otherwise intact.
pub fn expand(expression: &Expr) -> Expr {
    let vars = free_symbols(expression);
    if let Some(poly) = Poly::<Number>::from_expr(expression, &vars) {
        return poly.to_expr();
    }
    match &**expression {
        ExprNode::Add(terms) => canlab_core::add(terms.iter().map(expand)),
        ExprNode::Mul(factors) => {
            let mut product = ExprNode::int(1);
            for factor in factors {
                product = distribute(&product, &expand(factor));
            }
            product
        }
        ExprNode::Pow(base, exponent) => {
            // Only a non-negative integer power multiplies out. `(x+1)^(1/2)` is
            // not a polynomial and must not be expanded into one.
            let Some(Number::Int(power)) = exponent.as_number() else {
                return canlab_core::pow(expand(base), exponent.clone());
            };
            if *power < 0 || *power > 64 {
                return canlab_core::pow(expand(base), exponent.clone());
            }
            let base = expand(base);
            let mut product = ExprNode::int(1);
            for _ in 0..*power {
                product = distribute(&product, &base);
            }
            product
        }
        ExprNode::Fn(name, args) => ExprNode::fn_raw(*name, args.iter().map(expand).collect()),
        _ => expression.clone(),
    }
}

/// `left · right` with both sides multiplied out.
fn distribute(left: &Expr, right: &Expr) -> Expr {
    let left_terms: Vec<Expr> = match &**left {
        ExprNode::Add(terms) => terms.iter().cloned().collect(),
        _ => vec![left.clone()],
    };
    let right_terms: Vec<Expr> = match &**right {
        ExprNode::Add(terms) => terms.iter().cloned().collect(),
        _ => vec![right.clone()],
    };
    let mut products = Vec::with_capacity(left_terms.len() * right_terms.len());
    for left in &left_terms {
        for right in &right_terms {
            products.push(canlab_core::mul(vec![left.clone(), right.clone()]));
        }
    }
    canlab_core::add(products)
}

/// Group an expression by the powers of one variable.
///
/// Every other symbol becomes part of the coefficient, which is what makes
/// `collect(x² + a·x + b, x)` print as it was written.
pub fn collect(expression: &Expr, variable: SymbolId) -> Expr {
    match Poly::<Expr>::from_expr(expression, &[variable]) {
        Some(poly) => poly.to_expr(),
        None => expression.clone(),
    }
}

/// Divide two polynomials in one variable, exactly.
///
/// `None` when either side is not a polynomial in `variable`, because a quotient
/// that is not a polynomial cannot be printed as one.
pub fn divide(dividend: &Expr, divisor: &Expr, variable: SymbolId) -> Option<(Expr, Expr)> {
    let vars = vec![variable];
    let dividend = Poly::<Number>::from_expr(dividend, &vars)?;
    let divisor = Poly::<Number>::from_expr(divisor, &vars)?;
    let (quotient, remainder) = dividend.div_rem(&divisor)?;
    Some((quotient.to_expr(), remainder.to_expr()))
}

/// Reduce a ratio of two polynomials by their greatest common divisor.
///
/// `None` when either side is not a polynomial in `variable`, or when the
/// denominator is zero — the caller gets to say which of those it was.
pub fn cancel(
    numerator: &Expr,
    denominator: &Expr,
    variable: SymbolId,
) -> Option<(Expr, Expr)> {
    let vars = vec![variable];
    let numerator = Poly::<Number>::from_expr(numerator, &vars)?;
    let denominator = Poly::<Number>::from_expr(denominator, &vars)?;
    if denominator.is_zero() {
        return None;
    }
    let gcd = numerator.gcd(&denominator);
    match gcd {
        Some(gcd) if !gcd.is_zero() && gcd.degree() > 0 => {
            let (top, top_remainder) = numerator.div_rem(&gcd)?;
            let (bottom, bottom_remainder) = denominator.div_rem(&gcd)?;
            if !top_remainder.is_zero() || !bottom_remainder.is_zero() {
                return None;
            }
            Some((top.to_expr(), bottom.to_expr()))
        }
        _ => Some((numerator.to_expr(), denominator.to_expr())),
    }
}

/// Factor a polynomial in one variable over the rationals.
///
/// Rational roots are divided out one at a time, and what is left is returned as
/// a factor of its own. It is a *partial* factorisation by design: a cubic with no
/// rational root comes back with `NoFactors` rather than with a wrong product,
/// and the caller can decide to show the polynomial unchanged.
pub fn factor(expression: &Expr) -> Result<Expr, FactorError> {
    let vars = free_symbols(expression);
    if vars.len() != 1 {
        return Err(FactorError::NotAPolynomial);
    }
    let poly =
        Poly::<Number>::from_expr(expression, &vars).ok_or(FactorError::NotAPolynomial)?;
    if poly.degree() == 0 {
        return Ok(expression.clone());
    }

    let mut remaining = poly.clone();
    let mut factors: Vec<Expr> = Vec::new();
    loop {
        let Some(roots) = remaining.rational_roots() else {
            break;
        };
        let Some(root) = roots.first().cloned() else {
            break;
        };
        let linear = crate::univariate::from_coefficients(vars.clone(), &[root.neg(), Number::one()]);
        let Some((quotient, remainder)) = remaining.div_rem(&linear) else {
            break;
        };
        if !remainder.is_zero() {
            break;
        }
        factors.push(linear.to_expr());
        remaining = quotient;
    }
    if remaining.degree() >= 1 {
        // Nothing found at all is a refusal, not a "factorisation" that hands the
        // polynomial back unchanged: the caller asked a question and the honest
        // answer is that this crate cannot answer it.
        if factors.is_empty() {
            return Err(FactorError::NoFactors);
        }
        factors.push(remaining.to_expr());
    }
    if factors.is_empty() {
        return Err(FactorError::NoFactors);
    }
    Ok(canlab_core::mul(factors))
}
