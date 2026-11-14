//! W6.2: a system of first-order equations, and the front end that builds one.
//!
//! [`crate::euler`] and its neighbours integrate one scalar state. A *coupled*
//! system — `x' = y`, `y' = −x` — has a state that is a vector, and the
//! integrators' arithmetic is the same arithmetic applied component by
//! component. This module is that: an [`OdeSystem`] whose state is a `Vec<f64>`,
//! and [`rk4_system`] and [`dopri_system`] over it.
//!
//! # The front end
//!
//! A document asks for a system in the notation a student types:
//!
//! ```text
//! y' = -k*y
//! y(0) = 1
//! ```
//!
//! [`parse_system`] reads those lines into an [`OdeSystem`]: one equation per
//! derivative, one initial condition per unknown, and a time span from the
//! caller. It refuses — with a reason that names the line — when
//!
//! * a derivative is not isolated (`y' = 1 + y'`),
//! * an unknown has no initial condition, or two,
//! * an initial condition names something with no derivative,
//! * a right-hand side mentions an unknown the system does not define.
//!
//! Those are the four ways a system is underdetermined or contradictory, and
//! each is a refusal rather than a guess: an ODE with a missing initial
//! condition has infinitely many solutions, and returning one of them would be
//! the same mistake as returning one root of a quadratic.

use canlab_core::expr::Expr;
use canlab_core::symbol::SymbolId;

use crate::{Dopri, OdeError, Point};

/// A system `y' = f(t, y)` whose state is a vector.
#[derive(Clone)]
pub struct OdeSystem {
    /// The unknown names, in the order the state vector is stored.
    pub names: Vec<SymbolId>,
    /// The initial state, one value per name.
    pub initial: Vec<f64>,
    /// The right-hand sides, one per name, as compiled programs.
    pub rhs: Vec<canlab_core::Program>,
}

impl std::fmt::Debug for OdeSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OdeSystem")
            .field("names", &self.names)
            .field("initial", &self.initial)
            .finish_non_exhaustive()
    }
}

impl OdeSystem {
    /// The state's dimension.
    pub fn dimension(&self) -> usize {
        self.names.len()
    }

    /// `f(t, y)` as a vector.
    pub fn derivative(&self, t: f64, y: &[f64]) -> Vec<f64> {
        let mut args = Vec::with_capacity(y.len() + 1);
        args.push(t);
        args.extend_from_slice(y);
        self.rhs.iter().map(|program| program.eval(&args)).collect()
    }
}

/// Why a system's text could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemError {
    /// A line that is neither a derivative nor an initial condition.
    Unreadable {
        /// The line number, from 1.
        line: usize,
        /// What the line was.
        text: String,
    },
    /// A derivative whose right-hand side still mentions the derivative.
    NotIsolated {
        /// The line number.
        line: usize,
        /// The unknown.
        name: String,
    },
    /// An unknown with no initial condition.
    MissingInitial {
        /// The unknown.
        name: String,
    },
    /// Two initial conditions for one unknown.
    DuplicateInitial {
        /// The unknown.
        name: String,
    },
    /// An initial condition for something the system does not define.
    UnknownInitial {
        /// The name.
        name: String,
    },
    /// A right-hand side that mentions an unknown the system does not define.
    UnknownReference {
        /// The unknown that is not defined.
        name: String,
    },
}

impl SystemError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            SystemError::Unreadable { line, text } => {
                format!("line {line} (`{text}`) is neither a derivative nor an initial condition")
            }
            SystemError::NotIsolated { line, name } => format!(
                "line {line}: the derivative `{name}'` is not isolated — its right-hand side still contains `{name}'`"
            ),
            SystemError::MissingInitial { name } => {
                format!("`{name}` has no initial condition; `{name}(0) = …` is needed")
            }
            SystemError::DuplicateInitial { name } => {
                format!("`{name}` has two initial conditions")
            }
            SystemError::UnknownInitial { name } => {
                format!("`{name}` has an initial condition but no derivative")
            }
            SystemError::UnknownReference { name } => {
                format!("`{name}` is used but has no derivative equation")
            }
        }
    }
}

