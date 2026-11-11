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

use canlab_core::{Expr, ExprNode, Number, Relation, SymbolId, add, mul, pow};
use canlab_poly::Poly;

use crate::{Solutions, Solver, SolverError};

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

/// Rational roots of a polynomial of degree three or more.
///
/// None when the expression is not a polynomial in `var`, when the coefficients
/// are not integers small enough to factor by counting, or when a factor is left
/// that this backend cannot solve. All three are refusals, and the caller
/// reports them as such.
fn higher_degree_roots(solver: &NativeSolver, eq: &Expr, var: SymbolId) -> Option<Vec<Expr>> {
    let mut working = Poly::<Number>::from_expr(eq, &[var])?;
    let mut roots: Vec<Expr> = Vec::new();
    let mut seen: Vec<Number> = Vec::new();

    // Rational roots are divided out one at a time, repeatedly: `(x−1)²(x+2)`
    // loses both copies of `1` but lists it once, because that is what "the roots
    // are" means. Deflating rather than only *finding* is what makes the leftover
    // degree trustworthy — without it a partial search answers `{1}` for
    // `(x−1)(x²−2)` and drops two roots.
    loop {
        let found = working.rational_roots()?;
        if found.is_empty() {
            break;
        }
        let mut progressed = false;
        for candidate in found {
            let mut deflations = 0;
            while let Some(deflated) = working.deflate(&candidate) {
                working = deflated;
                deflations += 1;
            }
            if deflations == 0 {
                // `rational_roots` said this is a root and division disagrees,
                // which is a contradiction rather than an answer.
                return None;
            }
            progressed = true;
            if !seen.contains(&candidate) {
                seen.push(candidate.clone());
                roots.push(ExprNode::num(candidate));
            }
        }
        if !progressed {
            break;
        }
    }

    // What is left is a constant when the rational roots were the whole
    // factorisation, and a linear or quadratic factor otherwise. The quadratic
    // formula finishes those exactly, surds included: `(x−1)(x²−2)` answers
    // `1, ±√2` instead of `1`.
    let coefficients = working.univariate_coefficients()?;
    let degree = coefficients.keys().copied().max().unwrap_or(0);
    match degree {
        0 => Some(roots),
        1 | 2 => {
            let terms: Vec<Expr> = coefficients
                .iter()
                .filter(|(_, coefficient)| !coefficient.is_zero())
                .map(|(power, coefficient)| term(coefficient, i128::from(*power), var))
                .collect();
            let leftover = polynomial(terms);
            let more = solver.solve(&leftover, var).ok()?;
            roots.extend(more);
            Some(roots)
        }
        // A cubic or higher with no rational root: real roots exist and this
        // backend does not claim to know them.
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
    /// The expression is canonical, so there is nothing to *normalize* — but the
    /// rewrite rules know identities the canonical form does not: `ln e → 1`,
    /// `|x|² → x²`, `sin² + cos² → 1`. This is the button doing something for the
    /// first time.
    fn simplify(&self, e: &Expr) -> Result<Expr, SolverError> {
        Ok(canlab_core::rewrite::simplify(e))
    }

    /// `d/dvar`, by the real rules rather than by polynomial bookkeeping.
    ///
    /// [`canlab_core::differentiate`] does sums, the product rule, the power
    /// rule, the chain rule through `sin`/`cos`/`tan`/`exp`/`ln`, and the
    /// general `u^v` form. Higher orders repeat it, which is what `f''` will
    /// mean when definitions land.
    fn diff(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        canlab_core::differentiate(e, var).ok_or(SolverError::NotImplemented)
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
        let Some(map) = coefficients(eq, var) else {
            // Not a polynomial with numeric coefficients. An exponential comes
            // first: `2^x = 8` is not polynomial in `x` and is not parametric
            // either, and taking its logarithm is exact.
            if let Some(values) = crate::exponential::solve_exponential(self, eq, var) {
                return Ok(values);
            }
            // A sum of logarithms: `log₂x + log₂(x−2) = 3`. The roots outside the
            // domain are dropped here; W2.4's report of a *rejected* root needs a
            // place in `Solutions` and is not in this one.
            if let Some((kept, _rejected)) = crate::logarithmic::solve_logarithmic(self, eq, var) {
                return Ok(kept);
            }
            // It may still be one in `var` with symbolic coefficients — `x² + y² = 1`
            // has two answers in `x` — and that is the case the editor's autofill
            // needs.
            return crate::parametric::solve_parametric(eq, var)
                .ok_or(SolverError::NotImplemented);
        };
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
            // Degree three and up: rational roots, and a clean refusal when
            // there are none. x³ − 2 has a real root and this backend does not
            // claim to know it.
            Some(_) => higher_degree_roots(self, eq, var).ok_or(SolverError::NotImplemented),
        }
    }

    /// The real solutions of `eq == 0`, with the answers a list cannot say.
    ///
    /// Degree 1 and 2 reuse [`NativeSolver::solve`], so a quadratic keeps its
    /// roots and a negative discriminant becomes [`Solutions::None`] instead of
    /// an empty list. A constant equation is the trichotomy every school
    /// exercise has: `0 = 0` is every number, `0 = c` with `c ≠ 0` is none.
    /// Higher degrees and anything that is not a polynomial in `var` stay
    /// [`SolverError::NotImplemented`]; W2.3 and W2.5 will fill those in.
    fn solve_all(&self, eq: &Expr, var: SymbolId) -> Result<Solutions, SolverError> {
        let Some(map) = coefficients(eq, var) else {
            if let Some(values) = crate::exponential::solve_exponential(self, eq, var) {
                // An exponential whose right side is not positive has no real
                // solution, which is `None` and not an empty set of values.
                return if values.is_empty() {
                    Ok(Solutions::None)
                } else {
                    Ok(Solutions::Values(values))
                };
            }
            if let Some((kept, _rejected)) = crate::logarithmic::solve_logarithmic(self, eq, var) {
                return if kept.is_empty() {
                    Ok(Solutions::None)
                } else {
                    Ok(Solutions::Values(kept))
                };
            }
            if let Some(solutions) = crate::trig::solve_trigonometric(eq, var) {
                return Ok(solutions);
            }
            return crate::parametric::solve_parametric(eq, var)
                .map(Solutions::Values)
                .ok_or(SolverError::NotImplemented);
        };
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
            // The zero polynomial: every value of `var` solves it.
            None => Ok(Solutions::All),
            // A nonzero constant: no value does.
            Some(0) => Ok(Solutions::None),
            Some(1) | Some(2) => {
                let values = self.solve(eq, var)?;
                if values.is_empty() {
                    // Among degrees 1 and 2 only a quadratic with a negative
                    // discriminant reaches here, and that is a real answer.
                    Ok(Solutions::None)
                } else {
                    Ok(Solutions::Values(values))
                }
            }
            // Degree three and up: rational roots, and a clean refusal when there
            // are none. `x^3 - 2` has a real root and this backend does not claim
            // to know it.
            Some(_) => higher_degree_roots(self, eq, var)
                .map(Solutions::Values)
                .ok_or(SolverError::NotImplemented),
        }
    }

    /// Solve the system as a system: substitution first, then elimination.
    ///
    /// One equation at a time would silently drop the variables the other
    /// equations pin down — `x = 1, y = x` needs both answers.
    fn solve_system(
        &self,
        equations: &[Expr],
        vars: &[SymbolId],
    ) -> Result<Vec<(SymbolId, Expr)>, SolverError> {
        crate::system::solve_system(self, equations, vars)
    }

    /// The solution set of an inequality, by a sign chart over exact critical
    /// points. See [`crate::inequality`] for the construction.
    fn solve_relation(
        &self,
        relation: &Relation,
        var: SymbolId,
    ) -> Result<Solutions, SolverError> {
        crate::inequality::solve_relation(self, relation, var)
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

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{add, expr::ExprNode, int, mul, pow, sym};

    fn x() -> Expr {
        sym("x")
    }

    #[test]
    fn simplify_uses_the_rewrite_rules() {
        // `ln e + 1` is `2`, and the button that used to hand the expression
        // back unchanged now says so.
        let expression = add(vec![
            ExprNode::fn_raw(SymbolId::intern("ln"), vec![sym("e")]),
            int(1),
        ]);
        assert_eq!(NativeSolver.simplify(&expression).unwrap(), int(2));
    }

    #[test]
    fn simplify_does_not_invent_an_answer() {
        // Nothing here is an identity, so nothing changes: a rewrite that made
        // this smaller would be a bug, not a simplification.
        let expression = add(vec![pow(sym("x"), int(2)), int(1)]);
        assert_eq!(NativeSolver.simplify(&expression).unwrap(), expression);
        let _ = mul(vec![int(2), x()]);
    }

    fn var() -> SymbolId {
        SymbolId::intern("x")
    }

    #[test]
    fn solve_all_keeps_both_roots_of_a_quadratic() {
        // x² = 4
        let equation = add([pow(x(), int(2)), int(-4)]);
        assert_eq!(
            NativeSolver.solve_all(&equation, var()),
            Ok(Solutions::Values(vec![int(2), int(-2)]))
        );
    }

    #[test]
    fn solve_all_keeps_a_linear_root() {
        // 2x = 6
        let equation = add([mul([int(2), x()]), int(-6)]);
        assert_eq!(
            NativeSolver.solve_all(&equation, var()),
            Ok(Solutions::Values(vec![int(3)]))
        );
    }

    #[test]
    fn solve_all_says_none_for_a_negative_discriminant() {
        // x² + 1 = 0 has no real solutions.
        let equation = add([pow(x(), int(2)), int(1)]);
        assert_eq!(
            NativeSolver.solve_all(&equation, var()),
            Ok(Solutions::None)
        );
    }

    #[test]
    fn solve_all_says_none_for_a_contradiction() {
        // 0 = 3
        assert_eq!(NativeSolver.solve_all(&int(3), var()), Ok(Solutions::None));
    }

    #[test]
    fn solve_all_says_all_for_an_identity() {
        // 0 = 0
        assert_eq!(NativeSolver.solve_all(&int(0), var()), Ok(Solutions::All));
    }

    #[test]
    fn solve_all_answers_a_cubic_with_rational_roots() {
        // `x^3 - x = x(x-1)(x+1)` has three roots, and a missing constant term
        // must not hide two of them behind the zero.
        let equation = add(vec![pow(x(), int(3)), mul(vec![int(-1), x()])]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("three rational roots");
        let Solutions::Values(values) = solutions else {
            panic!("expected values");
        };
        let mut roots: Vec<f64> = values
            .iter()
            .filter_map(|value| value.as_number().map(Number::to_f64))
            .collect();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, vec![-1.0, 0.0, 1.0]);
    }

    #[test]
    fn solve_all_answers_a_quartic_with_four_rational_roots() {
        // x⁴ - 5x² + 4 = (x²-1)(x²-4) has roots ±1 and ±2, and the rational
        // root search has to find all four through a leading coefficient of one.
        let equation = add(vec![
            pow(x(), int(4)),
            mul(vec![int(-5), pow(x(), int(2))]),
            int(4),
        ]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("four rational roots");
        let Solutions::Values(values) = solutions else {
            panic!("expected values");
        };
        let mut roots: Vec<f64> = values
            .iter()
            .filter_map(|value| value.as_number().map(Number::to_f64))
            .collect();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, vec![-2.0, -1.0, 1.0, 2.0]);
    }
    #[test]
    fn solve_all_finishes_a_cubic_with_surds_left_over() {
        // (x−1)(x²−2) = x³ − x² − 2x + 2. One rational root, and the leftover
        // quadratic gives ±√2 — answering `{1}` here would be a wrong answer
        // rather than a partial one, which is why the leftover is solved instead
        // of the rational roots being reported alone.
        let equation = add(vec![
            pow(x(), int(3)),
            mul(vec![int(-1), pow(x(), int(2))]),
            mul(vec![int(-2), x()]),
            int(2),
        ]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("three real roots");
        let Solutions::Values(values) = solutions else {
            panic!("expected values");
        };
        let texts: Vec<String> = values.iter().map(canlab_tex::to_latex).collect();
        assert_eq!(texts.len(), 3, "{texts:?}");
        assert!(texts.iter().any(|text| text == "1"), "{texts:?}");
        let surds = texts.iter().filter(|text| *text != "1").count();
        assert_eq!(surds, 2, "both ±√2 roots are reported: {texts:?}");
    }

    #[test]
    fn solve_all_lists_a_double_root_once() {
        // (x−1)²(x+2) = x³ − 3x + 2: the answer is a *set*, so a repeated root is
        // one member of it, not two.
        let equation = add(vec![
            pow(x(), int(3)),
            mul(vec![int(-3), x()]),
            int(2),
        ]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("two distinct roots");
        let Solutions::Values(values) = solutions else {
            panic!("expected values");
        };
        let texts: Vec<String> = values.iter().map(canlab_tex::to_latex).collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
    }

    #[test]
    fn solve_all_declines_a_cubic_it_cannot_factor() {
        // `x^3 - 2` has a real root and this backend does not claim to know it.
        let equation = add(vec![pow(x(), int(3)), int(-2)]);
        let error = NativeSolver
            .solve_all(&equation, var())
            .expect_err("no rational root");
        assert_eq!(error, SolverError::NotImplemented);
        // A decline is a normal answer, not a failure: the caller keeps the
        // equation rather than showing an error banner.
        assert!(error.is_declined());
    }
}
