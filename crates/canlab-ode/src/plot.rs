//! W6.3: a trajectory as a curve the plot crate can draw.
//!
//! [`canlab_plot::Curve`] is a list of polylines where a *break* means "the
//! function is not connected here". A trajectory has the same problem a sampled
//! function does: `y' = y²` blows up at a finite time, and drawing a line from
//! `10⁶` to `10⁹` across the blow-up joins two points that are not connected.
//! The plot crate's own rule is that the sampler makes the continuity decision
//! and the renderer has none left to make; this module is the sampler for a
//! trajectory.
//!
//! # The three plots
//!
//! * **`y` against `t`** — the trajectory itself, one segment per connected
//!   stretch.
//! * **The phase portrait** — `y₂` against `y₁` for a two-dimensional system,
//!   which is the picture a pendulum's behaviour is read from.
//! * **A 3D path** — once W8's viewport exists. Until then a 3D trajectory is
//!   projected to its `(y₁, y₂)` pair, and the doc says that is a projection and
//!   not a 3D plot.
//!
//! # What makes a break
//!
//! A point is dropped, and a segment ends, when
//!
//! * a coordinate is not finite — the integrator reached infinity;
//! * the jump from the previous point is larger than the *scale* of the
//!   trajectory, which is how a blow-up looks in finite numbers: the step is
//!   small, the value is enormous, and the chord between them is a line that
//!   never existed.
//!
//! The threshold is a multiple of the trajectory's *typical* step and not of its
//! whole extent. The first version used a quarter of the range, and a smooth
//! trajectory whose points happen to be far apart — a coarse step on a steep
//! curve — was chopped into single points, because every step was a quarter of
//! the range. A blow-up is a step that is *orders of magnitude* larger than its
//! neighbours, and the median step is the right yardstick for that.

use canlab_plot::Curve;

use crate::system::SystemTrace;
use crate::Trace;

/// How many typical steps a jump may be before it is a break.
///
/// A blow-up's step is thousands of times its neighbours; a smooth trajectory's
/// is within a small factor of them. Fifty is far above any smooth curve's
/// variation and far below a pole's, and it is a judgement stated here rather
/// than a constant buried in a comparison.
const BREAK_FACTOR: f64 = 50.0;

/// `y` against `t`, split where the trajectory is not connected.
pub fn trace_curve(trace: &Trace) -> Curve {
    let points: Vec<[f64; 2]> = trace
        .points
        .iter()
        .map(|point| [point.t, point.y])
        .collect();
    split_into_segments(points, 1)
}

/// The phase portrait of a two-dimensional system: `y₂` against `y₁`.
///
/// `None` for a system whose state is not two-dimensional: a phase portrait of
/// one variable is the trajectory itself, and of three is a projection this
/// function does not claim to make.
pub fn phase_curve(trace: &SystemTrace) -> Option<Curve> {
    let dimension = trace.points.first()?.y.len();
    if dimension != 2 {
        return None;
    }
    let points: Vec<[f64; 2]> = trace
        .points
        .iter()
        .map(|point| [point.y[0], point.y[1]])
        .collect();
    // `scale_index = 0` and not `2`: the coordinate index is 0 or 1 in a
    // two-component array, and the phase portrait's *jump* test already looks
    // at both coordinates (the `else` branch of `split_into_segments`). Passing
    // `2` indexed past the end of every point.
    Some(split_into_segments(points, 0))
}

/// A component of a system's state against `t`.
pub fn component_curve(trace: &SystemTrace, index: usize) -> Option<Curve> {
    // A component that does not exist is `None`, not a zero: a system with two
    // unknowns has no third component, and drawing one at zero would be a
    // curve that is not in the data.
    let mut points = Vec::with_capacity(trace.points.len());
    for point in &trace.points {
        // The component must exist; a system with two unknowns has no third,
        // and `?` turns that into `None` rather than a panic.
        let value = point.y.get(index).copied()?;
        points.push([point.t, value]);
    }
    Some(split_into_segments(points, 1))
}

