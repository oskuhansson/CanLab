//! SVG rendering for [`Curve`]s.
//!
//! The renderer is deliberately dumb. Every decision about continuity was made
//! by the sampler, which expressed it as a segment break; all that is left here
//! is an affine map from data space to pixel space and a `<polyline>` per
//! segment. Keeping the two jobs apart is what stops "should these two points
//! be joined?" from being answered differently (and inconsistently) in the
//! sampler and in the printer.
//!
//! # Coordinate mapping
//!
//! SVG's `y` axis grows *downward*, data space grows upward, so the vertical
//! map is flipped: `py = height - (y - y_min) / (y_max - y_min) * height`.
//! Everything that leaves the plot is bounded: points outside the view are
//! clamped to a margin around the ranges rather than dropped, because dropping
//! them would shorten a polyline and visibly truncate a branch that merely
//! exits the top of the picture. Points that are not finite are dropped: they
//! cannot be mapped at all, and the document must never contain `NaN`.
//!
//! # Number formatting
//!
//! All numbers go through [`fmt_coord`] / [`fmt_label`], which use Rust's
//! fixed-precision formatting. That is locale-independent by construction
//! (a German locale must not turn `0.5` into `0,5` in an SVG attribute), and
//! the six-decimal output is precise to far better than a device pixel.

use crate::sampler::Curve;

/// Six decimals keep the error below 5e-7 pixels, well under the 1e-6 the
/// mapping contract promises, while staying readable.
const COORD_DECIMALS: usize = 6;

/// Points may sit this many view-widths outside the ranges before being
/// pinned. Generous on purpose: the line should still leave the frame at the
/// right angle, but a pole at `y = 1e300` must not emit a 300-digit number.
const PAD_FACTOR: f64 = 10.0;

/// Hard bound on a single emitted coordinate.
const COORD_LIMIT: f64 = 1e9;

/// Upper bound on ticks per axis; keeps a pathological range from producing a
/// megabyte of axis furniture.
const MAX_TICKS: usize = 64;

const AXIS_COLOR: &str = "#9e9e9e";
const LABEL_COLOR: &str = "#616161";

/// How to draw a [`Curve`].
#[derive(Clone, Debug, PartialEq)]
pub struct SvgOptions {
    /// Document width in pixels (at least 1).
    pub width: u32,
    /// Document height in pixels (at least 1).
    pub height: u32,
    /// Data range shown horizontally; must be increasing.
    pub x_range: (f64, f64),
    /// Data range shown vertically; must be increasing.
    pub y_range: (f64, f64),
    /// Stroke colour, usually `"#1e88e5"`.
    pub stroke: String,
    /// Draw the zero axes and their ticks when they fall inside the view.
    pub show_axes: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            x_range: (-10.0, 10.0),
            y_range: (-10.0, 10.0),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Render `curve` as a complete SVG document.
///
/// The result starts with `<svg` and ends with `</svg>`, contains a white
/// background, optional axes, and exactly one `<polyline fill="none" …>` per
/// drawable segment. It contains no `NaN` or `inf` text under any input,
/// because non-finite coordinates are skipped and every emitted number is
/// finite.
pub fn to_svg(curve: &Curve, opts: &SvgOptions) -> String {
    let (w, h) = view_size(opts);
    let wu = opts.width.max(1);
    let hu = opts.height.max(1);

    let mut out = String::with_capacity(256 + curve.point_count() * 16);
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\"");
    out.push_str(&format!(" width=\"{wu}\" height=\"{hu}\""));
    out.push_str(&format!(" viewBox=\"0 0 {w} {h}\">"));

    // Opaque background: a plot must not depend on the reader's page colour.
    out.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>"
    ));

    if opts.show_axes {
        write_axes(&mut out, opts);
    }

    let stroke = escape_xml(&opts.stroke);
    for seg in &curve.segments {
        let mut points = String::new();
        let mut count = 0usize;
        for &p in seg {
            // Clamp first (keeps coordinates bounded), then map; a point that
            // still cannot be mapped is non-finite and is skipped.
            let Some(px) = map_to_pixels(clamp_for_display(p, opts), opts) else {
                continue;
            };
            if count > 0 {
                points.push(' ');
            }
            points.push_str(&fmt_coord(px[0]));
            points.push(' ');
            points.push_str(&fmt_coord(px[1]));
            count += 1;
        }
        // A one-point polyline draws nothing; emitting it would only be noise.
        if count >= 2 {
            out.push_str(&format!(
                "<polyline fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" \
                 stroke-linejoin=\"round\" stroke-linecap=\"round\" points=\"{points}\"/>"
            ));
        }
    }

    out.push_str("</svg>");
    out
}

