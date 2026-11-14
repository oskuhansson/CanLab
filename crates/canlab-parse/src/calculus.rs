//! W3.3: the tangent line, the normal, and where a function is heading.
//!
//! Three questions an exam asks about a curve, answered from the derivative
//! that [`canlab_core::differentiate`] already computes:
//!
//! * **The tangent at `x₀`** is `y = f(x₀) + f'(x₀)(x − x₀)`, and the normal
//!   is perpendicular to it. Both are *lines*, so both are returned as the two
//!   numbers that describe one — a slope and a point — and the caller can print
//!   either the equation or the two points.
//! * **The extrema over `[a, b]`** are the critical points inside the interval
//!   plus the two endpoints. The endpoints are what a first version forgets, and
//!   the roadmap's own test is a cubic whose global maximum is at an endpoint.
//! * **Monotonicity** is the sign of the derivative on each piece between
//!   critical points, which is the sign table an exam writes out.
//!
//! # What "critical point" means here
//!
//! A point where `f' = 0`, found by the solver's own root finder over the
//! interval rather than by a second search. A point where `f'` does not exist
//! is *not* found, and the answer says so by omission: a cusp is a place this
//! module does not claim, and claiming it wrongly would be worse than not
//! claiming it.

use canlab_core::expr::Expr;
use canlab_core::symbol::SymbolId;

use crate::limit::evaluate;

/// A line, as a point and a slope.
///
/// `slope` is `None` for a vertical line, which a tangent is where the
/// derivative is infinite — a case that is reported rather than divided by
/// zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// A point on the line.
    pub at: (f64, f64),
    /// The slope, or `None` for a vertical line.
    pub slope: Option<f64>,
}

impl Line {
    /// The line's value at `x`, when it is not vertical.
    pub fn y_at(&self, x: f64) -> Option<f64> {
        self.slope.map(|m| self.at.1 + m * (x - self.at.0))
    }
}

/// Why a tangent or an extremum has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum CalculusError {
    /// The derivative could not be taken.
    NoDerivative,
    /// The point is not a number, or the function is not defined there.
    BadPoint,
    /// The interval is empty or its ends are not numbers.
    BadInterval,
}

impl CalculusError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            CalculusError::NoDerivative => {
                "the derivative of this expression could not be taken".to_string()
            }
            CalculusError::BadPoint => {
                "the function is not defined at this point".to_string()
            }
            CalculusError::BadInterval => {
                "the interval is empty or its ends are not numbers".to_string()
            }
        }
    }
}

/// The tangent line to `curve` at `x₀`.
pub fn tangent(curve: &Expr, var: SymbolId, at: f64) -> Result<Line, CalculusError> {
    let derivative = canlab_core::differentiate(curve, var).ok_or(CalculusError::NoDerivative)?;
    let y = evaluate(curve, var, at);
    let slope = evaluate(&derivative, var, at);
    if !y.is_finite() {
        return Err(CalculusError::BadPoint);
    }
    if !slope.is_finite() {
        // An infinite slope is a vertical tangent, which is a line and not a
        // failure: `y = x^(1/3)` at zero has one.
        return Ok(Line {
            at: (at, y),
            slope: None,
        });
    }
    Ok(Line {
        at: (at, y),
        slope: Some(slope),
    })
}

/// The normal line to `curve` at `x₀`: perpendicular to the tangent.
///
/// A horizontal tangent has a vertical normal and vice versa; both are lines
/// this type can describe, so neither is a special case at the call site.
pub fn normal(curve: &Expr, var: SymbolId, at: f64) -> Result<Line, CalculusError> {
    let tangent = tangent(curve, var, at)?;
    let slope = match tangent.slope {
        // A horizontal tangent's normal is vertical, and a vertical tangent's
        // normal is horizontal. `-1.0 / 0.0` is `-inf`, which is not a slope, so
        // the zero case has to be named rather than divided.
        Some(0.0) => None,
        None => Some(0.0),
        Some(m) => Some(-1.0 / m),
    };
    Ok(Line {
        at: tangent.at,
        slope,
    })
}

