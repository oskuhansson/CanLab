//! Brent's method for one bracket.
//!
//! Brent combines bisection with secant and inverse-quadratic interpolation and
//! falls back to bisection whenever an interpolated step is outside the bracket
//! or too large. That combination is what makes it safe to use on a document's
//! function: the bracket always shrinks by at least the bisection rate, while a
//! smooth function converges superlinearly in practice.
//!
//! The implementation follows the standard form of the algorithm: `a` and `b`
//! are the bracket, `b` is the better guess (`|f(b)| ≤ |f(a)|`), `c` is the
//! previous `b`, and `d` is the previous `c`. `mflag` records whether the last
//! accepted step was a bisection, because Brent's interpolation test uses the
//! step before it.

use crate::{
    Bracket, Root, RootError, check_finite, ordered_interval, relative_residual, validate_tolerance,
};

/// Enough for a bracket at the widest range a document can produce. Each
/// accepted interpolation step has to halve the previous step at least every
/// other iteration, so this is far beyond what convergence needs; a finite
/// function of one variable reaches the tolerance long before it.
const MAX_ITERATIONS: usize = 200;

/// Find a root inside `interval`, which must have opposite signs at its ends
/// (an end that evaluates to zero is a root directly).
///
/// The interval may be given in either order. `tolerance` is an absolute
/// tolerance on `x`: the returned bracket is narrower than it, unless the
/// floating-point representation of the point does not allow that.
///
/// A non-finite value at an end or at any point Brent evaluates is refused with
/// [`RootError::NonFinite`]; `NaN` is never returned as a root. A jump inside a
/// bracket cannot be distinguished from a root by evaluation alone, so this
/// function does not try: it returns the point the bracket closed on and its
/// residual. [`crate::scan`] performs the residual check and refuses a jump.
pub fn brent<F>(f: F, interval: (f64, f64), tolerance: f64) -> Result<Root, RootError>
where
    F: Fn(f64) -> f64,
{
    let (left, right) = ordered_interval(interval)?;
    validate_tolerance(tolerance)?;
    let fa = check_finite(left, f(left))?;
    let fb = check_finite(right, f(right))?;
    // The scale is fixed before iterating: the bracket shrinks to the root, and
    // a scale read from the last, tiny bracket would make every residual look
    // enormous.
    let scale = fa.abs().max(fb.abs()).max(1.0);
    brent_core(
        &f,
        Initial {
            left,
            right,
            left_value: fa,
            right_value: fb,
        },
        tolerance,
        scale,
        MAX_ITERATIONS,
    )
}

/// The same solver, with the residual scale supplied by the caller.
///
/// The scan uses one scale for the whole interval, so a root in a cell whose
/// function values happen to be small is not given a different meaning from a
/// root in a high cell.
pub(crate) fn brent_with_scale<F>(
    f: &F,
    interval: (f64, f64),
    tolerance: f64,
    scale: f64,
) -> Result<Root, RootError>
where
    F: Fn(f64) -> f64,
{
    let (left, right) = ordered_interval(interval)?;
    validate_tolerance(tolerance)?;
    let fa = check_finite(left, f(left))?;
    let fb = check_finite(right, f(right))?;
    brent_core(
        f,
        Initial {
            left,
            right,
            left_value: fa,
            right_value: fb,
        },
        tolerance,
        scale,
        MAX_ITERATIONS,
    )
}

/// What a bracket starts with: its ends and the values already evaluated there.
struct Initial {
    left: f64,
    right: f64,
    left_value: f64,
    right_value: f64,
}