/// Map a data point to pixel coordinates, or `None` when the point is not
/// finite or a range is degenerate.
///
/// Horizontal: `x_min` maps to `0`, `x_max` maps to `width`.
/// Vertical: `y_min` maps to `height`, `y_max` maps to `0` (SVG y grows down).
pub fn map_to_pixels(point: [f64; 2], opts: &SvgOptions) -> Option<[f64; 2]> {
    if !point[0].is_finite() || !point[1].is_finite() {
        return None;
    }
    let (x0, x1) = opts.x_range;
    let (y0, y1) = opts.y_range;
    if !x0.is_finite() || !x1.is_finite() || !y0.is_finite() || !y1.is_finite() {
        return None;
    }
    let sx = x1 - x0;
    let sy = y1 - y0;
    // `!(s > 0.0)` rejects both NaN spans and flipped ranges.
    if !(sx > 0.0) || !(sy > 0.0) {
        return None;
    }
    let (w, h) = view_size(opts);
    let px = (point[0] - x0) / sx * w;
    let py = h - (point[1] - y0) / sy * h;
    if !px.is_finite() || !py.is_finite() {
        return None;
    }
    Some([px, py])
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn view_size(opts: &SvgOptions) -> (f64, f64) {
    (opts.width.max(1) as f64, opts.height.max(1) as f64)
}

/// Keep a point within a generous margin of the view so that a pole does not
/// become an astronomically large coordinate. Points inside the ranges are
/// returned untouched, so the documented mapping stays exact.
fn clamp_for_display(p: [f64; 2], opts: &SvgOptions) -> [f64; 2] {
    let (x0, x1) = opts.x_range;
    let (y0, y1) = opts.y_range;
    let mut out = p;
    if x0.is_finite() && x1.is_finite() && x1 > x0 {
        let pad = (x1 - x0) * PAD_FACTOR;
        out[0] = p[0].clamp(x0 - pad, x1 + pad);
    }
    if y0.is_finite() && y1.is_finite() && y1 > y0 {
        let pad = (y1 - y0) * PAD_FACTOR;
        out[1] = p[1].clamp(y0 - pad, y1 + pad);
    }
    out
}

/// Axes are drawn only where they exist: an axis at `y = 0` is meaningless
/// when zero is off the top of the view.
fn write_axes(out: &mut String, opts: &SvgOptions) {
    let (w, h) = view_size(opts);
    let (x0, x1) = opts.x_range;
    let (y0, y1) = opts.y_range;

    let x_axis = if y0.is_finite() && y1.is_finite() && y0 < 0.0 && y1 > 0.0 {
        map_to_pixels([0.0, 0.0], opts).map(|p| p[1])
    } else {
        None
    };
    let y_axis = if x0.is_finite() && x1.is_finite() && x0 < 0.0 && x1 > 0.0 {
        map_to_pixels([0.0, 0.0], opts).map(|p| p[0])
    } else {
        None
    };

    out.push_str(&format!(
        "<g stroke=\"{AXIS_COLOR}\" stroke-width=\"1\" fill=\"none\">"
    ));
    if let Some(py) = x_axis {
        out.push_str(&format!(
            "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
            fmt_coord(py),
            fmt_coord(w),
            fmt_coord(py)
        ));
    }
    if let Some(px) = y_axis {
        out.push_str(&format!(
            "<line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{}\"/>",
            fmt_coord(px),
            fmt_coord(px),
            fmt_coord(h)
        ));
    }
    out.push_str("</g>");

    let mut labels = String::new();
    if let Some(py) = x_axis {
        for v in ticks(x0, x1) {
            let Some(p) = map_to_pixels([v, 0.0], opts) else {
                continue;
            };
            out.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{AXIS_COLOR}\"/>",
                fmt_coord(p[0]),
                fmt_coord(py - 3.0),
                fmt_coord(p[0]),
                fmt_coord(py + 3.0),
            ));
            if v != 0.0 {
                labels.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-size=\"11\" fill=\"{LABEL_COLOR}\" \
                     text-anchor=\"middle\">{}</text>",
                    fmt_coord(p[0]),
                    fmt_coord(py + 15.0),
                    fmt_label(v)
                ));
            }
        }
    }
    if let Some(px) = y_axis {
        for v in ticks(y0, y1) {
            let Some(p) = map_to_pixels([0.0, v], opts) else {
                continue;
            };
            out.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{AXIS_COLOR}\"/>",
                fmt_coord(px - 3.0),
                fmt_coord(p[1]),
                fmt_coord(px + 3.0),
                fmt_coord(p[1]),
            ));
            if v != 0.0 {
                labels.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-size=\"11\" fill=\"{LABEL_COLOR}\" \
                     text-anchor=\"end\">{}</text>",
                    fmt_coord(px - 6.0),
                    fmt_coord(p[1] + 4.0),
                    fmt_label(v)
                ));
            }
        }
    }
    if !labels.is_empty() {
        out.push_str(&labels);
    }
}

