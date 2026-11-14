//! A small native backend: the algebra a student does by hand.
//!
//! [`MockSolver`](crate::MockSolver) exists so the pipeline can run with no
//! backend at all, and it declines everything but `∫xⁿ`. That leaves the
//! Variables panel with nothing to show. This backend is the honest step up: it
//! solves the two cases that cover most school exercises and says nothing more.
//!
//! * **Solve** — the exact paths first: a polynomial equation of degree 1 or 2
//!   (rational roots, or the exact surd `(-b ± √D) / 2a`; a negative
//!   discriminant is *no real solutions*, which is an answer, not a failure),
//!   then an exponential, a sum of logarithms, a trigonometric family and a
//!   parametric answer. Only when every one of those declines does a numeric
//!   scan look for one real root in `[-32, 32]`, and that answer carries its
//!   residual — see [`Solutions::Approximation`].
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
/// An answer with the candidates it rejected, when there were any.
///
/// `log₂x + log₂(x−2) = 3` has the algebraic roots `4` and `−2`; `−2` is outside
/// the logarithm's domain and is *rejected*, and the answer says so rather than
/// dropping it silently. An answer with nothing rejected is returned unchanged,
/// so a caller sees the simple type whenever the simple type is the truth.
fn with_rejections(answer: Solutions, rejected: Vec<Expr>) -> Solutions {
    if rejected.is_empty() {
        return answer;
    }
    Solutions::Rejected {
        solutions: Box::new(answer),
        rejected: rejected
            .into_iter()
            .map(|value| (value, crate::solutions::Rejection::OutsideLogarithmDomain))
            .collect(),
    }
}

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

/// The interval the numeric fallback scans.
///
/// `[-32, 32]` is the school window: a root outside it is not what an exercise
/// is about, and a wider interval would spend the same 512 cells on a coarser
/// grid. It is a constant because no caller has asked for a different one yet.
const SCAN_INTERVAL: (f64, f64) = (-32.0, 32.0);

/// The tolerance the numeric fallback hands to the scan.
///
/// `1e-12` is two orders below the `1e-10` residual the scan accepts, so the
/// refinement is not what limits the answer; the residual check is.
const SCAN_TOLERANCE: f64 = 1e-12;

/// One real root of `eq`, found numerically, or a decline.
///
/// This is the last resort, after every exact path has declined. It reports
/// **one** root even when the scan finds several — `x^4 - x - 1` has two real
/// roots — because a [`Solutions::Approximation`] can carry one, and a list of
/// one would look complete. The residual travels with the value, which is the
/// whole reason the variant exists.
///
/// A [`canlab_num::RootError`] is a decline, never a root: the scan refusing an
/// interval (a pole on the grid, a tree the evaluator cannot compile) says "I
/// do not know", and answering the nearest number would be the one thing this
/// backend must not do.
fn approximate_root(eq: &Expr, var: SymbolId) -> Result<Solutions, SolverError> {
    // `scan_expression` compiles the equation with `Program::compile(eq, &[var])`
    // itself. This is the same call, made first so that a tree the evaluator
    // cannot read is refused before a grid is spent on it — and so the refusal
    // is a decision in this function rather than a side effect of an empty scan.
    if canlab_core::Program::compile(eq, &[var]).is_none() {
        return Err(SolverError::NotImplemented);
    }
    let roots = canlab_num::scan_expression(eq, var, SCAN_INTERVAL, SCAN_TOLERANCE)
        .map_err(|_| SolverError::NotImplemented)?;
    if roots.is_empty() {
        // An empty scan is "no root here", not "the root is zero".
        return Err(SolverError::NotImplemented);
    }
    // **Every** root the scan found, not just the first. `x⁴ − x − 1` has two
    // real roots and reporting one of them is the kind of half-answer this
    // project treats as a bug: the scan already returns all of them, and a union
    // of approximations says so. One root is still one approximation, because a
    // one-member union prints as its member.
    let approximations: Vec<Solutions> = roots
        .into_iter()
        .map(|root| Solutions::Approximation {
            value: ExprNode::num(Number::float(root.value)),
            residual: root.residual,
        })
        .collect();
    Ok(match approximations.len() {
        1 => approximations.into_iter().next().expect("length checked"),
        _ => Solutions::Union(approximations),
    })
}

