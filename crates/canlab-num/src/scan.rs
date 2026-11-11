//! A scan for every root in an interval, with deflation for repeated roots.
//!
//! A grid alone is not enough. Sampling `(x − 1)²` at `x = 0, 1, 2` finds a
//! zero, but sampling it at `x = 0, 0.9, 2` sees `1, 0.01, 1` and no sign
//! change; the root touches without crossing. The scan therefore does three
//! things:
//!
//! * an endpoint that evaluates to zero is a root, and is recorded as one;
//! * opposite signs at the ends of a cell are a bracket, refined by Brent;
//! * a sample that is a local minimum of `|f|`, with the same sign on both
//!   sides, opens a window one cell each way, minimised with golden-section
//!   search on `|f|`. That is how a touching root is found. The minimum is
//!   accepted only when it lies below both window ends and its residual is
//!   small; a smooth function that merely dips, or a root just outside the
//!   interval, is not claimed.
//!
//! Brent's result is checked the same way. A step function changes sign at its
//! jump, so Brent closes the bracket there, but the residual does not fall;
//! that candidate is refused rather than printed.
//!
//! # Deflation
//!
//! After a scan, every root it found is divided out of the function and the
//! scan runs again. A double root of `f` is still a root of `f/(x − r)`, so the
//! second pass finds it again, divides it again, and the root only disappears
//! once its multiplicity is exhausted. This is what lets the report contain the
//! root once: the first pass reports it, later passes recognise it as the same
//! root and only use it for the division.
//!
//! Dividing a black-box function by `(x − r)` needs one care: at `x = r` the
//! quotient is `0/0` or `finite/0`. The value there is estimated from the two
//! sides. Everywhere else the quotient is evaluated directly, so the
//! approximation stays smooth and the minimum search is not stopped beside the
//! root by an artificial plateau.

use crate::brent::brent_with_scale;
use crate::{
    ACCEPTANCE, Bracket, Root, RootError, check_finite, ordered_interval, relative_residual,
    same_root, validate_tolerance,
};

/// The number of cells in [`scan`]'s grid.
///
/// Five hundred and twelve cells is this crate's compromise: fine enough for a
/// school function's roots, cheap enough that a scan is not a visible pause.
/// [`scan_with_grid`] is there when the caller knows the function oscillates
/// faster.
pub const DEFAULT_GRID: usize = 512;

/// The largest grid [`scan_with_grid`] accepts.
///
/// The cap is about time, not correctness: every cell can run a Brent refinement
/// or a golden-section search, and 65536 cells would make a single scan a
/// visible pause on a document that has to redraw.
pub const MAX_GRID: usize = 65_536;

/// How many passes of "find, divide out, find again" one scan may make.
///
/// A root of multiplicity `m` costs `m + 1` passes; 32 is beyond any school
/// equation and keeps a pathological function from running forever.
const MAX_DEFLATION_ROUNDS: usize = 32;

/// The largest number of roots one scan will report.
const MAX_ROOTS: usize = 256;

/// Golden-section iterations. The bracket contracts by `0.618` per step, so 120
/// steps take a normal cell far below the smallest representable width.
const MINIMUM_ITERATIONS: usize = 120;

/// All roots of `f` in `interval`, using [`DEFAULT_GRID`].
///
/// The interval may be given in either order. `tolerance` is an absolute
/// tolerance on `x`, passed to Brent and to the minimum search; a touching root
/// needs a tolerance fine enough for its curvature, because the minimum is flat.
///
/// The returned roots are in ascending order and each appears once, however
/// many times its factor occurs. An interval with no root gives an empty vector;
/// a discontinuity that looks like a sign change is an error, not a root.
pub fn scan<F>(f: F, interval: (f64, f64), tolerance: f64) -> Result<Vec<Root>, RootError>
where
    F: Fn(f64) -> f64,
{
    scan_with_grid(f, interval, tolerance, DEFAULT_GRID)
}

