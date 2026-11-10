//! Behavioural tests for the implicit curve tracer.
//!
//! These use only the public API, so they also pin the names the GUI depends
//! on: [`ImplicitOptions`], [`sample_implicit`] and the shared [`Curve`] type.
//!
//! The assertions are geometric rather than snapshot-based: every emitted
//! point must actually lie on the zero set, and the chain structure must show
//! that adjacent cells were joined rather than left as loose chords.

use canlab_plot::{Curve, ImplicitOptions, sample_implicit};

fn box_options(
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    resolution: usize,
) -> ImplicitOptions {
    ImplicitOptions {
        resolution,
        x_min,
        x_max,
        y_min,
        y_max,
    }
}

/// Every vertex of every segment.
fn points(curve: &Curve) -> impl Iterator<Item = &[f64; 2]> {
    curve.segments.iter().flat_map(|segment| segment.iter())
}

/// `(min x, max x, min y, max y)`; meaningless on an empty curve.
fn bounds(curve: &Curve) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for point in points(curve) {
        min_x = min_x.min(point[0]);
        max_x = max_x.max(point[0]);
        min_y = min_y.min(point[1]);
        max_y = max_y.max(point[1]);
    }
    (min_x, max_x, min_y, max_y)
}

fn assert_finite(curve: &Curve) {
    for point in points(curve) {
        assert!(
            point[0].is_finite() && point[1].is_finite(),
            "non-finite vertex {point:?}"
        );
    }
}