/// Read a system from the lines a student types.
///
/// Each line is either `name' = right-hand side` or `name(t₀) = value`. The
/// right-hand side may mention the independent variable `t` and any of the
/// system's unknowns. The order of the state vector is the order the
/// derivatives appear.
pub fn parse_system(lines: &[String]) -> Result<OdeSystem, SystemError> {
    let mut names: Vec<SymbolId> = Vec::new();
    let mut right_sides: Vec<Expr> = Vec::new();
    let mut initials: Vec<(SymbolId, f64)> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let line_number = index + 1;
        // The `=` that separates the two halves. A line without one is not a
        // statement this front end knows.
        let Some((left, right)) = text.split_once('=') else {
            return Err(SystemError::Unreadable {
                line: line_number,
                text: text.to_string(),
            });
        };
        let left = left.trim();
        let right = right.trim();
        if let Some(name) = derivative_name(left) {
            // `y' = …`: the right-hand side must not mention `y'` again, which
            // is how `y' = 1 + y'` is caught rather than compiled into a
            // program that reads a variable it does not have.
            if right.contains(&format!("{name}'")) {
                return Err(SystemError::NotIsolated {
                    line: line_number,
                    name,
                });
            }
            let symbol = SymbolId::intern(&name);
            let expression = canlab_parse::parse_latex(right).map_err(|_| SystemError::Unreadable {
                line: line_number,
                text: text.to_string(),
            })?;
            if let Some(position) = names.iter().position(|existing| *existing == symbol) {
                right_sides[position] = expression;
            } else {
                names.push(symbol);
                right_sides.push(expression);
            }
            continue;
        }
        if let Some((name, _)) = initial_condition(left) {
            let value = canlab_parse::parse_latex(right)
                .ok()
                .and_then(|expression| expression.as_number().map(|number| number.to_f64()))
                .ok_or_else(|| SystemError::Unreadable {
                    line: line_number,
                    text: text.to_string(),
                })?;
            let symbol = SymbolId::intern(&name);
            if initials.iter().any(|(existing, _)| *existing == symbol) {
                return Err(SystemError::DuplicateInitial { name });
            }
            initials.push((symbol, value));
            continue;
        }
        return Err(SystemError::Unreadable {
            line: line_number,
            text: text.to_string(),
        });
    }

    if names.is_empty() {
        return Err(SystemError::Unreadable {
            line: 0,
            text: String::from("(no derivative equations)"),
        });
    }
    // Every unknown needs exactly one initial value, and every initial value
    // needs an unknown.
    let mut initial = Vec::with_capacity(names.len());
    for name in &names {
        let Some((_, value)) = initials.iter().find(|(symbol, _)| symbol == name) else {
            return Err(SystemError::MissingInitial {
                name: name.as_str().to_string(),
            });
        };
        initial.push(*value);
    }
    for (symbol, _) in &initials {
        if !names.contains(symbol) {
            return Err(SystemError::UnknownInitial {
                name: symbol.as_str().to_string(),
            });
        }
    }
    // Every symbol a right-hand side mentions must be `t` or one of the
    // unknowns; anything else is a name with no equation.
    let t = SymbolId::intern("t");
    let mut variables = vec![t];
    variables.extend(names.iter().copied());
    let mut rhs = Vec::with_capacity(right_sides.len());
    for expression in &right_sides {
        for symbol in canlab_poly::expr::free_symbols(expression) {
            if !variables.contains(&symbol) {
                return Err(SystemError::UnknownReference {
                    name: symbol.as_str().to_string(),
                });
            }
        }
        let program = canlab_core::Program::compile(expression, &variables).ok_or_else(|| {
            SystemError::Unreadable {
                line: 0,
                text: canlab_tex::to_latex(expression),
            }
        })?;
        rhs.push(program);
    }
    Ok(OdeSystem {
        names,
        initial,
        rhs,
    })
}

/// `y'` from `y'`, or `None`.
fn derivative_name(left: &str) -> Option<String> {
    let stripped = left.strip_suffix('\'')?;
    let name = stripped.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(name.to_string())
}

/// `(name, t₀)` from `y(0)`, or `None`.
fn initial_condition(left: &str) -> Option<(String, f64)> {
    let open = left.find('(')?;
    let close = left.rfind(')')?;
    if close < open {
        return None;
    }
    let name = left[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let at = left[open + 1..close].trim().parse::<f64>().ok()?;
    Some((name.to_string(), at))
}

/// One point of a system's trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemPoint {
    /// The independent variable.
    pub t: f64,
    /// The state.
    pub y: Vec<f64>,
}

/// A system's trajectory.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SystemTrace {
    /// The points, in increasing `t`.
    pub points: Vec<SystemPoint>,
}

impl SystemTrace {
    /// The final point.
    pub fn last(&self) -> Option<&SystemPoint> {
        self.points.last()
    }

