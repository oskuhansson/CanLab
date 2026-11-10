//! Adaptive sampling: turn a compiled [`canlab_core::Program`] into polyline
//! segments.
//!
//! # Why the output is segments, not points
//!
//! The hard part of plotting is not evaluating a function; it is deciding
//! where the function is *not* connected. `1/x` on `[-1, 1]` has two branches,
//! and a renderer handed one flat list of points has no way to know that the
//! point at `x = -0.001` and the next one at `x = +0.001` must not be joined.
//! The naive polyline draws a vertical line straight through the asymptote,
//! which is a false statement about the function — and one that every
//! hand-rolled plotter makes.
//!
//! The continuity decision therefore belongs here, at the only place that has
//! the information: the sampler is the component that saw the evaluation was
//! `NaN` (outside the domain) or `±Inf` / enormous with a sign flip (a pole).
//! It records that fact by **ending the segment** and starting a new one.
//! [`Curve::segments`] is consequently a list of genuinely connected
//! polylines, and the renderer can stay dumb: draw each list, never wonder.
//!
//! # The `NaN` / `Inf` convention
//!
//! [`canlab_core::Program::eval`] returns `NaN` outside a function's domain
//! (`sqrt(-1)`, `ln(-2)`) and `±Inf` (or a huge finite value) at a pole. The
//! sampler never emits a non-finite vertex: a non-finite value is a *gap*, not
//! a point. Every `[f64; 2]` in a returned [`Curve`] has two finite
//! coordinates, which is what lets [`crate::svg`] promise a document without
//! `NaN` in it.
//!
//! # Budget
//!
//! `max_evaluations` is a hard cap, not a hint. [`sample`] always returns,
//! for any options: a zero budget yields an empty curve, and an exhausted
//! budget simply stops refinement (the sampler connects what it already has or
//! cuts the segment, never panics and never spins). Recursion is bounded both
//! by `max_depth` and by the fact that intervals halve until the midpoint is
//! no longer representable.

use canlab_core::Program;

// ---------------------------------------------------------------------------
// Tunables
// ---------------------------------------------------------------------------

/// A jump is "huge" when the larger end exceeds this multiple of the typical
/// magnitude of the sampled values. Scale-relative, because `1/x` on
/// `[-1e-6, 1e-6]` and `1/x` on `[-1e6, 1e6]` are the same picture.
const POLE_FACTOR: f64 = 30.0;

/// A midpoint that is this many times larger than both ends is a blow-up.
const POLE_MID_GROWTH: f64 = 4.0;

/// How far to refine a sign change before deciding what it is. A root makes
/// `|y|` collapse; a pole makes it run away, and twelve halvings can multiply
/// it by thousands. A wrong answer here costs a mark on the picture, so the
/// budget for it is fixed and small.
const POLE_CONFIRM_STEPS: u32 = 12;

/// Per-halving growth that counts as "running away". A pole roughly doubles
/// the magnitude of the near bracket end each time the bracket halves.
const POLE_CONFIRM_RATIO: f64 = 1.8;

/// Consecutive runaway steps required. One big jump is not evidence — an
/// under-resolved oscillation produces those too — but three doublings in a
/// row (≈6x) is what only a pole does.
const POLE_CONFIRM_STREAK: u32 = 3;

/// Even a confirmed streak must clear this multiple of the plot's typical
/// upper magnitude. The streak is scale-free and can be fooled by aliasing;
/// this floor is what keeps `sin(50x)` sampled twice per period continuous.
const POLE_EVIDENCE_FLOOR: f64 = 4.0;

/// Chord-direction change (radians) above which an interval is subdivided.
/// Roughly 7 degrees: small enough that visible corners disappear, large
/// enough that a dense uniform grid (which is already smooth) is left alone.
const ANGLE_THRESHOLD: f64 = 0.12;

/// Above this multiple of the typical magnitude, refinement is pointless:
/// the points are off-screen anyway and the budget is better spent elsewhere.
/// Deliberately generous — normal curves sit many orders of magnitude below
/// this, so only the immediate neighbourhood of a pole is skipped.
const SUBDIVISION_GUARD: f64 = 1_000.0;

