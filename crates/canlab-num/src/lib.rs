//! Numerical root finding in one variable.
//!
//! A root is a number, and a number is exactly the thing this crate cannot
//! promise: `√2` is not a `f64`, and `(x − 1)²` has a root that no sign change
//! reveals. The crate therefore does two things and refuses to confuse them:
//!
//! * [`brent`] refines one **bracket** — two points whose function values have
//!   opposite signs — with Brent's method;
//! * [`scan`] samples an interval, finds every bracket it can, finds the roots
//!   that touch without changing sign by minimising `|f|` inside each cell, and
//!   deflates each root it finds so a double root is reported once rather than
//!   twice.
//!
//! Every answer is a [`Root`] carrying its residual. The type is the guard: a
//! caller cannot receive a root value without also receiving the evidence for
//! it. A residual near `1e-16` is a root; a residual near `1` is a jump between
//! two values that happened to change sign, and the caller can see that.
//!
//! # What "residual" means here
//!
//! `residual = |f(root)| / max(1, S)`, where `S` is the largest `|f|` sampled
//! while looking. For [`brent`] that is the larger of the two bracket ends; for
//! [`scan`] it is the largest value on the scan grid. The `max(1, ...)` is
//! deliberate: a function that is small everywhere gets an **absolute**
//! residual, which is the conservative reading, and a function whose scale is
//! `10⁶` gets a **relative** one, which is the only useful reading. The number
//! is not a proof of accuracy — no floating-point residual is — but it is the
//! quantity that separates a root from a sign change at a pole.
//!
//! # What this crate assumes
//!
//! [`brent`] assumes the function is continuous on the bracket. [`scan`] assumes
//! it is continuous on the interval apart from what the sampling can expose: a
//! sign change at a jump is refined to the jump and then **refused** because the
//! residual there is large. A pole the grid lands on is refused as a non-finite
//! value; a pole between two samples is refused by the same residual check.
//! Neither function can detect a jump whose two sides happen to evaluate to the
//! same number, and no amount of sampling can.

#![forbid(unsafe_code)]

use std::fmt;

pub mod brent;
pub mod distribution;
pub mod expression;
pub mod scan;
pub mod stats;

pub use brent::brent;
pub use expression::{scan_expression, scan_expression_with_grid};
pub use scan::{DEFAULT_GRID, scan, scan_with_grid};

/// The interval the solver can vouch for.
///
/// For a sign-change root this is the final Brent bracket, so its ends have
/// opposite signs. For a root evaluated exactly to zero it is the point itself.
/// For a root that touches without changing sign it is the cell (or window)
/// whose minimum was tested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bracket {
    pub left: f64,
    pub right: f64,
}

impl Bracket {
    /// The two ends in ascending order.
    pub fn new(left: f64, right: f64) -> Self {
        if left <= right {
            Self { left, right }
        } else {
            Self {
                left: right,
                right: left,
            }
        }
    }

    pub fn width(self) -> f64 {
        self.right - self.left
    }

    pub fn contains(self, value: f64) -> bool {
        self.left <= value && value <= self.right
    }
}

/// A root, its bracket, and the evidence that it is one.
///
/// `residual` is defined on the crate, not on each function, so two roots from
/// the same scan can be compared. It is `|f(value)|` divided by the function's
/// scale when that scale is above one, and `|f(value)|` itself otherwise. See
/// the crate documentation for the exact rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Root {
    /// The best estimate of the root.
    pub value: f64,
    /// `|f(value)|`, or the same divided by the largest `|f|` sampled.
    pub residual: f64,
    /// The interval this answer rests on.
    pub bracket: Bracket,
}

impl Root {
    /// Whether the residual is at most `limit`, the check a caller almost
    /// always wants before printing the number.
    pub fn is_within(&self, limit: f64) -> bool {
        self.residual <= limit
    }
}