impl NativeSolver {
    /// Every exact path, tried in the order that proves the most.
    ///
    /// A polynomial of degree 1 or 2 is answered by the quadratic formula; an
    /// exponential, a sum of logarithms, a trigonometric family and a
    /// parametric answer are each a closed form. Degree three and up are
    /// answered only when they factor rationally — the leftover linear or
    /// quadratic factor is finished exactly, so `(x−1)(x²−2)` still gives
    /// `±√2`.
    ///
    /// [`SolverError::NotImplemented`] here means "no exact path knows this",
    /// which is exactly when [`Solver::solve_all`] may try the numeric scan.
    /// Nothing in this method guesses, so an exact answer can never be replaced
    /// by a rounded one.
    fn solve_exactly(&self, eq: &Expr, var: SymbolId) -> Result<Solutions, SolverError> {
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
            if let Some((kept, rejected)) = crate::logarithmic::solve_logarithmic(self, eq, var) {
                let answer = if kept.is_empty() {
                    Solutions::None
                } else {
                    Solutions::Values(kept)
                };
                return Ok(with_rejections(answer, rejected));
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
            // Degree three and up: rational roots, and a clean refusal when
            // there are none. `x^3 - 2` has a real root and no rational one;
            // the numeric fallback is what answers it now.
            Some(_) => higher_degree_roots(self, eq, var)
                .map(Solutions::Values)
                .ok_or(SolverError::NotImplemented),
        }
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
            // A sum of logarithms: `log₂x + log₂(x−2) = 3`. The root outside
            // the domain is *rejected* and reported as rejected: the exact path
            // returns a `Vec`, so the rejection is attached by `solve_all`,
            // which is the path that can carry it.
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
                    // No *real* solutions, and the complex ones are exact:
                    // `x² + 1 = 0` is `±i`, not "no solutions". `i` is a symbol
                    // the CAS already prints and parses, and the quadratic
                    // formula over the complex numbers is the same formula with
                    // `√(−Δ)` written as `√|Δ|·i`, so the answer stays exact
                    // wherever the real one would have been.
                    //
                    // The magnitude is the *positive* discriminant's root, which
                    // is what keeps `x² + 2 = 0` answering `±√2·i` rather than a
                    // float.
                    let magnitude = discriminant.neg();
                    let root = if let Some(exact) = magnitude.exact_nth_root(2) {
                        number(&exact)
                    } else {
                        pow(number(&magnitude), ExprNode::num(Number::ratio(1, 2)))
                    };
                    // Divided *term by term*, not by multiplying the sum by
                    // `1/2`: `(a + b)/2` distributes over the two terms here
                    // because the division is exact, and the canonical form of
                    // `(−2 + 4i)/2` is `−1 + 2i`, which is what a reader expects.
                    // Multiplying the whole sum by `1/2` left it unexpanded.
                    let inverse = pow(number(&denominator), ExprNode::int(-1));
                    let real_part = mul([number(&minus_b), inverse.clone()]);
                    let imaginary_part = mul([root, inverse, ExprNode::sym(SymbolId::intern("i"))]);
                    // Simplified, so `x² + 2x + 5` answers `−1 ± 2i` and not
                    // `(−2 ± 4i)/2`: the division is exact and the canonical
                    // form is what a reader expects.
                    return Ok(vec![
                        add(vec![real_part.clone(), imaginary_part.clone()]),
                        add(vec![
                            real_part,
                            mul([ExprNode::int(-1), imaginary_part]),
                        ]),
                    ]);
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
            // there are none. `solve` answers with exact values only, so it
            // refuses `x³ − 2`; `solve_all` is where the numeric fallback can
            // answer it.
            Some(_) => higher_degree_roots(self, eq, var).ok_or(SolverError::NotImplemented),
        }
    }

    /// The real solutions of `eq == 0`, with the answers a list cannot say.
    ///
    /// The exact paths run first, in `solve_exactly`: a polynomial, an
    /// exponential, a sum of logarithms, a trigonometric family, a parametric
    /// answer. Only when every one of them declines does the numeric scan run,
    /// so `x² = 4` cannot come back as `2.0000`. A scan that errors or finds
    /// nothing is still a decline: a [`canlab_num::RootError`] is never turned
    /// into a root.
    fn solve_all(&self, eq: &Expr, var: SymbolId) -> Result<Solutions, SolverError> {
        match self.solve_exactly(eq, var) {
            // `solve_exactly` already attaches a rejected root, because that is
            // where the logarithmic path is. Attaching it here as well printed
            // the rejection twice; the answer is wrapped once, at the layer that
            // computed it.
            Ok(solutions) => Ok(solutions),
            Err(SolverError::NotImplemented) => approximate_root(eq, var),
            Err(other) => Err(other),
        }
    }

    /// Solve the system as a system: substitution first, then elimination.
    ///
    /// One equation at a time would silently drop the variables the other
    /// equations pin down — `x = 1, y = x` needs both answers.
    fn solve_nonlinear(
        &self,
        equations: &[Expr],
        vars: &[SymbolId],
    ) -> Result<crate::solutions::SystemAnswer, SolverError> {
        // The elimination answers with a `Result`: a system it cannot reduce is
        // `NotImplemented`, which is a decline and not an empty answer — "I
        // cannot do this" and "there is no solution" are different statements.
        crate::nonlinear::solve_nonlinear(self, equations, vars)
    }

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
    use canlab_core::{Hold, add, expr::ExprNode, int, mul, pow, sym};

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
    fn solve_all_answers_a_negative_discriminant_with_complex_roots() {
        // `x² + 1 = 0` has no *real* solutions, and the complex ones are exact:
        // `±i`. This test used to assert `Solutions::None`, which was the
        // behaviour before W4.4 — the change is the point, and the assertion
        // moved with it.
        let equation = add([pow(x(), int(2)), int(1)]);
        let answer = NativeSolver.solve_all(&equation, var()).expect("an answer");
        let Solutions::Values(values) = answer else {
            panic!("expected two values, got {answer:?}");
        };
        assert_eq!(values.len(), 2, "{values:?}");
        let text = Solutions::Values(values).to_latex();
        assert!(text.contains("i"), "{text}");
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
    fn solve_all_answers_a_quartic_with_no_exact_root() {
        // x⁴ − x − 1 has **two** real roots and no exact form this backend can
        // name. It used to decline; the numeric fallback answers both, and
        // substituting each back is what makes them roots rather than guesses.
        let equation = add(vec![pow(x(), int(4)), mul(vec![int(-1), x()]), int(-1)]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("numeric roots");
        let Solutions::Union(members) = &solutions else {
            panic!("two real roots are a union, got {solutions:?}");
        };
        assert_eq!(members.len(), 2, "{members:?}");
        let program = canlab_core::Program::compile(&equation, &[var()]).expect("compiles");
        for member in members {
            let Solutions::Approximation { value, residual } = member else {
                panic!("a member is not an approximation: {member:?}");
            };
            assert!(*residual < 1e-9, "residual {residual}");
            let at = canlab_core::approx::value_of(value).expect("a number");
            let at_root = program.eval(&[at]);
            assert!(at_root.abs() < 1e-9, "f({at}) = {at_root}");
        }
    }

    #[test]
    fn solve_all_answers_a_cubic_with_only_irrational_roots() {
        // x³ − 2 = 0 is x = ∛2. The exact paths find no rational root and
        // decline, which used to be the whole answer; the scan finds the root
        // and the residual says how well it does.
        let equation = add(vec![pow(x(), int(3)), int(-2)]);
        let solutions = NativeSolver
            .solve_all(&equation, var())
            .expect("a numeric root");
        let Solutions::Approximation { value, residual } = &solutions else {
            panic!("expected an approximation, got {solutions:?}");
        };
        let at = canlab_core::approx::value_of(value).expect("a number");
        assert!((at - 2.0_f64.cbrt()).abs() < 1e-9, "got {at}");
        assert!(*residual < 1e-9, "residual {residual}");
    }

    #[test]
    fn solve_all_declines_when_the_scan_finds_no_root() {
        // sin x − x² − 5 is below −4 on the whole real line, so the scan finds
        // nothing in its interval. An empty scan is a decline, not an answer:
        // the nearest point is not a root.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![x()]),
            mul(vec![int(-1), pow(x(), int(2))]),
            int(-5),
        ]);
        let error = NativeSolver
            .solve_all(&equation, var())
            .expect_err("no root in the interval");
        assert_eq!(error, SolverError::NotImplemented);
        assert!(error.is_declined());
    }

    #[test]
    fn solve_all_does_not_turn_an_exact_answer_into_an_approximation() {
        // x² − 4 is exactly ±2. The numeric fallback runs only after the exact
        // paths decline, so this must stay a set of exact values rather than
        // becoming `2.0000`.
        let equation = add(vec![pow(x(), int(2)), int(-4)]);
        let solutions = NativeSolver.solve_all(&equation, var()).expect("exact roots");
        assert_eq!(solutions, Solutions::Values(vec![int(2), int(-2)]));
        assert!(
            !matches!(solutions, Solutions::Approximation { .. }),
            "an exact answer was approximated"
        );
    }

    #[test]
    fn solve_all_declines_an_equation_the_evaluator_cannot_compile() {
        // A held integral has no numerical value. The compile guard is what
        // turns that into a decline instead of an empty scan or a panic.
        let equation = ExprNode::hold(Hold::Integral {
            body: x(),
            var: var(),
        });
        let error = NativeSolver
            .solve_all(&equation, var())
            .expect_err("nothing to evaluate");
        assert_eq!(error, SolverError::NotImplemented);
    }

    #[test]
    fn a_scan_error_is_a_decline_and_never_a_root() {
        // 1/x − 2 has the root 1/2, but the scan interval contains the pole at
        // zero and the scan refuses the whole interval. That refusal must reach
        // the caller as a decline: answering the nearest finite point would be
        // a guessed root.
        let equation = add(vec![pow(x(), int(-1)), int(-2)]);
        let error = NativeSolver
            .solve_all(&equation, var())
            .expect_err("the pole is refused");
        assert_eq!(error, SolverError::NotImplemented);
    }
    #[test]
    fn a_negative_discriminant_answers_with_complex_roots() {
        use crate::Solver as _;
        let x = SymbolId::intern("x");
        // `x² + 1 = 0` is `±i`, exactly — not "no solutions".
        let equation = canlab_core::add([
            canlab_core::pow(canlab_core::sym("x"), ExprNode::int(2)),
            ExprNode::int(1),
        ]);
        let solutions = NativeSolver.solve_all(&equation, x).expect("an answer");
        let text = solutions.to_latex();
        assert!(text.contains("i"), "{text}");
        assert!(text.contains("-i"), "{text}");
        // `x² + 2x + 5 = 0` is `−1 ± 2i`: the division by `2` is done term by
        // term, so the answer is not left as `(−2 ± 4i)/2`.
        let equation = canlab_core::add([
            canlab_core::pow(canlab_core::sym("x"), ExprNode::int(2)),
            canlab_core::mul([ExprNode::int(2), canlab_core::sym("x")]),
            ExprNode::int(5),
        ]);
        let text = NativeSolver.solve_all(&equation, x).expect("an answer").to_latex();
        assert!(text.contains("-1 + 2i"), "{text}");
        assert!(text.contains("-1 - 2i"), "{text}");
        // A positive discriminant is unchanged: `x² − 4 = 0` is still `{−2, 2}`.
        let equation = canlab_core::add([
            canlab_core::pow(canlab_core::sym("x"), ExprNode::int(2)),
            ExprNode::int(-4),
        ]);
        let text = NativeSolver.solve_all(&equation, x).expect("an answer").to_latex();
        assert!(text.contains("-2") && text.contains("2"), "{text}");
        // `contains("i")` would match the `i` in `\right`; the check is for the
        // imaginary unit as a term, which is a bare `i` beside a digit or a
        // brace.
        assert!(
            !text.contains("+ i") && !text.contains("- i") && !text.contains("2i"),
            "a real answer must not mention i: {text}"
        );
    }

}
