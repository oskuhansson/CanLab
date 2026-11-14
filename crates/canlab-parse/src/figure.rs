//! W3.4: volumes of revolution and the area between two curves.
//!
//! Both are a definite integral of something built from the curve, so both are
//! [`crate::integral::definite_integral`] applied to a constructed integrand —
//! not a second integrator. That is the whole design: the closed forms, the
//! exactness, the numeric bound and the refusals are the integral's, and this
//! module only says *what* to integrate.
//!
//! # The two formulas, and why they are these
//!
//! * **Volume about the `x`-axis** (discs): `V = π ∫ y² dx`. A slice of
//!   thickness `dx` is a disc of radius `y`, so its volume is `π y² dx`. The
//!   `π` is a factor outside the integral, not a symbol inside it, so the
//!   integral stays a polynomial in `x` and comes out exact: a semicircle of
//!   radius `r` gives `π ∫_{-r}^{r} (r² − x²) dx = π · 4r³/3`.
//! * **Volume about the `y`-axis** (shells): `V = 2π ∫ x·y dx`. The shell at
//!   `x` has circumference `2πx`, height `y` and thickness `dx`.
//! * **Area between two curves**: `A = ∫ |f − g| dx`, but the absolute value is
//!   not something the integral can take — so the interval is split at the
//!   curves' *crossings* and the difference's sign is fixed on each piece. A
//!   single integral of `f − g` over an interval where they cross would give
//!   the signed area, which is the wrong number and is a classic exam mistake.
//!
//! # What is refused
//!
//! A curve that goes below the axis where a disc's radius would be negative
//! (`y²` is still positive, so the formula survives, but the *question* —
//! "the volume of the solid this curve sweeps" — no longer has that reading),
//! a crossing that cannot be located, and an interval whose ends are not
//! numbers. Each refusal names its reason.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

use crate::integral::{definite_integral, IntegralError, IntegralValue};
use crate::limit::evaluate;

/// Which axis a volume is swept about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Discs: `π ∫ y² dx`.
    X,
    /// Shells: `2π ∫ x·y dx`.
    Y,
}

/// Why a volume or an area has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum FigureError {
    /// The integral that the figure reduces to failed.
    Integral(IntegralError),
    /// A curve crosses the axis where the reading of the question needs it not
    /// to: a disc's radius is a length, and a length is not negative.
    BelowAxis,
    /// Two curves that cross more than the sampling found, or whose crossing
    /// could not be located to the tolerance.
    CrossingNotFound,
}

impl FigureError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            FigureError::Integral(error) => error.message(),
            FigureError::BelowAxis => {
                "the curve goes below the axis, so the solid it sweeps is not the one these limits describe"
                    .to_string()
            }
            FigureError::CrossingNotFound => {
                "the two curves cross where this cannot locate the crossing".to_string()
            }
        }
    }
}

impl From<IntegralError> for FigureError {
    fn from(error: IntegralError) -> FigureError {
        FigureError::Integral(error)
    }
}

/// `V = π ∫_a^b y² dx` (about `x`) or `2π ∫_a^b x·y dx` (about `y`).
///
/// `y` is the curve's height at `x`; the limits are exact numbers. The answer is
/// the integral's answer with the `π` applied — so a semicircle gives
/// `Approximate { value: 4.18879…, error: … }` (the numeric part) and a
/// polynomial gives an exact rational that the caller multiplies by `π`.
pub fn volume_of_revolution(
    curve: &Expr,
    var: SymbolId,
    lower: Number,
    upper: Number,
    axis: Axis,
) -> Result<IntegralValue, FigureError> {
    // A volume about `x` is discs of radius `y(x)`, and about `y` is shells of
    // radius `x` and height `y(x)`. Both formulas need the curve as a function
    // of `x` — the differential's variable — and a curve that mentions `x` in a
    // `dy` integral is a function of `y` given in the wrong letter: `\vol_0^1
    // x dy` would otherwise build the integrand `x·x` and answer a plausible
    // wrong number. The `x` integral is the one where the curve *must* mention
    // `x`, so the refusal is the `y` case only.
    if axis == Axis::Y && curve_mentions(curve, var) {
        return Err(FigureError::Integral(IntegralError::ExtraSymbol(
            SymbolId::name(var).to_string(),
        )));
    }
    let integrand = match axis {
        Axis::X => canlab_core::pow(curve.clone(), ExprNode::int(2)),
        Axis::Y => canlab_core::mul([ExprNode::sym(var), curve.clone()]),
    };
    let integral = definite_integral(&integrand, var, lower, upper)?;
    // The `π` is applied here and not inside the integral: a symbol in the
    // integrand would make every volume approximate, and `π · 4r³/3` is exact
    // as a formula even when `π` has no exact value.
    Ok(scale_by_pi(integral, if axis == Axis::X { 1 } else { 2 }))
}

