//! Nonlinear systems: two equations in two unknowns, answered as *points*.
//!
//! A system can meet in more than one place. `x² + y² = 5` with `y = x + 1`
//! meets twice, and two circles meet twice as well — so the answer is a set of
//! points, not one assignment list. Returning the first point and calling it the
//! answer is the same mistake as returning one root of a quartic, which this
//! project treats as a bug.
//!
//! The method is elimination by subtraction and substitution, done on exact
//! expressions rather than through a Sylvester matrix:
//!
//! ```text
//! one equation linear in y       → solve it for y, substitute into the other
//! equal degree d in y            → (leading of one)·other − (leading of the other)·this
//!                                  which is lower degree in y, then repeat
//! what is left is one equation in x
//!     → solve it (the existing solver)
//!     → put each x back into an original equation and solve for y
//!     → check every point against *all* the original equations
//! ```
//!
//! `canlab-poly` has a Sylvester resultant, but it wants the two sides to be
//! *univariate* polynomials; with `Expr` coefficients the elimination it performs
//! collapses (observed: it returned the square of one equation's root and dropped
//! the other equation entirely). Subtracting the leading terms is the same idea
//! for the two-equation case and stays exact.
//!
//! The final check is not decoration. Elimination can introduce candidates that
//! solve the reduced equation and not the system — the same way combining
//! logarithms introduces roots outside their domain — so a point that does not
//! satisfy every original equation to [`TOLERANCE`] is dropped. If none survives,
//! the system declines rather than answering with a subset.

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, int, mul, pow};
use crate::native::NativeSolver;
use crate::solutions::SystemAnswer;
use crate::Solver as _;

/// How close to zero a residual must be for a point to count as a solution.
///
/// Absolute rather than relative: the equations a student writes have
/// coefficients of order one, and an absolute bound is the one that can be
/// explained. A system scaled by 1e9 would need the relative form, and that is a
/// limitation this constant admits rather than hides.
pub const TOLERANCE: f64 = 1e-8;

/// Distance below which two candidate points are the same point.
const SAME_POINT: f64 = 1e-9;

fn zero() -> Expr {
    ExprNode::int(0)
}

/// Does `expression` mention `variable`?
fn mentions(expression: &Expr, variable: SymbolId) -> bool {
    match &**expression {
        ExprNode::Num(_) => false,
        ExprNode::Sym(symbol) => *symbol == variable,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            terms.iter().any(|term| mentions(term, variable))
        }
        ExprNode::Pow(base, exponent) => {
            mentions(base, variable) || mentions(exponent, variable)
        }
        ExprNode::Fn(_, arguments) => arguments.iter().any(|argument| mentions(argument, variable)),
        // A held expression is not opened by this reader.
        ExprNode::Hold(_) => true,
    }
}

fn is_zero(expression: &Expr) -> bool {
    *expression == zero()
}

/// `symbol^k` for a non-negative `k`.
fn power_of(symbol: SymbolId, exponent: usize) -> Expr {
    match exponent {
        0 => ExprNode::int(1),
        1 => ExprNode::sym(symbol),
        _ => pow(ExprNode::sym(symbol), int(i64::try_from(exponent).ok().unwrap_or(1))),
    }
}

/// The coefficients of `expression` as a polynomial in `variable`, lowest power
/// first. `None` when a term is not `coefficient · variable^k`.
fn coefficients_in(expression: &Expr, variable: SymbolId) -> Option<Vec<Expr>> {
    let terms: Vec<&Expr> = match &**expression {
        ExprNode::Add(terms) => terms.iter().collect(),
        _ => vec![expression],
    };
    let mut out: Vec<Expr> = Vec::new();
    for term in terms {
        let (coefficient, power) = coefficient_and_power(term, variable)?;
        while out.len() <= power {
            out.push(zero());
        }
        out[power] = add(vec![out[power].clone(), coefficient]);
    }
    Some(out)
}

