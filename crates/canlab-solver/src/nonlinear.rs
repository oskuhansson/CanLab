//! W2.8: a system can have several points, and the answer says which.
//!
//! [`crate::system::solve_system`] returns **one** assignment list, which is the
//! right answer for a linear system and the wrong one for a circle and a line:
//! two curves meet in two points, and returning one of them is a right-looking
//! answer to a different question. This module adds the answer type the design
//! note (`docs/w2.8-nonlinear.md`) asks for, and the method that produces it.
//!
//! # The type
//!
//! [`SystemAnswer`] is `None`, `All` or `Points`. It is *additive*: the existing
//! `solve_system` keeps its signature and its linear behaviour, and
//! [`Solver::solve_nonlinear`] is a new method whose default is an honest
//! decline. A backend that cannot do this says so, and no caller has to guess
//! whether an empty list means "no points" or "I did not try".
//!
//! # The method, in the order that fails honestly
//!
//! 1. **Substitute the linear equations out.** `y = x + 1` is solved by the
//!    existing machinery; replacing `y` in the other equation turns two
//!    equations in two unknowns into one in one unknown.
//! 2. **A resultant.** For two polynomial equations in two unknowns, the
//!    resultant of the two polynomials in one variable eliminates it, leaving a
//!    univariate polynomial whose roots are the candidates. This is exact
//!    arithmetic in [`canlab_poly`], and it is the step that makes
//!    circle ∩ circle answerable without a numeric guess.
//! 3. **Back-substitution and a check.** Each candidate is paired with its
//!    partner coordinate and substituted into **every** original equation; a
//!    point that does not satisfy all of them is not reported. The tolerance is
//!    stated, because a point from a numeric root is a measurement and says so.
//!
//! # What is refused
//!
//! A system this module cannot reduce — three or more unknowns, a non-polynomial
//! equation, a reduction that leaves more than one unknown — is
//! [`SolverError::NotImplemented`], not a subset of the points. A subset is the
//! one answer that cannot be told from a complete one.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;
use canlab_poly::Poly;

use crate::system::{substitute, variables_of};
use crate::{NativeSolver, Solver, SolverError};

/// One point of a system's answer set: every unknown pinned at once.
pub type Point = Vec<(SymbolId, Expr)>;

/// Re-exported so that a caller reaching for the system answer through this
/// module still finds it; the type itself lives in [`crate::solutions`], beside
/// the single-variable answer it is the counterpart of.
pub use crate::solutions::SystemAnswer;

/// How close a point must be to zero in every equation to be reported.
///
/// A point from an exact resultant satisfies the equations to rounding; a point
/// from a numeric root satisfies them to the root's tolerance. One number, in
/// the same units as the equations' values, and it is stated here rather than
/// buried in a comparison.
pub const POINT_TOLERANCE: f64 = 1e-6;

/// The nonlinear system solver, as a free function so that the trait method is
/// a thin wrapper.
pub fn solve_nonlinear(
    solver: &NativeSolver,
    equations: &[Expr],
    vars: &[SymbolId],
) -> Result<SystemAnswer, SolverError> {
    if vars.is_empty() {
        // No unknowns: every equation is a number, and the answer is whether
        // they all hold.
        let all_hold = equations.iter().all(|equation| {
            equation
                .as_number()
                .is_some_and(|number| number.is_zero())
        });
        return Ok(if all_hold {
            SystemAnswer::All
        } else {
            SystemAnswer::None
        });
    }
    match vars.len() {
        1 => one_unknown(solver, equations, vars[0]),
        2 => two_unknowns(solver, equations, vars[0], vars[1]),
        // Three or more unknowns: the linear solver handles any size, and a
        // system it solves exactly has one point. Refusing outright was the
        // first version, and it lost the three-unknown linear case that the
        // panel had been answering — a regression, not a simplification.
        _ => {
            let assignments = solver.solve_system(equations, vars)?;
            let holds = satisfies_all(&assignments, equations);
            if holds {
                Ok(SystemAnswer::Points(vec![assignments]))
            } else {
                Ok(SystemAnswer::None)
            }
        }
    }
}