/// Tick positions on `[lo, hi]` at a 1-2-5 step. Bounded in count and in
/// iterations, so a tiny or non-finite range cannot spin here.
fn ticks(lo: f64, hi: f64) -> Vec<f64> {
    let step = nice_step(hi - lo);
    let mut out = Vec::new();
    if !lo.is_finite() || !hi.is_finite() || !(hi > lo) || !step.is_finite() || step <= 0.0 {
        return out;
    }
    let start = (lo / step).ceil();
    if !start.is_finite() {
        return out;
    }
    for i in 0..MAX_TICKS {
        let v = (start + i as f64) * step;
        if !v.is_finite() || v > hi {
            break;
        }
        out.push(v);
    }
    out
}

/// The classic 1-2-5 "nice" step, aiming at roughly eight intervals.
fn nice_step(span: f64) -> f64 {
    if !span.is_finite() || span <= 0.0 {
        return 1.0;
    }
    let rough = span / 8.0;
    if !rough.is_finite() || rough <= 0.0 {
        return 1.0;
    }
    let magn = 10f64.powf(rough.log10().floor());
    if !magn.is_finite() || magn <= 0.0 {
        return 1.0;
    }
    let norm = rough / magn;
    let mult = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = magn * mult;
    if step.is_finite() && step > 0.0 {
        step
    } else {
        1.0
    }
}

/// Fixed-point, trailing-zero-trimmed, never `NaN`/`inf`.
fn fmt_coord(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let clamped = v.clamp(-COORD_LIMIT, COORD_LIMIT);
    let mut s = format!("{clamped:.COORD_DECIMALS$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s.is_empty() || s == "-0" {
        s = "0".to_string();
    }
    s
}

