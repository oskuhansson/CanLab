//! `canlab-plot` — from a compiled expression or an implicit equation to
//! polylines, and from polylines to SVG.
//!
//! The crate is deliberately a set of small, separable stages:
//!
//! * [`sampler`] turns a [`canlab_core::Program`] into a [`Curve`]: a list of
//!   polylines where every break means "the function is not connected here".
//! * [`implicit`] turns an `f(x, y)` predicate into a [`Curve`] with marching
//!   squares, for equations that cannot be written as `y = g(x)`.
//! * [`svg`] turns a [`Curve`] into a self-contained SVG document.
//!
//! # Why a curve is a list of segments and not a list of points
//!
//! A renderer cannot tell a genuine corner from an asymptote. Given only
//! points, `1/x` sampled on `[-1, 1]` looks like a continuous curve that
//! happens to jump from `-∞` to `+∞`, and the natural rendering — join every
//! consecutive pair — draws a vertical line through the pole. That line is
//! wrong, and it is wrong in a way the reader cannot detect.
//!
//! The sampler is the only component that sees the raw evaluations, so it makes
//! the continuity decision there and encodes it in the type: an asymptote ends
//! a segment. The renderer then has no decision left to make, and the two
//! stages cannot disagree.
//!
//! # The `NaN` / `Inf` convention
//!
//! [`canlab_core::Program::eval`] answers `NaN` outside a function's domain and
//! `±Inf` (or a huge finite value) at a pole. Those are not points to be
//! plotted — they are gaps. Neither stage ever emits a non-finite coordinate,
//! which is what lets [`to_svg`] promise a document that contains no `NaN`.
//!
//! # Example
//!
//! ```
//! use canlab_core::{pow, int, sym, Program, SymbolId};
//! use canlab_plot::{sample, to_svg, Curve, SampleOptions, SvgOptions};
//!
//! let x = sym("x");
//! let expr = pow(x, int(-1)); // 1/x
//! let program = Program::compile(&expr, &[SymbolId::intern("x")]).expect("compiles");
//!
//! let curve: Curve = sample(
//!     &program,
//!     &SampleOptions { x_min: -1.0, x_max: 1.0, ..SampleOptions::default() },
//! );
//! assert!(curve.segment_count() >= 2, "the pole must split the curve");
//!
//! let svg = to_svg(&curve, &SvgOptions { x_range: (-1.0, 1.0), y_range: (-5.0, 5.0), ..SvgOptions::default() });
//! assert!(svg.starts_with("<svg"));
//! ```

pub mod implicit;
pub mod intersect;
pub mod sampler;
pub mod svg;

pub use implicit::{ImplicitOptions, sample_implicit};
pub use intersect::{Intersection, locate, refine};
pub use sampler::{Curve, SampleOptions, SampleStats, sample, sample_with_stats};
pub use svg::{SvgOptions, map_to_pixels, to_svg};