/// [`scan`] with an explicit number of grid cells.
///
/// A finer grid resolves closer roots but costs proportionally more
/// evaluations. `grid` must be between one and [`MAX_GRID`].
pub fn scan_with_grid<F>(
    f: F,
    interval: (f64, f64),
    tolerance: f64,
    grid: usize,
) -> Result<Vec<Root>, RootError>
where
    F: Fn(f64) -> f64,
{
    let interval = ordered_interval(interval)?;
    validate_tolerance(tolerance)?;
    if grid == 0 || grid > MAX_GRID {
        return Err(RootError::InvalidGrid { grid });
    }

    let (first, scale) = scan_once(&f, interval, tolerance, grid)?;
    if first.is_empty() {
        return Ok(Vec::new());
    }

    let mut reported: Vec<Root> = Vec::new();
    let mut removed: Vec<f64> = Vec::new();
    let mut candidates = first;

    for _ in 0..MAX_DEFLATION_ROUNDS {
        if candidates.is_empty() {
            break;
        }
        // Every candidate is a factor of the current function, whether or not
        // it is new to the report. Dividing it out lowers its multiplicity.
        for candidate in &candidates {
            removed.push(candidate.value);
        }
        for candidate in &candidates {
            if reported
                .iter()
                .any(|root| same_root(root.value, candidate.value))
            {
                continue;
            }
            if reported.len() >= MAX_ROOTS {
                return Err(RootError::TooManyRoots);
            }
            reported.push(report(&f, candidate.value, candidate.bracket, scale)?);
        }

        let work = |x: f64| deflated_eval(&f, &removed, x);
        let (next, _) = scan_once(&work, interval, tolerance, grid)?;
        candidates = next;
    }

    if !candidates.is_empty() {
        return Err(RootError::TooManyRoots);
    }

    reported.sort_by(|left, right| left.value.total_cmp(&right.value));
    Ok(reported)
}

/// One root found by a single scan of the function it was given.
#[derive(Clone, Copy)]
struct Candidate {
    value: f64,
    residual: f64,
    bracket: Bracket,
}