/// One term split into its `variable`-free coefficient and the power of `variable`.
fn coefficient_and_power(term: &Expr, variable: SymbolId) -> Option<(Expr, usize)> {
    // A term that does not mention the variable at all is a constant *with
    // respect to it* — `x²` is a coefficient when the variable is `y`. Without
    // this, every `x² + y² = r` reduces to "not a polynomial in y".
    if !mentions(term, variable) {
        return Some((term.clone(), 0));
    }
    match &**term {
        ExprNode::Sym(symbol) if *symbol == variable => Some((ExprNode::int(1), 1)),
        ExprNode::Sym(_) | ExprNode::Num(_) => Some((term.clone(), 0)),
        ExprNode::Pow(base, exponent) if **base == *ExprNode::sym(variable) => {
            let ExprNode::Num(Number::Int(k)) = &**exponent else {
                return None;
            };
            let power = usize::try_from(*k).ok()?;
            Some((ExprNode::int(1), power))
        }
        ExprNode::Mul(factors) => {
            let mut coefficient: Vec<Expr> = Vec::new();
            let mut power = 0usize;
            for factor in factors {
                match &**factor {
                    ExprNode::Sym(symbol) if *symbol == variable => power += 1,
                    ExprNode::Pow(base, exponent) if **base == *ExprNode::sym(variable) => {
                        let ExprNode::Num(Number::Int(k)) = &**exponent else {
                            return None;
                        };
                        power += usize::try_from(*k).ok()?;
                    }
                    _ => coefficient.push(factor.clone()),
                }
            }
            let coefficient = match coefficient.len() {
                0 => ExprNode::int(1),
                1 => coefficient.pop().expect("length checked"),
                _ => mul(coefficient),
            };
            Some((coefficient, power))
        }
        _ => None,
    }
}

fn degree_in(coefficients: &[Expr]) -> Option<usize> {
    coefficients
        .iter()
        .rposition(|coefficient| !is_zero(coefficient))
}

/// Substitute the root of a **linear** equation into `target`, times whatever
/// clears the denominator.
///
/// `linear` is `g1·y + g0 = 0`, so `y = −g0/g1`. Multiplying through by `g1^m`
/// keeps the result exact: no division appears in the equation that is then solved.
fn substitute_linear(target: &[Expr], linear: &[Expr]) -> Option<Expr> {
    let m = degree_in(target)?;
    let (g1, g0) = (linear.get(1)?.clone(), linear.first()?.clone());
    let mut terms: Vec<Expr> = Vec::new();
    for (k, coefficient) in target.iter().enumerate() {
        if is_zero(coefficient) {
            continue;
        }
        let numerator = pow(mul([int(-1), g0.clone()]), int(i64::try_from(k).ok()?));
        let denominator = pow(g1.clone(), int(i64::try_from(m - k).ok()?));
        terms.push(mul([coefficient.clone(), numerator, denominator]));
    }
    Some(add(terms))
}

/// Eliminate `variable` from a pair of equations, leaving one equation in the
/// other unknown.
fn eliminate(first: &Expr, second: &Expr, variable: SymbolId) -> Option<Expr> {
    let a = coefficients_in(first, variable)?;
    let b = coefficients_in(second, variable)?;
    let da = degree_in(&a);
    let db = degree_in(&b);

    // A side that does not mention the variable at all *is* the eliminated
    // equation, which is the case `x²+y²=4` against `(x−1)²+y²=4` reduces to.
    if da == Some(0) {
        return Some(first.clone());
    }
    if db == Some(0) {
        return Some(second.clone());
    }
    // One linear equation: solve it for the variable and put it in the other.
    if db == Some(1) && !is_zero(&b[1]) {
        return substitute_linear(&a, &b);
    }
    if da == Some(1) && !is_zero(&a[1]) {
        return substitute_linear(&b, &a);
    }
    // Equal degrees: the difference of the leading coefficients cancels the top
    // power, and the result mentions the variable to a lower degree.
    if da == db {
        let top = da?;
        let difference = add(vec![
            mul([a[top].clone(), second.clone()]),
            mul([int(-1), b[top].clone(), first.clone()]),
        ]);
        return eliminate(&difference, first, variable);
    }
    None
}

