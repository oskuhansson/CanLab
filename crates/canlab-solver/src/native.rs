//! A small native backend: the algebra a student does by hand.
//!
//! [`MockSolver`](crate::MockSolver) exists so the pipeline can run with no
//! backend at all, and it declines everything but `∫xⁿ`. That leaves the
//! Variables panel with nothing to show. This backend is the honest step up: it
//! solves the two cases that cover most school exercises and says nothing more.
//!
//! * **Solve** — a polynomial equation of degree 1 or 2 in one variable, exactly.
//!   A quadratic with a rational square discriminant gives rational roots; a
//!   negative discriminant gives *no real solutions*, which is an answer, not a
//!   failure; anything else gives the exact surd `(-b ± √D) / 2a`.
//! * **Integrate** — a sum of integer powers, term by term.
//! * **Differentiate** — the same polynomials.
//! * **Simplify** — already canonical, so it hands the expression back.
//!
//! Everything else returns [`SolverError::NotImplemented`], and the UI shows that
//! as "declined" rather than as an error. This is not a CAS; it is the part of
//! one that can be written in a few hundred lines and trusted.

use std::collections::BTreeMap;

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, mul, pow};

use crate::{Solver, SolverError};

/// The native backend. Stateless, so a unit struct.
pub struct NativeSolver;

/// One term as `coefficient · var^power`, if it is a monomial in `var`.
fn monomial(term: &Expr, var: SymbolId) -> Option<(Number, i128)> {
    match &**term {
        ExprNode::Num(number) => Some((number.clone(), 0)),
        ExprNode::Sym(symbol) if *symbol == var => Some((Number::one(), 1)),
        ExprNode::Pow(base, exponent) => match (&**base, &**exponent) {
            (ExprNode::Sym(symbol), ExprNode::Num(Number::Int(power))) if *symbol == var => {
                Some((Number::one(), *power))
            }
            _ => None,
        },
        ExprNode::Mul(factors) => {
            let mut coefficient = Number::one();
            let mut power = 0i128;
            let mut seen_variable = false;
            for factor in factors {
                match &**factor {
                    ExprNode::Num(number) => coefficient = coefficient.mul(number),
                    _ => {
                        let (inner, this) = monomial(factor, var)?;
                        if !inner.is_one() {
                            return None;
                        }
                        power += this;
                        seen_variable = true;
                    }
                }
            }
            if !seen_variable {
                return None;
            }
            Some((coefficient, power))
        }
        _ => None,
    }
}

/// Every coefficient of the polynomial in `var`, keyed by power.
///
/// `None` when the expression is not a polynomial with numeric coefficients in
/// that variable — a product of two variables, a function, a held integral.
fn coefficients(expression: &Expr, var: SymbolId) -> Option<BTreeMap<i128, Number>> {
    let terms: Vec<&Expr> = match &**expression {
        ExprNode::Add(terms) => terms.iter().collect(),
        _ => vec![expression],
    };
    let mut out: BTreeMap<i128, Number> = BTreeMap::new();
    for term in terms {
        let (coefficient, power) = monomial(term, var)?;
        out.entry(power)
            .and_modify(|existing| *existing = existing.add(&coefficient))
            .or_insert(coefficient);
    }
    Some(out)
}

fn number(value: &Number) -> Expr {
    ExprNode::num(value.clone())
}

/// `coefficient · var^power`, canonical.
fn term(coefficient: &Number, power: i128, var: SymbolId) -> Expr {
    let scaled = number(coefficient);
    match power {
        0 => scaled,
        1 => mul([scaled, ExprNode::sym(var)]),
        _ => mul([
            scaled,
            pow(ExprNode::sym(var), ExprNode::num(Number::int(power))),
        ]),
    }
}

fn polynomial(terms: Vec<Expr>) -> Expr {
    if terms.is_empty() {
        ExprNode::int(0)
    } else {
        add(terms)
    }
}

impl Solver for NativeSolver {
    /// The expression is already canonical; there is nothing to rewrite.
    fn simplify(&self, e: &Expr) -> Result<Expr, SolverError> {
        Ok(e.clone())
    }

    /// `d/dvar` of a polynomial.
    fn diff(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        let map = coefficients(e, var).ok_or(SolverError::NotImplemented)?;
        let mut terms = Vec::new();
        for (power, coefficient) in map {
            if power <= 0 {
                continue;
            }
            terms.push(term(
                &coefficient.mul(&Number::int(power)),
                power - 1,
                var,
            ));
        }
        Ok(polynomial(terms))
    }

    /// `∫ e dvar` of a sum of integer powers.
    fn integrate(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        let map = coefficients(e, var).ok_or(SolverError::NotImplemented)?;
        let mut terms = Vec::new();
        for (power, coefficient) in map {
            if power < 0 {
                // ∫x⁻¹ is a logarithm, which this backend does not do.
                return Err(SolverError::NotImplemented);
            }
            let raised = power.checked_add(1).ok_or(SolverError::NotImplemented)?;
            terms.push(term(
                &coefficient.div(&Number::int(raised)),
                raised,
                var,
            ));
        }
        Ok(polynomial(terms))
    }

    /// The real solutions of `eq == 0`, for degree 1 and 2.
    fn solve(&self, eq: &Expr, var: SymbolId) -> Result<Vec<Expr>, SolverError> {
        let map = coefficients(eq, var).ok_or(SolverError::NotImplemented)?;
        if map.keys().any(|power| *power < 0) {
            return Err(SolverError::NotImplemented);
        }
        let coefficient = |power: i128| map.get(&power).cloned().unwrap_or_else(Number::zero);
        let degree = map
            .keys()
            .copied()
            .filter(|power| !coefficient(*power).is_zero())
            .max();

        match degree {
            // `0 = 0` is not an equation with solutions; `c = 0` with `c ≠ 0`
            // has none, which the caller can say.
            None | Some(0) => Err(SolverError::NotImplemented),
            Some(1) => {
                let b = coefficient(1);
                let c = coefficient(0);
                Ok(vec![ExprNode::num(c.neg().div(&b))])
            }
            Some(2) => {
                let a = coefficient(2);
                let b = coefficient(1);
                let c = coefficient(0);
                let discriminant = b.mul(&b).sub(&Number::int(4).mul(&a).mul(&c));
                let denominator = Number::int(2).mul(&a);
                let minus_b = b.neg();

                if discriminant.is_negative() {
                    // No real solutions: a real answer, not a failure.
                    return Ok(Vec::new());
                }
                if let Some(root) = discriminant.exact_nth_root(2) {
                    if root.is_zero() {
                        return Ok(vec![ExprNode::num(minus_b.div(&denominator))]);
                    }
                    return Ok(vec![
                        ExprNode::num(minus_b.add(&root).div(&denominator)),
                        ExprNode::num(minus_b.sub(&root).div(&denominator)),
                    ]);
                }

                // Not a perfect square: give the exact surd rather than a
                // rounded float.
                let root = pow(number(&discriminant), ExprNode::num(Number::ratio(1, 2)));
                let inverse = pow(number(&denominator), ExprNode::int(-1));
                Ok(vec![
                    mul([add(vec![number(&minus_b), root.clone()]), inverse.clone()]),
                    mul([
                        add(vec![number(&minus_b), mul([ExprNode::int(-1), root])]),
                        inverse,
                    ]),
                ])
            }
            Some(_) => Err(SolverError::NotImplemented),
        }
    }

    fn limit(&self, _e: &Expr, _var: SymbolId, _at: &Expr) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }

    fn series(
        &self,
        _e: &Expr,
        _var: SymbolId,
        _at: &Expr,
        _order: usize,
    ) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }
}
