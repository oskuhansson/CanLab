//! Definite integrals: an integrand, two limits, and a number with a bound.
//!
//! `\int_a^b f\,dx` describes a number, and `Expr` has no node that holds the
//! limits. So the parser evaluates it while it reads it, and this module is the
//! evaluation. It is here rather than in a solver crate because `canlab-parse`
//! owns the expression tree, the exact numbers, and the `f64` conversion — and
//! because the alternative, a trait method that only a backend with an
//! integrator can answer, makes `canlab parse '\int_0^1 x dx'` fail on a machine
//! with no SymPy for a reason that has nothing to do with SymPy.
//!
//! # Exact first, a bound second, a refusal third
//!
//! The rule the whole CAS is built on (`docs/roadmap-v2-plan.md`, "Exact or
//! bounded, never approximate-and-unmarked") applies per term:
//!
//! 1. **Exact.** A term that is a power of the variable with an integer
//!    exponent and a rational coefficient integrates in closed form. The
//!    integral of a sum is the sum of the integrals, so a polynomial is always
//!    exact — `3x²` on `[0, 2]` is `8`, not `8.000000000000002`.
//! 2. **Closed form.** Several elementary terms have one that is worth writing
//!    down: `∫sin = −cos`, `∫1/x = ln|x|`, `∫eˣ = eˣ`. They are exact as
//!    *symbols* and are evaluated at the limits numerically, because `e^π` has
//!    no `Number` holding it.
//! 3. **Numeric with a bound.** Everything else is integrated by adaptive
//!    Simpson to a stated tolerance, and the answer carries the estimate of its
//!    own error. A bound that does not bracket the true value is the one failure
//!    mode this module is written against, so the bound is computed by
//!    comparison — the difference between two subdivisions — and then widened.
//! 4. **Refusal.** An integrand with a symbol in it other than the variable of
//!    integration, a pole inside the interval, or a limit that is not a number is
//!    refused with a message that says which. An integral this module can only
//!    answer *approximately* is never presented as exact.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

/// Why a definite integral has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegralError {
    /// A symbol in the integrand that is not the variable of integration.
    ExtraSymbol(String),
    /// The integrand is not defined somewhere inside the limits — a pole, a
    /// `NaN`, an infinity — so the integral is improper and this module does
    /// not claim it.
    Singular,
    /// The numeric method did not converge to the requested tolerance.
    NotConverged,
    /// A limit that cannot be represented.
    BadLimit,
}

impl IntegralError {
    /// The message the parser attaches to a position in the input.
    pub fn message(&self) -> String {
        match self {
            IntegralError::ExtraSymbol(name) => format!(
                "`{name}` is a parameter here; a definite integral needs a numeric answer, and this one has none until `{name}` is chosen"
            ),
            IntegralError::Singular => {
                "the integrand is not defined everywhere inside these limits".to_string()
            }
            IntegralError::NotConverged => {
                "the numeric integral did not converge to the stated tolerance".to_string()
            }
            IntegralError::BadLimit => "a limit is not a representable number".to_string(),
        }
    }
}

/// A definite integral's value, and how sure the parser is of it.
///
/// Both variants carry a `Number` so that arithmetic and printing stay exact
/// wherever they can: an `Approximate` whose value is a rational is a rational
/// with an error bar, not a float that forgot it had one.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegralValue {
    /// Closed form, no error.
    Exact(Number),
    /// Numeric, with a one-sided error bound in the same units.
    Approximate {
        /// The estimate.
        value: Number,
        /// The bound: the true value is within this much of `value`.
        error: f64,
    },
}

impl IntegralValue {
    /// The value as a number, whether or not it is exact.
    pub fn number(&self) -> Number {
        match self {
            IntegralValue::Exact(value) => value.clone(),
            IntegralValue::Approximate { value, .. } => value.clone(),
        }
    }

    /// `true` when the value is exact.
    pub fn is_exact(&self) -> bool {
        matches!(self, IntegralValue::Exact(_))
    }
}