/// Substitute every known value into `expression` and evaluate it.
fn residual(expression: &Expr, point: &[(SymbolId, Expr)]) -> Option<f64> {
    let mut substituted = expression.clone();
    for (symbol, value) in point {
        substituted = crate::system::substitute(&substituted, *symbol, value);
    }
    canlab_core::approx::value_of(&substituted)
}

/// The numeric values of a one-variable answer, when they are numbers.
fn values_of(solutions: &crate::Solutions) -> Option<Vec<f64>> {
    match solutions {
        crate::Solutions::None => Some(Vec::new()),
        crate::Solutions::Values(values) => values
            .iter()
            .map(canlab_core::approx::value_of)
            .collect(),
        crate::Solutions::Approximation { value, .. } => {
            canlab_core::approx::value_of(value).map(|value| vec![value])
        }
        // A family or an interval is not a finite set of points, and a union is
        // read by the caller so it can say so rather than dropping members.
        _ => None,
    }
}

/// Remove duplicates that elimination produces.
fn deduplicated(points: Vec<Vec<(SymbolId, Expr)>>) -> Vec<Vec<(SymbolId, Expr)>> {
    let mut out: Vec<Vec<(SymbolId, Expr)>> = Vec::new();
    let mut seen: Vec<Vec<f64>> = Vec::new();
    for point in points {
        let coordinates: Option<Vec<f64>> = point
            .iter()
            .map(|(_, value)| canlab_core::approx::value_of(value))
            .collect();
        let Some(coordinates) = coordinates else {
            // A point with an exact-but-not-numeric coordinate is kept: dropping
            // it for failing to be a float would answer a different question.
            out.push(point);
            continue;
        };
        let duplicate = seen.iter().any(|other| {
            other.len() == coordinates.len()
                && other
                    .iter()
                    .zip(&coordinates)
                    .all(|(left, right)| (left - right).abs() < SAME_POINT)
        });
        if !duplicate {
            seen.push(coordinates);
            out.push(point);
        }
    }
    out
}

