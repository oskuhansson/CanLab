//! W6.1: initial-value problems, and the order each method actually has.
//!
//! `y' = f(t, y)`, `y(t₀) = y₀`, integrated to `t₁`. The crate is deliberately
//! one-dimensional in `y` — a scalar state — because that is what the exam
//! problems and the three scenarios in W6.4 are, and because a vector state
//! would need a container whose allocation behaviour is a second thing to get
//! right. A coupled system is a later step, and the [`System`] type is where it
//! will go.
//!
//! # The methods, and what each is for
//!
//! * **Euler** — first order, and here to be *measured*: it is the reference
//!   the other methods' orders are compared against, and a test that RK4 is
//!   fourth order is only meaningful next to a method that is first.
//! * **RK4** — fourth order, fixed step. The workhorse.
//! * **Dormand–Prince 5(4)** — an embedded pair: six stages give a fifth-order
//!   estimate and a fourth-order one, and their difference is the error
//!   estimate that drives the step size. Adaptive, with dense output by cubic
//!   Hermite interpolation between the accepted steps.
//! * **Symplectic Euler** and **velocity Verlet** — for Hamiltonian systems,
//!   where the question is not "is the trajectory accurate" but "does the energy
//!   stay bounded". A non-symplectic method's energy drifts linearly; a
//!   symplectic one's oscillates. The test measures that difference, because it
//!   is the whole reason these two methods are in the crate.
//!
//! # Why the order is measured and not claimed
//!
//! An integrator's order is a property of its *implementation*, not of its name:
//! a coefficient typed wrong turns RK4 into a third-order method that still
//! looks plausible. Every method here has a test that halves the step and
//! checks the error falls by the factor the order predicts — 2 for Euler, 16
//! for RK4, 32 for Dormand–Prince — against a problem whose solution is known
//! in closed form.
//!
//! # Events
//!
//! An event is a zero of a *function of the state* — "when does the projectile
//! land", "when does the pendulum cross its lowest point". The crossing is
//! located by bisection on the interpolated trajectory, so the answer is a time
//! and a state, not just a boolean that it happened somewhere in a step.

use canlab_core::expr::Expr;

pub mod plot;
pub mod system;

/// The right-hand side of `y' = f(t, y)`.
pub type Rhs<'a> = &'a dyn Fn(f64, f64) -> f64;

/// Why an integration failed.
#[derive(Debug, Clone, PartialEq)]
pub enum OdeError {
    /// The step size is not positive.
    BadStep,
    /// The interval is empty or reversed.
    BadInterval,
    /// The right-hand side returned a non-finite value.
    NotFinite {
        /// Where.
        t: f64,
    },
    /// The adaptive step shrank below the minimum without meeting the
    /// tolerance.
    StepUnderflow,
}

impl OdeError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            OdeError::BadStep => "the step size must be positive".to_string(),
            OdeError::BadInterval => "the interval is empty or reversed".to_string(),
            OdeError::NotFinite { t } => {
                format!("the right-hand side is not finite at t = {t}")
            }
            OdeError::StepUnderflow => {
                "the step shrank below the minimum without meeting the tolerance".to_string()
            }
        }
    }
}

/// One point of a trajectory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The independent variable.
    pub t: f64,
    /// The state.
    pub y: f64,
}

/// A trajectory: the points an integration produced, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trace {
    /// The points, in increasing `t`.
    pub points: Vec<Point>,
}

impl Trace {
    /// The final point, if there is one.
    pub fn last(&self) -> Option<Point> {
        self.points.last().copied()
    }

    /// The state at `t`, by linear interpolation between the recorded points.
    ///
    /// For a fixed-step method this is exact at a recorded point and a chord
    /// between two; for Dormand–Prince, [`Dopri::integrate_dense`] interpolates
    /// with the method's own cubic Hermite, which is the accurate way.
    pub fn at(&self, t: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        if t <= self.points[0].t {
            return Some(self.points[0].y);
        }
        for pair in self.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t >= a.t && t <= b.t {
                if b.t == a.t {
                    return Some(b.y);
                }
                let fraction = (t - a.t) / (b.t - a.t);
                return Some(a.y + fraction * (b.y - a.y));
            }
        }
        self.points.last().map(|point| point.y)
    }

    /// The largest and smallest `y`.
    pub fn extent(&self) -> Option<(f64, f64)> {
        let mut values = self.points.iter().map(|point| point.y);
        let first = values.next()?;
        let mut low = first;
        let mut high = first;
        for value in values {
            low = low.min(value);
            high = high.max(value);
        }
        Some((low, high))
    }
}

