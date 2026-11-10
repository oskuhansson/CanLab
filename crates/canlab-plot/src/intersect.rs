//! Where two implicit curves meet, as a point rather than a blob.
//!
//! The implicit tracer follows sign changes, so a *crossing* of a level set is
//! easy to find and a *touching* is not: the sum-of-squares trick used to locate
//! intersections turns each crossing into a small ring whose thickness depends on
//! the grid, the epsilon and how steep the surface is. That ring is a hint, not an
//! answer.
//!
//! This module turns the hint into a number. Each cluster of ring samples is a
//! starting guess, and Newton's method on the pair `(f, g)` converges to the point
//! where both are zero — quadratically, so half a dozen iterations is enough even
//! from a guess a grid cell away.
//!
//! Tolerances here are relative to the problem's scale: convergence is judged by
//! the position a residual implies (`|f| / |∇f|`) against the coordinates, steps
//! and duplicate distances are relative to the coordinates, and a Jacobian is
//! singular only when its determinant is small compared with the products that
//! form it. Multiplying both equations or the coordinates by a constant therefore
//! leaves the crossing unchanged.
//!
//! # Why it is a separate module
//!
//! Nothing here knows about egui, curves or colours: two closures, a pair of
//! coordinates, and arithmetic. That is what makes it testable against known
//! answers (`x² + y² = 1` and `y = x` meet at `±1/√2`) instead of against a
//! picture.

use std::collections::BTreeMap;

/// A point where two curves meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intersection {
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

/// How many Newton steps are attempted before giving up.
const MAX_ITERATIONS: usize = 24;
/// A step this small relative to the coordinates means the method has stopped
/// moving; if the residuals are not small by then, the guess was not near a
/// crossing. At x = 10⁶ the threshold is 10⁻⁷, so a step that size counts as
/// no progress rather than as a real move.
const STEP_EPSILON: f64 = 1.0e-13;
/// Convergence tolerance, relative to the problem's scale. The cheap path uses
/// it on residuals compared with the starting values; the positional test uses
/// it on `|f| / |∇f|` compared with the coordinates. The circle
/// `x² + y² = 10¹²` subtracts numbers of order 10¹² near its edge, where one ulp
/// is about 2e-4: an absolute 10⁻⁹ asks for more precision than f64 has at that
/// magnitude.
const RESIDUAL: f64 = 1.0e-9;
/// A Jacobian is singular when its determinant is negligible compared with the
/// two products that form it. An absolute cutoff is a units error: multiplying
/// both functions by 10⁻⁸ scales the determinant by 10⁻¹⁶, so a real crossing
/// would be called a tangency. The sum of the product magnitudes has the same
/// units as the determinant, which makes this test scale-invariant.
const SINGULAR_EPSILON: f64 = 1.0e-12;
/// Roots closer than this fraction of their coordinates are the same root. At
/// x = 10⁶ the duplicate radius is 10⁻³; at x = 1 it stays 10⁻⁹. Scaling by the
/// coordinates is what lets one root be merged at both scales.
const DUPLICATE_EPSILON: f64 = 1.0e-9;

/// The central-difference Jacobian at `(x, y)`, as `(df/dx, df/dy, dg/dx, dg/dy)`.
fn jacobian<F, G>(f: &F, g: &G, x: f64, y: f64) -> (f64, f64, f64, f64)
where
    F: Fn(f64, f64) -> f64,
    G: Fn(f64, f64) -> f64,
{
    // Scale the step with the coordinates: a curve at x = 10⁶ needs a bigger
    // difference than one at x = 1 or the derivative is pure cancellation.
    let h = 1.0e-6 * x.abs().max(y.abs()).max(1.0);
    (
        (f(x + h, y) - f(x - h, y)) / (2.0 * h),
        (f(x, y + h) - f(x, y - h)) / (2.0 * h),
        (g(x + h, y) - g(x - h, y)) / (2.0 * h),
        (g(x, y + h) - g(x, y - h)) / (2.0 * h),
    )
}