/// Evaluate `∫_lower^upper integrand d(var)`.
///
/// `lower` and `upper` are exact numbers; an inverted interval is allowed and
/// gives the negative of the integral the other way round, which is what the
/// closed form does without being asked.
pub fn definite_integral(
    integrand: &Expr,
    var: SymbolId,
    lower: Number,
    upper: Number,
) -> Result<IntegralValue, IntegralError> {
    let low = as_f64(&lower).ok_or(IntegralError::BadLimit)?;
    let high = as_f64(&upper).ok_or(IntegralError::BadLimit)?;
    // Every symbol in the integrand must be the variable. A parameter is not an
    // error in the mathematics, but it is an error in an answer that has to be
    // a number, and saying so is better than integrating `a·x` as if `a` were
    // a constant the caller had told us about.
    let mut symbols = Vec::new();
    collect_symbols(integrand, &mut symbols);
    for symbol in symbols {
        // The constant symbols (`pi`, `e`) are numbers in disguise and are
        // evaluated by the numeric path; every other symbol is a parameter.
        let name = SymbolId::name(symbol);
        if symbol != var && &*name != "pi" && &*name != "e" {
            return Err(IntegralError::ExtraSymbol(name.to_string()));
        }
    }

    // 1. A sum of terms, each of which may have its own closed form. The
    //    polynomial case is exact here, term by term, in exact arithmetic.
    let mut terms = Vec::new();
    flatten_sum(integrand, &mut terms);
    let mut exact_total = Number::int(0);
    let mut numeric_total = 0.0;
    let mut numeric_error = 0.0;
    let mut numeric_terms = 0usize;
    let mut singular = false;
    for term in &terms {
        match closed_form_term(term, var, &lower, &upper) {
            TermValue::Exact(value) => {
                exact_total = exact_total.add(&value);
            }
            TermValue::Rounded(value) => {
                numeric_total += value;
                numeric_terms += 1;
            }
            TermValue::Singular => singular = true,
            TermValue::Integrate => match adaptive_simpson(term, var, low, high)? {
                Simpsons::Value { value, error } => {
                    numeric_total += value;
                    numeric_error += error;
                    numeric_terms += 1;
                }
                Simpsons::Singular => singular = true,
            },
        }
    }
    if singular {
        return Err(IntegralError::Singular);
    }
    if numeric_terms == 0 {
        return Ok(IntegralValue::Exact(exact_total));
    }
    // The exact part is added to the approximation *after* the error is
    // computed, so the error stays the numeric part's and does not pretend to
    // bound an exact term it cannot make inexact.
    let exact_part = exact_total.to_f64();
    let value = numeric_total + exact_part;
    Ok(IntegralValue::Approximate {
        value: Number::float(value),
        error: numeric_error + 32.0 * f64::EPSILON * value.abs().max(1.0),
    })
}

// ---------------------------------------------------------------------------
// Closed forms
// ---------------------------------------------------------------------------

/// What a single term's contribution is.
enum TermValue {
    /// An exact number.
    Exact(Number),
    /// A closed form evaluated at the limits: correct as a formula, a float as
    /// a value, with no error bar — which is why it counts as numeric.
    Rounded(f64),
    /// No closed form; integrate numerically.
    Integrate,
    /// The term is not defined somewhere inside the interval, and the *numeric*
    /// path cannot be trusted to notice: `(x^{-2})^{-1}` evaluates to `0` at
    /// zero (the inner power is infinite and the outer one inverts it), so a
    /// sampler sees a perfectly finite function and integrates across a point
    /// where the integrand does not exist. A term that knows it has a pole says
    /// so here instead of handing the sampler a lie.
    Singular,
}