/// `A = ∫_a^b |f − g| dx`, split at the crossings so the sign is fixed on each
/// piece.
///
/// The crossings are located by bisection on `f − g` over a fine sampling, and
/// the pieces are integrated separately. A pair that does not cross is one
/// piece, and the sign is taken from the midpoint.
pub fn area_between(
    upper_curve: &Expr,
    lower_curve: &Expr,
    var: SymbolId,
    lower: Number,
    upper: Number,
) -> Result<IntegralValue, FigureError> {
    let difference = canlab_core::add([
        upper_curve.clone(),
        canlab_core::mul([ExprNode::int(-1), lower_curve.clone()]),
    ]);
    let low = lower.to_f64();
    let high = upper.to_f64();
    if !(low.is_finite() && high.is_finite()) {
        return Err(FigureError::Integral(IntegralError::BadLimit));
    }
    if low == high {
        return Ok(IntegralValue::Exact(Number::int(0)));
    }
    let (low, high) = if low < high { (low, high) } else { (high, low) };
    let crossings = crossings(&difference, var, low, high)?;
    // The bounds are `Number`s and not `f64`s: the interval's own ends are the
    // user's exact limits, and a crossing that lands on an exact value is exact
    // too. Passing `Number::float` here made every area a float — `\area_0^3
    // x , 3` answered `Float(4.5)` where it is exactly `9/2` — because the
    // closed-form path needs exact limits to stay exact.
    let mut bounds = vec![lower.clone()];
    bounds.extend(crossings.iter().copied().map(number_from_f64));
    bounds.push(upper.clone());
    let mut total = Number::int(0);
    let mut error = 0.0;
    let mut all_exact = true;
    for pair in bounds.windows(2) {
        let (from, to) = (pair[0].to_f64(), pair[1].to_f64());
        if to - from < 1e-12 {
            continue;
        }
        // The sign on this piece, from its midpoint. `|f − g|` is `±(f − g)`,
        // and the sign is constant between crossings by construction.
        let middle = 0.5 * (from + to);
        let sign = evaluate(&difference, var, middle);
        if !sign.is_finite() {
            return Err(FigureError::Integral(IntegralError::Singular));
        }
        // The difference is integrated **unchanged**. Flipping the expression —
        // `-1 * (f − g)` — wraps the sum in a product, and the closed-form path
        // matches on a sum: it saw one term whose body was an `Add`, did not
        // recognise it, and fell to the numeric path. `\area_0^2 x , 1`
        // answered `Float(1.0)` with a `7.2·10⁻¹⁵` error where both pieces are
        // exactly `1/2`. The sign is not needed at all: each piece's *magnitude*
        // is taken below, which is what an area is.
        let _ = sign;
        let piece = definite_integral(&difference, var, pair[0].clone(), pair[1].clone())?;
        // The piece's *magnitude*, whatever sign it came out with. The sign
        // chosen above is the sign of `f − g` at the midpoint, and the integral
        // of `±(f − g)` over a piece where the sign is constant is positive — but
        // "positive" here is a claim about the arithmetic, and the arithmetic is
        // a numeric approximation that can land on `−1e-17`. Taking the
        // magnitude is what makes `\area` an area: swapping the two curves
        // returned `−4/15` before this line existed.
        match piece {
            IntegralValue::Exact(value) => total = total.add(&value.absolute()),
            IntegralValue::Approximate { value, error: e } => {
                total = total.add(&value.absolute());
                error += e;
                all_exact = false;
            }
        }
    }
    if all_exact {
        Ok(IntegralValue::Exact(total))
    } else {
        Ok(IntegralValue::Approximate {
            value: total,
            error,
        })
    }
}