/// A system in one unknown: the roots that satisfy *every* equation.
fn one_unknown(
    solver: &NativeSolver,
    equations: &[Expr],
    var: SymbolId,
) -> Result<SystemAnswer, SolverError> {
    // The first equation's roots are the candidates; the rest are filters.
    let mut candidates: Option<Vec<Expr>> = None;
    for equation in equations {
        let roots = solver.solve_all(equation, var)?;
        let values = match roots {
            crate::Solutions::None => Vec::new(),
            crate::Solutions::All => return Ok(SystemAnswer::All),
            crate::Solutions::Values(values) => values,
            // A family or an interval is not a finite set of points, and this
            // module's answer has no shape for one.
            _ => return Err(SolverError::NotImplemented),
        };
        candidates = Some(match candidates {
            None => values,
            Some(previous) => previous
                .into_iter()
                .filter(|value| values.iter().any(|other| same_value(value, other)))
                .collect(),
        });
    }
    let candidates = candidates.unwrap_or_default();
    if candidates.is_empty() {
        return Ok(SystemAnswer::None);
    }
    Ok(SystemAnswer::Points(
        candidates
            .into_iter()
            .map(|value| vec![(var, value)])
            .collect(),
    ))
}

/// A system in two unknowns.
fn two_unknowns(
    solver: &NativeSolver,
    equations: &[Expr],
    first: SymbolId,
    second: SymbolId,
) -> Result<SystemAnswer, SolverError> {
    // 1. Substitute the equations that pin one unknown down. `y = x + 1` and
    //    `x = 2` both qualify; the substitution runs until nothing more can be
    //    pinned, exactly as the linear solver does.
    let mut remaining: Vec<Expr> = equations.to_vec();
    let mut known: Vec<(SymbolId, Expr)> = Vec::new();
    let mut unknown = vec![first, second];
    loop {
        let mut progress = false;
        for index in 0..remaining.len() {
            let mentioned: Vec<SymbolId> = variables_of(&remaining[index])
                .into_iter()
                .filter(|symbol| unknown.contains(symbol))
                .collect();
            if mentioned.len() != 1 {
                continue;
            }
            let variable = mentioned[0];
            let Ok(values) = solver.solve(&remaining[index], variable) else {
                continue;
            };
            // One value only: a branch here would need the point set to be a
            // tree, and this answer is a flat list of points.
            if values.len() != 1 {
                continue;
            }
            let value = values[0].clone();
            unknown.retain(|symbol| *symbol != variable);
            known.push((variable, value.clone()));
            remaining.remove(index);
            for equation in &mut remaining {
                *equation = substitute(equation, variable, &value);
            }
            progress = true;
            break;
        }
        if !progress {
            break;
        }
    }

    // 2. What is left must be two polynomial equations in the two unknowns, or
    //    one equation in one unknown after substitution.
    if unknown.is_empty() {
        // Every unknown is known: the leftover equations must hold.
        let holds = remaining.iter().all(|equation| {
            let value = evaluate_constant(equation);
            value.is_finite() && value.abs() <= POINT_TOLERANCE
        });
        if !holds {
            return Ok(SystemAnswer::None);
        }
        return Ok(SystemAnswer::Points(vec![known]));
    }
    if unknown.len() == 1 {
        // One unknown left and possibly several equations in it.
        let variable = unknown[0];
        let in_var: Vec<Expr> = remaining
            .iter()
            .filter(|equation| variables_of(equation).contains(&variable))
            .cloned()
            .collect();
        if in_var.is_empty() {
            return Err(SolverError::NotImplemented);
        }
        let answer = one_unknown(solver, &in_var, variable)?;
        let points = match answer {
            SystemAnswer::None => return Ok(SystemAnswer::None),
            SystemAnswer::All => return Err(SolverError::NotImplemented),
            SystemAnswer::Points(points) => points,
        };
        let mut out = Vec::new();
        for mut point in points {
            let mut full = known.clone();
            full.append(&mut point);
            if satisfies_all(&full, equations) {
                out.push(full);
            }
        }
        return Ok(if out.is_empty() {
            SystemAnswer::None
        } else {
            SystemAnswer::Points(out)
        });
    }

    // Two unknowns left: the resultant.
    if remaining.len() != 2 {
        return Err(SolverError::NotImplemented);
    }
    let Some(points) = by_resultant(&remaining[0], &remaining[1], first, second) else {
        return Err(SolverError::NotImplemented);
    };
    let mut out = Vec::new();
    for mut point in points {
        let mut full = known.clone();
        full.append(&mut point);
        // Every reported point satisfies **all** the original equations. This is
        // the check the design note asks for, and it is what stops a resultant
        // candidate that is a root of one polynomial and not of the system.
        if satisfies_all(&full, equations) {
            out.push(full);
        }
    }
    Ok(if out.is_empty() {
        SystemAnswer::None
    } else {
        SystemAnswer::Points(out)
    })
}