/// Bisection steps for a single boundary / pole. Bounded so a pathological
/// bracket cannot eat the evaluation budget.
const MAX_BISECT_STEPS: u32 = 24;

/// Bisection stops once the bracket is this fraction of a sample spacing.
const BISECT_TARGET_PIXELS: f64 = 0.125;

/// Domain islands (a defined stretch between two undefined ends) are a guess,
/// not a requirement, so they get a shallower recursion than the main curve.
const ISLAND_DEPTH_CAP: u8 = 6;

/// Upper bound on the uniform pre-grid. Nobody renders a million-point
/// polyline; the cap keeps `resolution: usize::MAX` from becoming a hang.
const MAX_UNIFORM_SAMPLES: usize = 1_000_000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Polyline data for one plot frame.
///
/// For [`sample`] output each inner vector is one connected polyline in
/// increasing `x` order; a contour from [`crate::implicit`] may instead turn
/// and close on itself. A discontinuity *ends* a segment, so a renderer never
/// draws a vertical line across an asymptote: `1/x` on `[-1, 1]` produces two
/// segments and no segment contains points from both sides of the pole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub segments: Vec<Vec<[f64; 2]>>,
}

impl Curve {
    /// Total number of vertices across every segment.
    pub fn point_count(&self) -> usize {
        self.segments.iter().map(|seg| seg.len()).sum()
    }

    /// Number of connected polylines.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// `true` when there is nothing to draw (empty curve, or all `NaN`).
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// How [`sample`] should spend its evaluations.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleOptions {
    /// Left edge of the sampled interval.
    pub x_min: f64,
    /// Right edge of the sampled interval.
    pub x_max: f64,
    /// Target number of uniform samples across the range, e.g. 2 per pixel.
    pub resolution: usize,
    /// Maximum total evaluations, a hard budget.
    pub max_evaluations: usize,
    /// Maximum recursive subdivision depth.
    pub max_depth: u8,
}

impl Default for SampleOptions {
    fn default() -> Self {
        Self {
            x_min: -10.0,
            x_max: 10.0,
            resolution: 800,
            max_evaluations: 200_000,
            max_depth: 12,
        }
    }
}

/// What [`sample_with_stats`] actually spent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleStats {
    /// Number of program evaluations performed. Never exceeds
    /// [`SampleOptions::max_evaluations`].
    pub evaluations: usize,
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Sample `program` adaptively over `[opts.x_min, opts.x_max]`.
///
/// Returns connected polylines with discontinuities expressed as segment
/// breaks. Never panics; an unusable option set (non-finite or inverted range,
/// budget below two evaluations) yields an empty [`Curve`] rather than an
/// error, because a plot of nothing is a legitimate answer.
pub fn sample(program: &Program, opts: &SampleOptions) -> Curve {
    sample_with_stats(program, opts).0
}