/// The Jacobian determinant, or `None` when the Jacobian is singular at the
/// scale of its own terms.
fn invertible_determinant(dxx: f64, dxy: f64, dgx: f64, dgy: f64) -> Option<f64> {
    let first_product = dxx * dgy;
    let second_product = dxy * dgx;
    let determinant = first_product - second_product;
    let product_scale = first_product.abs() + second_product.abs();
    // Compare the determinant with the terms it came from, not with an
    // absolute number: the determinant has those terms' units. A zero Jacobian
    // has no scale at all and is the degenerate singular case.
    if !determinant.is_finite()
        || product_scale == 0.0
        || determinant.abs() <= SINGULAR_EPSILON * product_scale
    {
        None
    } else {
        Some(determinant)
    }
}

/// Whether both residuals imply a root close enough to count as this point.
///
/// `|f| / |∇f|` is the first-order distance from the point to `f = 0`. Comparing
/// that distance with the coordinate scale makes the test independent of both
/// the function's units and the coordinate's units. A zero gradient gives no
/// distance at all, so it is refused rather than read as a zero distance.
fn near_root(fx: f64, gx: f64, dxx: f64, dxy: f64, dgx: f64, dgy: f64, x: f64, y: f64) -> bool {
    let coordinate_scale = 1.0_f64.max(x.abs()).max(y.abs());
    let f_gradient = dxx.abs() + dxy.abs();
    let g_gradient = dgx.abs() + dgy.abs();
    f_gradient.is_finite()
        && f_gradient > 0.0
        && g_gradient.is_finite()
        && g_gradient > 0.0
        && fx.abs() <= RESIDUAL * f_gradient * coordinate_scale
        && gx.abs() <= RESIDUAL * g_gradient * coordinate_scale
}

/// Newton's method for `f(x, y) = 0` and `g(x, y) = 0`.
///
/// The Jacobian is taken by central differences. Symbolic derivatives exist in
/// `canlab-core`, but the curves here are arbitrary runtime expressions sampled
/// on a grid, and a numeric Jacobian of two cheap closures is faster than
/// compiling, differentiating and recompiling them.
pub fn refine<F, G>(f: F, g: G, start: (f64, f64)) -> Option<Intersection>
where
    F: Fn(f64, f64) -> f64,
    G: Fn(f64, f64) -> f64,
{
    let mut x = start.0;
    let mut y = start.1;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }

    // The tolerance has to follow the problem's own scale. On a unit circle a
    // residual of 10⁻⁹ is precise; on a circle of radius 10⁶ the function
    // subtracts x² + y² from 10¹², so values near the edge have a resolution
    // around 2e-4 and a fixed 10⁻⁹ can never be met. The starting values set
    // that scale without an absolute floor: a floor would break scale
    // invariance, so multiplying both equations by 10⁻⁸ would let a point far
    // from the root pass on an unscaled 10⁻⁹ residual.
    let f_start = f(x, y);
    let g_start = g(x, y);
    if !f_start.is_finite() || !g_start.is_finite() {
        return None;
    }
    let residual_scale = f_start.abs().max(g_start.abs());
    let residual_tolerance = RESIDUAL * residual_scale;

    for _ in 0..MAX_ITERATIONS {
        let fx = f(x, y);
        let gx = g(x, y);
        if !fx.is_finite() || !gx.is_finite() {
            return None;
        }
        if fx.abs() < residual_tolerance && gx.abs() < residual_tolerance {
            return Some(Intersection { x, y });
        }

        let (dxx, dxy, dgx, dgy) = jacobian(&f, &g, x, y);
        let Some(determinant) = invertible_determinant(dxx, dxy, dgx, dgy) else {
            // Parallel tangents: a tangency, a cusp, or no crossing at all.
            return None;
        };

        // Newton at an exact root of a large-valued function sees the f64 noise
        // floor, not zero. Judge the point by where that residual says it is,
        // not by the residual alone.
        if near_root(fx, gx, dxx, dxy, dgx, dgy, x, y) {
            return Some(Intersection { x, y });
        }

        // Solve `J · δ = -(f, g)` for the step and take it.
        let step_x = (-fx * dgy + gx * dxy) / determinant;
        let step_y = (-gx * dxx + fx * dgx) / determinant;
        x += step_x;
        y += step_y;

        // A step of 10⁻⁷ is tiny at x = 10⁶ but large at x = 1. Stop moving,
        // but do not refuse yet: the point arrived at is checked below.
        let step_tolerance = STEP_EPSILON * 1.0_f64.max(x.abs()).max(y.abs());
        if step_x.abs() < step_tolerance && step_y.abs() < step_tolerance {
            break;
        }
    }

    // The loop stopped moving or ran out of iterations. Check the point on the
    // same positional terms as the loop; a small step is not a refusal by
    // itself, and a residual alone is not an acceptance.
    let (fx, gx) = (f(x, y), g(x, y));
    if !fx.is_finite() || !gx.is_finite() {
        return None;
    }
    let (dxx, dxy, dgx, dgy) = jacobian(&f, &g, x, y);
    invertible_determinant(dxx, dxy, dgx, dgy)?;
    near_root(fx, gx, dxx, dxy, dgx, dgy, x, y).then_some(Intersection { x, y })
}

