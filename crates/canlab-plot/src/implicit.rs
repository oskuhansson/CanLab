//! Implicit curves: draw the set `{ (x, y) : f(x, y) = 0 }` without solving
//! for `y`.
//!
//! [`sample_implicit`] is the companion of [`crate::sample`] for equations
//! that are not written as `y = g(x)`. `x^2 + y^2 = 1` and `x + y = 3` are
//! one call each, with no algebraic rearrangement and no special case for
//! vertical tangents. The result is the same [`Curve`] the function sampler
//! returns, so [`crate::to_svg`] renders it without changes.
//!
//! # Marching squares
//!
//! The box is divided into a `resolution × resolution` grid of cells. `f` is
//! evaluated once at every grid corner; each cell then has four corner values.
//! A corner is "negative" when its value is below zero, and a cell is
//! classified by the four-bit mask of its corners. Every grid edge whose two
//! corners disagree about the sign contains exactly one crossing, found by
//! linear interpolation between the corner values, and the 16-case table says
//! which crossings belong to the same piece of contour.
//!
//! A value of exactly zero counts as non-negative. That choice is arbitrary
//! but deterministic: it keeps a corner that lies on the zero set on the same
//! side of every cell that shares it, while the cells that straddle the sign
//! change still join the contour through that corner.
//!
//! # From cell chords to polylines
//!
//! A cell contributes zero, one or two independent two-point chords. A
//! renderer needs connected chains instead: a circle sampled at a useful
//! resolution is a few hundred chords, and drawing them independently is both
//! slower and visibly wrong at the joins. The chords are therefore stitched by
//! endpoint. An
//! endpoint computed on a shared grid edge is bit-identical in both adjacent
//! cells (the interpolation always walks the edge in the same direction), but
//! the join still allows a small relative epsilon so that rounding cannot
//! split a chain. A hash of quantised endpoints makes each match a
//! near-constant-time lookup rather than a comparison against every other
//! chord, which keeps the whole trace O(resolution²).
//!
//! # What the tracer cannot see
//!
//! Marching squares follows *sign changes*. A zero set that does not change
//! sign — `(x^2 + y^2 - 1)^2`, or a function that is zero on a whole region —
//! has no crossing edges and is not drawn. A contour that merely touches a
//! grid corner tangentially can likewise disappear into that corner. Saddle
//! cells (two diagonally opposite corners negative) have two possible
//! pairings; this implementation keeps the negative corners apart, and a
//! caller who needs the other topology can negate `f`. Where more than two
//! chords meet (a self-intersection of the zero set), the continuation is
//! arbitrary: the output stays contiguous, but branch identity is not
//! meaningful. Finally, all accuracy is cell-sized: linear interpolation
//! places a crossing well for a smooth curve but cannot resolve a feature
//! smaller than one cell.
//!
//! Cells with any non-finite corner are skipped entirely. A pole or a `NaN`
//! therefore ends the contour instead of bending it towards a garbage point.
//!
//! # Cost
//!
//! Time is O(resolution²) and the corner values are cached in two rolling
//! rows, so evaluation memory is O(resolution) rather than O(resolution²);
//! only the output chords and their endpoint index grow with the contour.

use std::collections::HashMap;

use crate::sampler::Curve;

// ---------------------------------------------------------------------------
// Tunables
// ---------------------------------------------------------------------------

/// Default number of grid cells per axis.
const DEFAULT_RESOLUTION: usize = 160;

/// Default box is `[-10, 10]²`.
const DEFAULT_RANGE: f64 = 10.0;

/// Upper bound on the fine grid. A typo such as `usize::MAX` must not turn
/// into an allocation that cannot finish; this is a safety valve, not an
/// accuracy setting.
const MAX_RESOLUTION: usize = 2048;

/// Endpoints closer than this fraction of the box span are the same point.
/// Shared grid edges produce bit-identical endpoints; the epsilon exists only
/// to absorb floating-point rounding in the interpolation.
const JOIN_EPSILON: f64 = 1e-9;

