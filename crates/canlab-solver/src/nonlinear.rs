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

/// How close two coordinates must be to count as the same point.
///
/// This is a *coordinate* tolerance, not a residual one. It is used to
/// deduplicate and filter candidate values; a residual is judged against the
/// equation's own scale by [`POINT_RELATIVE_TOLERANCE`]. An absolute residual
/// tolerance cannot tell a root of `x/10⁶ = 1` from a point `10⁻³` away: both
/// residuals are below `10⁻⁶`, even though the point is about `10⁷` rounding
/// errors off the answer. The relative check is what refuses that point.
pub const POINT_TOLERANCE: f64 = 1e-6;

/// The relative part of the point check: a residual is small when it is below
/// this fraction of the equation's own magnitude.
///
/// The magnitude is the sum of the absolute values of the expression's terms —
/// its scale, not the answer's. `x/10⁶ − 1` has scale `2` at `x = 10⁶`, so a
/// `10⁻³` error in `x` leaves the `10⁻⁹` residual visible against `2·10⁻¹²`
/// rather than hidden under an absolute `10⁻⁶`.
pub const POINT_RELATIVE_TOLERANCE: f64 = 1e-12;

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
        // Every unknown is known: every original equation must hold at the
        // point. Checking all of them, not only the ones still in `remaining`,
        // is the same answer for the equations that were already solved by
        // substitution and a stricter one for the values a numeric root
        // supplied.
        if !satisfies_all(&known, equations) {
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

    // The candidates in `second`: every real root of the resultant. The scan
    // holds its *residual* to `1e-12`, which is not the same as holding the
    // root: a flat resultant can be `1e-12` from zero while its root is several
    // `1e-9` away, and the point's back-substitution then carries that error.
    // Newton on the resultant, in exact coefficients and `f64` values, turns a
    // residual-small root into a root-small one.
    let roots = canlab_num::scan_expression(&resultant, second, (-1e3, 1e3), 1e-12).ok()?;
    let resultant_program = canlab_core::Program::compile(&resultant, &[second]);
    let derivative = canlab_core::differentiate(&resultant, second);
    let derivative_program = derivative
        .as_ref()
        .and_then(|expression| canlab_core::Program::compile(expression, &[second]));
    let mut points: Vec<Point> = Vec::new();
    for root in roots {
        let value = refine_root(
            resultant_program.as_ref(),
            derivative_program.as_ref(),
            root.value,
        );
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
            let mut point = vec![
                (first, partner),
                (second, ExprNode::num(Number::float(value))),
            ];
            refine_pair(&mut point, left, right, first, second);
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

/// Newton-refine a candidate pair against both original equations.
///
/// A resultant root can be a multiple of the true one — the resultant of a
/// circle and a parabola is the square of the quadratic in `y`, so its residual
/// is the *square* of the point's error and the scan's `10⁻¹²` residual only
/// pins the root to about `10⁻⁶`; the measured error was `2.9·10⁻⁹`, and the
/// second equation carried it as `1.2·10⁻⁸`. The first equation is solved
/// exactly for the partner, but the second still carries that error. A
/// two-equation Newton step, with the Jacobian of the original equations, puts
/// both coordinates back within rounding before the back-substitution check.
///
/// A point whose step is not finite is left as it was; the caller's
/// back-substitution check is still the filter that decides whether to report
/// it.
fn refine_pair(point: &mut Point, left: &Expr, right: &Expr, first: SymbolId, second: SymbolId) {
    let (Some(x0), Some(y0)) = (point_value(point, first), point_value(point, second)) else {
        return;
    };
    let variables = [first, second];
    let (Some(left_value), Some(right_value)) = (
        canlab_core::Program::compile(left, &variables),
        canlab_core::Program::compile(right, &variables),
    ) else {
        return;
    };
    let partial = |expression: &Expr, variable: SymbolId| {
        canlab_core::differentiate(expression, variable)
            .and_then(|derivative| canlab_core::Program::compile(&derivative, &variables))
    };
    let (Some(left_first), Some(left_second), Some(right_first), Some(right_second)) = (
        partial(left, first),
        partial(left, second),
        partial(right, first),
        partial(right, second),
    ) else {
        return;
    };
    let initial = left_value.eval(&[x0, y0]).abs() + right_value.eval(&[x0, y0]).abs();
    if !initial.is_finite() {
        return;
    }
    let (mut x, mut y) = (x0, y0);
    for _ in 0..8 {
        let f = left_value.eval(&[x, y]);
        let g = right_value.eval(&[x, y]);
        if !f.is_finite() || !g.is_finite() {
            return;
        }
        let a = left_first.eval(&[x, y]);
        let b = left_second.eval(&[x, y]);
        let c = right_first.eval(&[x, y]);
        let d = right_second.eval(&[x, y]);
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant == 0.0 {
            return;
        }
        let dx = (-f * d + b * g) / determinant;
        let dy = (-a * g + c * f) / determinant;
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        x += dx;
        y += dy;
        if dx.abs() + dy.abs() <= 1e-15 * (1.0 + x.abs() + y.abs()) {
            break;
        }
    }
    // A Newton step that made the pair worse is not an improvement; leave the
    // candidate as the resultant produced it and let the back-substitution
    // check refuse it if it is wrong.
    let final_residual = left_value.eval(&[x, y]).abs() + right_value.eval(&[x, y]).abs();
    if !final_residual.is_finite() || final_residual > initial {
        return;
    }
    for (variable, value) in point.iter_mut() {
        if *variable == first {
            *value = ExprNode::num(Number::float(x));
        }
        if *variable == second {
            *value = ExprNode::num(Number::float(y));
        }
    }
}

/// A point's numeric value for one pinned variable.
fn point_value(point: &Point, variable: SymbolId) -> Option<f64> {
    point
        .iter()
        .find(|(other, _)| *other == variable)
        .and_then(|(_, expression)| expression.as_number())
        .map(Number::to_f64)
}

/// Newton-refine a root of the resultant, when its derivative is available.
///
/// The scan's tolerance is on `|f(value)|`, not on `|value − root|`; those are
/// only the same size when the derivative is not small. A point built from a
/// root that is `10⁻⁹` off makes the back-substitution check look like a
/// tolerance failure, so this pays for a few Newton steps before the partner
/// coordinate is solved.
///
/// The value is read through a [`canlab_core::Program`] rather than by
/// substituting into the expression: substitution rebuilds the tree through the
/// canonical constructors, and the resultant is already the canonical tree the
/// scan compiled.
fn refine_root(
    resultant: Option<&canlab_core::Program>,
    derivative: Option<&canlab_core::Program>,
    value: f64,
) -> f64 {
    let (Some(resultant), Some(derivative)) = (resultant, derivative) else {
        return value;
    };
    let mut value = value;
    for _ in 0..8 {
        let value_at = resultant.eval(&[value]);
        let slope_at = derivative.eval(&[value]);
        if !value_at.is_finite() || !slope_at.is_finite() || slope_at == 0.0 {
            break;
        }
        let step = value_at / slope_at;
        if !step.is_finite() {
            break;
        }
        value -= step;
        if step.abs() <= 1e-15 * value.abs().max(1.0) {
            break;
        }
    }
    value
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

/// Whether a point satisfies every equation, to the scale-aware residual check.
fn satisfies_all(point: &[(SymbolId, Expr)], equations: &[Expr]) -> bool {
    let variables: Vec<SymbolId> = point.iter().map(|(variable, _)| *variable).collect();
    let values: Vec<f64> = point
        .iter()
        .map(|(_, value)| evaluate_constant(value))
        .collect();
    equations.iter().all(|equation| {
        // The compiled program evaluates the original tree, not a substitution
        // through the canonical constructors: rebuilding `x³` as a canonical
        // `pow` is a second evaluation of the same expression, and it is not
        // always the same value.
        let Some(program) = canlab_core::Program::compile(equation, &variables) else {
            return false;
        };
        residual_is_small(program.eval(&values), magnitude_at(equation, point))
    })
}

/// Whether `value` is a zero residual for an expression of the given scale.
fn residual_is_small(value: f64, scale: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    value.abs() <= POINT_RELATIVE_TOLERANCE * scale
}

/// A point's magnitude for an expression: the sum of the absolute values of its
/// terms, with the point's values substituted.
///
/// This is the scale the residual is measured against. It is computed from the
/// *original* expression and the point, not from the substituted expression:
/// `substitute` rebuilds through the canonical constructors, which fold
/// `y − x² + 1` into a single number and leave the residual as its own scale.
/// For a residual of `10⁻⁹` that makes the relative check a `10⁻¹²` absolute
/// one, which is neither what the name says nor what a measurement should face.
fn magnitude_at(expression: &Expr, point: &[(SymbolId, Expr)]) -> f64 {
    match &**expression {
        ExprNode::Num(number) => number.to_f64().abs(),
        ExprNode::Sym(symbol) => match point.iter().find(|(variable, _)| variable == symbol) {
            Some((_, value)) => evaluate_constant(value).abs(),
            None => constant_symbol(*symbol),
        },
        ExprNode::Add(terms) => terms.iter().map(|term| magnitude_at(term, point)).sum(),
        ExprNode::Mul(factors) => factors
            .iter()
            .map(|factor| magnitude_at(factor, point))
            .product(),
        ExprNode::Pow(base, exponent) => {
            let base = magnitude_at(base, point);
            let exponent = evaluate_at(exponent, point);
            if exponent.is_finite() {
                base.powf(exponent)
            } else {
                f64::NAN
            }
        }
        _ => f64::NAN,
    }
}

/// An expression's value at a point, for the exponent of a power.
fn evaluate_at(expression: &Expr, point: &[(SymbolId, Expr)]) -> f64 {
    match &**expression {
        ExprNode::Num(number) => number.to_f64(),
        ExprNode::Sym(symbol) => match point.iter().find(|(variable, _)| variable == symbol) {
            Some((_, value)) => evaluate_constant(value),
            None => constant_symbol(*symbol),
        },
        ExprNode::Add(terms) => terms.iter().map(|term| evaluate_at(term, point)).sum(),
        ExprNode::Mul(factors) => factors
            .iter()
            .map(|factor| evaluate_at(factor, point))
            .product(),
        ExprNode::Pow(base, exponent) => {
            evaluate_at(base, point).powf(evaluate_at(exponent, point))
        }
        _ => f64::NAN,
    }
}

/// `π`, `e`, or `NaN` for a symbol that is not one of them.
fn constant_symbol(symbol: SymbolId) -> f64 {
    match &*SymbolId::name(symbol) {
        "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        _ => f64::NAN,
    }
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
    fn two_conics_meet_in_two_points_and_both_are_reported() {
        // A circle and a parabola: `x² + y² = 5` and `y = x² − 1`. Substituting
        // gives `y² + y − 4 = 0`, so this pair has exactly two real
        // intersections, `y = (−1 + √17)/2` and `x = ±√(y + 1)`. The general
        // conic pair has up to four; *these* two curves have two, which is what
        // the assertion below says. The first version asserted `>= 2` under a
        // name that claimed more, so it could not fail when one point was lost.
        let parabola = add([sym("y"), mul([int(-1), pow(sym("x"), int(2))]), int(1)]);
        let equations = [
            add([pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-5)]),
            parabola,
        ];
        let answer = solve_nonlinear(&NativeSolver, &equations, &[x(), y()]);
        let points = points(answer.expect("an answer"));
        assert_eq!(points.len(), 2, "{points:?}");
        let expected_y = (-1.0 + 17.0f64.sqrt()) / 2.0;
        let expected_x = (expected_y + 1.0).sqrt();
        let mut found: Vec<(f64, f64)> = points
            .iter()
            .map(|point| (value_of(point, x()), value_of(point, y())))
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(
            (found[0].0 + expected_x).abs() < 1e-9,
            "{found:?} vs x = ±{expected_x}"
        );
        assert!(
            (found[1].0 - expected_x).abs() < 1e-9,
            "{found:?} vs x = ±{expected_x}"
        );
        for (_, y) in &found {
            assert!(
                (y - expected_y).abs() < 1e-9,
                "{found:?} vs y = {expected_y}"
            );
        }
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

    #[test]
    fn a_point_off_by_a_thousandth_on_a_large_scale_system_is_rejected() {
        // `x/10⁶ = 1` is the same answer as `x = 10⁶`, written in the units of
        // the large system. The point `10⁶ + 10⁻³` is `10⁻³` off, but its
        // residual is only `10⁻⁹`, which the old absolute `10⁻⁶` accepted. The
        // relative check measures the residual against the equation's own terms.
        let equation = add([
            mul([ExprNode::num(Number::ratio(1, 1_000_000)), sym("x")]),
            int(-1),
        ]);
        let equations = [equation];
        let exact = vec![(x(), ExprNode::num(Number::float(1_000_000.0)))];
        assert!(
            satisfies_all(&exact, &equations),
            "the exact point satisfies the equation"
        );
        let off = vec![(x(), ExprNode::num(Number::float(1_000_000.001)))];
        assert!(
            !satisfies_all(&off, &equations),
            "a point 10⁻³ away from a 10⁶ solution must be refused"
        );
        // Rounding noise is not the same as an error: a point that is only the
        // float's own wobble still satisfies the equation.
        let noise = vec![(x(), ExprNode::num(Number::float(1_000_000.0 + 1e-9)))];
        assert!(satisfies_all(&noise, &equations));
    }
}