/// Solve two equations in two unknowns by elimination.
///
/// `None` when the shape is not one this method claims: not exactly two
/// unknowns, not two equations, or a pair that is not polynomial in the second
/// unknown.
pub fn solve_nonlinear(
    solver: &NativeSolver,
    equations: &[Expr],
    vars: &[SymbolId],
) -> Option<SystemAnswer> {
    if vars.len() != 2 || equations.len() < 2 {
        return None;
    }
    let (horizontal, vertical) = (vars[0], vars[1]);

    for first in 0..equations.len() {
        for second in (first + 1)..equations.len() {
            let Some(reduced) = eliminate(&equations[first], &equations[second], vertical) else {
                continue;
            };
            // The reduced equation must be about the other unknown; otherwise the
            // pair said nothing new and the system is not solved by this method.
            let mut symbols = Vec::new();
            crate::variables_of(&reduced).into_iter().for_each(|symbol| {
                if !symbols.contains(&symbol) {
                    symbols.push(symbol);
                }
            });
            if !symbols.contains(&horizontal) {
                continue;
            }
            let Ok(horizontal_solutions) = solver.solve_all(&reduced, horizontal) else {
                continue;
            };
            let Some(x_values) = values_of(&horizontal_solutions) else {
                continue;
            };
            if x_values.is_empty() {
                return Some(SystemAnswer::None);
            }

            let mut points: Vec<Vec<(SymbolId, Expr)>> = Vec::new();
            for x in x_values {
                let x_expr = ExprNode::num(Number::float(x));
                let at_x = crate::system::substitute(&equations[first], horizontal, &x_expr);
                let Ok(vertical_solutions) = solver.solve_all(&at_x, vertical) else {
                    continue;
                };
                let Some(y_values) = values_of(&vertical_solutions) else {
                    continue;
                };
                for y in y_values {
                    let mut point: Vec<(SymbolId, Expr)> = vec![
                        (horizontal, x_expr.clone()),
                        (vertical, ExprNode::num(Number::float(y))),
                    ];
                    point.sort_by_key(|(symbol, _)| *symbol);
                    points.push(point);
                }
            }

            // The check that makes the answer trustworthy: every point must
            // satisfy **every** original equation, not only the pair that
            // produced it.
            let checked: Vec<Vec<(SymbolId, Expr)>> = points
                .into_iter()
                .filter(|point| {
                    equations.iter().all(|equation| {
                        residual(equation, point).is_some_and(|value| value.abs() <= TOLERANCE)
                    })
                })
                .collect();
            let checked = deduplicated(checked);
            if checked.is_empty() {
                // Candidates were found and none survived: a refusal, not "no
                // solutions", because this method cannot tell a wrong candidate
                // from a system that genuinely does not meet.
                continue;
            }
            return Some(SystemAnswer::Points(checked));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::sym;

    fn x() -> SymbolId {
        SymbolId::intern("x")
    }
    fn y() -> SymbolId {
        SymbolId::intern("y")
    }

    /// `x² + y² − 1`, the unit circle.
    fn unit_circle() -> Expr {
        add(vec![pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-1)])
    }

    fn points(answer: &SystemAnswer) -> &Vec<Vec<(SymbolId, Expr)>> {
        match answer {
            SystemAnswer::Points(points) => points,
            other => panic!("expected points, got {other:?}"),
        }
    }

    #[test]
    fn a_circle_and_a_line_meet_twice() {
        // x² + y² = 5 and y = x + 1 → (−2, −1) and (1, 2).
        let circle = add(vec![pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-5)]);
        let line = add(vec![sym("y"), mul([int(-1), sym("x")]), int(-1)]);
        let answer = solve_nonlinear(&NativeSolver, &[circle, line], &[x(), y()])
            .expect("a circle and a line");
        let found = points(&answer);
        assert_eq!(found.len(), 2, "{found:?}");
        let mut xs: Vec<f64> = found
            .iter()
            .map(|point| canlab_core::approx::value_of(&point[0].1).expect("a number"))
            .collect();
        xs.sort_by(f64::total_cmp);
        assert!((xs[0] + 2.0).abs() < 1e-9, "{xs:?}");
        assert!((xs[1] - 1.0).abs() < 1e-9, "{xs:?}");
    }

    #[test]
    fn two_circles_meet_twice_and_the_answer_is_irrational() {
        // x² + y² = 4 and (x−1)² + y² = 4 → x = 1/2, y = ±√15/2.
        let first = add(vec![pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-4)]);
        let second = add(vec![
            pow(add(vec![sym("x"), int(-1)]), int(2)),
            pow(sym("y"), int(2)),
            int(-4),
        ]);
        let answer =
            solve_nonlinear(&NativeSolver, &[first, second], &[x(), y()]).expect("two circles");
        let found = points(&answer);
        assert_eq!(found.len(), 2, "{found:?}");
        for point in found {
            let x = canlab_core::approx::value_of(&point[0].1).expect("a number");
            assert!((x - 0.5).abs() < 1e-9, "{x}");
        }
    }

    #[test]
    fn circles_that_do_not_meet_report_none() {
        // x² + y² = 1 and x² + y² = 4 share no point.
        let inner = unit_circle();
        let outer = add(vec![pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-4)]);
        let answer = solve_nonlinear(&NativeSolver, &[inner, outer], &[x(), y()])
            .expect("a shape this method knows");
        assert!(
            matches!(answer, SystemAnswer::None),
            "expected none, got {answer:?}"
        );
    }

    #[test]
    fn a_candidate_that_does_not_satisfy_the_second_equation_is_dropped() {
        // y = 5 misses the unit circle; elimination still produces candidates,
        // and the check must drop them rather than reporting a point.
        let line = add(vec![sym("y"), int(-5)]);
        let answer = solve_nonlinear(&NativeSolver, &[unit_circle(), line], &[x(), y()]);
        assert!(answer.is_none(), "no point should survive: {answer:?}");
    }

    #[test]
    fn one_unknown_is_not_this_method() {
        assert!(solve_nonlinear(&NativeSolver, &[unit_circle()], &[x()]).is_none());
    }
}