/// A point where the function is a candidate for an extremum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extremum {
    /// Where.
    pub x: f64,
    /// The value there.
    pub y: f64,
    /// Whether it is an endpoint of the interval rather than a critical point.
    pub endpoint: bool,
}

/// Where a function's largest and smallest values on `[a, b]` are.
#[derive(Debug, Clone, PartialEq)]
pub struct Extrema {
    /// The largest value and where it is. Ties go to the smaller `x`.
    pub maximum: Extremum,
    /// The smallest value and where it is.
    pub minimum: Extremum,
    /// The critical points inside the interval that the searches certify, in
    /// increasing order — the points a sign table is built from, whether or not
    /// they are the extrema.
    ///
    /// Not a proof of completeness: the scan's resolution is one of its 1024
    /// cells, so two zeros inside one cell can be missed, a derivative that is
    /// zero on a whole interval has no isolated point to report, and points
    /// closer than a thousandth of the interval (with a `10⁻¹⁵` floor) are
    /// reported as one. See `critical_points`.
    pub critical_points: Vec<f64>,
}

/// The extrema of `curve` on `[a, b]`, including the endpoints.
pub fn extrema(curve: &Expr, var: SymbolId, a: f64, b: f64) -> Result<Extrema, CalculusError> {
    if !(a.is_finite() && b.is_finite()) || a >= b {
        return Err(CalculusError::BadInterval);
    }
    let derivative = canlab_core::differentiate(curve, var).ok_or(CalculusError::NoDerivative)?;
    let critical_points = critical_points(&derivative, var, a, b);
    let mut candidates = vec![
        Extremum {
            x: a,
            y: evaluate(curve, var, a),
            endpoint: true,
        },
        Extremum {
            x: b,
            y: evaluate(curve, var, b),
            endpoint: true,
        },
    ];
    for &x in &critical_points {
        let y = evaluate(curve, var, x);
        if y.is_finite() {
            candidates.push(Extremum {
                x,
                y,
                endpoint: false,
            });
        }
    }
    if candidates.iter().any(|c| !c.y.is_finite()) {
        return Err(CalculusError::BadPoint);
    }
    // The first maximum and the first minimum, with ties going to the smaller
    // `x` because the candidates are built in increasing order and the
    // comparison is strict.
    let mut maximum = candidates[0];
    let mut minimum = candidates[0];
    for candidate in &candidates[1..] {
        if candidate.y > maximum.y {
            maximum = *candidate;
        }
        if candidate.y < minimum.y {
            minimum = *candidate;
        }
    }
    Ok(Extrema {
        maximum,
        minimum,
        critical_points,
    })
}