/// [`sample`], plus how many evaluations it took.
///
/// Useful for callers that share one budget across several curves, and for
/// tests that must prove the budget is a hard cap.
pub fn sample_with_stats(program: &Program, opts: &SampleOptions) -> (Curve, SampleStats) {
    let empty = || (Curve::default(), SampleStats::default());

    let (x_min, x_max) = (opts.x_min, opts.x_max);
    // `!(a > b)` also rejects NaN, which `a <= b` would not.
    if !x_min.is_finite() || !x_max.is_finite() || !(x_max > x_min) {
        return empty();
    }
    // Two points is the smallest drawable polyline; below that there is
    // nothing to evaluate.
    if opts.max_evaluations < 2 {
        return empty();
    }

    let n = opts
        .resolution
        .max(2)
        .min(opts.max_evaluations)
        .min(MAX_UNIFORM_SAMPLES);
    let span = x_max - x_min;
    let last = (n - 1) as f64;

    let mut sampler = Sampler {
        program,
        max_evals: opts.max_evaluations,
        evals: 0,
        max_depth: opts.max_depth,
        x_span: span,
        pixel: span / last,
        y_scale: 1.0,
        y_span: 1.0,
        pole_anchor: 1.0,
        // Speculative domain-island hunting gets its own capped allowance so
        // that a long NaN stretch cannot eat the budget of the visible curve.
        probe_budget: (opts.max_evaluations / 8).max(16),
        segments: Vec::new(),
        current: Vec::new(),
    };

    // 1. Uniform pre-grid. Every later decision (scale, pole threshold) is
    //    relative to what this grid saw, so it must be complete before the
    //    adaptive pass starts.
    let mut grid: Vec<[f64; 2]> = Vec::with_capacity(n.min(4096));
    for i in 0..n {
        let x = if i + 1 == n {
            x_max
        } else {
            x_min + span * (i as f64 / last)
        };
        let y = sampler.eval(x).unwrap_or(f64::NAN);
        grid.push([x, y]);
    }

    let ys: Vec<f64> = grid.iter().map(|p| p[1]).collect();
    let (y_scale, y_span, pole_anchor) = robust_scales(&ys);
    sampler.y_scale = y_scale;
    sampler.y_span = y_span;
    sampler.pole_anchor = pole_anchor;

    // 2. Adaptive pass. `walk` maintains the invariant that its left endpoint
    //    is already in `current` when it is finite, and that it has appended
    //    its right endpoint when that one is finite.
    if grid[0][1].is_finite() {
        sampler.emit(grid[0]);
    }
    for i in 0..(n - 1) {
        sampler.walk(grid[i], grid[i + 1], 0);
    }
    sampler.end_segment();

    (
        Curve {
            segments: sampler.segments,
        },
        SampleStats {
            evaluations: sampler.evals,
        },
    )
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Which side of a bracket is known to be finite.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

struct Sampler<'a> {
    program: &'a Program,
    max_evals: usize,
    evals: usize,
    max_depth: u8,
    x_span: f64,
    pixel: f64,
    /// Typical `|y|`, used to make the pole test scale-free.
    y_scale: f64,
    /// Robust vertical extent, used to make the angle test scale-free.
    y_span: f64,
    /// Typical *upper* magnitude (95th percentile of `|y|`), the evidence bar
    /// a confirmed runaway must clear before it is called a pole.
    pole_anchor: f64,
    probe_budget: usize,
    segments: Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
}