/// One pass over the grid. Returns the roots and the scale used for residuals.
fn scan_once<F>(
    f: &F,
    interval: (f64, f64),
    tolerance: f64,
    grid: usize,
) -> Result<(Vec<Candidate>, f64), RootError>
where
    F: Fn(f64) -> f64,
{
    let (left, right) = interval;
    let width = right - left;
    let mut points = Vec::with_capacity(grid + 1);
    let mut values = Vec::with_capacity(grid + 1);
    for index in 0..=grid {
        // The last point is set to `right` rather than recomputed, so rounding
        // cannot leave the final cell shorter or past the interval.
        let x = if index == grid {
            right
        } else if index == 0 {
            left
        } else {
            left + width * (index as f64 / grid as f64)
        };
        let value = check_finite(x, f(x))?;
        points.push(x);
        values.push(value);
    }

    let scale = values
        .iter()
        .fold(1.0_f64, |largest, value| largest.max(value.abs()));
    let mut candidates: Vec<Candidate> = Vec::new();

    // A grid point that evaluates to exactly zero is a root without any
    // interpolation. It also means the two neighbouring cells are not brackets,
    // which is why the loops below skip them.
    for index in 0..=grid {
        if values[index] == 0.0 {
            let point = points[index];
            push_candidate(
                &mut candidates,
                Candidate {
                    value: point,
                    residual: 0.0,
                    bracket: Bracket::new(point, point),
                },
            );
        }
    }

    for index in 0..grid {
        let left_value = values[index];
        let right_value = values[index + 1];
        if left_value == 0.0 || right_value == 0.0 {
            continue;
        }
        if (left_value > 0.0) != (right_value > 0.0) {
            let root = brent_with_scale(f, (points[index], points[index + 1]), tolerance, scale)?;
            if root.residual > ACCEPTANCE {
                // The signs changed, but the function did not reach zero. That
                // is a jump, and reporting its location as a root would be the
                // silent lie the doc forbids.
                return Err(RootError::ResidualTooLarge {
                    value: root.value,
                    residual: root.residual,
                    limit: ACCEPTANCE,
                });
            }
            push_candidate(
                &mut candidates,
                Candidate {
                    value: root.value,
                    residual: root.residual,
                    bracket: root.bracket,
                },
            );
        }
    }

    // No sign change in this cell: the only way a root can be in it is a touch,
    // which is a local minimum of `|f|`. Minimising every cell would also
    // minimise the monotone cells, whose "minimum" is an endpoint that can be
    // close enough to a root to look small; a high-multiplicity root makes that
    // endpoint value tiny. Only the sampled local minima are worth a search, and
    // only where the sign does not change, because a simple root is Brent's job.
    for index in 0..=grid {
        if values[index] == 0.0 {
            continue;
        }
        let same_sign_as_left = index == 0 || (values[index - 1] > 0.0) == (values[index] > 0.0);
        let same_sign_as_right =
            index == grid || (values[index + 1] > 0.0) == (values[index] > 0.0);
        if !same_sign_as_left || !same_sign_as_right {
            continue;
        }
        let magnitude = values[index].abs();
        let no_larger_on_the_left = index == 0 || magnitude <= values[index - 1].abs();
        let no_larger_on_the_right = index == grid || magnitude <= values[index + 1].abs();
        if !no_larger_on_the_left || !no_larger_on_the_right {
            continue;
        }

        // The window reaches one sample past the minimum, so a root just off a
        // sample is interior to the search and golden section is not asked to
        // choose between two nearly equal endpoint values.
        let low = if index == 0 {
            points[0]
        } else {
            points[index - 1]
        };
        let high = if index == grid {
            points[grid]
        } else {
            points[index + 1]
        };
        let (minimum, minimum_value) = golden_minimise(f, low, high, tolerance)?;
        let window_left = values[if index == 0 { 0 } else { index - 1 }].abs();
        let window_right = values[if index == grid { grid } else { index + 1 }].abs();
        if minimum_value >= window_left.min(window_right) {
            // The minimum is at an end of the window: this is a monotone cell
            // (or an interval whose root lies just outside it), not a dip. An
            // exact endpoint root was recorded above, and a root just inside the
            // interval still has its minimum strictly below both window ends.
            continue;
        }
        let residual = relative_residual(minimum_value, scale);
        if residual <= ACCEPTANCE {
            push_candidate(
                &mut candidates,
                Candidate {
                    value: minimum,
                    residual,
                    bracket: Bracket::new(low, high),
                },
            );
        }
    }

    candidates.sort_by(|left, right| left.value.total_cmp(&right.value));
    Ok((candidates, scale))
}

/// Keep one candidate per root, preferring the one with the smaller residual.
fn push_candidate(candidates: &mut Vec<Candidate>, candidate: Candidate) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| same_root(existing.value, candidate.value))
    {
        if candidate.residual < existing.residual {
            *existing = candidate;
        }
    } else {
        candidates.push(candidate);
    }
}

/// The report for a confirmed root, with its residual measured on the
/// *original* function.
fn report<F>(f: &F, value: f64, bracket: Bracket, scale: f64) -> Result<Root, RootError>
where
    F: Fn(f64) -> f64,
{
    let f_value = check_finite(value, f(value))?;
    let residual = relative_residual(f_value, scale);
    if residual > ACCEPTANCE {
        return Err(RootError::ResidualTooLarge {
            value,
            residual,
            limit: ACCEPTANCE,
        });
    }
    Ok(Root {
        value,
        residual,
        bracket,
    })
}