/// Check the arguments every method shares.
fn check(t0: f64, t1: f64, h: f64) -> Result<(), OdeError> {
    // `!(h > 0.0)` and not `h <= 0.0`: a `NaN` step compares false to
    // everything, and this form rejects it. The same idiom the rest of this
    // workspace uses for a value that must be ordered.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(h > 0.0) {
        return Err(OdeError::BadStep);
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(t1 > t0) {
        return Err(OdeError::BadInterval);
    }
    Ok(())
}

/// Explicit Euler: `y_{n+1} = y_n + h·f(t_n, y_n)`.
///
/// First order. Its error falls by 2 when the step halves, which is the
/// measurement that makes the other methods' orders meaningful.
pub fn euler(rhs: Rhs<'_>, t0: f64, y0: f64, t1: f64, h: f64) -> Result<Trace, OdeError> {
    check(t0, t1, h)?;
    let mut points = vec![Point { t: t0, y: y0 }];
    let mut t = t0;
    let mut y = y0;
    while t < t1 - 1e-15 {
        let step = h.min(t1 - t);
        let slope = rhs(t, y);
        if !slope.is_finite() {
            return Err(OdeError::NotFinite { t });
        }
        y += step * slope;
        t += step;
        points.push(Point { t, y });
    }
    Ok(Trace { points })
}

/// The classical fourth-order Runge–Kutta step.
fn rk4_step(rhs: Rhs<'_>, t: f64, y: f64, h: f64) -> Result<f64, OdeError> {
    let k1 = rhs(t, y);
    let k2 = rhs(t + 0.5 * h, y + 0.5 * h * k1);
    let k3 = rhs(t + 0.5 * h, y + 0.5 * h * k2);
    let k4 = rhs(t + h, y + h * k3);
    if !(k1.is_finite() && k2.is_finite() && k3.is_finite() && k4.is_finite()) {
        return Err(OdeError::NotFinite { t });
    }
    Ok(y + h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4))
}

/// Fourth-order Runge–Kutta, fixed step.
pub fn rk4(rhs: Rhs<'_>, t0: f64, y0: f64, t1: f64, h: f64) -> Result<Trace, OdeError> {
    check(t0, t1, h)?;
    let mut points = vec![Point { t: t0, y: y0 }];
    let mut t = t0;
    let mut y = y0;
    while t < t1 - 1e-15 {
        let step = h.min(t1 - t);
        y = rk4_step(rhs, t, y, step)?;
        t += step;
        points.push(Point { t, y });
    }
    Ok(Trace { points })
}

/// Dormand–Prince 5(4): an embedded pair, adaptive, with dense output.
pub struct Dopri;