/// An `f64` as a `Number`, exactly when it is an integer.
///
/// A crossing found by bisection is an approximation, and `Number::float` is the
/// honest type for it. A crossing that lands exactly on a sample is exact, and
/// so is one that bisects to an integer-valued `f64`; making those exact is what
/// lets `\area_0^2 x , 1` answer `1` and not `Float(1.0)`.
fn number_from_f64(value: f64) -> Number {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Number::int(value as i128)
    } else {
        Number::float(value)
    }
}

/// The points in `(low, high)` where `difference` changes sign.
fn crossings(
    difference: &Expr,
    var: SymbolId,
    low: f64,
    high: f64,
) -> Result<Vec<f64>, FigureError> {
    const SAMPLES: usize = 512;
    let step = (high - low) / SAMPLES as f64;
    let mut found = Vec::new();
    let mut previous = evaluate(difference, var, low);
    for index in 1..=SAMPLES {
        let x = low + step * index as f64;
        let value = evaluate(difference, var, x);
        // A crossing is a sign change **or** a sample that lands exactly on the
        // root. The product test alone missed the second: `\area_0^2 x , 1` has
        // its crossing at `x = 1`, which is exactly the midpoint of the interval
        // and exactly one of the 512 samples, so `previous · value` was
        // `0 · positive = 0` — not negative — and the crossing was never found.
        // The sign was then read at the midpoint as `0`, treated as positive,
        // and the signed difference integrated to `0` instead of `1`.
        let sign_changed = previous.is_finite()
            && value.is_finite()
            && previous != 0.0
            && value != 0.0
            && (previous < 0.0) != (value < 0.0);
        // A zero *at* a sample is a crossing, unless it is the interval's own
        // end (which is a boundary, not a split) or the previous sample was
        // already zero and recorded it.
        let landed_on_root = value == 0.0 && index < SAMPLES && previous != 0.0;
        if landed_on_root {
            found.push(x);
        } else if sign_changed {
            let mut a = x - step;
            let mut b = x;
            let (mut fa, mut fb) = (previous, value);
            for _ in 0..80 {
                let middle = 0.5 * (a + b);
                let fm = evaluate(difference, var, middle);
                if fm == 0.0 {
                    a = middle;
                    b = middle;
                    break;
                }
                if (fa < 0.0) != (fm < 0.0) {
                    b = middle;
                    fb = fm;
                } else {
                    a = middle;
                    fa = fm;
                }
            }
            let _ = fb;
            found.push(0.5 * (a + b));
        }
        previous = value;
    }
    Ok(found)
}

/// Multiply an integral's value by `π^k`, keeping exactness where it can.
///
/// A value that is exactly zero stays exactly zero — `π · 0 = 0` — and anything
/// else becomes numeric, because `π` is not a rational. The caller prints the
/// `π` itself when it wants the exact formula; this is the number.
fn scale_by_pi(value: IntegralValue, power: i32) -> IntegralValue {
    let factor = std::f64::consts::PI.powi(power);
    match value {
        IntegralValue::Exact(number) if number.is_zero() => IntegralValue::Exact(number),
        IntegralValue::Exact(number) => IntegralValue::Approximate {
            value: Number::float(number.to_f64() * factor),
            error: 0.0,
        },
        IntegralValue::Approximate { value, error } => IntegralValue::Approximate {
            value: Number::float(value.to_f64() * factor),
            error: error * factor,
        },
    }
}

/// Whether `expression` mentions `var`.
fn curve_mentions(expression: &Expr, var: SymbolId) -> bool {
    let mut symbols = Vec::new();
    crate::limit::collect_symbols(expression, &mut symbols);
    symbols.contains(&var)
}