#[test]
fn circle_is_one_closed_ring_in_all_quadrants() {
    let curve = sample_implicit(
        |x, y| x * x + y * y - 1.0,
        &box_options(-2.0, 2.0, -2.0, 2.0, 80),
    );

    assert!(!curve.is_empty(), "a circle must produce a curve");
    assert_finite(&curve);
    for point in points(&curve) {
        let residual = (point[0] * point[0] + point[1] * point[1] - 1.0).abs();
        assert!(
            residual < 0.05,
            "point {point:?} is {residual} away from x^2 + y^2 = 1"
        );
    }

    let (min_x, max_x, min_y, max_y) = bounds(&curve);
    assert!(min_x < -0.5, "left half is missing: min x = {min_x}");
    assert!(max_x > 0.5, "right half is missing: max x = {max_x}");
    assert!(min_y < -0.5, "bottom half is missing: min y = {min_y}");
    assert!(max_y > 0.5, "top half is missing: max y = {max_y}");

    // The join step, not the per-cell tracing, is what turns roughly 400
    // chords into a ring. Anything more than a handful of pieces means the
    // adjacent cells failed to connect.
    assert_eq!(
        curve.segment_count(),
        1,
        "the circle must be one closed polyline, got {} pieces",
        curve.segment_count()
    );

    let ring = &curve.segments[0];
    for step in ring.windows(2) {
        let dx = step[1][0] - step[0][0];
        let dy = step[1][1] - step[0][1];
        assert!(
            dx.hypot(dy) < 0.08,
            "chain jumps between non-adjacent cells: {step:?}"
        );
    }
    let first = ring.first().expect("ring has a first point");
    let last = ring.last().expect("ring has a last point");
    assert!(
        (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-6,
        "ring is not closed: {first:?} .. {last:?}"
    );
}

#[test]
fn diagonal_line_spans_the_whole_box() {
    let curve = sample_implicit(|x, y| x + y, &box_options(-1.0, 1.0, -1.0, 1.0, 80));

    assert!(!curve.is_empty(), "the line x + y = 0 must be traced");
    assert_finite(&curve);
    for point in points(&curve) {
        let residual = (point[0] + point[1]).abs();
        assert!(
            residual < 0.05,
            "point {point:?} is {residual} away from x + y = 0"
        );
    }

    let (min_x, max_x, _, _) = bounds(&curve);
    assert!(
        min_x < -0.95,
        "line stops short of the left edge: min x = {min_x}"
    );
    assert!(
        max_x > 0.95,
        "line stops short of the right edge: max x = {max_x}"
    );
    assert_eq!(
        curve.segment_count(),
        1,
        "the line must be one contiguous polyline, got {}",
        curve.segment_count()
    );
}

#[test]
fn function_without_zeros_returns_no_segments() {
    let curve = sample_implicit(|_, _| 1.0, &ImplicitOptions::default());
    assert!(curve.is_empty(), "f = 1 has no zero set: {:?}", curve);
    assert_eq!(curve.point_count(), 0);
}

#[test]
fn non_finite_values_are_skipped_without_panicking() {
    // Every corner NaN: no usable cell anywhere.
    let everywhere = sample_implicit(|_, _| f64::NAN, &ImplicitOptions::default());
    assert!(everywhere.is_empty(), "NaN everywhere must not draw");
    assert_eq!(everywhere.point_count(), 0);

    // NaN in a band, finite and positive elsewhere: still no zero set.
    let band = sample_implicit(
        |x, _| if x.abs() < 0.5 { f64::NAN } else { 1.0 },
        &box_options(-2.0, 2.0, -2.0, 2.0, 60),
    );
    assert!(band.is_empty(), "a NaN band must not become a curve");

    // A pole is not a zero, even though the values run off to infinity.
    let pole = sample_implicit(
        |x, y| 1.0 / (x * x + y * y),
        &box_options(-1.0, 1.0, -1.0, 1.0, 60),
    );
    assert!(pole.is_empty(), "a pole must not be reported as a root");
}

#[test]
fn two_disjoint_circles_are_both_traced() {
    let curve = sample_implicit(
        |x, y| (x * x + y * y - 1.0) * ((x - 3.0) * (x - 3.0) + y * y - 1.0),
        &box_options(-2.0, 5.0, -2.0, 2.0, 100),
    );

    assert!(!curve.is_empty(), "both circles must produce a curve");
    assert_finite(&curve);

    let mut near_origin = false;
    let mut near_three = false;
    for point in points(&curve) {
        if (point[0].hypot(point[1]) - 1.0).abs() < 0.05 {
            near_origin = true;
        }
        if ((point[0] - 3.0).hypot(point[1]) - 1.0).abs() < 0.05 {
            near_three = true;
        }
    }
    assert!(near_origin, "the circle around the origin is missing");
    assert!(near_three, "the circle around (3, 0) is missing");
    assert_eq!(
        curve.segment_count(),
        2,
        "two disjoint rings must stay separate, got {}",
        curve.segment_count()
    );
}

#[test]
fn resolution_below_two_returns_empty_curve() {
    for resolution in [0, 1] {
        let curve = sample_implicit(
            |x, y| x + y,
            &ImplicitOptions {
                resolution,
                ..ImplicitOptions::default()
            },
        );
        assert!(
            curve.is_empty(),
            "resolution {resolution} has no cell, so nothing can be traced"
        );
        assert_eq!(curve.point_count(), 0);
    }
}

#[test]
fn vertical_line_needs_no_solving_for_y() {
    let curve = sample_implicit(|x, _| x, &box_options(-1.0, 1.0, -1.0, 1.0, 20));

    assert!(!curve.is_empty(), "x = 0 must be traced");
    assert_finite(&curve);
    for point in points(&curve) {
        assert!(
            point[0].abs() < 1e-6,
            "vertical line has a point off x = 0: {point:?}"
        );
    }
    let (_, _, min_y, max_y) = bounds(&curve);
    assert!(min_y < -0.95, "line stops short at the bottom: {min_y}");
    assert!(max_y > 0.95, "line stops short at the top: {max_y}");
}

#[test]
fn default_options_match_the_documented_box() {
    let options = ImplicitOptions::default();
    assert_eq!(options.resolution, 160);
    assert_eq!(
        (options.x_min, options.x_max, options.y_min, options.y_max),
        (-10.0, 10.0, -10.0, 10.0)
    );
}

#[test]
fn unusable_boxes_are_empty_not_a_panic() {
    let cases = [
        ImplicitOptions {
            x_min: 1.0,
            x_max: 1.0,
            ..ImplicitOptions::default()
        },
        ImplicitOptions {
            x_min: 2.0,
            x_max: 1.0,
            ..ImplicitOptions::default()
        },
        ImplicitOptions {
            y_min: 1.0,
            y_max: -1.0,
            ..ImplicitOptions::default()
        },
        ImplicitOptions {
            x_max: f64::INFINITY,
            ..ImplicitOptions::default()
        },
        ImplicitOptions {
            y_min: f64::NAN,
            ..ImplicitOptions::default()
        },
    ];
    for options in cases {
        let curve = sample_implicit(|x, y| x + y, &options);
        assert!(curve.is_empty(), "unusable box {options:?} drew a curve");
    }
}