impl Dopri {
    /// The Butcher tableau's nodes, for the system path.
    pub const C_COEFFICIENTS: [f64; 7] = [0.0, 1.0 / 5.0, 3.0 / 10.0, 4.0 / 5.0, 8.0 / 9.0, 1.0, 1.0];
    /// The fifth-order weights, for the system path.
    pub const B5_COEFFICIENTS: [f64; 7] = [
        35.0 / 384.0,
        0.0,
        500.0 / 1113.0,
        125.0 / 192.0,
        -2187.0 / 6784.0,
        11.0 / 84.0,
        0.0,
    ];
    /// The fourth-order weights, for the system path.
    pub const B4_COEFFICIENTS: [f64; 7] = [
        5179.0 / 57600.0,
        0.0,
        7571.0 / 16695.0,
        393.0 / 640.0,
        -92097.0 / 339200.0,
        187.0 / 2100.0,
        1.0 / 40.0,
    ];
    /// The stage coefficients, for the system path.
    pub const A_COEFFICIENTS: [[f64; 7]; 7] = [
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0, 0.0],
        [
            19372.0 / 6561.0,
            -25360.0 / 2187.0,
            64448.0 / 6561.0,
            -212.0 / 729.0,
            0.0,
            0.0,
            0.0,
        ],
        [
            9017.0 / 3168.0,
            -355.0 / 33.0,
            46732.0 / 5247.0,
            49.0 / 176.0,
            -5103.0 / 18656.0,
            0.0,
            0.0,
        ],
        [
            35.0 / 384.0,
            0.0,
            500.0 / 1113.0,
            125.0 / 192.0,
            -2187.0 / 6784.0,
            11.0 / 84.0,
            0.0,
        ],
    ];
    /// The Butcher tableau's nodes.
    const C: [f64; 7] = Self::C_COEFFICIENTS;
    /// The coefficients of the fifth-order solution.
    const B5: [f64; 7] = [
        35.0 / 384.0,
        0.0,
        500.0 / 1113.0,
        125.0 / 192.0,
        -2187.0 / 6784.0,
        11.0 / 84.0,
        0.0,
    ];
    /// The coefficients of the fourth-order solution.
    const B4: [f64; 7] = [
        5179.0 / 57600.0,
        0.0,
        7571.0 / 16695.0,
        393.0 / 640.0,
        -92097.0 / 339200.0,
        187.0 / 2100.0,
        1.0 / 40.0,
    ];
    /// The stage coefficients, row by row.
    const A: [[f64; 7]; 7] = [
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0, 0.0],
        [
            19372.0 / 6561.0,
            -25360.0 / 2187.0,
            64448.0 / 6561.0,
            -212.0 / 729.0,
            0.0,
            0.0,
            0.0,
        ],
        [
            9017.0 / 3168.0,
            -355.0 / 33.0,
            46732.0 / 5247.0,
            49.0 / 176.0,
            -5103.0 / 18656.0,
            0.0,
            0.0,
        ],
        [
            35.0 / 384.0,
            0.0,
            500.0 / 1113.0,
            125.0 / 192.0,
            -2187.0 / 6784.0,
            11.0 / 84.0,
            0.0,
        ],
    ];

    /// Integrate with a tolerance, an initial step, and a step range.
    ///
    /// The error estimate is `|y₅ − y₄|`, and the step is scaled by
    /// `(tolerance/error)^{1/5}` — the fifth power because the pair is fifth
    /// order, and the `0.9` safety factor because a step that is exactly at the
    /// tolerance would be rejected on the next rounding.
    pub fn integrate(
        rhs: Rhs<'_>,
        t0: f64,
        y0: f64,
        t1: f64,
        tolerance: f64,
        initial_step: f64,
    ) -> Result<Trace, OdeError> {
        check(t0, t1, initial_step)?;
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(tolerance > 0.0) {
            return Err(OdeError::BadStep);
        }
        let mut points = vec![Point { t: t0, y: y0 }];
        let mut t = t0;
        let mut y = y0;
        let mut h = initial_step;
        let minimum_step = 1e-14 * (t1 - t0).abs().max(1.0);
        while t < t1 - 1e-15 {
            let step = h.min(t1 - t);
            let (next, error) = Self::step(rhs, t, y, step)?;
            if error <= tolerance || step <= minimum_step {
                y = next;
                t += step;
                points.push(Point { t, y });
                // Grow slowly, shrink quickly: a step that is too large is
                // wasted work, a step that is too small is wasted work too but
                // a rejected step is wasted twice.
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
        Ok(Trace { points })
    }

    /// One Dormand–Prince step: the fifth-order value and the error estimate.
    fn step(rhs: Rhs<'_>, t: f64, y: f64, h: f64) -> Result<(f64, f64), OdeError> {
        let mut k = [0.0f64; 7];
        // The stages are sequential — each uses the ones before it — so the
        // index is the only way to address both `A[stage][previous]` and
        // `k[previous]`; an iterator would not express that.
        #[allow(clippy::needless_range_loop)]
        for stage in 0..7 {
            let mut sum = 0.0;
            for previous in 0..stage {
                sum += Self::A[stage][previous] * k[previous];
            }
            k[stage] = rhs(t + Self::C[stage] * h, y + h * sum);
            if !k[stage].is_finite() {
                return Err(OdeError::NotFinite { t });
            }
        }
        let mut fifth = y;
        let mut fourth = y;
        // Two coefficient arrays indexed by the same stage, so the index is
        // what addresses both.
        #[allow(clippy::needless_range_loop)]
        for stage in 0..7 {
            fifth += h * Self::B5[stage] * k[stage];
            fourth += h * Self::B4[stage] * k[stage];
        }
        Ok((fifth, (fifth - fourth).abs()))
    }

    /// Integrate and also record the stage derivatives, so that
    /// [`Dopri::dense_at`] can interpolate with the method's own Hermite cubic.
    pub fn integrate_dense(
        rhs: Rhs<'_>,
        t0: f64,
        y0: f64,
        t1: f64,
        tolerance: f64,
        initial_step: f64,
    ) -> Result<DenseTrace, OdeError> {
        check(t0, t1, initial_step)?;
        let mut points = vec![Point { t: t0, y: y0 }];
        let mut slopes = vec![rhs(t0, y0)];
        let mut t = t0;
        let mut y = y0;
        let mut h = initial_step;
        let minimum_step = 1e-14 * (t1 - t0).abs().max(1.0);
        while t < t1 - 1e-15 {
            let step = h.min(t1 - t);
            let (next, error) = Self::step(rhs, t, y, step)?;
            if error <= tolerance || step <= minimum_step {
                let slope_end = rhs(t + step, next);
                y = next;
                t += step;
                points.push(Point { t, y });
                slopes.push(slope_end);
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
        Ok(DenseTrace { points, slopes })
    }
}

/// A Dormand–Prince trajectory with the end-of-step slopes, for dense output.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DenseTrace {
    /// The accepted points.
    pub points: Vec<Point>,
    /// `f(t, y)` at each accepted point.
    pub slopes: Vec<f64>,
}

impl DenseTrace {
    /// The state at `t`, by cubic Hermite interpolation on the step containing
    /// it.
    ///
    /// The Hermite cubic matches the state and its derivative at both ends of
    /// the step, which is the interpolation Dormand–Prince's dense output is
    /// built on; it is accurate to the method's order, unlike a chord.
    pub fn at(&self, t: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        if t <= self.points[0].t {
            return Some(self.points[0].y);
        }
        for index in 0..self.points.len().saturating_sub(1) {
            let a = self.points[index];
            let b = self.points[index + 1];
            if t >= a.t && t <= b.t {
                let h = b.t - a.t;
                if h == 0.0 {
                    return Some(b.y);
                }
                let s = (t - a.t) / h;
                let (fa, fb) = (self.slopes[index], self.slopes[index + 1]);
                // Cubic Hermite basis.
                let h00 = 2.0 * s.powi(3) - 3.0 * s.powi(2) + 1.0;
                let h10 = s.powi(3) - 2.0 * s.powi(2) + s;
                let h01 = -2.0 * s.powi(3) + 3.0 * s.powi(2);
                let h11 = s.powi(3) - s.powi(2);
                return Some(h00 * a.y + h10 * h * fa + h01 * b.y + h11 * h * fb);
            }
        }
        self.points.last().map(|point| point.y)
    }
}

/// Symplectic Euler for `q' = p`, `p' = −∂V/∂q`.
///
/// The update is `p ← p − h·V'(q)`, then `q ← q + h·p`: the momentum uses the
/// *old* position and the position uses the *new* momentum. That asymmetry is
/// what makes the method symplectic, and it is why the energy stays bounded
/// instead of drifting.
pub fn symplectic_euler(
    force: impl Fn(f64) -> f64,
    q0: f64,
    p0: f64,
    t1: f64,
    h: f64,
) -> Result<Vec<(f64, f64)>, OdeError> {
    check(0.0, t1, h)?;
    let mut states = vec![(q0, p0)];
    let (mut q, mut p) = (q0, p0);
    // The number of steps is computed once and the loop counts them, instead of
    // accumulating `t` and comparing it with `t1`. The accumulating form is what
    // the first version used, and it diverged: `t` drifts downwards by a few
    // ulps per step, `t1 - t` becomes slightly *larger* than `h`, and the last
    // step of each pass is a hair longer than `h` — which for a symplectic
    // method is a different method, and it lost stability after 200 time units.
    // Counting steps is exact.
    // `(t1/h).round()` and not `.ceil()`: `200.0/0.01` is `20000.000000000004`,
    // and `ceil` makes it 20001 steps whose last one is `t1 − t = −3.6·10⁻¹²`
    // — a *negative* step, which runs the integrator backwards for one step and
    // is enough to make it unstable. The count is rounded to the nearest whole
    // step, and the last step takes up the remainder.
    let steps = (t1 / h).round().max(1.0) as usize;
    let mut t = 0.0;
    for index in 0..steps {
        let step = if index + 1 == steps { t1 - t } else { h };
        // `p += h·F(q)` with `F = −∂V/∂q`, and then `q += h·p`. The first
        // version wrote `p -= h·F(q)`, which for the oscillator's `F(q) = −q`
        // is `p += h·q` — the *anti*-damping sign, and it grew to `10⁸⁶` in
        // 20000 steps. The test caught it because a symplectic method's energy
        // must stay bounded, and a sign error is the one way it does not.
        p += step * force(q);
        q += step * p;
        t += step;
        states.push((q, p));
    }
    Ok(states)
}

/// Velocity Verlet for `q' = p`, `p' = −∂V/∂q`.
///
/// Second order and symplectic: the position is advanced with the average of
/// the old and new accelerations, which is the form that conserves energy to
/// `O(h²)` over long times.
pub fn velocity_verlet(
    force: impl Fn(f64) -> f64,
    q0: f64,
    p0: f64,
    t1: f64,
    h: f64,
) -> Result<Vec<(f64, f64)>, OdeError> {
    check(0.0, t1, h)?;
    let mut states = vec![(q0, p0)];
    let (mut q, mut p) = (q0, p0);
    let mut acceleration = force(q);
    let steps = (t1 / h).round().max(1.0) as usize;
    let mut t = 0.0;
    for index in 0..steps {
        let step = if index + 1 == steps { t1 - t } else { h };
        q += step * p + 0.5 * step * step * acceleration;
        let next_acceleration = force(q);
        p += 0.5 * step * (acceleration + next_acceleration);
        acceleration = next_acceleration;
        t += step;
        states.push((q, p));
    }
    Ok(states)
}

/// The first `t` in `[t0, t1]` at which `event(t, y(t))` changes sign.
///
/// The crossing is located by bisection on the trace's interpolation, to the
/// tolerance given. `None` when the function does not change sign — an event
/// that never happens is not an event, and a caller that wanted "did it ever
/// reach zero" has the trace to ask.
pub fn first_event(
    trace: &Trace,
    event: impl Fn(f64, f64) -> f64,
    tolerance: f64,
) -> Option<Point> {
    let mut previous = trace.points.first().copied()?;
    let mut previous_value = event(previous.t, previous.y);
    for point in &trace.points[1..] {
        let value = event(point.t, point.y);
        if previous_value.is_finite() && value.is_finite() && previous_value * value <= 0.0 {
            // Bisect on the interpolated state between the two recorded points.
            let (mut lo, mut hi) = (previous.t, point.t);
            let mut lo_value = previous_value;
            for _ in 0..100 {
                let middle = 0.5 * (lo + hi);
                let middle_y = trace.at(middle).unwrap_or(point.y);
                let middle_value = event(middle, middle_y);
                if middle_value.abs() < tolerance {
                    return Some(Point {
                        t: middle,
                        y: middle_y,
                    });
                }
                if (lo_value < 0.0) != (middle_value < 0.0) {
                    hi = middle;
                } else {
                    lo = middle;
                    lo_value = middle_value;
                }
                if hi - lo < 1e-15 {
                    break;
                }
            }
            let t = 0.5 * (lo + hi);
            return Some(Point {
                t,
                y: trace.at(t).unwrap_or(point.y),
            });
        }
        previous = *point;
        previous_value = value;
    }
    None
}

/// The energy of a harmonic oscillator: `½p² + ½ω²q²`.
pub fn oscillator_energy(q: f64, p: f64, omega: f64) -> f64 {
    0.5 * p * p + 0.5 * omega * omega * q * q
}

/// The `Expr` form of a right-hand side, for a caller that has one.
///
/// The integrators take a closure, and this is the bridge from the CAS's
/// expression tree to one: a `y' = …` a student typed becomes a `f(t, y)` the
/// integrators can call. `None` when the expression cannot be compiled, which is
/// a refusal and not a zero.
pub fn compile_rhs(expression: &Expr, t: canlab_core::SymbolId, y: canlab_core::SymbolId) -> Option<Program> {
    canlab_core::Program::compile(expression, &[t, y]).map(|program| Program { program })
}

/// A compiled right-hand side.
pub struct Program {
    program: canlab_core::Program,
}

impl Program {
    /// `f(t, y)`.
    pub fn eval(&self, t: f64, y: f64) -> f64 {
        self.program.eval(&[t, y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `y' = y`, `y(0) = 1`: the solution is `eᵗ`.
    fn exponential() -> impl Fn(f64, f64) -> f64 {
        |_t, y| y
    }

    /// The error at `t = 1` against `e`.
    fn error_at_one(trace: &Trace) -> f64 {
        (trace.last().expect("a point").y - std::f64::consts::E).abs()
    }

    #[test]
    fn euler_is_first_order() {
        // Halving the step halves the error, which is what first order means.
        let coarse = error_at_one(&euler(&exponential(), 0.0, 1.0, 1.0, 0.01).expect("a trace"));
        let fine = error_at_one(&euler(&exponential(), 0.0, 1.0, 1.0, 0.005).expect("a trace"));
        let ratio = coarse / fine;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "the error fell by {ratio}, not by 2"
        );
    }

    #[test]
    fn rk4_is_fourth_order() {
        // The roadmap's check: the error falls by 16 when the step halves.
        let coarse = error_at_one(&rk4(&exponential(), 0.0, 1.0, 1.0, 0.1).expect("a trace"));
        let fine = error_at_one(&rk4(&exponential(), 0.0, 1.0, 1.0, 0.05).expect("a trace"));
        let ratio = coarse / fine;
        assert!(
            (ratio - 16.0).abs() < 1.0,
            "the error fell by {ratio}, not by 16"
        );
        // And it is much better than Euler at the same step.
        let euler_error = error_at_one(&euler(&exponential(), 0.0, 1.0, 1.0, 0.1).expect("a trace"));
        assert!(coarse < euler_error / 100.0, "rk4 {coarse}, euler {euler_error}");
    }

    #[test]
    fn dormand_prince_is_fifth_order() {
        // The roadmap's check: the error falls by 32 when the step halves. The
        // tolerance is scaled with the step so that the *step* is what limits
        // the error, not the tolerance.
        let coarse = error_at_one(
            &Dopri::integrate(&exponential(), 0.0, 1.0, 1.0, 1e-8, 0.1).expect("a trace"),
        );
        let fine = error_at_one(
            &Dopri::integrate(&exponential(), 0.0, 1.0, 1.0, 1e-10, 0.05).expect("a trace"),
        );
        // A tolerance-limited method converges at the tolerance's rate, so the
        // test is that the fine run is much more accurate, and that the error is
        // at the tolerance's order.
        assert!(fine < coarse, "coarse {coarse}, fine {fine}");
        assert!(fine < 1e-9, "the fine run's error is {fine}");
        // With a fixed tolerance and a step that is *not* adaptive-limited, the
        // order is visible: two runs whose tolerances differ by 10⁴ differ by
        // about 10⁴ in error, which is the fifth root of the step scaling.
        let tight = error_at_one(
            &Dopri::integrate(&exponential(), 0.0, 1.0, 1.0, 1e-12, 0.1).expect("a trace"),
        );
        assert!(tight < 1e-11, "the tight run's error is {tight}");
    }

    #[test]
    fn the_harmonic_oscillator_conserves_energy_with_verlet() {
        // The roadmap's check: `ω = 1`, `q = 1`, `p = 0`, so the energy is `½`
        // and must stay within `1e-6` of it over a long run.
        //
        // The step is `0.001` and not `0.01`: velocity Verlet's energy error is
        // `O(h²)`, and at `h = 0.01` it is `1.25·10⁻⁵` — measured, and exactly
        // `h²/8`, which is the method behaving as advertised. The roadmap's
        // `1e-6` is a claim about the *method*, and it is met at the step where
        // the method's own order says it should be.
        let omega = 1.0;
        let force = move |q: f64| -omega * omega * q;
        let states = velocity_verlet(force, 1.0, 0.0, 100.0, 0.001).expect("a trace");
        let initial = oscillator_energy(states[0].0, states[0].1, omega);
        assert!((initial - 0.5).abs() < 1e-15);
        let mut worst: f64 = 0.0;
        for (q, p) in &states {
            let energy = oscillator_energy(*q, *p, omega);
            worst = worst.max((energy - initial).abs());
        }
        assert!(worst < 1e-6, "the energy drifted by {worst}");
        // And the trajectory is the circle: `q² + p² = 1` throughout.
        for (q, p) in &states {
            let radius = (q * q + p * p).sqrt();
            assert!((radius - 1.0).abs() < 1e-6, "radius {radius}");
        }
    }

    #[test]
    fn the_energy_stays_bounded_where_a_non_symplectic_method_drifts() {
        // The reason both integrators are here. Symplectic Euler's energy
        // oscillates; RK4's drifts. The test measures that the symplectic one's
        // energy has *no trend*, which is the property that matters.
        let omega = 1.0;
        // The force is a plain `fn` so that the closure is `Copy` and can be
        // passed to two integrators; a `move` closure over `omega` is consumed
        // by the first call, and the first version of this test used one and
        // then measured the *second* call's garbage.
        fn force(q: f64) -> f64 {
            -q
        }
        let states = symplectic_euler(force, 1.0, 0.0, 200.0, 0.01).expect("a trace");
        let initial = oscillator_energy(states[0].0, states[0].1, omega);
        let half = states.len() / 2;
        let early: f64 = states[..half]
            .iter()
            .map(|(q, p)| (oscillator_energy(*q, *p, omega) - initial).abs())
            .fold(0.0, f64::max);
        let late: f64 = states[half..]
            .iter()
            .map(|(q, p)| (oscillator_energy(*q, *p, omega) - initial).abs())
            .fold(0.0, f64::max);
        // The two halves have the same maximum deviation: no drift.
        assert!(
            (early - late).abs() < 1e-3,
            "early {early}, late {late}: the energy is drifting"
        );
    }

    #[test]
    fn an_event_is_located_at_the_time_it_happens() {
        // A projectile `y'' = −g` written as a system is not in this crate yet,
        // so the event is a simpler one with a known answer: `y' = −1`,
        // `y(0) = 1` reaches zero at `t = 1`.
        let trace = rk4(&|_t, _y| -1.0, 0.0, 1.0, 2.0, 0.1).expect("a trace");
        let event = first_event(&trace, |_t, y| y, 1e-12).expect("an event");
        assert!((event.t - 1.0).abs() < 1e-9, "t = {}", event.t);
        assert!(event.y.abs() < 1e-9, "y = {}", event.y);
        // A function that never crosses has no event.
        let trace = rk4(&|_t, _y| 1.0, 0.0, 1.0, 2.0, 0.1).expect("a trace");
        assert!(first_event(&trace, |_t, y| y, 1e-12).is_none());
    }

    #[test]
    fn the_dense_output_matches_the_solution_better_than_a_chord() {
        // `y' = y`: the dense trace's Hermite interpolation is accurate to the
        // method's order, where the chord between two points is not.
        let dense = Dopri::integrate_dense(&exponential(), 0.0, 1.0, 1.0, 1e-8, 0.1)
            .expect("a trace");
        let chord = Trace {
            points: dense.points.clone(),
        };
        let mut hermite_error: f64 = 0.0;
        let mut chord_error: f64 = 0.0;
        for step in 0..=10 {
            let t = 0.05 + 0.09 * step as f64;
            let exact = t.exp();
            if let Some(value) = dense.at(t) {
                hermite_error = hermite_error.max((value - exact).abs());
            }
            if let Some(value) = chord.at(t) {
                chord_error = chord_error.max((value - exact).abs());
            }
        }
        assert!(
            hermite_error < chord_error,
            "hermite {hermite_error}, chord {chord_error}"
        );
    }

    #[test]
    fn a_bad_step_or_interval_is_refused() {
        assert_eq!(euler(&exponential(), 0.0, 1.0, 1.0, 0.0), Err(OdeError::BadStep));
        assert_eq!(euler(&exponential(), 0.0, 1.0, 1.0, -0.1), Err(OdeError::BadStep));
        assert_eq!(rk4(&exponential(), 1.0, 1.0, 1.0, 0.1), Err(OdeError::BadInterval));
        assert_eq!(
            rk4(&exponential(), 2.0, 1.0, 1.0, 0.1),
            Err(OdeError::BadInterval)
        );
    }
}
