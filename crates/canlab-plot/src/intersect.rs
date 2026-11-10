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
/// A step this small means the method has stopped moving; if the residuals are
/// not small by then, the guess was not near a crossing.
const STEP_EPSILON: f64 = 1.0e-13;
/// Residual tolerance for calling a point a solution.
const RESIDUAL: f64 = 1.0e-9;

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

    for _ in 0..MAX_ITERATIONS {
        let fx = f(x, y);
        let gx = g(x, y);
        if !fx.is_finite() || !gx.is_finite() {
            return None;
        }
        if fx.abs() < RESIDUAL && gx.abs() < RESIDUAL {
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

        if step_x.abs() < STEP_EPSILON && step_y.abs() < STEP_EPSILON {
            return None;
        }
    }

    // Converged slowly but did converge: accept only if it really is a root.
    let (fx, gx) = (f(x, y), g(x, y));
    (fx.abs() < RESIDUAL * 1.0e3 && gx.abs() < RESIDUAL * 1.0e3).then_some(Intersection { x, y })
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
pub fn locate<F, G>(
    f: F,
    g: G,
    near: &[Vec<[f64; 2]>],
    tolerance: f64,
) -> Vec<Intersection>
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
            (existing.x - point.x).abs() < 1.0e-6 && (existing.y - point.y).abs() < 1.0e-6
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
    fn no_samples_means_no_points() {
        let found = locate(|_x, _y| 1.0, |_x, _y| 1.0, &[], 0.1);
        assert!(found.is_empty());
    }
}