    /// The value of the unknown at `index` at time `t`, by linear interpolation.
    pub fn at(&self, index: usize, t: f64) -> Option<f64> {
        let first = self.points.first()?;
        if t <= first.t {
            return first.y.get(index).copied();
        }
        for pair in self.points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t >= a.t && t <= b.t {
                let (Some(ya), Some(yb)) = (a.y.get(index), b.y.get(index)) else {
                    return None;
                };
                if b.t == a.t {
                    return Some(*yb);
                }
                let fraction = (t - a.t) / (b.t - a.t);
                return Some(ya + fraction * (yb - ya));
            }
        }
        self.points.last().and_then(|point| point.y.get(index).copied())
    }
}

/// Fourth-order Runge–Kutta over a system, fixed step.
pub fn rk4_system(
    system: &OdeSystem,
    t0: f64,
    t1: f64,
    h: f64,
) -> Result<SystemTrace, OdeError> {
    // The same argument checks as the scalar path, and the same reason for
    // `!(x > 0.0)`: a `NaN` compares false to everything and this form rejects
    // it.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(h > 0.0) {
        return Err(OdeError::BadStep);
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(t1 > t0) {
        return Err(OdeError::BadInterval);
    }
    let steps = ((t1 - t0) / h).round().max(1.0) as usize;
    let mut points = vec![SystemPoint {
        t: t0,
        y: system.initial.clone(),
    }];
    let mut t = t0;
    let mut y = system.initial.clone();
    for index in 0..steps {
        let step = if index + 1 == steps { t1 - t } else { h };
        let k1 = system.derivative(t, &y);
        let k2 = system.derivative(t + 0.5 * step, &add_scaled(&y, &k1, 0.5 * step));
        let k3 = system.derivative(t + 0.5 * step, &add_scaled(&y, &k2, 0.5 * step));
        let k4 = system.derivative(t + step, &add_scaled(&y, &k3, step));
        for component in 0..y.len() {
            y[component] += step / 6.0
                * (k1[component] + 2.0 * k2[component] + 2.0 * k3[component] + k4[component]);
        }
        t += step;
        points.push(SystemPoint { t, y: y.clone() });
    }
    Ok(SystemTrace { points })
}