/// The contribution of one term, if it has a closed form.
///
/// The rule is deliberately narrow: what it does not recognise goes to the
/// numeric path, which is honest about its own error. What it does recognise is
/// exact or a formula, and a formula evaluated at two limits is as good as the
/// arithmetic that evaluated it.
fn closed_form_term(term: &Expr, var: SymbolId, lower: &Number, upper: &Number) -> TermValue {
    let (coefficient, body) = split_coefficient(term);
    let coefficient_f64 = coefficient.as_ref().and_then(as_f64);
    let (low, high) = match (as_f64(lower), as_f64(upper)) {
        (Some(low), Some(high)) => (low, high),
        _ => return TermValue::Integrate,
    };
    match &*body {
        // `c·x^n` for an integer `n ≥ 0`: `c·(b^{n+1} − a^{n+1})/(n+1)`, exact
        // whenever the coefficient and the limits are exact.
        ExprNode::Pow(base, exponent) => {
            // `\frac{1}{x^2}` parses to `(x^2)^{-1}`, and the canonicaliser
            // deliberately does *not* flatten that to `x^{-2}`: `(x²)^{-1/2}`
            // is `1/|x|`, not `1/x`, and the flattening is only valid for a
            // negative integer exponent on an interval that excludes zero. That
            // is exactly the case here — the interval is checked below — so the
            // flattening happens here and not in the canonical form.
            if let (ExprNode::Pow(inner_base, inner_exp), ExprNode::Num(outer)) =
                (&**base, &**exponent)
                && let Some(inner) = inner_exp.as_number()
                && let Some(inner_power) = as_integer(inner)
                && let Some(outer_power) = as_integer(outer)
                && is_symbol(inner_base, var)
            {
                let combined = inner_power.checked_mul(outer_power);
                if let Some(power) = combined {
                    // `(x^a)^b = x^{ab}` only where `x^a` is defined. When `a`
                    // is negative the inner power has a pole at zero, and
                    // flattening to a *positive* `ab` removes it: the numeric
                    // path then evaluates `(x^{-2})^{-1}` at zero as
                    // `1/inf = 0`, sees a finite function, and integrates `x²`
                    // across a point where the integrand does not exist. That
                    // answered `1/3` for `\int_0^1 (x^{-2})^{-1} dx`.
                    if inner_power < 0 && power > 0 && low <= 0.0 && high >= 0.0 {
                        return TermValue::Singular;
                    }
                    return closed_form_negative_power(
                        power,
                        coefficient_f64,
                        low,
                        high,
                        coefficient.as_ref(),
                        Some(lower),
                        Some(upper),
                    );
                }
            }
            if !is_symbol(base, var) {
                return TermValue::Integrate;
            }
            let Some(exponent) = exponent.as_number() else {
                return TermValue::Integrate;
            };
            let Some(power) = as_integer(exponent) else {
                return TermValue::Integrate;
            };
            // `n = −1` is `ln|x|`, handled below; every other negative power is
            // a pole at zero, and the numeric path says so if it is inside the
            // interval.
            if power < 0 {
                return closed_form_negative_power(
                    power,
                    coefficient_f64,
                    low,
                    high,
                    coefficient.as_ref(),
                    Some(lower),
                    Some(upper),
                );
            }
            let raised = power + 1;
            let Some(rational) = &coefficient else {
                let f = coefficient_f64.unwrap_or(1.0);
                return TermValue::Rounded(f * (high.powi(power as i32 + 1) - low.powi(power as i32 + 1)) / raised as f64);
            };
            // Exact: `(c/(n+1))·(b^{n+1} − a^{n+1})`.
            let denominator = Number::int(raised);
            let scale = rational.div(&denominator);
            let Some(span) = exact_power_span(lower, upper, raised) else {
                return TermValue::Integrate;
            };
            TermValue::Exact(scale.mul(&span))
        }
        // `c·x`, the power `1`.
        ExprNode::Sym(symbol) if *symbol == var => {
            let span = exact_power_span(lower, upper, 2);
            match (coefficient, span) {
                (Some(coefficient), Some(span)) => {
                    TermValue::Exact(coefficient.div(&Number::int(2)).mul(&span))
                }
                _ => TermValue::Integrate,
            }
        }
        // `c`, a constant: `c·(b − a)`.
        ExprNode::Num(_) => {
            let Some(coefficient) = coefficient else {
                return TermValue::Integrate;
            };
            TermValue::Exact(coefficient.mul(&upper.sub(lower)))
        }
        // `c·sin(x)`: `−c·(cos b − cos a)`.
        ExprNode::Fn(name, args) if is_function(*name, "sin") && single_var_arg(args, var) => {
            let c = coefficient_f64.unwrap_or(1.0);
            TermValue::Rounded(-c * (high.cos() - low.cos()))
        }
        ExprNode::Fn(name, args) if is_function(*name, "cos") && single_var_arg(args, var) => {
            let c = coefficient_f64.unwrap_or(1.0);
            TermValue::Rounded(c * (high.sin() - low.sin()))
        }
        ExprNode::Fn(name, args) if is_function(*name, "exp") && single_var_arg(args, var) => {
            let c = coefficient_f64.unwrap_or(1.0);
            TermValue::Rounded(c * (high.exp() - low.exp()))
        }
        _ => TermValue::Integrate,
    }
}