/// Tick labels: short, readable, still never `NaN`/`inf`.
fn fmt_label(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let a = v.abs();
    let mut s = if a != 0.0 && (a < 1e-4 || a >= 1e6) {
        format!("{v:e}")
    } else {
        let mut t = format!("{v:.4}");
        if t.contains('.') {
            t = t.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        t
    };
    if s.is_empty() || s == "-0" {
        s = "0".to_string();
    }
    s
}

/// Escaping is not cosmetic: an unescaped quote in a caller-supplied stroke
/// would break the document's well-formedness.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> SvgOptions {
        SvgOptions {
            width: 800,
            height: 600,
            x_range: (0.0, 10.0),
            y_range: (-5.0, 5.0),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
        }
    }

    fn curve_of(segments: Vec<Vec<[f64; 2]>>) -> Curve {
        Curve { segments }
    }

    /// Every `points="…"` payload in the document, as pixel pairs.
    fn polyline_points(svg: &str) -> Vec<Vec<[f64; 2]>> {
        let mut out = Vec::new();
        let mut rest = svg;
        while let Some(i) = rest.find("points=\"") {
            let after = &rest[i + "points=\"".len()..];
            let Some(j) = after.find('"') else { break };
            let body = &after[..j];
            let nums: Vec<f64> = body
                .split_whitespace()
                .filter_map(|t| t.parse::<f64>().ok())
                .collect();
            out.push(nums.chunks_exact(2).map(|c| [c[0], c[1]]).collect());
            rest = &after[j..];
        }
        out
    }

    fn polyline_count(svg: &str) -> usize {
        svg.matches("<polyline").count()
    }

    #[test]
    fn document_is_well_formed() {
        let svg = to_svg(&curve_of(vec![vec![[1.0, 0.0], [2.0, 1.0]]]), &opts());
        assert!(
            svg.starts_with("<svg"),
            "starts with: {:?}",
            &svg[..20.min(svg.len())]
        );
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 800 600\""));
        assert!(svg.contains("<rect"), "missing background");
        assert!(svg.contains("fill=\"#ffffff\""), "background must be white");
    }

    #[test]
    fn contains_no_nan_or_inf_text() {
        let curve = curve_of(vec![vec![
            [f64::NAN, 1.0],
            [1.0, f64::INFINITY],
            [2.0, f64::NEG_INFINITY],
            [3.0, 1e308],
            [4.0, -1e308],
            [5.0, 0.0],
        ]]);
        let svg = to_svg(&curve, &opts());
        assert!(!svg.contains("NaN"), "document mentions NaN");
        assert!(!svg.contains("nan"), "document mentions nan");
        assert!(!svg.contains("inf"), "document mentions inf");
        assert!(!svg.contains("Inf"), "document mentions Inf");
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn one_polyline_per_segment() {
        let curve = curve_of(vec![
            vec![[0.0, -1.0], [1.0, -2.0], [2.0, -1.0]],
            vec![[3.0, 1.0], [4.0, 2.0]],
            vec![[5.0, 0.0], [6.0, 1.0], [7.0, 2.0], [8.0, 1.0]],
        ]);
        let svg = to_svg(&curve, &opts());
        assert_eq!(polyline_count(&svg), 3);
        let total: usize = polyline_points(&svg).iter().map(|p| p.len()).sum();
        assert_eq!(
            total,
            curve.point_count(),
            "point count must match the curve"
        );
    }

    #[test]
    fn known_point_maps_to_expected_pixel() {
        let o = SvgOptions {
            width: 800,
            height: 600,
            x_range: (0.0, 3.0),
            y_range: (0.0, 7.0),
            stroke: "#000000".to_string(),
            show_axes: false,
        };
        let p = map_to_pixels([1.0, 2.0], &o).expect("finite point maps");
        assert!((p[0] - 800.0 / 3.0).abs() < 1e-6, "px = {}", p[0]);
        assert!(
            (p[1] - (600.0 - 1200.0 / 7.0)).abs() < 1e-6,
            "py = {}",
            p[1]
        );

        // …and the same value must survive the round trip through the text.
        let svg = to_svg(&curve_of(vec![vec![[1.0, 2.0], [2.0, 3.0]]]), &o);
        let points = polyline_points(&svg);
        assert_eq!(points.len(), 1);
        assert!((points[0][0][0] - 800.0 / 3.0).abs() < 1e-6);
        assert!((points[0][0][1] - (600.0 - 1200.0 / 7.0)).abs() < 1e-6);
    }

    #[test]
    fn corners_map_to_the_corners() {
        let o = SvgOptions {
            x_range: (0.0, 10.0),
            y_range: (-5.0, 5.0),
            ..opts()
        };
        let bl = map_to_pixels([0.0, -5.0], &o).expect("finite");
        let tr = map_to_pixels([10.0, 5.0], &o).expect("finite");
        assert!((bl[0] - 0.0).abs() < 1e-6 && (bl[1] - 600.0).abs() < 1e-6);
        assert!((tr[0] - 800.0).abs() < 1e-6 && (tr[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn svg_y_axis_points_down() {
        let o = opts();
        let high = map_to_pixels([0.0, 4.0], &o).expect("finite");
        let low = map_to_pixels([0.0, -4.0], &o).expect("finite");
        assert!(high[1] < low[1], "larger y must be nearer the top");
    }

    #[test]
    fn empty_curve_still_renders_a_document() {
        let svg = to_svg(&Curve::default(), &opts());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert_eq!(polyline_count(&svg), 0);
    }

    #[test]
    fn axes_can_be_switched_off() {
        let curve = curve_of(vec![vec![[1.0, 0.0], [2.0, 1.0]]]);
        let with_axes = to_svg(&curve, &opts());
        assert!(with_axes.contains("<line"), "axes should draw axis lines");
        let without = to_svg(
            &curve,
            &SvgOptions {
                show_axes: false,
                ..opts()
            },
        );
        assert!(!without.contains("<line"), "no axis lines expected");
        assert!(without.contains("<polyline"));
    }

    #[test]
    fn single_point_segments_are_not_drawn() {
        let svg = to_svg(&curve_of(vec![vec![[1.0, 1.0]]]), &opts());
        assert_eq!(polyline_count(&svg), 0);
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn stroke_is_xml_escaped() {
        let svg = to_svg(
            &curve_of(vec![vec![[1.0, 0.0], [2.0, 1.0]]]),
            &SvgOptions {
                stroke: "#000\" onload=\"alert(1)".to_string(),
                ..opts()
            },
        );
        assert!(!svg.contains("onload=\"alert(1)\""));
        assert!(svg.contains("&quot;"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn degenerate_ranges_never_produce_bad_documents() {
        let o = SvgOptions {
            width: 0,
            height: 0,
            x_range: (5.0, 5.0),
            y_range: (f64::NAN, 1.0),
            stroke: "#000000".to_string(),
            show_axes: true,
        };
        let svg = to_svg(&curve_of(vec![vec![[1.0, 1.0], [2.0, 2.0]]]), &o);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(!svg.contains("NaN"));
        assert!(!svg.contains("inf"));
        assert!(map_to_pixels([1.0, 1.0], &o).is_none());
    }

    #[test]
    fn far_out_of_range_values_are_clamped_not_huge() {
        let svg = to_svg(
            &curve_of(vec![vec![[1.0, 1e300], [2.0, -1e300], [3.0, 0.0]]]),
            &opts(),
        );
        let points = polyline_points(&svg);
        assert_eq!(points.len(), 1);
        for p in &points[0] {
            assert!(
                p[0].abs() < 1e7 && p[1].abs() < 1e7,
                "unbounded point {p:?}"
            );
        }
    }
}