/// Where the derivative vanishes in `(a, b)`, by a sign-change scan and a
/// bracketed minimum of `|f'|`.
///
/// Two kinds of zero of `f'` are critical points, and they need two searches:
///
/// * a **crossing** (`x²` at 0) changes sign, and the sign-change scan finds it;
/// * a **touching** zero (`x³` at 0) does not. It is found on the bracket around
///   a sampled *local minimum of `|f'|`*, by minimising `|f'|` there rather than
///   waiting for the sampled value to fall under a threshold. That is the change
///   that makes a root which lands halfway between two samples findable: its
///   nearest sampled magnitude can be above the threshold and the zero is still
///   a minimum of the bracket. A candidate is confirmed by evaluating the
///   derivative at the minimum; a local minimum of `|f'|` that is not near zero
///   is a place where the slope is merely small, not a critical point.
///
/// What this does not do is prove completeness: the scan has `SAMPLES` cells, so
/// two zeros inside one cell are one sample's worth of evidence and may be
/// missed, a derivative that is zero on a whole interval has no isolated
/// minimum, and the final deduplication reports points closer than
/// `10⁻³·(b − a)` (with a `10⁻¹⁵` floor) as one. The claim in
/// [`Extrema::critical_points`] is
/// correspondingly the points this search certifies, not every point a
/// symbolic root finder would.
fn critical_points(derivative: &Expr, var: SymbolId, a: f64, b: f64) -> Vec<f64> {
    const SAMPLES: usize = 1024;
    let step = (b - a) / SAMPLES as f64;
    let mut found = Vec::new();
    let mut previous = evaluate(derivative, var, a);
    for index in 1..=SAMPLES {
        let x = a + step * index as f64;
        let value = evaluate(derivative, var, x);
        if previous.is_finite() && value.is_finite() {
            if previous == 0.0 {
                // A zero exactly on a sample. The interval's own ends are not
                // "inside" it, and `extrema` adds them as endpoints itself.
                let sample = x - step;
                if sample > a && sample < b {
                    found.push(sample);
                }
            } else if previous * value < 0.0 {
                let (mut lo, mut hi) = (x - step, x);
                for _ in 0..80 {
                    let middle = 0.5 * (lo + hi);
                    let fm = evaluate(derivative, var, middle);
                    if fm == 0.0 || hi - lo < 1e-15 {
                        lo = middle;
                        hi = middle;
                        break;
                    }
                    if (previous < 0.0) != (fm < 0.0) {
                        hi = middle;
                    } else {
                        lo = middle;
                    }
                }
                let root = 0.5 * (lo + hi);
                if root > a && root < b {
                    found.push(root);
                }
            }
        }
        previous = value;
    }
    // The touching zeros: the minimum of `|f'|` around each sampled local
    // minimum. The local-minimum test is what keeps the search on the zero and
    // not on every cell on the way down; the *threshold* is only a check on the
    // refined minimum, not a gate on whether to look. Without that split, a
    // multiplicity-two zero halfway between samples has a nearest sampled
    // magnitude above the threshold and was not looked for at all.
    let scale = {
        let mut largest: f64 = 0.0;
        for index in 0..=SAMPLES {
            let x = a + step * index as f64;
            let value = evaluate(derivative, var, x).abs();
            if value.is_finite() {
                largest = largest.max(value);
            }
        }
        largest.max(1.0)
    };
    let threshold = 1e-7 * scale;
    let mut previous_magnitude = evaluate(derivative, var, a).abs();
    let mut previous_x = a;
    for index in 1..=SAMPLES {
        let x = a + step * index as f64;
        let magnitude = evaluate(derivative, var, x).abs();
        // The bracket is the sample on each side of this one. The right end of
        // the interval has no sample beyond it, so its "next" is `+∞`: a
        // minimum at `b` is not inside the interval and is filtered below.
        let (next_magnitude, bracket_end) = if index < SAMPLES {
            (
                evaluate(derivative, var, a + step * (index + 1) as f64).abs(),
                a + step * (index + 1) as f64,
            )
        } else {
            (f64::INFINITY, b)
        };
        if magnitude.is_finite()
            && previous_magnitude.is_finite()
            && magnitude < previous_magnitude
            && magnitude <= next_magnitude
        {
            let refined = minimise_magnitude(derivative, var, previous_x, bracket_end);
            if refined > a && refined < b && evaluate(derivative, var, refined).abs() < threshold {
                found.push(refined);
            }
        }
        previous_magnitude = magnitude;
        previous_x = x;
    }
    found.sort_by(f64::total_cmp);
    // The tolerance is the scan's own resolution, one cell, not a claim that
    // two distinct roots this close are the same: the search cannot tell them
    // apart, and reporting one point is the honest version of that. The
    // sampling step is `(b − a)/1024`, so a thousandth of the interval is one
    // cell to the precision this scan can speak to; the `10⁻¹⁵` floor keeps a
    // tiny interval from collapsing more than its own cell.
    let tolerance = 1e-3 * (b - a).abs().max(1e-12);
    found.dedup_by(|latter, former| (*latter - *former).abs() < tolerance);
    found
}