/// `c·x^n` for `n < 0`.
fn closed_form_negative_power(
    power: i64,
    coefficient: Option<f64>,
    low: f64,
    high: f64,
    rational: Option<&Number>,
    lower: Option<&Number>,
    upper: Option<&Number>,
) -> TermValue {
    if power == -1 {
        // `ln|b| − ln|a|` diverges if the interval crosses zero; the check is
        // the interval's sign, and a zero end is a pole at a limit.
        if low <= 0.0 && high >= 0.0 {
            return TermValue::Integrate;
        }
        let c = coefficient.unwrap_or(1.0);
        return TermValue::Rounded(c * (high.abs().ln() - low.abs().ln()));
    }
    // `x^{−n}` on an interval that contains zero has a pole; on one that does
    // not, the closed form is `x^{−n+1}/(−n+1)`.
    if low <= 0.0 && high >= 0.0 {
        return TermValue::Integrate;
    }
    let raised = power + 1;
    if raised == 0 {
        return TermValue::Integrate;
    }
    // Exact when the coefficient and the limits are exact: `\int_1^2 x^{-2} dx`
    // is `1/2`, and returning `Float(0.5)` with a `6.75·10^-13` bound was the
    // bug that made an exact answer look approximate. The `Rounded` path stays
    // for a coefficient or a limit that is not exact.
    // `exact_power_span` takes the **antiderivative's** exponent, so the call
    // passes `raised = power + 1` and the span is `b^{n+1} − a^{n+1}`; the
    // scale above divides by the same `raised`. Passing the integrand's power
    // would be the off-by-one the function's own doc records.
    if let (Some(coefficient), Some(lower), Some(upper)) = (rational, lower, upper)
        && let Some(span) = exact_power_span(lower, upper, raised)
    {
        let scale = coefficient.div(&Number::int(raised));
        return TermValue::Exact(scale.mul(&span));
    }
    let c = coefficient.unwrap_or(1.0);
    let span = high.powi(raised as i32) - low.powi(raised as i32);
    TermValue::Rounded(c * span / raised as f64)
}

/// `b^k − a^k` in exact arithmetic, or `None` if a value cannot hold it.
///
/// `k` is the **antiderivative's** exponent, which is the integrand's plus one —
/// the callers pass `n + 1`, not `n`. For a negative `k` the span is a rational
/// whenever the limits are: `∫_1^2 x^{-2} dx` has `k = −1`, and
/// `2^{-1} − 1^{-1} = −1/2`, which the caller divides by `k = −1` to get `+1/2`.
///
/// Two bugs lived here. The first version refused `power <= 0`, so every
/// reciprocal went to the numeric path and an exact `1/2` came back as
/// `Float(0.5)` with a bound. The second computed `b^n − a^n` and then *also*
/// divided by a denominator, applying the `+1` twice. Both were caught by
/// `∫_1^2 x^{-2} dx`, which must be exactly `1/2`, and by `∫_{-1}^{1} x dx`,
/// which must be exactly `0`.
fn exact_power_span(lower: &Number, upper: &Number, k: i64) -> Option<Number> {
    if k == 0 {
        return None;
    }
    let magnitude = k.unsigned_abs();
    if magnitude > 64 {
        // A power this large is a number the exact type cannot hold; the numeric
        // path is honest about its own error.
        return None;
    }
    // `b^m` and `a^m` with `m = |k|`, then inverted for a negative `k`. The
    // first version computed `b^m` and `a^m` and never inverted, so `x^{-2}`
    // on `[1, 2]` gave `2² − 1² = 3` where the span is `1/4 − 1 = −3/4`.
    let mut upper_power = Number::int(1);
    let mut lower_power = Number::int(1);
    for _ in 0..magnitude {
        upper_power = upper_power.mul(upper);
        lower_power = lower_power.mul(lower);
    }
    if k < 0 {
        if upper_power.is_zero() || lower_power.is_zero() {
            return None;
        }
        // `b^{-m} = 1/b^m`.
        let upper_inverse = Number::int(1).div(&upper_power);
        let lower_inverse = Number::int(1).div(&lower_power);
        return Some(upper_inverse.sub(&lower_inverse));
    }
    Some(upper_power.sub(&lower_power))
}