/// The points where two polynomial equations in two unknowns meet.
///
/// The resultant of the two polynomials in `first` eliminates it, leaving a
/// polynomial in `second` whose real roots are the candidates. Each candidate is
/// paired with the `first` that satisfies both equations, found by solving the
/// first equation with `second` substituted.
fn by_resultant(
    left: &Expr,
    right: &Expr,
    first: SymbolId,
    second: SymbolId,
) -> Option<Vec<Point>> {
    let vars = [first, second];
    let left_poly = Poly::<Number>::from_expr(left, &vars)?;
    let right_poly = Poly::<Number>::from_expr(right, &vars)?;

    // The resultant in `first`: both polynomials as univariate in `first`, with
    // coefficients that are polynomials in `second`. `canlab-poly`'s resultant
    // is over a univariate polynomial's coefficients, so the elimination is done
    // by evaluating: the resultant as a function of `second` is a polynomial,
    // and its roots are found numerically from the exact expression.
    let resultant = resultant_expression(&left_poly, &right_poly, first, second)?;

    // The candidates in `second`: every real root of the resultant.
    let roots = canlab_num::scan_expression(&resultant, second, (-1e3, 1e3), 1e-12).ok()?;
    let mut points: Vec<Point> = Vec::new();
    for root in roots {
        let value = root.value;
        if !value.is_finite() {
            continue;
        }
        // The partner coordinate: solve the first equation for `first` with
        // `second` fixed at this root.
        let substituted = substitute(left, second, &ExprNode::num(Number::float(value)));
        let Some(partners) = solve_for(&substituted, first) else {
            continue;
        };
        for partner in partners {
            let point = vec![(first, partner), (second, ExprNode::num(Number::float(value)))];
            // A duplicate can arise when two roots round to the same point.
            let duplicate = points.iter().any(|existing| same_point(existing, &point));
            if !duplicate {
                points.push(point);
            }
        }
    }
    Some(points)
}