fn brent_core<F>(
    f: &F,
    initial: Initial,
    tolerance: f64,
    scale: f64,
    max_iterations: usize,
) -> Result<Root, RootError>
where
    F: Fn(f64) -> f64,
{
    let Initial {
        left,
        right,
        left_value: fa,
        right_value: fb,
    } = initial;
    let root_at = |value: f64, value_f: f64, bracket: Bracket| Root {
        value,
        residual: relative_residual(value_f, scale),
        bracket,
    };

    if fa == 0.0 {
        return Ok(root_at(left, fa, Bracket::new(left, left)));
    }
    if fb == 0.0 {
        return Ok(root_at(right, fb, Bracket::new(right, right)));
    }
    if (fa > 0.0) == (fb > 0.0) {
        return Err(RootError::NotBracketed { left, right });
    }

    let mut a = left;
    let mut b = right;
    let mut fa = fa;
    let mut fb = fb;
    // Put the better guess in `b`; the bracket is still [a, b] as a set.
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a;
    let mut fc = fa;
    let mut d = 0.0;
    let mut mflag = true;

    for _ in 0..max_iterations {
        if fb == 0.0 {
            return Ok(root_at(b, fb, Bracket::new(b, b)));
        }

        // `2ε|b|` is the narrowest bracket that is representable near `b`; with
        // a tolerance below it the loop would compare distances that rounding
        // has already made equal.
        let stop = tolerance.max(2.0 * f64::EPSILON * b.abs());
        if (b - a).abs() <= stop {
            return Ok(root_at(b, fb, Bracket::new(a, b)));
        }

        let mut s = if fa != fb && fb != fc && fa != fc {
            // Inverse quadratic interpolation through (a, fa), (b, fb), (c, fc).
            a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb))
        } else {
            // Secant through (a, fa) and (b, fb).
            b - fb * (b - a) / (fb - fa)
        };

        let quarter = (3.0 * a + b) / 4.0;
        let reject = !s.is_finite()
            || !strictly_between(s, quarter, b)
            || (mflag && (s - b).abs() >= (b - c).abs() / 2.0)
            || (!mflag && (s - b).abs() >= (c - d).abs() / 2.0)
            || (mflag && (b - c).abs() < stop)
            || (!mflag && (c - d).abs() < stop);
        if reject {
            s = (a + b) / 2.0;
            mflag = true;
        } else {
            mflag = false;
        }

        let fs = check_finite(s, f(s))?;
        d = c;
        c = b;
        // `c` now names the previous iterate, so its value is the old `fb`.
        fc = fb;
        if (fa > 0.0) != (fs > 0.0) {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }
        if fb == 0.0 {
            return Ok(root_at(b, fb, Bracket::new(b, b)));
        }
        // Keep `b` as the endpoint with the smaller value, as the interpolation
        // formulas and the stopping rule both assume.
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
            // `c` still names the previous `b`; the next iteration's
            // interpolation test reads it through `fc`, which stays with the
            // point it was evaluated at.
        }
    }

    Err(RootError::NoConvergence {
        iterations: max_iterations,
    })
}