// ---------------------------------------------------------------------------
// Numeric fallback
// ---------------------------------------------------------------------------

/// Adaptive Simpson over `[low, high]` of one term.
enum Simpsons {
    Value { value: f64, error: f64 },
    /// The integrand is not finite somewhere inside the interval.
    Singular,
}

/// How deep the subdivision may go. Twenty halvings resolve a feature a
/// millionth of the interval wide, and an integrand whose behaviour is finer
/// than that is one this module should decline rather than chase.
const MAX_DEPTH: u32 = 20;

fn adaptive_simpson(
    term: &Expr,
    var: SymbolId,
    low: f64,
    high: f64,
) -> Result<Simpsons, IntegralError> {
    if !(low.is_finite() && high.is_finite()) {
        return Err(IntegralError::BadLimit);
    }
    if low == high {
        return Ok(Simpsons::Value { value: 0.0, error: 0.0 });
    }
    let f = |x: f64| evaluate(term, var, x);
    let mut singular = false;
    let tolerance = 1e-12 * (high - low).abs().max(1.0);
    let value = simpson(&f, low, high, tolerance, 0, &mut singular);
    let Some((value, error)) = value else {
        if singular {
            return Ok(Simpsons::Singular);
        }
        return Err(IntegralError::NotConverged);
    };
    if singular {
        return Ok(Simpsons::Singular);
    }
    Ok(Simpsons::Value { value, error })
}

fn simpson(
    // `&dyn Fn` and not `impl Fn`: the recursion passes `&f` down, and a generic
    // parameter would instantiate one more layer of `&&&&…` per depth until the
    // compiler's recursion limit is what stops it.
    f: &dyn Fn(f64) -> f64,
    low: f64,
    high: f64,
    tolerance: f64,
    depth: u32,
    singular: &mut bool,
) -> Option<(f64, f64)> {
    let mid = 0.5 * (low + high);
    let (fa, fb, fm) = (f(low), f(high), f(mid));
    if !(fa.is_finite() && fb.is_finite() && fm.is_finite()) {
        *singular = true;
        return None;
    }
    let whole = (high - low) * (fa + 4.0 * fm + fb) / 6.0;
    let quarter = 0.5 * (low + mid);
    let three_quarter = 0.5 * (mid + high);
    let (fq, ft) = (f(quarter), f(three_quarter));
    if !(fq.is_finite() && ft.is_finite()) {
        *singular = true;
        return None;
    }
    let left = (mid - low) * (fa + 4.0 * fq + fm) / 6.0;
    let right = (high - mid) * (fm + 4.0 * ft + fb) / 6.0;
    let refined = left + right;
    let delta = refined - whole;
    if depth >= MAX_DEPTH {
        // Out of depth and still not converged: the estimate is what it is, and
        // the error is the Richardson term, which does not shrink any further.
        return None;
    }
    if delta.abs() <= 15.0 * tolerance {
        // Richardson extrapolation: the error of the refined estimate is
        // `(refined − whole)/15`, and the return is the extrapolated value with
        // that error, widened by a factor of two because the estimate of a
        // fourth-order method from three points is itself an estimate.
        let value = refined + delta / 15.0;
        return Some((value, (delta / 15.0).abs() * 2.0 + f64::EPSILON * value.abs()));
    }
    let (a, ea) = simpson(f, low, mid, tolerance * 0.5, depth + 1, singular)?;
    let (b, eb) = simpson(f, mid, high, tolerance * 0.5, depth + 1, singular)?;
    Some((a + b, ea + eb))
}