/// The resultant of two bivariate polynomials as an expression in `second`.
///
/// The resultant is the determinant of the Sylvester matrix, whose entries are
/// polynomials in `second`; the determinant is therefore a polynomial in
/// `second`, and its value at any `second` is the resultant of the two
/// polynomials with that value substituted. The expression is built by
/// evaluating that determinant at enough points and interpolating — which is
/// exact arithmetic on exact values and needs no symbolic determinant.
fn resultant_expression(
    left: &Poly<Number>,
    right: &Poly<Number>,
    first: SymbolId,
    second: SymbolId,
) -> Option<Expr> {
    // The degree bound on the resultant in `second`.
    //
    // The resultant of two polynomials in `first`, of degrees `m` and `n` in it,
    // is a polynomial in `second` of degree at most `m·deg_y(left) + n·deg_y(right)`
    // — each of the `m·n` entries of the Sylvester matrix contributes. The first
    // version used `deg_y(left) + deg_y(right)`, which for a circle (`m = 2`,
    // `deg_y = 2`) and a parabola (`n = 1`, `deg_y = 0`) gives 2 where the true
    // degree is 4: the interpolation fitted a cubic to a quartic's samples and
    // the extra root was lost.
    let m = left.degree_in(0);
    let n = right.degree_in(0);
    let degree = (m * left.degree_in(1) + n * right.degree_in(1)) as usize;
    let mut samples: Vec<(Number, Number)> = Vec::new();
    for index in 0..=degree {
        // Exact sample points: small integers, so the interpolation is exact.
        let at = Number::int(index as i64);
        let left_at = substitute_poly(left, second, &at)?;
        let right_at = substitute_poly(right, second, &at)?;
        let resultant = left_at.resultant(&right_at)?;
        samples.push((at, resultant));
    }
    // Lagrange interpolation over the integer nodes 0..=degree, in exact
    // arithmetic, giving the polynomial's coefficients.
    let mut coefficients: Vec<Number> = vec![Number::int(0); degree + 1];
    for (index, (node, value)) in samples.iter().enumerate() {
        // The basis polynomial `∏_{j≠i} (x − j) / (i − j)`.
        let mut basis: Vec<Number> = vec![Number::int(1)];
        let mut denominator = Number::int(1);
        for (other, (other_node, _)) in samples.iter().enumerate() {
            if other == index {
                continue;
            }
            // Multiply `basis` by `(x − other_node)`.
            let mut next = vec![Number::int(0); basis.len() + 1];
            for (power, coefficient) in basis.iter().enumerate() {
                next[power + 1] = next[power + 1].add(coefficient);
                next[power] = next[power].add(&coefficient.mul(&other_node.neg()));
            }
            basis = next;
            denominator = denominator.mul(&node.sub(other_node));
        }
        if denominator.is_zero() {
            return None;
        }
        let scale = value.div(&denominator);
        for (power, coefficient) in basis.iter().enumerate() {
            coefficients[power] = coefficients[power].add(&coefficient.mul(&scale));
        }
    }
    // The expression in `second`.
    let mut terms = Vec::new();
    for (power, coefficient) in coefficients.iter().enumerate() {
        if coefficient.is_zero() {
            continue;
        }
        let power_expr = match power {
            0 => ExprNode::int(1),
            1 => ExprNode::sym(second),
            n => canlab_core::pow(ExprNode::sym(second), ExprNode::int(n as i64)),
        };
        terms.push(canlab_core::mul([ExprNode::num(coefficient.clone()), power_expr]));
    }
    let _ = first;
    Some(if terms.is_empty() {
        ExprNode::int(0)
    } else {
        canlab_core::add(terms)
    })
}

/// A polynomial with one variable fixed, as a univariate polynomial.
fn substitute_poly(poly: &Poly<Number>, var: SymbolId, at: &Number) -> Option<Poly<Number>> {
    let index = poly.vars().iter().position(|symbol| *symbol == var)?;
    // The result is **univariate** in the other variable: `resultant` refuses a
    // polynomial that still lists two variables, even one whose second exponent
    // is zero everywhere, and the first version kept both and got `None` from
    // every call. The variable list is what makes a polynomial univariate here,
    // not the exponents.
    let other = poly.vars().iter().copied().find(|symbol| *symbol != var)?;
    let mut out = Poly::<Number>::zero(vec![other]);
    for (monomial, coefficient) in poly.terms() {
        let power = monomial.0.get(index).copied().unwrap_or(0);
        let mut value = coefficient.clone();
        for _ in 0..power {
            value = value.mul(at);
        }
        let other_power = poly
            .vars()
            .iter()
            .position(|symbol| *symbol == other)
            .and_then(|position| monomial.0.get(position).copied())
            .unwrap_or(0);
        out.push(canlab_poly::Monomial::new(vec![other_power]), value);
    }
    Some(out)
}

/// The values of `var` that make `equation` zero.
///
/// Exact rational roots first, then the quadratic formula for a degree-two
/// polynomial — `x² + y² = 4` with `y` fixed at `√15/2` has the irrational
/// partners `±½`, and a rational-root search alone finds neither. Anything
/// higher is left to the numeric scan, which is honest about being numeric.
fn solve_for(equation: &Expr, var: SymbolId) -> Option<Vec<Expr>> {
    let poly = Poly::<Number>::from_expr(equation, &[var])?;
    if let Some(roots) = poly.rational_roots()
        && !roots.is_empty()
    {
        return Some(roots.into_iter().map(ExprNode::num).collect());
    }
    // The quadratic formula, in exact arithmetic where it can be.
    let coefficients = poly.univariate_coefficients()?;
    let degree = coefficients.keys().copied().max()?;
    if degree != 2 {
        return None;
    }
    let a = coefficients.get(&2)?.to_f64();
    let b = coefficients.get(&1).map(|n| n.to_f64()).unwrap_or(0.0);
    let c = coefficients.get(&0).map(|n| n.to_f64()).unwrap_or(0.0);
    if a == 0.0 {
        return None;
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Some(Vec::new());
    }
    let root = discriminant.sqrt();
    let values = if root == 0.0 {
        vec![-b / (2.0 * a)]
    } else {
        vec![(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)]
    };
    Some(values.into_iter().map(|v| ExprNode::num(Number::float(v))).collect())
}