/// The value of `f(x) / ∏(x − r)` for the roots already divided out.
///
/// At a point that is exactly a removed root the quotient is `0/0` or
/// `finite/0`, so the value is estimated from the two sides. A point that
/// merely comes close is evaluated directly: snapping a whole neighbourhood to
/// an estimate would flatten the quotient and move the minimum off the root.
fn deflated_eval<F>(f: &F, removed: &[f64], x: f64) -> f64
where
    F: Fn(f64) -> f64,
{
    let mut denominator = 1.0;
    for root in removed {
        denominator *= x - root;
    }

    let value = f(x);
    // A numerator that is exactly zero makes the quotient exactly zero wherever
    // the denominator is non-zero. This matters when the point is close to a
    // removed root: a two-sided estimate in that neighbourhood would flatten the
    // quotient into a plateau above the true zero, and the minimum search would
    // stop beside the root instead of on it.
    if denominator != 0.0 && value == 0.0 {
        return 0.0;
    }

    if denominator == 0.0 {
        // Exactly at a removed root the quotient is `0/0` or `finite/0`; taking
        // the two sides is the only defined answer. This is the only place the
        // estimate is needed, and keeping it here rather than in a whole
        // neighbourhood is what leaves the approximation smooth everywhere else.
        let step = 1e-7 * x.abs().max(1.0);
        let left = raw_quotient(f, removed, x - step);
        let right = raw_quotient(f, removed, x + step);
        if left.is_finite() && right.is_finite() {
            0.5 * (left + right)
        } else {
            f64::NAN
        }
    } else {
        value / denominator
    }
}

fn raw_quotient<F>(f: &F, removed: &[f64], x: f64) -> f64
where
    F: Fn(f64) -> f64,
{
    let mut denominator = 1.0;
    for root in removed {
        denominator *= x - root;
    }
    f(x) / denominator
}