/// Evaluate a term at `x`, in exact arithmetic where it can and `f64` where it
/// cannot.
fn evaluate(expr: &Expr, var: SymbolId, x: f64) -> f64 {
    match &**expr {
        ExprNode::Num(number) => as_f64(number).unwrap_or(f64::NAN),
        ExprNode::Sym(symbol) if *symbol == var => x,
        ExprNode::Sym(symbol) => match &*SymbolId::name(*symbol) {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            _ => f64::NAN,
        },
        ExprNode::Add(members) => members.iter().map(|m| evaluate(m, var, x)).sum(),
        ExprNode::Mul(factors) => factors.iter().map(|m| evaluate(m, var, x)).product(),
        ExprNode::Pow(base, exponent) => evaluate(base, var, x).powf(evaluate(exponent, var, x)),
        ExprNode::Fn(name, args) => {
            let mut values = args.iter().map(|argument| evaluate(argument, var, x));
            let Some(first) = values.next() else {
                return f64::NAN;
            };
            match &*SymbolId::name(*name) {
                "sin" => first.sin(),
                "cos" => first.cos(),
                "tan" => first.tan(),
                "exp" => first.exp(),
                "ln" => first.ln(),
                "sqrt" => first.sqrt(),
                "abs" => first.abs(),
                "arcsin" => first.asin(),
                "arccos" => first.acos(),
                "arctan" => first.atan(),
                "sinh" => first.sinh(),
                "cosh" => first.cosh(),
                "tanh" => first.tanh(),
                _ => f64::NAN,
            }
        }
        _ => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Flatten a sum into its terms; a term that is not a sum is itself.
fn flatten_sum(expr: &Expr, out: &mut Vec<Expr>) {
    match &**expr {
        ExprNode::Add(members) => {
            for member in members {
                flatten_sum(member, out);
            }
        }
        _ => out.push(expr.clone()),
    }
}

/// Split a term into its numeric coefficient and its body.
fn split_coefficient(expr: &Expr) -> (Option<Number>, Expr) {
    match &**expr {
        ExprNode::Mul(factors) => {
            let mut coefficient = Number::int(1);
            let mut rest = Vec::new();
            for factor in factors {
                match factor.as_number() {
                    Some(number) => coefficient = coefficient.mul(number),
                    None => rest.push(factor.clone()),
                }
            }
            if rest.is_empty() {
                (Some(coefficient.clone()), ExprNode::num(coefficient))
            } else if rest.len() == 1 {
                (Some(coefficient), rest.into_iter().next().expect("one"))
            } else {
                (Some(coefficient), canlab_core::normalize::mul(rest))
            }
        }
        other => match other.as_number() {
            Some(number) => (Some(number.clone()), ExprNode::num(number.clone())),
            None => (Some(Number::int(1)), expr.clone()),
        },
    }
}

fn is_symbol(expr: &Expr, var: SymbolId) -> bool {
    matches!(&**expr, ExprNode::Sym(symbol) if *symbol == var)
}

fn is_function(name: SymbolId, expected: &str) -> bool {
    &*SymbolId::name(name) == expected
}

fn single_var_arg(args: &[Expr], var: SymbolId) -> bool {
    args.len() == 1 && is_symbol(&args[0], var)
}

fn collect_symbols(expr: &Expr, out: &mut Vec<SymbolId>) {
    match &**expr {
        ExprNode::Sym(symbol) => out.push(*symbol),
        ExprNode::Add(members) | ExprNode::Mul(members) => {
            for member in members {
                collect_symbols(member, out);
            }
        }
        ExprNode::Pow(base, exponent) => {
            collect_symbols(base, out);
            collect_symbols(exponent, out);
        }
        ExprNode::Fn(_, args) => {
            for argument in args {
                collect_symbols(argument, out);
            }
        }
        _ => {}
    }
}

/// A `Number` as an `i64`, for an exponent.
fn as_integer(number: &Number) -> Option<i64> {
    match number {
        Number::Int(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// A `Number` as an `f64`, or `None` when it does not fit.
fn as_f64(number: &Number) -> Option<f64> {
    let value = number.to_f64();
    value.is_finite().then_some(value)
}