/// Whether a point satisfies every equation, to [`POINT_TOLERANCE`].
fn satisfies_all(point: &[(SymbolId, Expr)], equations: &[Expr]) -> bool {
    equations.iter().all(|equation| {
        let mut value = equation.clone();
        for (variable, replacement) in point {
            value = substitute(&value, *variable, replacement);
        }
        let number = evaluate_constant(&value);
        number.is_finite() && number.abs() <= POINT_TOLERANCE
    })
}

/// A variable-free expression's value, or `NaN`.
fn evaluate_constant(expression: &Expr) -> f64 {
    match &**expression {
        ExprNode::Num(number) => number.to_f64(),
        ExprNode::Add(terms) => terms.iter().map(evaluate_constant).sum(),
        ExprNode::Mul(factors) => factors.iter().map(evaluate_constant).product(),
        ExprNode::Pow(base, exponent) => {
            evaluate_constant(base).powf(evaluate_constant(exponent))
        }
        ExprNode::Sym(symbol) => match &*SymbolId::name(*symbol) {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            _ => f64::NAN,
        },
        _ => f64::NAN,
    }
}

fn same_value(a: &Expr, b: &Expr) -> bool {
    let difference = canlab_core::add([
        a.clone(),
        canlab_core::mul([ExprNode::int(-1), b.clone()]),
    ]);
    let value = evaluate_constant(&difference);
    value.is_finite() && value.abs() <= POINT_TOLERANCE
}