/// A point in `[lo, hi]` where `|f'|` is smallest, by golden-section search.
///
/// The search is on the magnitude and not on the sign of `f'`: through a
/// touching zero `f'` does not change sign, so a bisection on the signed
/// derivative has no crossing to find and walks to the bracket's end. `|f'|`
/// does have a minimum there, and golden section follows it. A non-finite value
/// is treated as `+∞` so a pole cannot win the search.
fn minimise_magnitude(derivative: &Expr, var: SymbolId, lo: f64, hi: f64) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_894_9;
    let mut lo = lo;
    let mut hi = hi;
    let mut left = hi - INV_PHI * (hi - lo);
    let mut right = lo + INV_PHI * (hi - lo);
    let mut left_value = magnitude(derivative, var, left);
    let mut right_value = magnitude(derivative, var, right);
    for _ in 0..100 {
        if hi - lo <= 1e-15 * (1.0 + lo.abs().max(hi.abs())) {
            break;
        }
        if left_value < right_value {
            hi = right;
            right = left;
            right_value = left_value;
            left = hi - INV_PHI * (hi - lo);
            left_value = magnitude(derivative, var, left);
        } else {
            lo = left;
            left = right;
            left_value = right_value;
            right = lo + INV_PHI * (hi - lo);
            right_value = magnitude(derivative, var, right);
        }
    }
    0.5 * (lo + hi)
}

/// `|f'(x)|`, with a non-finite value read as `+∞` for the minimum search.
fn magnitude(derivative: &Expr, var: SymbolId, x: f64) -> f64 {
    let value = evaluate(derivative, var, x).abs();
    if value.is_finite() {
        value
    } else {
        f64::INFINITY
    }
}

/// One piece of a sign table: an interval and what the function does on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// Left end.
    pub from: f64,
    /// Right end.
    pub to: f64,
    /// `+1` where the derivative is positive (increasing), `−1` where it is
    /// negative (decreasing), `0` where it is zero throughout.
    pub direction: i8,
}