/// Group nearby sample points into one guess per crossing.
///
/// The ring around a crossing is a handful of points a grid cell apart; averaging
/// each group is a better starting guess than any single point, and the grouping
/// is what makes four crossings come out as four points rather than forty.
fn cluster(points: &[[f64; 2]], tolerance: f64) -> Vec<(f64, f64)> {
    if tolerance <= 0.0 || !tolerance.is_finite() {
        return Vec::new();
    }
    let mut groups: BTreeMap<(i64, i64), (f64, f64, usize)> = BTreeMap::new();
    for point in points {
        if !point[0].is_finite() || !point[1].is_finite() {
            continue;
        }
        let cell = (
            (point[0] / tolerance).round() as i64,
            (point[1] / tolerance).round() as i64,
        );
        let entry = groups.entry(cell).or_insert((0.0, 0.0, 0));
        entry.0 += point[0];
        entry.1 += point[1];
        entry.2 += 1;
    }
    // Neighbouring cells can belong to the same crossing; merge groups whose
    // means are within one tolerance of each other.
    let mut means: Vec<(f64, f64)> = groups
        .into_values()
        .map(|(sum_x, sum_y, count)| (sum_x / count as f64, sum_y / count as f64))
        .collect();
    means.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for mean in means {
        let near = merged.iter().any(|existing| {
            (existing.0 - mean.0).abs() <= tolerance && (existing.1 - mean.1).abs() <= tolerance
        });
        if !near {
            merged.push(mean);
        }
    }
    merged
}

/// Whether two refined points are the same root at their coordinate scale.
fn same_root(first: &Intersection, second: &Intersection) -> bool {
    // The same root can come from two adjacent rings, and the two refinements
    // need only agree to a relative precision. Scale the radius by the
    // coordinates: 10⁻⁹ at x = 1 and 10⁻³ at x = 10⁶, so the test means the same
    // thing at both scales. An absolute 10⁻⁶ split one root at small scale and
    // was too fine at large scale.
    let x_tolerance = DUPLICATE_EPSILON * 1.0_f64.max(first.x.abs()).max(second.x.abs());
    let y_tolerance = DUPLICATE_EPSILON * 1.0_f64.max(first.y.abs()).max(second.y.abs());
    (first.x - second.x).abs() <= x_tolerance && (first.y - second.y).abs() <= y_tolerance
}