/// One endpoint bucket is `JOIN_EPSILON` wide, so the integer key is simply
/// the normalised coordinate multiplied by this factor.
const KEY_SCALE: f64 = 1.0 / JOIN_EPSILON;

/// Case table for the 16 sign masks, as pairs of crossed grid edges.
///
/// Edge order is `0 = bottom`, `1 = right`, `2 = top`, `3 = left`, and the
/// mask bit order is `bottom-left`, `bottom-right`, `top-right`, `top-left`.
/// Two masks with four crossings (the saddles `0101` and `1010`) are resolved
/// by isolating the negative corners from one another.
const CASE_EDGES: [&[u8]; 16] = [
    &[],           // 0000: no crossing
    &[0, 3],       // 0001: bottom-left corner negative
    &[0, 1],       // 0010: bottom-right corner negative
    &[1, 3],       // 0011: bottom edge negative
    &[1, 2],       // 0100: top-right corner negative
    &[0, 3, 1, 2], // 0101: saddle, negative corners kept apart
    &[0, 2],       // 0110: right edge negative
    &[2, 3],       // 0111: all but top-left negative
    &[2, 3],       // 1000: top-left corner negative
    &[0, 2],       // 1001: left edge negative
    &[0, 1, 2, 3], // 1010: saddle, negative corners kept apart
    &[1, 2],       // 1011: all but top-right negative
    &[1, 3],       // 1100: top edge negative
    &[0, 1],       // 1101: all but bottom-right negative
    &[0, 3],       // 1110: all but bottom-left negative
    &[],           // 1111: no crossing
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Where and how finely to look for the zero set.
///
/// `resolution` counts *cells* per axis, so the tracer evaluates `f` at
/// `(resolution + 1)²` grid corners. A resolution below 2 (a box with no
/// full cell) yields an empty curve; values above 2048 are clamped, which
/// bounds the work rather than panics.
#[derive(Clone, Debug, PartialEq)]
pub struct ImplicitOptions {
    /// Grid cells per axis, e.g. 160.
    pub resolution: usize,
    /// Left edge of the box.
    pub x_min: f64,
    /// Right edge of the box.
    pub x_max: f64,
    /// Bottom edge of the box.
    pub y_min: f64,
    /// Top edge of the box.
    pub y_max: f64,
}

impl Default for ImplicitOptions {
    fn default() -> Self {
        Self {
            resolution: DEFAULT_RESOLUTION,
            x_min: -DEFAULT_RANGE,
            x_max: DEFAULT_RANGE,
            y_min: -DEFAULT_RANGE,
            y_max: DEFAULT_RANGE,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Trace the set `{ (x, y) : f(x, y) == 0 }` inside the box, as polylines in
/// the same coordinate space as the inputs.
///
/// The curve is found by marching squares: `f` is evaluated once per grid
/// corner, sign-changing cells contribute chords, and the chords are joined
/// into contiguous chains. A closed contour therefore comes back as a closed
/// chain rather than as a bag of cell-sized fragments.
///
/// Never panics on an unusable option set. A non-finite or inverted range, a
/// range whose span overflows, or a `resolution` below 2 yields an empty
/// [`Curve`], for the same reason [`crate::sample`] does: a plot of nothing is
/// a legitimate answer.
///
/// ```
/// use canlab_plot::{ImplicitOptions, sample_implicit};
///
/// let circle = sample_implicit(
///     |x, y| x * x + y * y - 1.0,
///     &ImplicitOptions {
///         resolution: 80,
///         x_min: -2.0,
///         x_max: 2.0,
///         y_min: -2.0,
///         y_max: 2.0,
///     },
/// );
///
/// assert!(!circle.is_empty());
/// assert_eq!(circle.segment_count(), 1, "the cell chords form one ring");
/// ```
pub fn sample_implicit<F: Fn(f64, f64) -> f64>(f: F, options: &ImplicitOptions) -> Curve {
    let (x_min, x_max) = (options.x_min, options.x_max);
    let (y_min, y_max) = (options.y_min, options.y_max);

    if !x_min.is_finite() || !x_max.is_finite() || !y_min.is_finite() || !y_max.is_finite() {
        return Curve::default();
    }
    // The ends are finite here, so a plain ordering test is enough; `<=`
    // rejects both an empty box and an inverted one.
    if x_max <= x_min || y_max <= y_min {
        return Curve::default();
    }
    // A grid needs at least two rows and columns of corners to have a cell.
    if options.resolution < 2 {
        return Curve::default();
    }

    let span_x = x_max - x_min;
    let span_y = y_max - y_min;
    if !span_x.is_finite() || !span_y.is_finite() {
        return Curve::default();
    }

    let cells = options.resolution.min(MAX_RESOLUTION);
    let xs = axis_points(x_min, x_max, cells);
    let ys = axis_points(y_min, y_max, cells);
    let frame = JoinFrame {
        x_min,
        y_min,
        span_x,
        span_y,
    };

    // Rolling pair of rows: row `j` is in `lower`, row `j + 1` is filled into
    // `upper`, then the two swap. Every corner is still evaluated exactly once;
    // the values simply do not all have to be alive at the same time.
    let mut lower: Vec<f64> = xs.iter().map(|&x| f(x, ys[0])).collect();
    let mut upper = vec![0.0; xs.len()];
    // A smooth contour crosses O(resolution) cells; this reserve keeps the
    // common case from repeatedly reallocating without pretending to know the
    // worst case (a checkerboard function crosses every cell twice).
    let mut chords: Vec<[[f64; 2]; 2]> = Vec::with_capacity(cells.saturating_mul(4));

    for j in 0..cells {
        let (y0, y1) = (ys[j], ys[j + 1]);
        for (slot, &x) in upper.iter_mut().zip(xs.iter()) {
            *slot = f(x, y1);
        }

        for i in 0..cells {
            let corners = [lower[i], lower[i + 1], upper[i + 1], upper[i]];
            // A cell touching a pole or a hole is not part of the contour.
            // Skipping the whole cell is what keeps non-finite values from
            // being interpolated into garbage points.
            if !corners.iter().all(|value| value.is_finite()) {
                continue;
            }

            let case = sign_mask(&corners);
            let (x0, x1) = (xs[i], xs[i + 1]);
            for [first, second] in CASE_EDGES[case].as_chunks::<2>().0 {
                let start = edge_point(*first, &corners, x0, x1, y0, y1);
                let end = edge_point(*second, &corners, x0, x1, y0, y1);
                // A cell whose corner lies on (or within rounding of) the
                // zero set can produce two crossings that are the same point
                // for joining purposes. Contract such a chord instead of
                // emitting it: otherwise the joiner sees a little cluster of
                // endpoints, may close the main contour across it, and leaves
                // the sub-epsilon chord behind as an isolated 2-point piece.
                // The cells on either side of the shared edges computed
                // bit-identical crossings, so they still join across the
                // contracted point.
                if !frame.same_point(start, end) {
                    chords.push([start, end]);
                }
            }
        }

        std::mem::swap(&mut lower, &mut upper);
    }

    Curve {
        segments: join_chords(chords, frame),
    }
}

// ---------------------------------------------------------------------------
// Marching squares internals
// ---------------------------------------------------------------------------

/// `cells + 1` evenly spaced coordinates from `min` to `max`.
///
/// The last coordinate is `max` itself rather than `min + span`, so the box
/// edge is exactly where the caller asked for it. Both endpoints being exact
/// also keeps chains that reach a box corner from acquiring a sub-ulp gap.
fn axis_points(min: f64, max: f64, cells: usize) -> Vec<f64> {
    let span = max - min;
    let last = cells as f64;
    (0..=cells)
        .map(|i| {
            if i == cells {
                max
            } else {
                min + span * (i as f64 / last)
            }
        })
        .collect()
}

/// Four-bit mask of "corner is below zero", in `[bottom-left, bottom-right,
/// top-right, top-left]` order.
fn sign_mask(corners: &[f64; 4]) -> usize {
    let mut mask = 0;
    for (bit, &value) in corners.iter().enumerate() {
        if value < 0.0 {
            mask |= 1 << bit;
        }
    }
    mask
}

/// The crossing of one cell edge, in data coordinates.
///
/// `corners` is `[bottom-left, bottom-right, top-right, top-left]` and the
/// edge indices are `0 = bottom`, `1 = right`, `2 = top`, `3 = left`. Each
/// edge is always walked in the same direction (left to right for horizontal
/// edges, bottom to top for vertical ones), which is what makes the crossing
/// computed by two adjacent cells bit-identical.
fn edge_point(edge: u8, corners: &[f64; 4], x0: f64, x1: f64, y0: f64, y1: f64) -> [f64; 2] {
    match edge {
        0 => [lerp(x0, x1, crossing(corners[0], corners[1])), y0],
        1 => [x1, lerp(y0, y1, crossing(corners[1], corners[2]))],
        // Walk the top edge left-to-right, matching the bottom edge of the
        // cell above.
        2 => [lerp(x0, x1, crossing(corners[3], corners[2])), y1],
        // Walk the left edge bottom-to-top, matching the right edge of the
        // cell to the left.
        _ => [x0, lerp(y0, y1, crossing(corners[0], corners[3]))],
    }
}

/// Fraction of the way from `a` to `b` at which the linear interpolant is
/// zero.
///
/// `a` and `b` straddle zero (that is why the edge is crossed), so the result
/// is in `[0, 1]`. It is clamped anyway, and the midpoint is used when the
/// subtraction overflows, which only happens for enormous values of opposite
/// sign where the midpoint is in fact the best answer available.
fn crossing(a: f64, b: f64) -> f64 {
    let difference = a - b;
    if !difference.is_finite() || difference == 0.0 {
        return 0.5;
    }
    let t = a / difference;
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Linear interpolation with exact endpoints.
///
/// Returning `a` for `t <= 0` and `b` for `t >= 1` matters: when a corner lies
/// exactly on the zero set, the crossing is the corner itself, and it must be
/// the same coordinate in every cell that reports it.
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

// ---------------------------------------------------------------------------
// Joining chords into polylines
// ---------------------------------------------------------------------------

/// The affine frame that maps data coordinates to fractions of the box.
///
/// Both the bucket key and the match test live in this normalised space, so a
/// huge or tiny axis range cannot overflow the integer key or underflow the
/// epsilon.
#[derive(Clone, Copy, Debug)]
struct JoinFrame {
    x_min: f64,
    y_min: f64,
    span_x: f64,
    span_y: f64,
}

impl JoinFrame {
    /// Position of `point` as a fraction of the box along each axis.
    fn units(&self, point: [f64; 2]) -> (f64, f64) {
        (
            (point[0] - self.x_min) / self.span_x,
            (point[1] - self.y_min) / self.span_y,
        )
    }

    /// Quantised endpoint key. One step is [`JOIN_EPSILON`] of the box.
    fn key(&self, point: [f64; 2]) -> (i64, i64) {
        let (u, v) = self.units(point);
        (
            (u * KEY_SCALE).round() as i64,
            (v * KEY_SCALE).round() as i64,
        )
    }

    /// `true` when two endpoints are the same point for joining purposes.
    fn same_point(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let (u, v) = self.units(a);
        let (w, z) = self.units(b);
        let (du, dv) = (u - w, v - z);
        du * du + dv * dv <= JOIN_EPSILON * JOIN_EPSILON
    }
}

/// Stitch independent cell chords into maximal contiguous polylines.
///
/// Every chord is used exactly once. A chain starts from an unused chord and
/// is grown at both ends: at each end, [`next_chord`] finds any unused chord
/// sharing that endpoint, and only its far endpoint is appended. Growing
/// "before" and "after" in separate vectors keeps the whole join linear in
/// the number of chords; the start chord's endpoints are inserted between
/// them at the end.
fn join_chords(chords: Vec<[[f64; 2]; 2]>, frame: JoinFrame) -> Vec<Vec<[f64; 2]>> {
    if chords.is_empty() {
        return Vec::new();
    }

    // Endpoint ids are `2 * chord + end`, so a bucket entry knows both which
    // chord touches the point and which of its two ends did the touching.
    let mut buckets: HashMap<(i64, i64), Vec<u32>> =
        HashMap::with_capacity(chords.len().saturating_mul(2));
    for (index, chord) in chords.iter().enumerate() {
        let base = (index * 2) as u32;
        buckets.entry(frame.key(chord[0])).or_default().push(base);
        buckets
            .entry(frame.key(chord[1]))
            .or_default()
            .push(base + 1);
    }

    let mut used = vec![false; chords.len()];
    let mut polylines = Vec::new();

    for start in 0..chords.len() {
        if used[start] {
            continue;
        }
        used[start] = true;

        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut front = chords[start][0];
        let mut back = chords[start][1];

        while let Some(next) = next_chord(&buckets, &chords, &used, front, frame) {
            used[next] = true;
            let other = if frame.same_point(chords[next][0], front) {
                chords[next][1]
            } else {
                chords[next][0]
            };
            before.push(other);
            front = other;
        }
        while let Some(next) = next_chord(&buckets, &chords, &used, back, frame) {
            used[next] = true;
            let other = if frame.same_point(chords[next][0], back) {
                chords[next][1]
            } else {
                chords[next][0]
            };
            after.push(other);
            back = other;
        }

        before.reverse();
        before.push(chords[start][0]);
        before.push(chords[start][1]);
        before.extend(after);
        polylines.push(before);
    }

    polylines
}

/// Find an unused chord with an endpoint within [`JOIN_EPSILON`] of `point`.
///
/// The quantised key is only a bucket: an endpoint within the epsilon can
/// round into one of the eight neighbouring buckets, so all nine are
/// inspected. The distance test, not the key, is what actually decides.
fn next_chord(
    buckets: &HashMap<(i64, i64), Vec<u32>>,
    chords: &[[[f64; 2]; 2]],
    used: &[bool],
    point: [f64; 2],
    frame: JoinFrame,
) -> Option<usize> {
    let (key_x, key_y) = frame.key(point);
    for dx in -1..=1 {
        for dy in -1..=1 {
            let Some(endpoints) = buckets.get(&(key_x + dx, key_y + dy)) else {
                continue;
            };
            for &endpoint in endpoints {
                let index = (endpoint / 2) as usize;
                if used[index] {
                    continue;
                }
                let which = (endpoint % 2) as usize;
                if frame.same_point(chords[index][which], point) {
                    return Some(index);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Corner values that realise an exact sign mask.
    fn corners_for(mask: usize) -> [f64; 4] {
        std::array::from_fn(|bit| if mask & (1 << bit) != 0 { -1.0 } else { 1.0 })
    }

    #[test]
    fn case_table_lists_exactly_the_sign_changing_edges() {
        for (mask, &listed) in CASE_EDGES.iter().enumerate() {
            let corners = corners_for(mask);
            assert_eq!(sign_mask(&corners), mask, "mask {mask:04b} round-trips");

            assert_eq!(
                listed.len() % 2,
                0,
                "mask {mask:04b}: edges must come in pairs"
            );

            // Edge `k` joins corners `k` and `k + 1` (mod 4), so it changes
            // sign exactly when those two mask bits differ.
            let crossed: Vec<u8> = (0..4u8)
                .filter(|&edge| {
                    let a = edge as usize;
                    let b = (a + 1) % 4;
                    ((mask >> a) & 1) != ((mask >> b) & 1)
                })
                .collect();
            let mut listed_sorted = listed.to_vec();
            listed_sorted.sort_unstable();
            assert_eq!(
                listed_sorted, crossed,
                "mask {mask:04b}: wrong edges in the case table"
            );
            for [first, second] in listed.as_chunks::<2>().0 {
                assert_ne!(
                    *first, *second,
                    "mask {mask:04b}: a pair repeats the same edge"
                );
            }
        }
    }

    #[test]
    fn crossing_is_exact_at_a_zero_corner() {
        assert_eq!(lerp(2.0, 4.0, crossing(0.0, 1.0)), 2.0);
        assert_eq!(lerp(2.0, 4.0, crossing(-1.0, 0.0)), 4.0);
        assert!((lerp(2.0, 4.0, crossing(1.0, -1.0)) - 3.0).abs() < 1e-15);
    }
}