impl Sampler<'_> {
    /// Evaluate once, or `None` when the budget is gone.
    fn eval(&mut self, x: f64) -> Option<f64> {
        if self.evals >= self.max_evals || !x.is_finite() {
            return None;
        }
        self.evals += 1;
        Some(self.program.eval(&[x]))
    }

    /// Append a vertex, ignoring non-finite points and any attempt to walk
    /// backwards in `x` (which would mean the sampler contradicts itself).
    fn emit(&mut self, p: [f64; 2]) {
        if !p[0].is_finite() || !p[1].is_finite() {
            return;
        }
        if let Some(last) = self.current.last() {
            if p[0] <= last[0] {
                return;
            }
        }
        self.current.push(p);
    }

    /// Close the current polyline. Single-point segments are degenerate, so
    /// they are dropped instead of being handed to the renderer.
    fn end_segment(&mut self) {
        if self.current.len() >= 2 {
            self.segments.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    /// Width below which a bracket counts as resolved.
    fn target_width(&self) -> f64 {
        let floor = (self.x_span * 1e-12).max(f64::MIN_POSITIVE);
        let w = self.pixel * BISECT_TARGET_PIXELS;
        if w.is_finite() && w > 0.0 {
            w.max(floor)
        } else {
            floor
        }
    }

    /// Exact midpoint, or `None` when the bracket cannot be split further
    /// (adjacent floats, non-finite or reversed ends). This is what makes the
    /// recursion terminate in floating point, not just in `depth`.
    fn midpoint(lo: f64, hi: f64) -> Option<f64> {
        if !lo.is_finite() || !hi.is_finite() || !(hi > lo) {
            return None;
        }
        let m = lo + (hi - lo) * 0.5;
        if m.is_finite() && m > lo && m < hi {
            Some(m)
        } else {
            None
        }
    }

    fn probe_mid(&mut self, a: [f64; 2], b: [f64; 2]) -> Option<[f64; 2]> {
        let xm = Self::midpoint(a[0], b[0])?;
        let ym = self.eval(xm)?;
        Some([xm, ym])
    }

    /// Is a midpoint probe worth an evaluation at all?
    fn probe_needed(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        sign_opposite(a[1], b[1]) || {
            let mx = a[1].abs().max(b[1].abs());
            mx.is_finite() && mx > POLE_FACTOR * self.y_scale
        }
    }

    /// A same-sign break: both ends are already far above the typical scale
    /// and the point between them is undefined or far larger still.
    ///
    /// This is the case `1/x^2` produces, which no sign test could catch.
    fn blowup_is_pole(&self, a: [f64; 2], b: [f64; 2], mid: Option<[f64; 2]>) -> bool {
        let mx = a[1].abs().max(b[1].abs());
        if !(mx > POLE_FACTOR * self.y_scale) {
            return false;
        }
        let Some(m) = mid else { return false };
        if !m[1].is_finite() {
            return true;
        }
        m[1].abs() > POLE_MID_GROWTH * mx
    }

    /// Tell a pole from a root at a sign change, and bracket it in one go.
    ///
    /// An absolute magnitude threshold cannot do this: at a coarse resolution
    /// the grid may barely sample the function, so "30x the median" can be a
    /// value as ordinary as `0.1`. What always distinguishes the two cases is
    /// the *trend* as the bracket shrinks — the root pulls `|y|` towards zero,
    /// the pole drives it towards infinity, doubling on every halving because
    /// it goes like `1/distance`.
    ///
    /// Requiring a *streak* of such doublings, rather than one large value,
    /// is what keeps an under-resolved oscillation (`sin(50x)` with two samples
    /// per period) from being mistaken for a chain of poles.
    ///
    /// Returns the tightest finite bracket around the pole, or `None` for a
    /// root. Exhausting the budget answers `None`: an unconfirmed suspicion
    /// draws a steep line, which is a smaller lie than a gap in a curve that
    /// is actually continuous.
    fn confirm_sign_change(&mut self, a: [f64; 2], b: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
        let (mut lo, mut ylo) = (a[0], a[1]);
        let (mut hi, mut yhi) = (b[0], b[1]);
        // A pole has to be extraordinary even for this function, otherwise
        // aliasing noise looks the same.
        let floor = POLE_EVIDENCE_FLOOR * self.pole_anchor;
        let mut streak = 0u32;

        for _ in 0..POLE_CONFIRM_STEPS {
            let Some(m) = Self::midpoint(lo, hi) else {
                break;
            };
            let Some(ym) = self.eval(m) else { break };
            if !ym.is_finite() {
                // Landed on the pole itself. Approach it from both sides so
                // each branch runs off the picture instead of stopping at the
                // last grid sample.
                let left = self.bisect_edge(lo, m, Side::Left, ylo);
                let right = self.bisect_edge(m, hi, Side::Right, yhi);
                return Some((left, right));
            }
            if ym == 0.0 {
                // A root that the bisection hit exactly.
                return None;
            }

            // Each bisection replaces the end that lies on the same side of
            // the pole. For a pole that end is strictly closer to it, so its
            // magnitude must grow; for a root it shrinks towards zero.
            let replaces_hi = sign_opposite(ym, ylo);
            let previous = if replaces_hi { yhi.abs() } else { ylo.abs() };
            let current = ym.abs();
            if current > previous * POLE_CONFIRM_RATIO {
                streak += 1;
            } else if current <= previous {
                streak = 0;
            }
            if replaces_hi {
                hi = m;
                yhi = ym;
            } else {
                lo = m;
                ylo = ym;
            }

            if streak >= POLE_CONFIRM_STREAK && current > floor {
                return Some(([lo, ylo], [hi, yhi]));
            }
        }
        None
    }

    /// Close the branch on one side and open it on the other. The two points
    /// are never joined, which is the whole point of the segment type.
    fn cut_between(&mut self, left: [f64; 2], right: [f64; 2]) {
        self.emit(left);
        self.end_segment();
        self.emit(right);
    }

    /// Does this interval need another vertex?
    fn needs_split(&self, a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> bool {
        if !m[1].is_finite() {
            return false;
        }
        // Near a pole extra vertices buy no visible accuracy.
        let peak = a[1].abs().max(m[1].abs()).max(b[1].abs());
        if !peak.is_finite() || peak > SUBDIVISION_GUARD * self.y_scale {
            return false;
        }
        if !(m[0] > a[0]) || !(m[0] < b[0]) {
            return false;
        }
        let d1 = [(m[0] - a[0]) / self.x_span, (m[1] - a[1]) / self.y_span];
        let d2 = [(b[0] - m[0]) / self.x_span, (b[1] - m[1]) / self.y_span];
        if !d1[0].is_finite() || !d1[1].is_finite() {
            return false;
        }
        if !d2[0].is_finite() || !d2[1].is_finite() {
            return false;
        }
        angle_between(d1, d2) > ANGLE_THRESHOLD
    }

    /// Move a bracket whose *one* end is finite until the two ends are less
    /// than a sub-pixel apart, and report the finite point nearest the
    /// discontinuity. Side tells which end is finite; `y_def` is its value.
    fn bisect_edge(&mut self, lo: f64, hi: f64, side: Side, y_def: f64) -> [f64; 2] {
        let (mut lo, mut hi) = (lo, hi);
        if !(hi > lo) || !y_def.is_finite() {
            return [if side == Side::Right { hi } else { lo }, y_def];
        }
        let target = self.target_width();
        let mut best = match side {
            Side::Right => [hi, y_def],
            Side::Left => [lo, y_def],
        };
        let mut steps = 0u32;
        while steps < MAX_BISECT_STEPS && (hi - lo) > target {
            let Some(m) = Self::midpoint(lo, hi) else {
                break;
            };
            let Some(ym) = self.eval(m) else { break };
            steps += 1;
            if ym.is_finite() {
                match side {
                    Side::Right => {
                        hi = m;
                        best = [m, ym];
                    }
                    Side::Left => {
                        lo = m;
                        best = [m, ym];
                    }
                }
            } else {
                match side {
                    Side::Right => lo = m,
                    Side::Left => hi = m,
                }
            }
        }
        best
    }

    /// Handle one interval. On return, `b` is in `current` when it is finite,
    /// so the next interval can assume its left endpoint is already drawn.
    fn walk(&mut self, a: [f64; 2], b: [f64; 2], depth: u8) {
        let da = a[1].is_finite();
        let db = b[1].is_finite();

        match (da, db) {
            (true, true) => {
                let want_mid = depth < self.max_depth || self.probe_needed(a, b);
                let mid = if want_mid { self.probe_mid(a, b) } else { None };

                if sign_opposite(a[1], b[1]) {
                    // A sign change is either a root or a pole, and the two
                    // must be drawn differently.
                    if let Some((left, right)) = self.confirm_sign_change(a, b) {
                        self.cut_between(left, right);
                        self.emit(b);
                        return;
                    }
                } else if let Some(m) = mid {
                    // A non-finite value strictly between two finite ones is a
                    // gap (a narrow hole, or a pole the grid stepped over),
                    // never a vertex.
                    if !m[1].is_finite() || self.blowup_is_pole(a, b, Some(m)) {
                        self.cut_between(a, b);
                        self.emit(b);
                        return;
                    }
                }

                if depth < self.max_depth {
                    if let Some(m) = mid {
                        if self.needs_split(a, m, b) {
                            self.walk(a, m, depth.saturating_add(1));
                            self.walk(m, b, depth.saturating_add(1));
                            return;
                        }
                    }
                }
                self.emit(b);
            }
            (false, true) => {
                // Enter the domain from the left: the current branch must end
                // before the boundary and a new one starts just inside it.
                self.end_segment();
                let start = self.bisect_edge(a[0], b[0], Side::Right, b[1]);
                self.emit(start);
                self.walk(start, b, depth.saturating_add(1));
            }
            (true, false) => {
                // Leave the domain towards the right: approach the boundary as
                // closely as the budget allows, then stop.
                let end = self.bisect_edge(a[0], b[0], Side::Left, a[1]);
                self.emit(end);
                self.end_segment();
            }
            (false, false) => {
                // Nothing can be drawn on this interval, but a defined island
                // may still be hiding inside it. Hunt for one, cheaply and
                // with a separate allowance.
                self.end_segment();
                if depth >= self.max_depth.min(ISLAND_DEPTH_CAP) || self.probe_budget == 0 {
                    return;
                }
                let Some(xm) = Self::midpoint(a[0], b[0]) else {
                    return;
                };
                self.probe_budget -= 1;
                let Some(ym) = self.eval(xm) else { return };
                let m = [xm, ym];
                self.walk(a, m, depth.saturating_add(1));
                self.walk(m, b, depth.saturating_add(1));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small numeric helpers (all NaN-safe)
// ---------------------------------------------------------------------------

/// `true` only for two strictly opposite non-zero finite signs. Zeros and NaN
/// answer `false`: a sign change through zero is a root, not a pole.
fn sign_opposite(a: f64, b: f64) -> bool {
    (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0)
}

/// Angle between two 2-D vectors in `[0, π]`. Degenerate input answers 0,
/// which means "do not subdivide", the safe direction.
fn angle_between(u: [f64; 2], v: [f64; 2]) -> f64 {
    let cross = (u[0] * v[1] - u[1] * v[0]).abs();
    let dot = u[0] * v[0] + u[1] * v[1];
    if !cross.is_finite() || !dot.is_finite() {
        return 0.0;
    }
    let angle = cross.atan2(dot);
    if angle.is_finite() { angle } else { 0.0 }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let last = sorted.len() - 1;
    let pos = (q.clamp(0.0, 1.0) * last as f64).round();
    let idx = if pos.is_finite() && pos >= 0.0 {
        (pos as usize).min(last)
    } else {
        last / 2
    };
    sorted[idx]
}

/// Three scale estimates from the uniform grid: a typical magnitude (median of
/// `|y|`), a robust vertical extent (5th..95th percentile) and a typical
/// *upper* magnitude (95th percentile of `|y|`).
///
/// Percentiles rather than min/max throughout, so a single sample that landed
/// near a pole cannot define what "normal" means for the rest of the picture.
fn robust_scales(values: &[f64]) -> (f64, f64, f64) {
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return (1.0, 1.0, 1.0);
    }
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut magnitudes: Vec<f64> = finite.iter().map(|v| v.abs()).collect();
    magnitudes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = percentile(&magnitudes, 0.5);
    let scale = if median.is_finite() && median > 0.0 {
        median
    } else {
        let peak = magnitudes.last().copied().unwrap_or(0.0);
        if peak.is_finite() && peak > 0.0 {
            peak
        } else {
            1.0
        }
    };

    let lo = percentile(&finite, 0.05);
    let hi = percentile(&finite, 0.95);
    let mut span = hi - lo;
    if !span.is_finite() || span <= 0.0 {
        span = finite.last().copied().unwrap_or(0.0) - finite.first().copied().unwrap_or(0.0);
    }
    if !span.is_finite() || span <= 0.0 {
        span = 1.0;
    }

    let anchor = percentile(&magnitudes, 0.95)
        .max(span)
        .max(f64::MIN_POSITIVE);
    (scale, span, anchor)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::expr::ExprNode;
    use canlab_core::normalize::{add, mul, pow};
    use canlab_core::{Expr, SymbolId, int, ratio, sym};

    fn xi() -> SymbolId {
        SymbolId::intern("x")
    }

    fn x() -> Expr {
        sym("x")
    }

    fn unary(name: &str) -> Expr {
        ExprNode::fn_raw(SymbolId::intern(name), vec![x()])
    }

    fn prog(e: &Expr) -> Program {
        Program::compile(e, &[xi()]).expect("test expression compiles")
    }

    fn options(x_min: f64, x_max: f64, resolution: usize) -> SampleOptions {
        SampleOptions {
            x_min,
            x_max,
            resolution,
            ..SampleOptions::default()
        }
    }

    fn all_finite(curve: &Curve) -> bool {
        curve
            .segments
            .iter()
            .flat_map(|seg| seg.iter())
            .all(|p| p[0].is_finite() && p[1].is_finite())
    }

    #[test]
    fn sin_is_one_segment_with_many_finite_points() {
        let p = prog(&unary("sin"));
        let curve = sample(&p, &options(0.0, 6.28, 800));
        assert_eq!(curve.segment_count(), 1, "sin has no discontinuity");
        assert!(
            curve.point_count() >= 100,
            "expected a dense polyline, got {} points",
            curve.point_count()
        );
        assert!(all_finite(&curve));
        let seg = &curve.segments[0];
        assert!((seg[0][0] - 0.0).abs() < 1e-12);
        assert!((seg[seg.len() - 1][0] - 6.28).abs() < 1e-12);
    }

    #[test]
    fn reciprocal_is_split_at_the_pole() {
        let e = pow(x(), int(-1));
        let p = prog(&e);
        let curve = sample(&p, &options(-1.0, 1.0, 800));
        assert!(
            curve.segment_count() >= 2,
            "1/x must not be one polyline, got {:?}",
            curve.segments
        );
        assert_eq!(curve.segment_count(), 2, "exactly two branches on [-1, 1]");
    }

    #[test]
    fn reciprocal_does_not_bridge_the_pole() {
        let e = pow(x(), int(-1));
        let p = prog(&e);
        let curve = sample(&p, &options(-1.0, 1.0, 800));
        for seg in &curve.segments {
            let crosses = seg.iter().any(|q| q[0] < 0.0) && seg.iter().any(|q| q[0] > 0.0);
            assert!(!crosses, "a segment spans the pole: {seg:?}");
        }
        assert!(all_finite(&curve));
        let left = curve.segments[0].last().expect("left branch has points");
        let right = curve.segments[1].first().expect("right branch has points");
        assert!(left[0] < 0.0 && right[0] > 0.0);
    }

    #[test]
    fn sqrt_starts_at_the_domain_boundary() {
        let e = pow(x(), ratio(1, 2));
        let p = prog(&e);
        let curve = sample(&p, &options(-2.0, 4.0, 800));
        assert_eq!(curve.segment_count(), 1, "sqrt is connected on its domain");
        let seg = &curve.segments[0];
        assert!(
            seg[0][0] >= 0.0,
            "segment must begin on the domain, not at -2: {}",
            seg[0][0]
        );
        assert!(
            seg[0][0] < 0.01,
            "boundary should be found, got {}",
            seg[0][0]
        );
        assert!(all_finite(&curve));
        let end = seg[seg.len() - 1][0];
        assert!((end - 4.0).abs() < 1e-12);
    }

    #[test]
    fn tan_is_split_at_its_poles() {
        let p = prog(&unary("tan"));
        let curve = sample(&p, &options(-4.0, 4.0, 800));
        // Only +-pi/2 lie inside [-4, 4], so tan has exactly three branches.
        assert!(
            curve.segment_count() > 1,
            "tan must not be one polyline, got {}",
            curve.segment_count()
        );
        assert_eq!(curve.segment_count(), 3, "two poles = three branches");
        assert!(all_finite(&curve));
    }

    #[test]
    fn constant_function_is_one_segment() {
        let p = prog(&int(3));
        let curve = sample(&p, &options(-5.0, 5.0, 100));
        assert_eq!(curve.segment_count(), 1);
        assert_eq!(curve.point_count(), 100);
        assert!(curve.segments[0].iter().all(|q| q[1] == 3.0));
    }

    #[test]
    fn every_nan_function_yields_no_segments() {
        // -x^2 - 1 is always negative, so its square root is always NaN.
        let e = pow(
            add([mul([int(-1), pow(x(), int(2))]), int(-1)]),
            ratio(1, 2),
        );
        let p = prog(&e);
        let (curve, stats) = sample_with_stats(&p, &options(-3.0, 3.0, 200));
        assert!(curve.is_empty(), "nothing is defined: {:?}", curve.segments);
        assert!(curve.point_count() == 0);
        // The island hunt is bounded: it must not eat a whole default budget.
        assert!(stats.evaluations <= 200 + 200_000 / 8 + 16);
    }

    #[test]
    fn tiny_budget_is_never_exceeded() {
        let e = pow(x(), int(-1));
        let p = prog(&e);
        let opts = SampleOptions {
            x_min: -1.0,
            x_max: 1.0,
            resolution: 800,
            max_evaluations: 5,
            max_depth: 12,
        };
        let (curve, stats) = sample_with_stats(&p, &opts);
        assert!(stats.evaluations <= 5, "spent {} of 5", stats.evaluations);
        assert!(all_finite(&curve));
        assert!(curve.point_count() <= 5);
    }

    #[test]
    fn zero_budget_yields_no_segments() {
        let p = prog(&unary("sin"));
        let opts = SampleOptions {
            max_evaluations: 0,
            ..SampleOptions::default()
        };
        let (curve, stats) = sample_with_stats(&p, &opts);
        assert!(curve.is_empty());
        assert_eq!(stats.evaluations, 0);
    }

    #[test]
    fn degenerate_x_range_yields_no_segments() {
        let p = prog(&unary("sin"));
        assert!(sample(&p, &options(1.0, 1.0, 100)).is_empty());
        assert!(sample(&p, &options(1.0, -1.0, 100)).is_empty());
        assert!(sample(&p, &options(f64::NAN, 1.0, 100)).is_empty());
        assert!(sample(&p, &options(0.0, f64::INFINITY, 100)).is_empty());
    }

    #[test]
    fn every_segment_has_at_least_two_points() {
        for e in [unary("sin"), unary("tan"), pow(x(), int(-1)), unary("ln")] {
            let p = prog(&e);
            let curve = sample(&p, &options(-4.0, 4.0, 400));
            for seg in &curve.segments {
                assert!(seg.len() >= 2, "degenerate segment {seg:?} for {e}");
            }
        }
    }

    #[test]
    fn low_resolution_is_refined_by_curvature() {
        let p = prog(&unary("sin"));
        let curve = sample(&p, &options(0.0, 6.28, 8));
        assert_eq!(curve.segment_count(), 1);
        assert!(
            curve.point_count() > 8,
            "curvature subdivision added nothing: {}",
            curve.point_count()
        );
    }

    #[test]
    fn linear_function_is_not_subdivided() {
        let e = add([mul([int(2), x()]), int(1)]);
        let p = prog(&e);
        let curve = sample(&p, &options(0.0, 1.0, 32));
        assert_eq!(curve.segment_count(), 1);
        assert_eq!(
            curve.point_count(),
            32,
            "a straight line needs no extra vertices"
        );
    }

    #[test]
    fn max_depth_zero_still_splits_poles() {
        let e = pow(x(), int(-1));
        let p = prog(&e);
        let opts = SampleOptions {
            x_min: -1.0,
            x_max: 1.0,
            resolution: 200,
            max_evaluations: 5_000,
            max_depth: 0,
        };
        let curve = sample(&p, &opts);
        assert!(curve.segment_count() >= 2);
    }

    #[test]
    fn budget_is_respected_under_subdivision() {
        // High curvature everywhere plus a coarse grid: the expensive case.
        let e = unary("sin");
        let p = prog(&e);
        let opts = SampleOptions {
            x_min: 0.0,
            x_max: 60.0,
            resolution: 40,
            max_evaluations: 500,
            max_depth: 12,
        };
        let (curve, stats) = sample_with_stats(&p, &opts);
        assert!(stats.evaluations <= 500, "spent {}", stats.evaluations);
        assert!(curve.point_count() >= 2);
        assert!(all_finite(&curve));
    }

    #[test]
    fn all_emitted_points_are_finite() {
        for e in [unary("tan"), pow(x(), int(-1)), unary("ln"), unary("sqrt")] {
            let p = prog(&e);
            let curve = sample(&p, &options(-6.0, 6.0, 500));
            assert!(all_finite(&curve), "non-finite vertex for {e}");
        }
    }
}