/// Dormand–Prince over a system, adaptive.
pub fn dopri_system(
    system: &OdeSystem,
    t0: f64,
    t1: f64,
    tolerance: f64,
    initial_step: f64,
) -> Result<SystemTrace, OdeError> {
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(initial_step > 0.0) {
        return Err(OdeError::BadStep);
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(t1 > t0) {
        return Err(OdeError::BadInterval);
    }
    let mut points = vec![SystemPoint {
        t: t0,
        y: system.initial.clone(),
    }];
    let mut t = t0;
    let mut y = system.initial.clone();
    let mut h = initial_step;
    let minimum_step = 1e-14 * (t1 - t0).abs().max(1.0);
    while t < t1 - 1e-15 {
        let step = h.min(t1 - t);
        let (next, error) = system_step(system, t, &y, step)?;
        if error <= tolerance || step <= minimum_step {
            y = next;
            t += step;
            points.push(SystemPoint { t, y: y.clone() });
            let factor = if error == 0.0 {
                2.0
            } else {
                (0.9 * (tolerance / error).powf(0.2)).clamp(0.2, 2.0)
            };
            h = (step * factor).clamp(minimum_step, (t1 - t0).abs());
        } else {
            h = (step * (0.9 * (tolerance / error).powf(0.2)).clamp(0.2, 0.9))
                .max(minimum_step);
            if h <= minimum_step && error > tolerance {
                return Err(OdeError::StepUnderflow);
            }
        }
    }
    Ok(SystemTrace { points })
}

/// One Dormand–Prince step for a system: the fifth-order state and the largest
/// component of the error estimate.
///
/// The error is the *maximum* over the components, not the sum: a system is
/// accurate when every component is, and a sum would let one component hide
/// behind another's small error.
fn system_step(
    system: &OdeSystem,
    t: f64,
    y: &[f64],
    h: f64,
) -> Result<(Vec<f64>, f64), OdeError> {
    let mut k: Vec<Vec<f64>> = Vec::with_capacity(7);
    // Each stage uses the ones before it, so the index addresses both
    // `A[stage][previous]` and `k[previous]`.
    #[allow(clippy::needless_range_loop)]
    for stage in 0..7 {
        let mut state = y.to_vec();
        for previous in 0..stage {
            let coefficient = Dopri::A_COEFFICIENTS[stage][previous];
            if coefficient == 0.0 {
                continue;
            }
            for component in 0..state.len() {
                state[component] += h * coefficient * k[previous][component];
            }
        }
        let derivative = system.derivative(t + Dopri::C_COEFFICIENTS[stage] * h, &state);
        if derivative.iter().any(|value| !value.is_finite()) {
            return Err(OdeError::NotFinite { t });
        }
        k.push(derivative);
    }
    let mut fifth = y.to_vec();
    let mut fourth = y.to_vec();
    for component in 0..y.len() {
        #[allow(clippy::needless_range_loop)]
        for stage in 0..7 {
            fifth[component] += h * Dopri::B5_COEFFICIENTS[stage] * k[stage][component];
            fourth[component] += h * Dopri::B4_COEFFICIENTS[stage] * k[stage][component];
        }
    }
    let error = fifth
        .iter()
        .zip(&fourth)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    Ok((fifth, error))
}

fn add_scaled(y: &[f64], k: &[f64], scale: f64) -> Vec<f64> {
    y.iter()
        .zip(k)
        .map(|(value, slope)| value + scale * slope)
        .collect()
}

/// The scalar trace's points, for a caller that has one unknown.
pub fn as_scalar_points(trace: &SystemTrace) -> Vec<Point> {
    trace
        .points
        .iter()
        .map(|point| Point {
            t: point.t,
            y: point.y.first().copied().unwrap_or(0.0),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn y_prime_equals_y_reproduces_the_exponential() {
        // The roadmap's check: `y' = y, y(0) = 1` reproduces `eᵗ` to `1e-6`.
        let system = parse_system(&lines(&["y' = y", "y(0) = 1"])).expect("a system");
        assert_eq!(system.dimension(), 1);
        let trace = rk4_system(&system, 0.0, 1.0, 0.01).expect("a trace");
        let last = trace.last().expect("a point");
        assert!(
            (last.y[0] - std::f64::consts::E).abs() < 1e-6,
            "y(1) = {}",
            last.y[0]
        );
        // And the adaptive method agrees.
        let trace = dopri_system(&system, 0.0, 1.0, 1e-10, 0.1).expect("a trace");
        let last = trace.last().expect("a point");
        assert!((last.y[0] - std::f64::consts::E).abs() < 1e-9, "{}", last.y[0]);
    }

    #[test]
    fn three_unknowns_in_three_lines() {
        // The roadmap's check. `x' = y`, `y' = -x`, `z' = 0` with initial
        // conditions: the first two trace a circle, the third stays put.
        let system = parse_system(&lines(&[
            "x' = y",
            "y' = -x",
            "z' = 0",
            "x(0) = 1",
            "y(0) = 0",
            "z(0) = 5",
        ]))
        .expect("a system");
        assert_eq!(system.dimension(), 3);
        let trace = rk4_system(&system, 0.0, std::f64::consts::TAU, 0.001).expect("a trace");
        let last = trace.last().expect("a point");
        // After a full turn the state returns to where it started.
        assert!((last.y[0] - 1.0).abs() < 1e-6, "x = {}", last.y[0]);
        assert!(last.y[1].abs() < 1e-6, "y = {}", last.y[1]);
        assert!((last.y[2] - 5.0).abs() < 1e-12, "z = {}", last.y[2]);
        // And the radius is constant throughout, which is what a circle is.
        for point in &trace.points {
            let radius = (point.y[0] * point.y[0] + point.y[1] * point.y[1]).sqrt();
            assert!((radius - 1.0).abs() < 1e-6, "radius {radius}");
        }
    }

    #[test]
    fn a_derivative_that_is_not_isolated_is_refused() {
        // The roadmap's check: `y' = 1 + y'` is refused, with a reason that
        // names the line.
        let error = parse_system(&lines(&["y' = 1 + y'", "y(0) = 1"])).expect_err("refused");
        match &error {
            SystemError::NotIsolated { line, name } => {
                assert_eq!(*line, 1);
                assert_eq!(name, "y");
            }
            other => panic!("expected NotIsolated, got {other:?}"),
        }
        assert!(error.message().contains("not isolated"), "{}", error.message());
    }

    #[test]
    fn a_system_that_is_underdetermined_is_refused() {
        // No initial condition.
        let error = parse_system(&lines(&["y' = y"])).expect_err("refused");
        assert!(matches!(error, SystemError::MissingInitial { .. }), "{error:?}");
        // Two initial conditions for one unknown.
        let error = parse_system(&lines(&["y' = y", "y(0) = 1", "y(0) = 2"])).expect_err("refused");
        assert!(matches!(error, SystemError::DuplicateInitial { .. }), "{error:?}");
        // An initial condition with no derivative.
        let error = parse_system(&lines(&["y' = y", "y(0) = 1", "z(0) = 2"])).expect_err("refused");
        assert!(matches!(error, SystemError::UnknownInitial { .. }), "{error:?}");
        // A right-hand side that mentions an unknown with no equation.
        let error = parse_system(&lines(&["y' = z", "y(0) = 1"])).expect_err("refused");
        assert!(matches!(error, SystemError::UnknownReference { .. }), "{error:?}");
    }

    #[test]
    fn the_independent_variable_may_appear_in_a_right_hand_side() {
        // `y' = t`, `y(0) = 0` → `y = t²/2`, so `y(2) = 2`.
        let system = parse_system(&lines(&["y' = t", "y(0) = 0"])).expect("a system");
        let trace = rk4_system(&system, 0.0, 2.0, 0.001).expect("a trace");
        let last = trace.last().expect("a point");
        assert!((last.y[0] - 2.0).abs() < 1e-9, "y(2) = {}", last.y[0]);
    }

    #[test]
    fn a_line_that_is_neither_a_derivative_nor_an_initial_condition_is_refused() {
        let error = parse_system(&lines(&["y' = y", "y(0) = 1", "hello"])).expect_err("refused");
        assert!(matches!(error, SystemError::Unreadable { line: 3, .. }), "{error:?}");
        let error = parse_system(&lines(&["y' = y", "y(0) = 1", "y = 2"])).expect_err("refused");
        assert!(matches!(error, SystemError::Unreadable { line: 3, .. }), "{error:?}");
    }

    #[test]
    fn a_decaying_system_matches_its_closed_form() {
        // `y' = -k·y` with `k = 2`, `y(0) = 1` → `y = e^{-2t}`.
        let system = parse_system(&lines(&["y' = -2*y", "y(0) = 1"])).expect("a system");
        let trace = dopri_system(&system, 0.0, 1.0, 1e-10, 0.05).expect("a trace");
        let last = trace.last().expect("a point");
        let expected = (-2.0f64).exp();
        assert!((last.y[0] - expected).abs() < 1e-9, "{} vs {expected}", last.y[0]);
        // And the interpolation agrees with the closed form between the points,
        // to the *interpolation's* accuracy and not the integrator's: `at` is a
        // chord between two accepted points, and a chord's error is `O(h²)` with
        // `h` the step the adaptive method chose (about `0.018` here), so `1e-4`
        // is the honest tolerance. The first version of this assertion asked for
        // `1e-6` and got `8.8·10⁻⁵` — the method was right and the expectation
        // was not.
        let value = trace.at(0, 0.5).expect("a value");
        assert!((value - (-1.0f64).exp()).abs() < 1e-4, "{value}");
    }

    #[test]
    fn the_error_estimate_is_the_largest_component() {
        // A system whose components have very different scales: the step must
        // be limited by the larger error, not the sum.
        let system = parse_system(&lines(&["a' = 100*a", "b' = b", "a(0) = 1", "b(0) = 1"]))
            .expect("a system");
        let trace = dopri_system(&system, 0.0, 0.01, 1e-8, 0.1).expect("a trace");
        let last = trace.last().expect("a point");
        // `a(0.01) = e^1` and `b(0.01) = e^{0.01}`.
        assert!((last.y[0] - 1.0f64.exp()).abs() < 1e-6, "{}", last.y[0]);
        assert!((last.y[1] - 0.01f64.exp()).abs() < 1e-6, "{}", last.y[1]);
        // The step was limited by `a`: many more steps than a single one.
        assert!(trace.points.len() > 2, "{} points", trace.points.len());
    }

    #[test]
    fn the_scalar_and_system_paths_agree_on_one_unknown() {
        // `y' = y` through both: the scalar RK4 and the system RK4 must give the
        // same number, because they are the same method.
        let system = parse_system(&lines(&["y' = y", "y(0) = 1"])).expect("a system");
        let system_trace = rk4_system(&system, 0.0, 1.0, 0.01).expect("a trace");
        let scalar = crate::rk4(&|_t, y| y, 0.0, 1.0, 1.0, 0.01).expect("a trace");
        let system_value = system_trace.last().expect("a point").y[0];
        let scalar_value = scalar.last().expect("a point").y;
        assert!(
            (system_value - scalar_value).abs() < 1e-12,
            "system {system_value}, scalar {scalar_value}"
        );
        // And `as_scalar_points` is the bridge, with the same values.
        let points = as_scalar_points(&system_trace);
        assert_eq!(points.len(), system_trace.points.len());
        assert!((points.last().expect("a point").y - scalar_value).abs() < 1e-12);
    }

    #[test]
    fn an_unreadable_right_hand_side_is_refused_with_its_line() {
        let error = parse_system(&lines(&["y' = y +", "y(0) = 1"])).expect_err("refused");
        assert!(matches!(error, SystemError::Unreadable { line: 1, .. }), "{error:?}");

    }
}