/// Split a polyline where a point is not finite or the jump is too large.
///
/// `scale_index` is the coordinate the scale is measured in: for `y` against `t`
/// it is `y`, because `t` is the axis the trajectory is parameterised by and a
/// long time does not make a trajectory discontinuous. For a phase portrait both
/// coordinates are state, and the scale is the larger of the two.
fn split_into_segments(points: Vec<[f64; 2]>, scale_index: usize) -> Curve {
    // A non-finite point is *removed and breaks the segment*: filtering it out
    // and joining the two sides would draw a line across the infinity, which is
    // the same mistake as joining across a pole. The marker is `None` in the
    // walk below.
    let walk: Vec<Option<[f64; 2]>> = points
        .into_iter()
        .map(|point| {
            if point[0].is_finite() && point[1].is_finite() {
                Some(point)
            } else {
                None
            }
        })
        .collect();
    let finite: Vec<[f64; 2]> = walk.iter().flatten().copied().collect();
    if finite.is_empty() {
        return Curve::default();
    }
    let threshold = typical_step(&finite, scale_index) * BREAK_FACTOR;
    let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut previous: Option<[f64; 2]> = None;
    for entry in walk {
        let Some(point) = entry else {
            // The non-finite point: end the segment here.
            if current.len() >= 2 {
                segments.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            previous = None;
            continue;
        };
        if let Some(last) = previous {
            let jump = if scale_index == 1 {
                // `y` against `t`: a jump in `y` is the discontinuity.
                (point[1] - last[1]).abs()
            } else {
                // A phase portrait: either coordinate jumping is a break.
                (point[0] - last[0]).abs().max((point[1] - last[1]).abs())
            };
            if jump > threshold {
                if current.len() >= 2 {
                    // The break: the segment ends at `last`, and the new one
                    // *starts* at the point that follows the gap. Clearing
                    // `current` instead of starting it with the new point lost
                    // every point after the first break, which is how the
                    // blow-up test saw one segment ending at `100` and nothing
                    // beyond it.
                    segments.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
        current.push(point);
        previous = Some(point);
    }
    if current.len() >= 2 {
        segments.push(current);
    }
    Curve { segments }
}

/// The median step of a coordinate along a polyline.
///
/// The median and not the mean: a single huge step — the blow-up itself — would
/// drag a mean up and make the threshold useless, while the median ignores it.
fn typical_step(points: &[[f64; 2]], index: usize) -> f64 {
    let mut steps: Vec<f64> = points
        .windows(2)
        .map(|pair| (pair[1][index] - pair[0][index]).abs())
        .filter(|step| step.is_finite())
        .collect();
    if steps.is_empty() {
        return 0.0;
    }
    steps.sort_by(f64::total_cmp);
    steps[steps.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::SystemPoint;
    use crate::Point;

    fn trace_of(points: &[(f64, f64)]) -> Trace {
        Trace {
            points: points
                .iter()
                .map(|(t, y)| Point { t: *t, y: *y })
                .collect(),
        }
    }

    #[test]
    fn the_plotted_trace_matches_the_tabulated_trace() {
        // The roadmap's first check: every point of the trace is a point of the
        // curve, in the same order and with the same values.
        let trace = trace_of(&[(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)]);
        let curve = trace_curve(&trace);
        assert_eq!(curve.segment_count(), 1);
        let points: Vec<[f64; 2]> = curve
            .segments
            .iter()
            .flat_map(|segment| segment.iter().copied())
            .collect();
        assert_eq!(points, vec![[0.0, 1.0], [0.5, 2.0], [1.0, 3.0]]);
    }

    #[test]
    fn a_blow_up_is_split_into_segments_and_not_joined() {
        // The roadmap's second check. `y' = y²` from `y(0) = 1` blows up at
        // `t = 1`; an integrator that steps past it produces a value that jumps
        // from large to enormous. The curve must break there rather than draw a
        // line across the gap.
        // A trajectory that is smooth, then blows up, then is smooth again on
        // the far side: three connected stretches and two gaps. Each stretch
        // needs two points, because a one-point polyline draws nothing — the
        // first version of this test had a single point after each gap and saw
        // one segment, which was the *test's* geometry and not a bug.
        let trace = trace_of(&[
            (0.0, 1.0),
            (0.5, 2.0),
            (0.9, 10.0),
            (0.99, 100.0),
            // The step that crossed the pole: a jump of four orders of
            // magnitude in one step.
            (1.0, 1.0e6),
            (1.01, 1.0e9),
            // The far side, smooth again.
            (1.02, 1.0000001e9),
            (1.03, 1.0000002e9),
        ]);
        let curve = trace_curve(&trace);
        assert!(
            curve.segment_count() >= 2,
            "the blow-up was joined: {curve:?}"
        );
        // No segment contains two points more than a quarter of the range apart,
        // which is the rule the split is built on.
        let range = 1.0e9 - 1.0;
        for segment in &curve.segments {
            for pair in segment.windows(2) {
                assert!(
                    (pair[1][1] - pair[0][1]).abs() <= 0.25 * range,
                    "a segment spans a jump of {}",
                    (pair[1][1] - pair[0][1]).abs()
                );
            }
        }
    }

    #[test]
    fn a_non_finite_point_is_dropped_and_ends_the_segment() {
        let trace = trace_of(&[(0.0, 1.0), (0.5, 2.0), (0.6, f64::INFINITY), (0.7, 3.0), (1.0, 4.0)]);
        let curve = trace_curve(&trace);
        // The infinite point is gone, and the two sides are separate segments.
        for segment in &curve.segments {
            for point in segment {
                assert!(point[1].is_finite(), "{point:?}");
            }
        }
        assert_eq!(curve.segment_count(), 2, "{curve:?}");
    }

    #[test]
    fn a_smooth_trajectory_is_one_segment() {
        // `y = t²` on `[0, 1]`: no jump anywhere, so one polyline.
        let points: Vec<(f64, f64)> = (0..=100)
            .map(|index| {
                let t = index as f64 / 100.0;
                (t, t * t)
            })
            .collect();
        let curve = trace_curve(&trace_of(&points));
        assert_eq!(curve.segment_count(), 1);
        assert_eq!(curve.point_count(), 101);
    }

    #[test]
    fn a_phase_portrait_is_y2_against_y1() {
        // A circle in the phase plane: `x' = y`, `y' = -x` from `(1, 0)`.
        let system = crate::system::parse_system(&[
            String::from("x' = y"),
            String::from("y' = -x"),
            String::from("x(0) = 1"),
            String::from("y(0) = 0"),
        ])
        .expect("a system");
        let trace = crate::system::rk4_system(&system, 0.0, std::f64::consts::TAU, 0.01)
            .expect("a trace");
        let curve = phase_curve(&trace).expect("a phase portrait");
        assert_eq!(curve.segment_count(), 1);
        // Every point is on the unit circle, which is what the portrait is of.
        for segment in &curve.segments {
            for point in segment {
                let radius = (point[0] * point[0] + point[1] * point[1]).sqrt();
                assert!((radius - 1.0).abs() < 1e-6, "radius {radius}");
            }
        }
        // And a component against `t` has the right values.
        let component = component_curve(&trace, 0).expect("a component");
        let first = component.segments[0][0];
        assert_eq!(first, [0.0, 1.0]);
    }

    #[test]
    fn a_phase_portrait_needs_two_dimensions() {
        // One unknown has no phase portrait, and three is a projection this
        // function does not claim.
        let one = crate::system::parse_system(&[
            String::from("y' = y"),
            String::from("y(0) = 1"),
        ])
        .expect("a system");
        let trace = crate::system::rk4_system(&one, 0.0, 1.0, 0.1).expect("a trace");
        assert!(phase_curve(&trace).is_none());
        // The component curve still works.
        assert!(component_curve(&trace, 0).is_some());
        assert!(component_curve(&trace, 1).is_none());
    }

    #[test]
    fn an_empty_trace_is_an_empty_curve() {
        let curve = trace_curve(&Trace::default());
        assert!(curve.is_empty());
        let system = SystemTrace::default();
        assert!(phase_curve(&system).is_none());
    }

    #[test]
    fn a_single_point_is_not_a_segment() {
        // A polyline needs two points; one point draws nothing.
        let curve = trace_curve(&trace_of(&[(0.0, 1.0)]));
        assert!(curve.is_empty());
        let points: Vec<SystemPoint> = vec![SystemPoint {
            t: 0.0,
            y: vec![1.0, 2.0],
        }];
        let trace = SystemTrace { points };
        assert!(phase_curve(&trace).expect("a portrait").is_empty());
    }
}