/// The sign table of `curve` on `[a, b]`: the pieces between critical points,
/// and the direction on each.
///
/// This is the artefact an exam writes out, and the one a plot can shade: a
/// function is increasing exactly where its derivative is positive, and the
/// table says which pieces those are.
pub fn sign_table(curve: &Expr, var: SymbolId, a: f64, b: f64) -> Result<Vec<Piece>, CalculusError> {
    if !(a.is_finite() && b.is_finite()) || a >= b {
        return Err(CalculusError::BadInterval);
    }
    let derivative = canlab_core::differentiate(curve, var).ok_or(CalculusError::NoDerivative)?;
    let mut bounds = vec![a];
    bounds.extend(critical_points(&derivative, var, a, b));
    bounds.push(b);
    let mut pieces = Vec::new();
    for pair in bounds.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if to - from < 1e-12 {
            continue;
        }
        let middle = 0.5 * (from + to);
        let value = evaluate(&derivative, var, middle);
        let direction = if !value.is_finite() {
            // A pole of the derivative: the table cannot say, and a `0` would
            // claim "flat". The piece is reported as neither and the caller
            // sees a `0` with the value of the derivative available elsewhere.
            0
        } else if value > 0.0 {
            1
        } else if value < 0.0 {
            -1
        } else {
            0
        };
        pieces.push(Piece {
            from,
            to,
            direction,
        });
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::SymbolId;

    fn curve(source: &str) -> Expr {
        crate::parse_latex(source).expect("parses")
    }

    fn x() -> SymbolId {
        SymbolId::intern("x")
    }

    #[test]
    fn a_cubics_extrema_include_the_endpoints() {
        // The roadmap's own case: `f = x³ − 3x² + 1` on `[−1, 3]`. The critical
        // points are 0 and 2, the local maximum at 0 is 1, and the *global*
        // maximum is 1 at the endpoint `x = 3` — a version that only looks at
        // critical points answers `x = 0` and is wrong.
        let f = curve("x^3 - 3x^2 + 1");
        let extrema = extrema(&f, x(), -1.0, 3.0).expect("extrema");
        assert_eq!(extrema.critical_points.len(), 2, "{:?}", extrema.critical_points);
        assert!((extrema.critical_points[0] - 0.0).abs() < 1e-9);
        assert!((extrema.critical_points[1] - 2.0).abs() < 1e-9);
        assert!((extrema.maximum.x - 3.0).abs() < 1e-9, "{:?}", extrema.maximum);
        assert!(extrema.maximum.endpoint, "the maximum is at an endpoint");
        assert!((extrema.minimum.x - (-1.0)).abs() < 1e-9, "{:?}", extrema.minimum);
        assert!((extrema.minimum.y - (-3.0)).abs() < 1e-9);
    }

    #[test]
    fn a_local_maximum_inside_the_interval_is_found_too() {
        // `f = 4x − x²` on `[0, 5]`: the maximum is at `x = 2`, inside.
        let f = curve("4x - x^2");
        let extrema = extrema(&f, x(), 0.0, 5.0).expect("extrema");
        assert!(!extrema.maximum.endpoint);
        assert!((extrema.maximum.x - 2.0).abs() < 1e-9);
        assert!((extrema.maximum.y - 4.0).abs() < 1e-9);
    }

    #[test]
    fn the_tangent_touches_the_curve_at_its_point_and_nowhere_else_nearby() {
        // The roadmap's second check. `f = x²` at `x₀ = 1`: the tangent is
        // `y = 2x − 1`, it agrees with `f` at 1, and it is *below* the curve
        // everywhere else — which is what "touches and does not cross" means for
        // a convex curve.
        let f = curve("x^2");
        let tangent = tangent(&f, x(), 1.0).expect("a tangent");
        assert_eq!(tangent.at, (1.0, 1.0));
        assert_eq!(tangent.slope, Some(2.0));
        assert!((tangent.y_at(1.0).expect("not vertical") - 1.0).abs() < 1e-12);
        for step in [-0.5, -0.1, 0.1, 0.5] {
            let x = 1.0 + step;
            let on_curve = x * x;
            let on_line = tangent.y_at(x).expect("not vertical");
            assert!(on_line < on_curve, "at {x}: line {on_line} curve {on_curve}");
        }
    }

    #[test]
    fn the_normal_is_perpendicular_to_the_tangent() {
        let f = curve("x^2");
        let the_tangent = tangent(&f, x(), 1.0).expect("a tangent");
        let the_normal = normal(&f, x(), 1.0).expect("a normal");
        let m = the_tangent.slope.expect("not vertical");
        let n = the_normal.slope.expect("not vertical");
        assert!((m * n + 1.0).abs() < 1e-12, "{m} * {n}");
        // A horizontal tangent has a vertical normal, and the normal of a
        // vertical tangent is horizontal.
        let flat = curve("3");
        let flat_tangent = tangent(&flat, x(), 0.0).expect("a tangent");
        assert_eq!(flat_tangent.slope, Some(0.0));
        let flat_normal = normal(&flat, x(), 0.0).expect("a normal");
        assert_eq!(flat_normal.slope, None);
    }

    #[test]
    fn the_sign_table_says_where_the_function_increases() {
        // `f = x³ − 3x² + 1`: increasing on `[−1, 0]`, decreasing on `[0, 2]`,
        // increasing on `[2, 3]`. The table is the artefact an exam writes.
        let f = curve("x^3 - 3x^2 + 1");
        let table = sign_table(&f, x(), -1.0, 3.0).expect("a table");
        let directions: Vec<i8> = table.iter().map(|piece| piece.direction).collect();
        assert_eq!(directions, vec![1, -1, 1], "{table:?}");
        // The pieces are contiguous and cover the interval.
        assert!((table[0].from - (-1.0)).abs() < 1e-12);
        assert!((table.last().expect("pieces").to - 3.0).abs() < 1e-12);
        for pair in table.windows(2) {
            assert!((pair[0].to - pair[1].from).abs() < 1e-12);
        }
    }

    #[test]
    fn a_touching_zero_of_the_derivative_is_a_critical_point() {
        // `x³` at 0: `f' = 3x²` touches zero and does not change sign. The
        // first version of `critical_points` found only sign changes and missed
        // it, which the module's documentation said out loud; this is the test
        // that makes the sentence false.
        let f = curve("x^3");
        let extrema = extrema(&f, x(), -1.0, 1.0).expect("extrema");
        assert_eq!(extrema.critical_points.len(), 1, "{:?}", extrema.critical_points);
        assert!(extrema.critical_points[0].abs() < 1e-9, "{:?}", extrema.critical_points);
        // And the sign table has two pieces, both increasing: the function is
        // increasing either side of a stationary point of inflection.
        let table = sign_table(&f, x(), -1.0, 1.0).expect("a table");
        assert_eq!(table.len(), 2, "{table:?}");
        assert!(table.iter().all(|piece| piece.direction == 1), "{table:?}");
    }

    #[test]
    fn a_flat_minimum_is_one_critical_point_and_not_three() {
        // `x⁴` at 0: `f' = 4x³` is very flat there, and a search for a small
        // `|f'|` found the cell that first dipped under the threshold *and* the
        // real minimum. A local-minimum test and a bisection on the signed
        // derivative leave one point.
        let f = curve("x^4");
        let extrema = extrema(&f, x(), -1.0, 1.0).expect("extrema");
        assert_eq!(extrema.critical_points.len(), 1, "{:?}", extrema.critical_points);
        assert!(extrema.critical_points[0].abs() < 1e-9);
    }

    #[test]
    fn an_empty_or_unbounded_interval_is_refused() {
        let f = curve("x^2");
        assert_eq!(extrema(&f, x(), 1.0, 1.0), Err(CalculusError::BadInterval));
        assert_eq!(extrema(&f, x(), 2.0, 1.0), Err(CalculusError::BadInterval));
        assert_eq!(sign_table(&f, x(), 0.0, f64::INFINITY), Err(CalculusError::BadInterval));
    }

    #[test]
    fn a_touching_zero_between_samples_is_still_a_critical_point() {
        // The root is exactly halfway between two samples: the nearest sampled
        // `|f'|` is about `9.5e-7` of the derivative's own scale, which is above
        // the `1e-7` threshold the sampled scan uses. The old guard therefore
        // missed a genuine even-multiplicity zero. Minimising `|f'|` on the
        // bracket around the sampled local minimum finds it.
        let f = curve("(x - 0.50048828125)^3");
        let extrema = extrema(&f, x(), 0.0, 1.0).expect("extrema");
        assert_eq!(
            extrema.critical_points.len(),
            1,
            "{:?}",
            extrema.critical_points
        );
        assert!(
            (extrema.critical_points[0] - 0.50048828125).abs() < 1e-9,
            "{:?}",
            extrema.critical_points
        );
    }

    #[test]
    fn two_critical_points_within_one_cell_are_reported_as_one() {
        // `f' = 2(x − 0.5)(x − 0.5001)(2x − 1.0001)` has three roots, all within
        // `1.1·10⁻⁴` of each other, inside one 1/1024 cell. The scan cannot
        // separate them, and the documented tolerance reports one point; this
        // pins the limitation rather than pretending the answer is complete.
        let f = curve("(x - 0.5)^2 (x - 0.5001)^2");
        let extrema = extrema(&f, x(), 0.0, 1.0).expect("extrema");
        assert_eq!(
            extrema.critical_points.len(),
            1,
            "{:?}",
            extrema.critical_points
        );
    }
}