/// Why a root was not returned.
///
/// A refusal is a normal answer. It says what stopped the solver, not that the
/// program failed.
#[derive(Debug, Clone, PartialEq)]
pub enum RootError {
    /// The interval was empty, non-finite, or its width was not representable.
    InvalidInterval { left: f64, right: f64 },
    /// A tolerance must be finite and positive.
    InvalidTolerance { tolerance: f64 },
    /// A scan grid needs at least one cell and at most [`scan::MAX_GRID`].
    InvalidGrid { grid: usize },
    /// `f` produced `NaN` or an infinity. Continuing would mean guessing.
    NonFinite { at: f64, value: f64 },
    /// The two ends do not have opposite signs, so no bracket exists.
    NotBracketed { left: f64, right: f64 },
    /// The bracket closed on a point whose residual is too large to be a root.
    ///
    /// This is how a sign change at a discontinuity is refused: Brent finds the
    /// jump, and the residual says the function did not reach zero there.
    ResidualTooLarge {
        value: f64,
        residual: f64,
        limit: f64,
    },
    /// The iteration budget ran out before the bracket met the tolerance.
    NoConvergence { iterations: usize },
    /// There were more roots, or more deflation rounds, than this crate will
    /// report. The cap exists so a pathological function cannot loop forever.
    TooManyRoots,
    /// `Program::compile` refused the expression, for example because it holds
    /// an unevaluated integral or mentions a second variable.
    UnsupportedExpression,
}

impl fmt::Display for RootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterval { left, right } => write!(
                formatter,
                "the interval [{left}, {right}] is empty or non-finite"
            ),
            Self::InvalidTolerance { tolerance } => write!(
                formatter,
                "the tolerance {tolerance} is not finite and positive"
            ),
            Self::InvalidGrid { grid } => {
                write!(formatter, "a scan grid of {grid} cells is not usable")
            }
            Self::NonFinite { at, value } => write!(
                formatter,
                "the function is not finite at {at} (it gave {value}), so there is no continuity to use"
            ),
            Self::NotBracketed { left, right } => write!(
                formatter,
                "the function has the same sign at {left} and {right}, so the interval does not bracket a root"
            ),
            Self::ResidualTooLarge {
                value,
                residual,
                limit,
            } => write!(
                formatter,
                "the bracket closed near {value}, but the residual {residual} is above {limit}; this looks like a jump rather than a root"
            ),
            Self::NoConvergence { iterations } => write!(
                formatter,
                "Brent's method used all {iterations} iterations without meeting the tolerance"
            ),
            Self::TooManyRoots => write!(
                formatter,
                "the function has more roots, or more repeated factors, than this crate will report"
            ),
            Self::UnsupportedExpression => write!(
                formatter,
                "the expression cannot be compiled for numerical evaluation"
            ),
        }
    }
}

impl std::error::Error for RootError {}

/// The residual below which a scanned candidate is accepted as a root.
///
/// It is relative to the function's sampled scale. A true root normally lands
/// far below it; a local minimum that merely comes close can land below it too,
/// which is why the residual is reported rather than treated as proof. It is
/// deliberately not `1e-12`: the test promise is about the residual of ordinary
/// equations, and the acceptance threshold has to leave room for the
/// measurement's own rounding.
pub(crate) const ACCEPTANCE: f64 = 1e-10;

/// Two roots closer than this (relative) are the same root.
///
/// Brent can name the same repeated root from two neighbouring cells, and the
/// deflation rounds can name it again with a slightly different last bit. The
/// tolerance is far above that floating-point difference and far below the
/// separation of the roots this crate is meant to list; roots closer than this
/// are reported as one, which is a stated limitation rather than an accident.
pub(crate) fn same_root(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

/// `|f|` relative to the sampled scale, with the absolute reading used when the
/// scale is small. See the crate documentation for why.
pub(crate) fn relative_residual(value: f64, scale: f64) -> f64 {
    value.abs() / scale.max(1.0)
}

pub(crate) fn ordered_interval(interval: (f64, f64)) -> Result<(f64, f64), RootError> {
    let (left, right) = interval;
    if !left.is_finite() || !right.is_finite() || left == right || !(right - left).is_finite() {
        return Err(RootError::InvalidInterval { left, right });
    }
    Ok(if left < right {
        (left, right)
    } else {
        (right, left)
    })
}

pub(crate) fn validate_tolerance(tolerance: f64) -> Result<(), RootError> {
    if tolerance.is_finite() && tolerance > 0.0 {
        Ok(())
    } else {
        Err(RootError::InvalidTolerance { tolerance })
    }
}

pub(crate) fn check_finite(at: f64, value: f64) -> Result<f64, RootError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(RootError::NonFinite { at, value })
    }
}
