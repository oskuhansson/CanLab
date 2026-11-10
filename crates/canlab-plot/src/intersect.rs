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
/// Residual tolerance for calling a point a solution, relative to the function
/// values at the starting guess. The circle `x² + y² = 10¹²` subtracts numbers
/// of order 10¹² near its edge, where one ulp is about 2e-4: an absolute 10⁻⁹
/// asks for more precision than f64 has at that magnitude.
const RESIDUAL: f64 = 1.0e-9;
/// Roots closer than this fraction of their coordinates are the same root. At
/// x = 10⁶ the duplicate radius is 10⁻³; at x = 1 it stays 10⁻⁹. Scaling by the
/// coordinates is what lets one root be merged at both scales.
const DUPLICATE_EPSILON: f64 = 1.0e-9;

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
    // around 2e-4 and a fixed 10⁻⁹ can never be met. Scaling by the starting
    // values keeps both cases meaningful without pretending to more precision
    // than f64 has.
    let f_start = f(x, y);
    let g_start = g(x, y);
    if !f_start.is_finite() || !g_start.is_finite() {
        return None;
    }
    let residual_tolerance = RESIDUAL * 1.0_f64.max(f_start.abs()).max(g_start.abs());

    for _ in 0..MAX_ITERATIONS {
        let fx = f(x, y);
        let gx = g(x, y);
        if !fx.is_finite() || !gx.is_finite() {
            return None;
        }
        if fx.abs() < residual_tolerance && gx.abs() < residual_tolerance {
            return Some(Intersection { x, y });
        }

        // Scale the step with the coordinates: a curve at x = 10⁶ needs a bigger
        // difference than one at x = 1 or the derivative is pure cancellation.
        let h = 1.0e-6 * x.abs().max(y.abs()).max(1.0);
        let dxx = (f(x + h, y) - f(x - h, y)) / (2.0 * h);
        let dxy = (f(x, y + h) - f(x, y - h)) / (2.0 * h);
        let dgx = (g(x + h, y) - g(x - h, y)) / (2.0 * h);
        let dgy = (g(x, y + h) - g(x, y - h)) / (2.0 * h);

        let determinant = dxx * dgy - dxy * dgx;
        if !determinant.is_finite() || determinant.abs() < 1.0e-14 {
            // Parallel tangents: a tangency, a cusp, or no crossing at all.
            return None;
        }

        // Solve `J · δ = -(f, g)` for the step and take it.
        let step_x = (-fx * dgy + gx * dxy) / determinant;
        let step_y = (-gx * dxx + fx * dgx) / determinant;
        x += step_x;
        y += step_y;

        // A step of 10⁻⁷ is tiny at x = 10⁶ but large at x = 1. Compare it with
        // the coordinate scale so "no progress" means the same at both. The
        // point it stopped at may be the root: check it here rather than refuse
        // it for having arrived with a small step.
        let step_tolerance = STEP_EPSILON * 1.0_f64.max(x.abs()).max(y.abs());
        if step_x.abs() < step_tolerance && step_y.abs() < step_tolerance {
            let (fx, gx) = (f(x, y), g(x, y));
            let arrived = fx.is_finite()
                && gx.is_finite()
                && fx.abs() < residual_tolerance
                && gx.abs() < residual_tolerance;
            return arrived.then_some(Intersection { x, y });
        }
    }

    // Converged slowly but did converge: accept only if it really is a root.
    let (fx, gx) = (f(x, y), g(x, y));
    (fx.abs() < residual_tolerance * 1.0e3 && gx.abs() < residual_tolerance * 1.0e3)
        .then_some(Intersection { x, y })
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
        let duplicate = found.iter().any(|existing| {
            // The same root can come from two adjacent rings, and the two
            // refinements need only agree to a relative precision. Scale the
            // radius by the coordinates: 10⁻⁹ at x = 1 and 10⁻³ at x = 10⁶, so
            // the test means the same thing at both scales. An absolute 10⁻⁶
            // split one root at small scale and was too fine at large scale.
            let x_tolerance = DUPLICATE_EPSILON * 1.0_f64.max(existing.x.abs()).max(point.x.abs());
            let y_tolerance = DUPLICATE_EPSILON * 1.0_f64.max(existing.y.abs()).max(point.y.abs());
            (existing.x - point.x).abs() <= x_tolerance
                && (existing.y - point.y).abs() <= y_tolerance
        });
        if !duplicate {
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
    fn a_small_step_at_a_large_coordinate_is_not_a_refusal_by_itself() {
        // The guess is 5e-8 from the root at (10⁶, 10⁶). At that coordinate the
        // step is below the relative step tolerance (10⁻⁷), but it lands on the
        // root; refusing it only because the step is small would discard a real
        // answer.
        let root = 1.0e6;
        let first = |_x: f64, y: f64| y - root;
        let second = |x: f64, _y: f64| x - root;
        let point = refine(first, second, (root + 5.0e-8, root)).expect("converges");
        assert!((point.x - root).abs() < 1e-9, "{point:?}");
        assert!((point.y - root).abs() < 1e-9, "{point:?}");
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
        assert!((found[0].x + root).abs() < 1e-6, "{found:?}");
        assert!((found[0].y + root).abs() < 1e-6, "{found:?}");
        assert!((found[1].x - root).abs() < 1e-6, "{found:?}");
        assert!((found[1].y - root).abs() < 1e-6, "{found:?}");
    }

    #[test]
    fn duplicate_roots_a_million_units_out_are_judged_on_a_relative_distance() {
        // Two hints half a thousandth either side of (10⁶, 10⁶), so 10⁻³
        // apart. The functions are scaled down so both already satisfy the
        // residual tolerance and `refine` returns them unchanged; that isolates
        // the duplicate radius. It is 10⁻⁹ of the coordinate, or 10⁻³ here, so
        // the pair is one root. The old absolute 10⁻⁶ called them two.
        let root = 1.0e6;
        let vertical = |_x: f64, y: f64| (y - root) * 1.0e-6;
        let horizontal = |x: f64, _y: f64| (x - root) * 1.0e-6;
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