/// Whether `value` is strictly inside the interval between `left` and `right`,
/// in either order.
fn strictly_between(value: f64, left: f64, right: f64) -> bool {
    if left < right {
        left < value && value < right
    } else {
        right < value && value < left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x_squared_minus_two(x: f64) -> f64 {
        x * x - 2.0
    }

    #[test]
    fn finds_the_root_and_keeps_the_bracket_that_proves_it() {
        let root = brent(x_squared_minus_two, (1.0, 2.0), 1e-13).expect("√2 is bracketed");
        assert!(
            (root.value - 2.0_f64.sqrt()).abs() <= 1e-12,
            "value {} is not √2",
            root.value
        );
        assert!(
            root.residual <= 1e-12,
            "residual {} is too large",
            root.residual
        );
        assert!(root.bracket.contains(root.value));
        let left = x_squared_minus_two(root.bracket.left);
        let right = x_squared_minus_two(root.bracket.right);
        assert!(
            (left > 0.0) != (right > 0.0) || left == 0.0 || right == 0.0,
            "the final bracket no longer changes sign: {left} and {right}"
        );
    }

    #[test]
    fn a_reversed_interval_is_the_same_bracket() {
        let root =
            brent(x_squared_minus_two, (2.0, 1.0), 1e-13).expect("√2 is bracketed either way");
        assert!((root.value - 2.0_f64.sqrt()).abs() <= 1e-12);
        assert!(root.bracket.contains(root.value));
    }

    #[test]
    fn a_transcendental_bracket_converges() {
        // ln has a root at 1 and no polynomial shape; the bracket arithmetic is
        // the same, which is the point of Brent refusing to assume a polynomial.
        let root = brent(|x| x.ln(), (0.5, 2.0), 1e-13).expect("ln is bracketed");
        assert!((root.value - 1.0).abs() <= 1e-12, "{root:?}");
        assert!(root.residual <= 1e-12, "{root:?}");
    }

    #[test]
    fn bracket_width_and_contains_describe_the_interval() {
        let bracket = Bracket::new(3.0, -1.0);
        assert_eq!(bracket.left, -1.0);
        assert_eq!(bracket.right, 3.0);
        assert_eq!(bracket.width(), 4.0);
        assert!(bracket.contains(0.0));
        assert!(bracket.contains(-1.0));
        assert!(!bracket.contains(3.5));
    }

    #[test]
    fn an_exact_endpoint_is_a_root_with_no_residual() {
        let root = brent(|x| x - 1.0, (1.0, 2.0), 1e-12).expect("the left end is the root");
        assert_eq!(root.value, 1.0);
        assert_eq!(root.residual, 0.0);
        assert_eq!(root.bracket, Bracket::new(1.0, 1.0));
    }

    #[test]
    fn an_interval_without_a_sign_change_is_refused() {
        let error = brent(|x| x * x + 1.0, (-2.0, 2.0), 1e-12).expect_err("x² + 1 has no root");
        assert!(
            matches!(error, RootError::NotBracketed { .. }),
            "unexpected refusal: {error}"
        );
        assert!(error.to_string().contains("same sign"));
    }

    #[test]
    fn a_non_finite_endpoint_is_refused_not_returned() {
        let error = brent(|x| 1.0 / x, (0.0, 1.0), 1e-12).expect_err("1/0 is not a bracket end");
        assert!(
            matches!(error, RootError::NonFinite { at, .. } if at == 0.0),
            "unexpected refusal: {error}"
        );
        assert!(error.to_string().contains("not finite"));
    }

    #[test]
    fn an_empty_or_non_finite_interval_is_refused() {
        for interval in [(1.0, 1.0), (f64::NAN, 1.0), (0.0, f64::INFINITY)] {
            let error = brent(x_squared_minus_two, interval, 1e-12)
                .expect_err("this interval has no usable bracket");
            assert!(
                matches!(error, RootError::InvalidInterval { .. }),
                "interval {interval:?} gave {error}"
            );
        }
    }

    #[test]
    fn a_tolerance_must_be_finite_and_positive() {
        for tolerance in [0.0, -1e-12, f64::NAN, f64::INFINITY] {
            let error = brent(x_squared_minus_two, (1.0, 2.0), tolerance)
                .expect_err("this tolerance cannot stop an iteration");
            assert!(
                matches!(error, RootError::InvalidTolerance { .. }),
                "tolerance {tolerance} gave {error}"
            );
        }
    }

    #[test]
    fn a_jump_is_returned_with_a_large_residual_rather_than_hidden() {
        // Brent is a bracket solver, not a continuity checker. This test pins
        // the division of labour: the value is finite, and the residual is what
        // says there is no zero here. `scan` is the function that refuses it.
        let step = |x: f64| if x < 0.5 { -1.0 } else { 1.0 };
        let root = brent(step, (0.0, 1.0), 1e-12).expect("a sign change exists");
        assert!(root.value.is_finite());
        assert!(
            root.residual > 0.5,
            "the jump was presented with residual {}",
            root.residual
        );
    }

    #[test]
    fn the_iteration_budget_has_a_refusal() {
        // The public entry point cannot exhaust 200 iterations on a finite
        // bracket, so the guard is exercised with a budget of zero. This is the
        // only way to reach it without relying on a pathological function that
        // no document would contain.
        let error = brent_core(
            &x_squared_minus_two,
            Initial {
                left: 1.0,
                right: 2.0,
                left_value: x_squared_minus_two(1.0),
                right_value: x_squared_minus_two(2.0),
            },
            1e-12,
            1.0,
            0,
        )
        .expect_err("zero iterations cannot converge");
        assert_eq!(error, RootError::NoConvergence { iterations: 0 });
    }
}