/// Locate the intersections of two curves, given samples *near* them.
///
/// `near` is the tracer's near-zero level set — the hint. Each cluster of it is
/// refined; points that converge to the same root are reported once, so a ring
/// becomes a point and two rings that overlap do not become two answers.
pub fn locate<F, G>(f: F, g: G, near: &[Vec<[f64; 2]>], tolerance: f64) -> Vec<Intersection>
where
    F: Fn(f64, f64) -> f64,
    G: Fn(f64, f64) -> f64,
{
    let points: Vec<[f64; 2]> = near.iter().flatten().copied().collect();
    let mut found: Vec<Intersection> = Vec::new();
    for guess in cluster(&points, tolerance) {
        let Some(point) = refine(&f, &g, guess) else {
            continue;
        };
        if !found.iter().any(|existing| same_root(existing, &point)) {
            found.push(point);
        }
    }
    found.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_and_a_line_meet_at_the_expected_points() {
        // x² + y² = 1 and y = x  =>  (±1/√2, ±1/√2)
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let line = |x: f64, y: f64| y - x;
        let root = std::f64::consts::FRAC_1_SQRT_2;

        let point = refine(circle, line, (0.6, 0.55)).expect("converges");
        assert!((point.x - root).abs() < 1e-9, "{point:?}");
        assert!((point.y - root).abs() < 1e-9, "{point:?}");

        let point = refine(circle, line, (-0.6, -0.55)).expect("converges");
        assert!((point.x + root).abs() < 1e-9, "{point:?}");
    }

    #[test]
    fn scaling_both_functions_leaves_the_crossing_where_it_was() {
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let line = |x: f64, y: f64| y - x;
        let guess = (0.6, 0.55);
        let expected = refine(circle, line, guess).expect("unscaled converges");

        // Multiplying both equations by the same constant does not move their
        // zero set. It scales the determinant by the square of the constant, so
        // the scaled copies are where an absolute singularity cutoff fails: at
        // 10⁻⁸ the determinant is around 10⁻¹⁶ even though the crossing is real.
        for scale in [1.0e-8, 1.0e8] {
            let point = refine(
                |x, y| scale * circle(x, y),
                |x, y| scale * line(x, y),
                guess,
            )
            .unwrap_or_else(|| panic!("scale {scale:e}"));
            assert!(
                (point.x - expected.x).abs() <= 1.0e-9 * expected.x.abs(),
                "scale {scale:e}: {point:?} vs {expected:?}"
            );
            assert!(
                (point.y - expected.y).abs() <= 1.0e-9 * expected.y.abs(),
                "scale {scale:e}: {point:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn the_answer_does_not_depend_on_the_quality_of_the_guess() {
        let circle = |x: f64, y: f64| x * x + y * y - 4.0;
        let line = |x: f64, y: f64| x + y - 2.5;
        // Two guesses on different sides of the circle. They converge to
        // different crossings, so the check is "both are real answers", not
        // "they are equal".
        for guess in [(2.0, 1.0), (0.5, 1.5)] {
            let point = refine(circle, line, guess).unwrap_or_else(|| panic!("{guess:?}"));
            assert!(
                (point.x * point.x + point.y * point.y - 4.0).abs() < 1e-9,
                "not on the circle: {point:?}"
            );
            assert!(
                (point.x + point.y - 2.5).abs() < 1e-9,
                "not on the line: {point:?}"
            );
        }
    }

    #[test]
    fn a_guess_where_the_jacobian_is_singular_is_refused_not_guessed_at() {
        // On the ray perpendicular to `x + y = 2.5` the two gradients are
        // parallel, so the Newton step is undefined. That is a "no", not a point.
        let circle = |x: f64, y: f64| x * x + y * y - 4.0;
        let line = |x: f64, y: f64| x + y - 2.5;
        assert!(refine(circle, line, (0.1, 0.1)).is_none());
    }

    #[test]
    fn a_tangency_is_not_reported_as_a_crossing() {
        // x² + y² = 1 touches y = 1 at exactly (0, 1): the Jacobian is singular
        // there, and inventing a nearby point would be worse than saying nothing.
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let tangent = |_x: f64, _y: f64| 0.0;
        let refined = refine(circle, tangent, (0.05, 0.95));
        assert!(refined.is_none(), "got {refined:?}");
    }

    #[test]
    fn a_tangency_at_its_touch_point_is_not_reported_as_a_crossing() {
        // Starting exactly at (0, 1) makes both residuals zero, but the
        // Jacobian is still singular. Zero residuals do not make a tangency a
        // crossing.
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let tangent = |_x: f64, _y: f64| 0.0;
        let refined = refine(circle, tangent, (0.0, 1.0));
        assert!(refined.is_none(), "got {refined:?}");
    }

    #[test]
    fn a_zero_gradient_is_not_convergence() {
        // The zero function has a zero gradient everywhere. At (5, 0) the
        // circle residual is 24, so this is not a solution; a zero gradient
        // gives no distance to the zero set and must not be read as one.
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let refined = refine(circle, |_x: f64, _y: f64| 0.0, (5.0, 0.0));
        assert!(refined.is_none(), "got {refined:?}");

        // The same rule inside the positional test itself: a zero gradient is
        // not a zero implied distance.
        assert!(!near_root(0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 5.0, 0.0));
    }

    #[test]
    fn a_zero_jacobian_is_still_singular() {
        // Both functions are non-zero constants: every derivative is zero and
        // the two products are both zero. There is no scale to compare with, so
        // this is the degenerate singular case rather than a relative pass.
        let refined = refine(|_x: f64, _y: f64| 1.0, |_x: f64, _y: f64| 2.0, (0.0, 0.0));
        assert!(refined.is_none(), "got {refined:?}");
    }

    #[test]
    fn parallel_lines_do_not_converge() {
        let first = |x: f64, y: f64| y - x;
        let second = |x: f64, y: f64| y - x - 1.0;
        assert!(refine(first, second, (0.0, 0.5)).is_none());
    }

    #[test]
    fn a_ring_of_samples_becomes_one_point_per_crossing() {
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let line = |x: f64, y: f64| y - x;
        // A crude stand-in for the tracer: a few points scattered around each
        // crossing, which is what the epsilon-ring actually looks like.
        let near = vec![
            vec![[0.69, 0.71], [0.72, 0.70], [0.70, 0.72]],
            vec![[-0.70, -0.71], [-0.68, -0.71]],
        ];
        let found = locate(circle, line, &near, 0.1);
        assert_eq!(found.len(), 2, "{found:?}");
        let root = std::f64::consts::FRAC_1_SQRT_2;
        assert!((found[0].x + root).abs() < 1e-6, "{found:?}");
        assert!((found[1].x - root).abs() < 1e-6, "{found:?}");
    }

    #[test]
    fn duplicate_roots_are_reported_once() {
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let line = |x: f64, y: f64| y - x;
        // The same crossing sampled twice, a hundredth apart.
        let near = vec![vec![[0.70, 0.70]], vec![[0.701, 0.699]]];
        assert_eq!(locate(circle, line, &near, 0.05).len(), 1);
    }

    #[test]
    fn a_circle_of_radius_a_million_refines_at_the_scale_of_its_values() {
        // x² + y² = 10¹² and y = x meet at ±(10⁶/√2, 10⁶/√2). The residuals
        // here are of order 10⁶, so the old absolute 10⁻⁹ tolerance could never
        // be reached even though the root is perfectly representable.
        let radius = 1.0e6;
        let circle = |x: f64, y: f64| x * x + y * y - radius * radius;
        let line = |x: f64, y: f64| y - x;
        let root = radius / std::f64::consts::SQRT_2;

        // Exactly one unit away along the line. The root is 10⁶ from the
        // origin, where the old absolute residual tolerance had no chance.
        let offset = std::f64::consts::FRAC_1_SQRT_2;
        let point = refine(circle, line, (root + offset, root + offset)).expect("converges");
        assert!((point.x - root).abs() < 1e-6, "{point:?}");
        assert!((point.y - root).abs() < 1e-6, "{point:?}");
    }

    #[test]
    fn an_exact_root_of_a_large_valued_function_is_accepted() {
        // At the root of x² + y² = 10¹² the computed residual is the f64 noise
        // floor, about 2e-4, not zero. Judging that residual alone refuses a
        // point that is as exact as its coordinates allow.
        let radius = 1.0e6;
        let circle = |x: f64, y: f64| x * x + y * y - radius * radius;
        let line = |x: f64, y: f64| y - x;
        let root = radius / std::f64::consts::SQRT_2;

        let point = refine(circle, line, (root, root)).expect("exact root converges");
        assert!((point.x - root).abs() < 1e-6, "{point:?}");
        assert!((point.y - root).abs() < 1e-6, "{point:?}");
    }

    #[test]
    fn an_exact_root_at_unit_scale_is_accepted() {
        let circle = |x: f64, y: f64| x * x + y * y - 1.0;
        let line = |x: f64, y: f64| y - x;
        let root = std::f64::consts::FRAC_1_SQRT_2;

        let point = refine(circle, line, (root, root)).expect("exact root converges");
        assert!((point.x - root).abs() < 1e-9, "{point:?}");
        assert!((point.y - root).abs() < 1e-9, "{point:?}");
    }

    #[test]
    fn a_large_crossing_is_unchanged_by_the_function_scale() {
        let radius = 1.0e6;
        let circle = |x: f64, y: f64| x * x + y * y - radius * radius;
        let line = |x: f64, y: f64| y - x;
        let root = radius / std::f64::consts::SQRT_2;
        let offset = std::f64::consts::FRAC_1_SQRT_2;
        let guess = (root + offset, root + offset);
        let expected = refine(circle, line, guess).expect("unscaled converges");

        // One unit from the crossing is far enough that the positional test has
        // to walk there, and scaling both functions must not move the result.
        for scale in [1.0e-8, 1.0e8] {
            let point = refine(
                |x, y| scale * circle(x, y),
                |x, y| scale * line(x, y),
                guess,
            )
            .unwrap_or_else(|| panic!("scale {scale:e}"));
            assert!(
                (point.x - expected.x).abs() <= 1.0e-9 * expected.x.abs(),
                "scale {scale:e}: {point:?} vs {expected:?}"
            );
            assert!(
                (point.y - expected.y).abs() <= 1.0e-9 * expected.y.abs(),
                "scale {scale:e}: {point:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn a_near_root_at_a_large_coordinate_is_accepted_by_its_position() {
        // The guess is 5e-8 from the root at (10⁶, 10⁶). Its residual is 5e-8,
        // which the residual-only test rejects, but the implied distance is
        // 5e-8, well below the positional tolerance (10⁻³) at this coordinate.
        // It is a real answer, not a refused one.
        let root = 1.0e6;
        let first = |_x: f64, y: f64| y - root;
        let second = |x: f64, _y: f64| x - root;
        let point = refine(first, second, (root + 5.0e-8, root)).expect("converges");
        assert!((point.x - root).abs() <= 1e-3, "{point:?}");
        assert!((point.y - root).abs() <= 1e-3, "{point:?}");
    }

    #[test]
    fn locate_finds_both_crossings_at_a_millionfold_scale() {
        let radius = 1.0e6;
        let circle = |x: f64, y: f64| x * x + y * y - radius * radius;
        let line = |x: f64, y: f64| y - x;
        let root = radius / std::f64::consts::SQRT_2;

        // A few hint points a couple of units from each crossing. The caller's
        // tolerance is a data-space distance here, so a handful of units is
        // enough to group each ring without reaching across the diagram.
        let near = vec![
            vec![
                [root + 1.0, root + 2.0],
                [root + 2.0, root + 1.0],
                [root + 1.5, root + 1.5],
            ],
            vec![
                [-root - 1.0, -root - 2.0],
                [-root - 2.0, -root - 1.0],
                [-root - 1.5, -root - 1.5],
            ],
        ];
        let found = locate(circle, line, &near, 5.0);
        assert_eq!(found.len(), 2, "{found:?}");
        // The positional tolerance is 10⁻⁹ of the coordinate, not an absolute
        // 10⁻⁶: at this scale it is the relative accuracy that matters.
        let tolerance = 1.0e-9 * root;
        assert!((found[0].x + root).abs() <= tolerance, "{found:?}");
        assert!((found[0].y + root).abs() <= tolerance, "{found:?}");
        assert!((found[1].x - root).abs() <= tolerance, "{found:?}");
        assert!((found[1].y - root).abs() <= tolerance, "{found:?}");
    }

    #[test]
    fn duplicate_roots_a_million_units_out_are_judged_on_a_relative_distance() {
        // Two hints half a thousandth either side of (10⁶, 10⁶), so 10⁻³
        // apart. The duplicate radius is 10⁻⁹ of the coordinate, or 10⁻³ here,
        // so the pair is one root. The old absolute 10⁻⁶ called them two.
        let root = 1.0e6;
        let first = Intersection {
            x: root - 5.0e-4,
            y: root,
        };
        let second = Intersection {
            x: root + 5.0e-4,
            y: root,
        };
        assert!(same_root(&first, &second), "{first:?} {second:?}");

        // Twice that distance is no longer the same root.
        let farther = Intersection {
            x: root + 1.5e-3,
            y: root,
        };
        assert!(!same_root(&first, &farther), "{first:?} {farther:?}");
    }

    #[test]
    fn locate_still_reports_one_point_when_two_hints_refine_the_same_root() {
        // Two hints a thousandth apart at 10⁶. Both refine to the same
        // crossing, so the result is one point rather than two.
        let root = 1.0e6;
        let vertical = |_x: f64, y: f64| y - root;
        let horizontal = |x: f64, _y: f64| x - root;
        let near = vec![vec![[root - 5.0e-4, root]], vec![[root + 5.0e-4, root]]];
        let found = locate(vertical, horizontal, &near, 1.0e-4);
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn no_samples_means_no_points() {
        let found = locate(|_x, _y| 1.0, |_x, _y| 1.0, &[], 0.1);
        assert!(found.is_empty());
    }
}