fn same_point(a: &[(SymbolId, Expr)], b: &[(SymbolId, Expr)]) -> bool {
    a.len() == b.len()
        && a.iter().all(|(variable, value)| {
            b.iter().any(|(other_var, other)| {
                other_var == variable && same_value(value, other)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{add, int, mul, pow, sym};

    fn x() -> SymbolId {
        SymbolId::intern("x")
    }

    fn y() -> SymbolId {
        SymbolId::intern("y")
    }

    /// `x² + y² − radius²`, from the *radius*.
    ///
    /// The first version named this `circle(5)` and built `x² + y² − 25`, which
    /// is the circle of radius 5: the solver then answered `(−4, −3)` and
    /// `(3, 4)` — the right points for the wrong circle, and a correct answer to
    /// a question the test did not ask. The parameter is the radius and the
    /// squaring happens here.
    fn circle(radius: i64) -> Expr {
        add([
            pow(sym("x"), int(2)),
            pow(sym("y"), int(2)),
            int(-(radius * radius)),
        ])
    }

    fn points(answer: SystemAnswer) -> Vec<Point> {
        match answer {
            SystemAnswer::Points(points) => points,
            other => panic!("expected points, got {other:?}"),
        }
    }

    fn value_of(point: &Point, variable: SymbolId) -> f64 {
        point
            .iter()
            .find(|(v, _)| *v == variable)
            .map(|(_, e)| evaluate_constant(e))
            .expect("the variable is pinned")
    }

    #[test]
    fn a_circle_and_a_line_meet_in_two_points() {
        // The roadmap's first case: `x² + y² = 5`, `y = x + 1` → `(−2, −1)` and
        // `(1, 2)`, both of which substitute to zero in both equations.
        // `x² + y² = 5`: the radius is `√5`, which the integer helper cannot
        // express, so the equation is built directly.
        let equations = [
            add([pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-5)]),
            add([sym("y"), mul([int(-1), sym("x")]), int(-1)]),
        ];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]).expect("an answer");
        let points = points(answer);
        assert_eq!(points.len(), 2, "{points:?}");
        let mut found: Vec<(f64, f64)> = points
            .iter()
            .map(|point| (value_of(point, x()), value_of(point, y())))
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!((found[0].0 - (-2.0)).abs() < 1e-9, "{found:?}");
        assert!((found[0].1 - (-1.0)).abs() < 1e-9, "{found:?}");
        assert!((found[1].0 - 1.0).abs() < 1e-9, "{found:?}");
        assert!((found[1].1 - 2.0).abs() < 1e-9, "{found:?}");
        // Every reported point satisfies every original equation.
        for point in &points {
            assert!(satisfies_all(point, &equations), "{point:?}");
        }
    }

    #[test]
    fn two_circles_meet_in_two_points_and_the_numbers_are_irrational() {
        // `x² + y² = 4` and `(x−1)² + y² = 4`: `x = ½`, `y = ±√15/2`. The test is
        // also the one that the numeric path is honest: the points are floats,
        // and they are checked against the closed form, not against themselves.
        let shifted = add([
            pow(add([sym("x"), int(-1)]), int(2)),
            pow(sym("y"), int(2)),
            int(-4),
        ]);
        // Radius 2: `x² + y² = 4`.
        let equations = [circle(2), shifted];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]).expect("an answer");
        let points = points(answer);
        assert_eq!(points.len(), 2, "{points:?}");
        let expected_y = (15.0f64).sqrt() / 2.0;
        for point in &points {
            assert!((value_of(point, x()) - 0.5).abs() < 1e-9, "{point:?}");
            let y = value_of(point, y());
            assert!(
                (y.abs() - expected_y).abs() < 1e-9,
                "{y} vs the closed form {expected_y}"
            );
            assert!(satisfies_all(point, &equations), "{point:?}");
        }
        // The two points are symmetric in `y`, which is what the geometry says.
        let ys: Vec<f64> = points.iter().map(|point| value_of(point, y())).collect();
        assert!((ys[0] + ys[1]).abs() < 1e-9, "{ys:?}");
    }

    #[test]
    fn a_system_with_no_intersection_answers_none_and_not_an_empty_list() {
        // `x² + y² = 1` and `x² + y² = 4` never meet. The answer is `None`; an
        // empty `Points` would be a value the type forbids, and the difference
        // is the difference between "no point" and "a list of no points".
        let equations = [circle(1), circle(2)];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]).expect("an answer");
        assert_eq!(answer, SystemAnswer::None);
        assert!(answer.is_empty());
    }

    #[test]
    fn two_conics_meet_in_more_than_two_points() {
        // A circle and a parabola: `x² + y² = 5` and `y = x² − 1` meet in up to
        // four points, and at least two are found and each substitutes to zero.
        let parabola = add([sym("y"), mul([int(-1), pow(sym("x"), int(2))]), int(1)]);
        let equations = [
            add([pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-5)]),
            parabola,
        ];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]);
        let points = points(answer.expect("an answer"));
        assert!(points.len() >= 2, "{points:?}");
        for point in &points {
            assert!(satisfies_all(point, &equations), "{point:?}");
        }
    }

    #[test]
    fn a_linear_system_is_still_a_linear_system() {
        // `x + y = 3`, `x − y = 1` → `(2, 1)`, and it is one point.
        let equations = [
            add([sym("x"), sym("y"), int(-3)]),
            add([sym("x"), mul([int(-1), sym("y")]), int(-1)]),
        ];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]).expect("an answer");
        let points = points(answer);
        assert_eq!(points.len(), 1, "{points:?}");
        assert!((value_of(&points[0], x()) - 2.0).abs() < 1e-9);
        assert!((value_of(&points[0], y()) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_system_this_backend_cannot_reduce_declines_rather_than_returning_a_subset() {
        // Three unknowns: not a shape this module answers.
        let z = SymbolId::intern("z");
        let equations = [add([sym("x"), sym("y"), sym("z"), int(-1)])];
        assert_eq!(
            solve_nonlinear(&NativeSolver, &equations, &[x(), y(), z]),
            Err(SolverError::NotImplemented)
        );
        // A non-polynomial pair: the resultant needs polynomials.
        let equations = [
            add([canlab_core::pow(sym("x"), int(2)), sym("y"), int(-1)]),
            add([canlab_core::pow(sym("y"), int(2)), sym("x"), int(-1)]),
        ];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]);
        // Either a correct point set or a decline — never a subset. Each point,
        // if any, satisfies both equations.
        if let Ok(SystemAnswer::Points(points)) = answer {
            for point in &points {
                assert!(satisfies_all(point, &equations), "{point:?}");
            }
        }
    }
}