/// Golden-section search for the minimum of `|f|` in `[left, right]`.
///
/// Returns the point and `|f(point)|`. The function is assumed unimodal in the
/// cell, which is what the scan's grid spacing is for; a cell that contains two
/// minima can return either of them.
fn golden_minimise<F>(f: &F, left: f64, right: f64, tolerance: f64) -> Result<(f64, f64), RootError>
where
    F: Fn(f64) -> f64,
{
    // 1/φ, the golden ratio's smaller reciprocal. Both inner points are placed
    // this fraction in from an end, so one survives each contraction.
    const INVERSE_PHI: f64 = 0.618_033_988_749_894_9;

    let mut a = left;
    let mut b = right;
    let mut c = b - INVERSE_PHI * (b - a);
    let mut d = a + INVERSE_PHI * (b - a);
    let mut fc = check_finite(c, f(c))?.abs();
    let mut fd = check_finite(d, f(d))?.abs();

    for _ in 0..MINIMUM_ITERATIONS {
        let width = b - a;
        let floor = 4.0 * f64::EPSILON * c.abs().max(d.abs()).max(1.0);
        if width <= tolerance.max(floor) {
            break;
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - INVERSE_PHI * (b - a);
            fc = check_finite(c, f(c))?.abs();
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + INVERSE_PHI * (b - a);
            fd = check_finite(d, f(d))?.abs();
        }
    }

    Ok(if fc <= fd { (c, fc) } else { (d, fd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nearly(left: f64, right: f64) -> bool {
        (left - right).abs() <= 1e-8 * left.abs().max(right.abs()).max(1.0)
    }

    fn assert_root(roots: &[Root], expected: f64) -> Root {
        roots
            .iter()
            .copied()
            .find(|root| nearly(root.value, expected))
            .unwrap_or_else(|| panic!("no root near {expected}; found {roots:?}"))
    }

    #[test]
    fn every_root_of_a_cubic_is_found_in_order() {
        let f = |x: f64| (x + 2.0) * (x - 0.5) * (x - 1.5);
        let roots = scan(f, (-3.0, 3.0), 1e-12).expect("a cubic has three roots");
        assert_eq!(roots.len(), 3, "found {roots:?}");
        assert_root(&roots, -2.0);
        assert_root(&roots, 0.5);
        assert_root(&roots, 1.5);
        for root in &roots {
            assert!(root.residual <= 1e-12, "{root:?}");
            assert!((f)(root.value).abs() <= 1e-12 * 10.0, "{root:?}");
        }
        assert!(
            roots.windows(2).all(|pair| pair[0].value < pair[1].value),
            "the report is not sorted: {roots:?}"
        );
    }

    #[test]
    fn a_double_root_is_found_without_a_sign_change_and_reported_once() {
        // The function is positive on both sides, so no cell changes sign. The
        // only evidence is a minimum of |f|.
        let f = |x: f64| (x - 0.37) * (x - 0.37);
        assert!((f)(0.36) > 0.0 && (f)(0.38) > 0.0);
        let roots = scan(f, (-2.0, 2.0), 1e-12).expect("a double root is a root");
        assert_eq!(
            roots.len(),
            1,
            "the double root was reported more than once: {roots:?}"
        );
        let root = roots[0];
        assert!((root.value - 0.37).abs() <= 1e-9, "{root:?}");
        assert!(root.residual <= 1e-12, "{root:?}");
        assert!(root.is_within(1e-12));
    }

    #[test]
    fn a_triple_root_is_reported_once() {
        // The sign does change here, so Brent finds it; deflation is what proves
        // the root is not reported three times by three passes.
        let f = |x: f64| {
            let shifted = x - 0.25;
            shifted * shifted * shifted
        };
        let roots = scan(f, (-2.0, 2.0), 1e-12).expect("a triple root is a root");
        assert_eq!(
            roots.len(),
            1,
            "the triple root was reported more than once: {roots:?}"
        );
        assert!((roots[0].value - 0.25).abs() <= 1e-8, "{:?}", roots[0]);
        assert!(roots[0].residual <= 1e-12, "{:?}", roots[0]);
    }

    #[test]
    fn a_quartic_touch_is_reported_once() {
        // Multiplicity four: an even root, so no sign change at all, and four
        // divisions are needed before the factor disappears.
        let f = |x: f64| {
            let shifted = x - 1.13;
            shifted * shifted * shifted * shifted
        };
        let roots = scan(f, (-2.0, 2.0), 1e-12).expect("a quartic touch is a root");
        assert_eq!(roots.len(), 1, "found {roots:?}");
        assert!((roots[0].value - 1.13).abs() <= 1e-8, "{:?}", roots[0]);
        assert!(roots[0].residual <= 1e-12, "{:?}", roots[0]);
    }

    #[test]
    fn deflation_keeps_mixed_multiplicities_to_one_entry_each() {
        // (x + 1)²(x − 2)³: one touching root and one crossing root, with
        // multiplicities two and three. Four deflation rounds are needed before
        // both factors are gone.
        let f = |x: f64| {
            let left = x + 1.0;
            let right = x - 2.0;
            left * left * right * right * right
        };
        let roots = scan(f, (-3.0, 3.0), 1e-12).expect("two roots");
        assert_eq!(roots.len(), 2, "found {roots:?}");
        assert_root(&roots, -1.0);
        assert_root(&roots, 2.0);
        for root in &roots {
            assert!(root.residual <= 1e-12, "{root:?}");
        }
    }

    #[test]
    fn a_root_at_each_end_of_the_interval_is_found_once() {
        let f = |x: f64| x * (x - 1.0);
        let roots = scan(f, (0.0, 1.0), 1e-12).expect("both ends are roots");
        assert_eq!(roots.len(), 2, "found {roots:?}");
        assert_eq!(roots[0].value, 0.0);
        assert_eq!(roots[1].value, 1.0);
    }

    #[test]
    fn an_interval_with_no_root_reports_none() {
        let roots = scan(|x| x * x + 1.0, (-2.0, 2.0), 1e-12).expect("x² + 1 is finite");
        assert!(roots.is_empty(), "a root was invented: {roots:?}");
    }

    #[test]
    fn a_near_miss_is_not_a_root() {
        // The minimum is 1e-6, far above the residual a root has to show. This
        // is the refusal that keeps "find a minimum" from becoming "invent a
        // root wherever |f| is small".
        let roots =
            scan(|x| (x - 1.0) * (x - 1.0) + 1e-6, (-2.0, 2.0), 1e-12).expect("a finite function");
        assert!(roots.is_empty(), "a near miss became a root: {roots:?}");
    }

    #[test]
    fn a_root_just_outside_the_interval_is_not_pulled_inside() {
        // (x + 0.001)⁴ rises from 1e-12 at x = 0. A residual-only test would
        // accept the interval's end as a root; the minimum-at-an-end rule keeps
        // it outside.
        let f = |x: f64| {
            let shifted = x + 0.001;
            shifted * shifted * shifted * shifted
        };
        let roots = scan(f, (0.0, 1.0), 1e-12).expect("the function is finite");
        assert!(
            roots.is_empty(),
            "a root outside the interval became one inside: {roots:?}"
        );
    }

    #[test]
    fn a_jump_is_refused_instead_of_reported_as_a_root() {
        let step = |x: f64| if x < 0.5 { -1.0 } else { 1.0 };
        let error =
            scan(step, (-1.0, 1.0), 1e-12).expect_err("a step changes sign without reaching zero");
        assert!(
            matches!(error, RootError::ResidualTooLarge { .. }),
            "unexpected refusal: {error}"
        );
        assert!(error.to_string().contains("jump"));
    }

    #[test]
    fn a_pole_is_refused_as_a_non_finite_value() {
        let error = scan(|x| 1.0 / x, (-1.0, 1.0), 1e-12).expect_err("the grid lands on the pole");
        assert!(
            matches!(error, RootError::NonFinite { .. }),
            "unexpected refusal: {error}"
        );
    }

    #[test]
    fn a_reversed_interval_is_accepted() {
        let roots = scan(|x| (x + 1.0) * (x - 2.0), (3.0, -3.0), 1e-12)
            .expect("a reversed interval is still an interval");
        assert_eq!(roots.len(), 2, "found {roots:?}");
        assert_root(&roots, -1.0);
        assert_root(&roots, 2.0);
    }

    #[test]
    fn an_invalid_interval_is_refused() {
        for interval in [(1.0, 1.0), (f64::NAN, 1.0), (0.0, f64::INFINITY)] {
            let error = scan(|x| x, interval, 1e-12).expect_err("this interval cannot be scanned");
            assert!(
                matches!(error, RootError::InvalidInterval { .. }),
                "interval {interval:?} gave {error}"
            );
        }
    }

    #[test]
    fn a_transcendental_root_is_found_by_the_scan() {
        let roots = scan(|x| x.sin(), (3.0, 4.0), 1e-12).expect("sin is finite on this interval");
        assert_eq!(roots.len(), 1, "found {roots:?}");
        assert!(
            (roots[0].value - std::f64::consts::PI).abs() <= 1e-12,
            "{:?}",
            roots[0]
        );
        assert!(roots[0].residual <= 1e-12, "{:?}", roots[0]);
    }

    #[test]
    fn a_bad_tolerance_or_grid_is_refused() {
        for tolerance in [0.0, -1.0, f64::NAN] {
            let error = scan(|x| x, (0.0, 1.0), tolerance)
                .expect_err("this tolerance cannot stop a search");
            assert!(
                matches!(error, RootError::InvalidTolerance { .. }),
                "tolerance {tolerance} gave {error}"
            );
        }
        for grid in [0, MAX_GRID + 1] {
            let error = scan_with_grid(|x| x, (0.0, 1.0), 1e-12, grid)
                .expect_err("this grid is outside the cap");
            assert!(
                matches!(error, RootError::InvalidGrid { .. }),
                "grid {grid} gave {error}"
            );
        }
    }

    #[test]
    fn too_many_roots_has_a_refusal() {
        // 128.25 periods of a sine put 257 zeros in the interval, one more than
        // the cap, with every zero away from an end so the scan reports the
        // count rather than the endpoint rule.
        let f = |x: f64| (std::f64::consts::TAU * x).sin();
        let error = scan_with_grid(f, (0.0, 128.25), 1e-12, 8192)
            .expect_err("257 roots exceed the report cap");
        assert_eq!(error, RootError::TooManyRoots);
        assert!(error.to_string().contains("more roots"));
    }

    #[test]
    fn the_residual_of_a_large_function_is_relative() {
        // f is a million times (x − 1); its absolute residual near the root is
        // large for any ordinary tolerance, while its relative residual is not.
        let f = |x: f64| 1e6 * (x - 1.0);
        let roots = scan(f, (0.0, 2.0), 1e-12).expect("a straight line has one root");
        assert_eq!(roots.len(), 1);
        assert!(roots[0].residual <= 1e-12, "{:?}", roots[0]);
    }

    const ROOTS: [(i64, i64); 11] = [
        (-8, 3),
        (-2, 1),
        (-4, 3),
        (-1, 1),
        (-2, 3),
        (-1, 3),
        (1, 3),
        (2, 3),
        (4, 3),
        (2, 1),
        (8, 3),
    ];

    #[test]
    fn fifty_known_products_keep_their_residual_below_1e_12() {
        // The roots are exact rationals and the function is the product of the
        // three factors, so the true roots are known without solving anything.
        // The first fifty triples in lexicographic order are used, not a random
        // sample: a failure names the same equation on every run.
        let mut equations = 0usize;
        'triples: for (first, &(a_num, a_den)) in ROOTS.iter().enumerate() {
            for (second, &(b_num, b_den)) in ROOTS.iter().enumerate().skip(first + 1) {
                for &(c_num, c_den) in ROOTS.iter().skip(second + 1) {
                    if equations == 50 {
                        break 'triples;
                    }
                    let a = a_num as f64 / a_den as f64;
                    let b = b_num as f64 / b_den as f64;
                    let c = c_num as f64 / c_den as f64;
                    let f = |x: f64| (x - a) * (x - b) * (x - c);

                    // The same scale the default scan samples, recomputed here
                    // so the relative-residual assertion does not merely trust
                    // the number the crate put in the struct.
                    let mut scale = 1.0_f64;
                    for index in 0..=DEFAULT_GRID {
                        let x = -4.0 + 8.0 * (index as f64 / DEFAULT_GRID as f64);
                        scale = scale.max(f(x).abs());
                    }

                    let roots = scan(f, (-4.0, 4.0), 1e-12)
                        .unwrap_or_else(|error| panic!("equation {equations}: {error}"));
                    assert_eq!(
                        roots.len(),
                        3,
                        "equation {equations} ({a}, {b}, {c}) found {roots:?}"
                    );

                    let mut expected = [a, b, c];
                    expected.sort_by(f64::total_cmp);
                    for (root, wanted) in roots.iter().zip(expected) {
                        assert!(
                            (root.value - wanted).abs() <= 1e-8,
                            "equation {equations}: {} is not {wanted}",
                            root.value
                        );
                        assert!(
                            root.residual <= 1e-12,
                            "equation {equations} ({a}, {b}, {c}): root {} has residual {}",
                            root.value,
                            root.residual
                        );
                        let relative = f(root.value).abs() / scale;
                        assert!(
                            relative <= 1e-12,
                            "equation {equations} ({a}, {b}, {c}): |f| / scale is {relative} at {}",
                            root.value
                        );
                        assert!(root.residual.is_finite());
                    }
                    equations += 1;
                }
            }
        }
        assert_eq!(equations, 50);
    }
}
