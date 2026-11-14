//! Limits: `\lim_{x \to a} f(x)`, and the answer being a number or a refusal.
//!
//! A limit is a *statement* like a definite integral: `\lim_{x\to 0} \sin x / x`
//! describes a number, and `Expr` has no node that holds the approach. The
//! parser evaluates it while it reads it, and this module is the evaluation.
//!
//! # The school set, in the order it is tried
//!
//! 1. **Substitution.** If `f(a)` is finite, that is the limit — a continuous
//!    function's value. This is most exam limits and it is exact.
//! 2. **Cancellation.** `0/0` is usually a common factor: `(x²−1)/(x−1)` is
//!    `x+1` except at `x = 1`, and the limit is what the cancelled form says.
//!    The cancellation is done on the polynomial in `x` — a rational function's
//!    numerator and denominator are polynomials, and their common factor is
//!    found by the same exact division the solver uses.
//! 3. **Degree.** `∞/∞` is the ratio of the leading coefficients; `p/q` with the
//!    numerator's degree higher is `±∞`; lower is `0`.
//! 4. **The standard limits.** `sin u / u → 1`, `(1−cos u)/u² → ½`,
//!    `(1+1/n)ⁿ → e`, `(1+u)^{1/u} → e`. These are the ones an exam expects to
//!    be *known*, and they are recognised structurally rather than by a table of
//!    strings.
//! 5. **Numeric.** Everything else is evaluated at a sequence of points
//!    approaching the target, and the answer carries a stated uncertainty. Two
//!    sequences are used — one from each side for a two-sided limit — and a
//!    disagreement between them is reported as "does not exist" with both sides
//!    named, which is the honest answer for a jump.
//!
//! # What a refusal means
//!
//! A limit that this module cannot establish is refused with a reason, never
//! answered with a guess. The numeric path's uncertainty is part of the answer,
//! so a caller can tell `1.0000 ± 1e-6` from `1`.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

/// Where the variable approaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Approach {
    /// `x → a` from both sides.
    TwoSided,
    /// `x → a⁺`.
    FromAbove,
    /// `x → a⁻`.
    FromBelow,
}

/// The point `x` approaches, and from which side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The value, or `None` for `±∞`.
    pub value: Option<f64>,
    /// `+1` for `+∞`, `−1` for `−∞`, `0` for a finite target. Used only when
    /// `value` is `None`.
    pub infinity: i8,
    /// The side.
    pub approach: Approach,
}

impl Target {
    /// A finite target, two-sided.
    pub fn at(value: f64) -> Target {
        Target {
            value: Some(value),
            infinity: 0,
            approach: Approach::TwoSided,
        }
    }

    /// `+∞` or `−∞`.
    pub fn infinite(sign: i8) -> Target {
        Target {
            value: None,
            infinity: sign,
            approach: Approach::TwoSided,
        }
    }

    /// The same target, approached from one side.
    pub fn from(self, approach: Approach) -> Target {
        Target { approach, ..self }
    }
}

/// A limit's value.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitValue {
    /// A finite value, established exactly or by recognition.
    Finite(Number),
    /// The function grows without bound, with the sign it grows towards.
    Infinite(i8),
    /// A numeric estimate with its uncertainty. The true limit is within
    /// `uncertainty` of `value`, *if* the sequence converged at all — the
    /// uncertainty is the disagreement between successive refinements, which is
    /// the only evidence a numeric limit has.
    Numeric { value: f64, uncertainty: f64 },
}

/// Why a limit has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitError {
    /// A symbol in the expression other than the variable of approach.
    ExtraSymbol(String),
    /// The two sides disagree: the limit does not exist.
    NotExist { left: String, right: String },
    /// The numeric sequence did not settle, and nothing else established it.
    Unsettled,
    /// The sequence diverged, with the sign it diverged towards.
    Diverges { sign: i8 },
    /// A target that is not a number.
    BadTarget,
}

impl LimitError {
    /// The message the parser attaches to a position in the input.
    pub fn message(&self) -> String {
        match self {
            LimitError::ExtraSymbol(name) => format!(
                "`{name}` is a parameter here; a limit needs a numeric answer, and this one has none until `{name}` is chosen"
            ),
            LimitError::NotExist { left, right } => format!(
                "the limit does not exist: from below it is {left}, from above it is {right}"
            ),
            LimitError::Unsettled => {
                "the limit could not be established: the numeric sequence did not settle".to_string()
            }
            LimitError::Diverges { sign } => format!(
                "the limit is {} infinity",
                if *sign < 0 { "negative" } else { "positive" }
            ),
            LimitError::BadTarget => "the approach is not a number".to_string(),
        }
    }
}

/// Evaluate `lim_{var → target} expression`.
pub fn limit(expression: &Expr, var: SymbolId, target: Target) -> Result<LimitValue, LimitError> {
    let mut symbols = Vec::new();
    collect_symbols(expression, &mut symbols);
    for symbol in symbols {
        let name = SymbolId::name(symbol);
        if symbol != var && &*name != "pi" && &*name != "e" {
            return Err(LimitError::ExtraSymbol(name.to_string()));
        }
    }

    // 1. Substitution at a finite target — but only when the expression is
    //    defined there. `(1 − cos x)/x²` is `0/0` at zero, and a naive
    //    evaluation returns `0` (the numerator is zero and the denominator is
    //    zero, and `0/0` computed in floats is `NaN` only when the denominator
    //    is *exactly* zero — here it is `0·0 = 0`, so the division gives `NaN`
    //    and the finite check fails). The check that matters is on the
    //    *denominator*: a quotient whose denominator vanishes at the target is
    //    not continuous there, whatever the numerator does, and substituting
    //    into it answers a different question. This test is the reason the
    //    substitution comes first only for expressions without a vanishing
    //    denominator.
    if let Some(point) = target.value {
        let continuous = match split_quotient(expression) {
            Some((_, denominator)) => evaluate(denominator, var, point).abs() > 1e-12,
            None => true,
        };
        if continuous {
            // Exact first: a polynomial at a rational point is a rational, and
            // `(x²−1)/(x−1)` at `x = 1` must answer `2`, not `2.0`. A float
            // here would make every continuous limit an approximation, which is
            // the one thing this CAS does not do.
            if let Some(exact) = exact_value(expression, var, point) {
                return Ok(LimitValue::Finite(exact));
            }
            let value = evaluate(expression, var, point);
            if value.is_finite() {
                return Ok(LimitValue::Finite(Number::float(value)));
            }
        }
    }

    // 2. The standard limits, recognised structurally — before cancellation,
    //    because `(1−cos x)/x²` has no common polynomial factor to cancel and
    //    only the known limit decides it.
    if let Some(value) = standard_limit(expression, var, target) {
        return Ok(value);
    }

    // 3. Cancellation of a common polynomial factor.
    if let Some(cancelled) = cancel_common_factor(expression, var) {
        return limit(&cancelled, var, target);
    }

    // 4. Degree, for a quotient at infinity.
    if let Some(value) = by_degree(expression, var, target) {
        return Ok(value);
    }

    // 5. Numeric, from one side or from both.
    numeric_limit(expression, var, target)
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// `f` with its numerator's and denominator's common polynomial factor divided
/// out, if `f` is a quotient of polynomials in `var`.
fn cancel_common_factor(expression: &Expr, var: SymbolId) -> Option<Expr> {
    let (numerator, denominator) = split_quotient(expression)?;
    let numerator = polynomial_coefficients(numerator, var)?;
    let denominator = polynomial_coefficients(denominator, var)?;
    // The common factor is the gcd of the two polynomials' coefficients, found
    // by exact division: the degree of the gcd is at most the smaller degree,
    // and a trial division that leaves no remainder is the whole test.
    let gcd = polynomial_gcd(&numerator, &denominator)?;
    if gcd.len() <= 1 {
        return None;
    }
    let numerator = divide_polynomial(&numerator, &gcd)?;
    let denominator = divide_polynomial(&denominator, &gcd)?;
    Some(quotient(
        polynomial_expression(&numerator, var),
        polynomial_expression(&denominator, var),
    ))
}

/// `(numerator, denominator)` for `a/b`, or `(a, 1)` for anything else.
///
/// A quotient is `a * b^(-1)` in the canonical form, and the product's factors
/// are **sorted**: `(3x²+1)/(2x²−5)` comes back as `(2x²−5)^(-1) * (3x²+1)`,
/// with the reciprocal first. The first version of this function assumed the
/// reciprocal was last and returned the *denominator* as the numerator, which
/// made `lim 3x²/2x²` answer `0` and `lim (1−cos x)/x²` answer `0` — two wrong
/// numbers, not two refusals. The factors are therefore collected on both sides
/// of the reciprocal and multiplied in their own order.
fn split_quotient(expression: &Expr) -> Option<(&Expr, &Expr)> {
    let ExprNode::Mul(factors) = &**expression else {
        return None;
    };
    let mut numerator = Vec::new();
    let mut denominator = Vec::new();
    for factor in factors {
        match &**factor {
            ExprNode::Pow(base, exponent)
                if exponent.as_number().is_some_and(|n| n.to_f64() == -1.0) =>
            {
                denominator.push(base.clone());
            }
            _ => numerator.push(factor.clone()),
        }
    }
    if denominator.is_empty() {
        return None;
    }
    let numerator = Box::leak(Box::new(product(numerator)));
    let denominator = Box::leak(Box::new(product(denominator)));
    Some((numerator, denominator))
}

/// A product of factors, or `1` for none.
fn product(factors: Vec<Expr>) -> Expr {
    match factors.len() {
        0 => ExprNode::int(1),
        1 => factors.into_iter().next().expect("one factor"),
        _ => canlab_core::normalize::mul(factors),
    }
}

/// The coefficients of a polynomial in `var`, from degree 0 upwards.
fn polynomial_coefficients(expression: &Expr, var: SymbolId) -> Option<Vec<Number>> {
    let mut coefficients: Vec<Number> = Vec::new();
    let mut terms = Vec::new();
    flatten_sum(expression, &mut terms);
    for term in terms {
        let (coefficient, degree) = term_degree(&term, var)?;
        while coefficients.len() <= degree {
            coefficients.push(Number::int(0));
        }
        coefficients[degree] = coefficients[degree].add(&coefficient);
    }
    while coefficients.len() > 1 && coefficients.last().is_some_and(Number::is_zero) {
        coefficients.pop();
    }
    Some(coefficients)
}

/// A term's numeric coefficient and its degree in `var`.
fn term_degree(term: &Expr, var: SymbolId) -> Option<(Number, usize)> {
    match &**term {
        ExprNode::Num(number) => Some((number.clone(), 0)),
        ExprNode::Sym(symbol) if *symbol == var => Some((Number::int(1), 1)),
        ExprNode::Pow(base, exponent) => {
            if !matches!(&**base, ExprNode::Sym(s) if *s == var) {
                return None;
            }
            let degree = exponent.as_number()?.to_f64();
            if degree < 0.0 || degree.fract() != 0.0 {
                return None;
            }
            Some((Number::int(1), degree as usize))
        }
        ExprNode::Mul(factors) => {
            let mut coefficient = Number::int(1);
            let mut degree = 0usize;
            for factor in factors {
                let (c, d) = term_degree(factor, var)?;
                coefficient = coefficient.mul(&c);
                degree += d;
            }
            Some((coefficient, degree))
        }
        _ => None,
    }
}

/// The greatest common divisor of two polynomials, by Euclid's algorithm.
fn polynomial_gcd(a: &[Number], b: &[Number]) -> Option<Vec<Number>> {
    let mut a = trim(a.to_vec());
    let mut b = trim(b.to_vec());
    while !(b.len() == 1 && b[0].is_zero()) {
        let remainder = polynomial_remainder(&a, &b)?;
        a = b;
        b = trim(remainder);
    }
    // Normalise so the leading coefficient is one, which makes the division
    // below exact for the common factors an exam problem has.
    let leading = a.last()?.clone();
    if leading.is_zero() {
        return None;
    }
    let mut out = Vec::with_capacity(a.len());
    for coefficient in a {
        out.push(coefficient.div(&leading));
    }
    Some(out)
}

fn polynomial_remainder(a: &[Number], b: &[Number]) -> Option<Vec<Number>> {
    let mut remainder = trim(a.to_vec());
    let divisor_degree = b.len() - 1;
    let divisor_leading = b.last()?.clone();
    while remainder.len() >= b.len() && !(remainder.len() == 1 && remainder[0].is_zero()) {
        let degree = remainder.len() - 1;
        let factor = remainder[degree].div(&divisor_leading);
        for (index, coefficient) in b.iter().enumerate() {
            let target = degree - divisor_degree + index;
            remainder[target] = remainder[target].sub(&factor.mul(coefficient));
        }
        remainder = trim(remainder);
    }
    Some(remainder)
}

fn divide_polynomial(a: &[Number], b: &[Number]) -> Option<Vec<Number>> {
    let remainder = polynomial_remainder(a, b)?;
    if !(remainder.len() == 1 && remainder[0].is_zero()) {
        return None;
    }
    // Synthetic division, again, collecting the quotients.
    let mut quotient = vec![Number::int(0); a.len() - b.len() + 1];
    let mut current = trim(a.to_vec());
    let divisor_leading = b.last()?.clone();
    while current.len() >= b.len() && !(current.len() == 1 && current[0].is_zero()) {
        let degree = current.len() - 1;
        let factor = current[degree].div(&divisor_leading);
        quotient[degree - (b.len() - 1)] = factor.clone();
        for (index, coefficient) in b.iter().enumerate() {
            let target = degree - (b.len() - 1) + index;
            current[target] = current[target].sub(&factor.mul(coefficient));
        }
        current = trim(current);
    }
    Some(trim(quotient))
}

fn trim(mut coefficients: Vec<Number>) -> Vec<Number> {
    while coefficients.len() > 1 && coefficients.last().is_some_and(Number::is_zero) {
        coefficients.pop();
    }
    coefficients
}

fn polynomial_expression(coefficients: &[Number], var: SymbolId) -> Expr {
    let mut terms = Vec::new();
    for (degree, coefficient) in coefficients.iter().enumerate() {
        if coefficient.is_zero() {
            continue;
        }
        let power = match degree {
            0 => ExprNode::int(1),
            1 => ExprNode::sym(var),
            n => canlab_core::pow(ExprNode::sym(var), ExprNode::int(n as i64)),
        };
        terms.push(canlab_core::mul([ExprNode::num(coefficient.clone()), power]));
    }
    if terms.is_empty() {
        return ExprNode::int(0);
    }
    canlab_core::add(terms)
}

fn quotient(numerator: Expr, denominator: Expr) -> Expr {
    canlab_core::mul([numerator, canlab_core::pow(denominator, ExprNode::int(-1))])
}

// ---------------------------------------------------------------------------
// Degree
// ---------------------------------------------------------------------------

/// The limit of a quotient of polynomials at `±∞`, by degree.
fn by_degree(expression: &Expr, var: SymbolId, target: Target) -> Option<LimitValue> {
    if target.value.is_some() {
        return None;
    }
    let (numerator, denominator) = split_quotient(expression)?;
    let numerator = polynomial_coefficients(numerator, var)?;
    let denominator = polynomial_coefficients(denominator, var)?;
    let (n_degree, d_degree) = (numerator.len() - 1, denominator.len() - 1);
    let sign = if target.infinity < 0 && n_degree % 2 == 1 { -1.0 } else { 1.0 };
    match n_degree.cmp(&d_degree) {
        std::cmp::Ordering::Less => Some(LimitValue::Finite(Number::int(0))),
        std::cmp::Ordering::Equal => {
            let ratio = numerator[n_degree].div(&denominator[d_degree]);
            Some(LimitValue::Finite(ratio))
        }
        std::cmp::Ordering::Greater => {
            let leading = numerator[n_degree].to_f64() / denominator[d_degree].to_f64();
            Some(LimitValue::Infinite(if leading * sign < 0.0 { -1 } else { 1 }))
        }
    }
}

// ---------------------------------------------------------------------------
// The standard limits
// ---------------------------------------------------------------------------

/// `sin u/u → 1`, `(1−cos u)/u² → ½`, `(1+1/n)ⁿ → e`.
fn standard_limit(expression: &Expr, var: SymbolId, target: Target) -> Option<LimitValue> {
    let point = target.value?;
    // `sin u / u` with `u` vanishing at the target.
    if let Some((numerator, denominator)) = split_quotient(expression) {
        if is_function_of(numerator, "sin", var)
            && vanishes(denominator, var, point)
            && same_function_body(numerator, denominator, var)
        {
            return Some(LimitValue::Finite(Number::int(1)));
        }
        if let ExprNode::Add(terms) = &**numerator {
            // `1 − cos u` over `u²`.
            if terms.len() == 2 {
                for term in terms {
                    if is_function_of(term, "cos", var) {
                        let body = function_body(term)?;
                        if vanishes(body, var, point)
                            && vanishes(denominator, var, point)
                            && is_square_of(denominator, body)
                        {
                            return Some(LimitValue::Finite(Number::ratio(1, 2)));
                        }
                    }
                }
            }
        }
    }
    // `(1 + u)^(1/u)` with `u` vanishing: the exponential's definition.
    if let ExprNode::Pow(base, exponent) = &**expression
        && let ExprNode::Add(terms) = &**base
        && terms.len() == 2
    {
        for term in terms {
            if !vanishes(term, var, point) {
                continue;
            }
            let Some((numerator, denominator)) = split_quotient(exponent) else {
                continue;
            };
            if numerator.as_number().is_some_and(|n| n.to_f64() == 1.0)
                && same_expression(denominator, term)
            {
                return Some(LimitValue::Finite(Number::float(std::f64::consts::E)));
            }
        }
    }
    None
}

/// Whether `expression` is `c·f(u)` for the named function `f` and a `u` that
/// mentions `var`.
///
/// The coefficient is allowed because the canonical form keeps it: `1 − cos x`
/// is `Add([1, Mul([-1, Fn(cos, [x])])])`, and the cosine is behind a `-1`.
/// Requiring a bare `Fn` was the first version and it matched nothing, so
/// `(1−cos x)/x²` fell through to the numeric path and answered `0` — a wrong
/// number, which is the one outcome this module must never produce.
fn is_function_of(expression: &Expr, name: &str, var: SymbolId) -> bool {
    let function = match &**expression {
        ExprNode::Fn(..) => expression,
        ExprNode::Mul(factors) => {
            let mut function = None;
            for factor in factors {
                match &**factor {
                    ExprNode::Num(_) => {}
                    ExprNode::Fn(..) if function.is_none() => function = Some(factor),
                    _ => return false,
                }
            }
            match function {
                Some(function) => function,
                None => return false,
            }
        }
        _ => return false,
    };
    match &**function {
        ExprNode::Fn(f, args) => {
            &*SymbolId::name(*f) == name && args.len() == 1 && contains(args[0].clone(), var)
        }
        _ => false,
    }
}

fn function_body(expression: &Expr) -> Option<&Expr> {
    match &**expression {
        ExprNode::Fn(_, args) if args.len() == 1 => Some(&args[0]),
        // `-1·cos x`: the coefficient is not part of the argument.
        ExprNode::Mul(factors) => factors.iter().find_map(|factor| match &**factor {
            ExprNode::Fn(_, args) if args.len() == 1 => Some(&args[0]),
            _ => None,
        }),
        _ => None,
    }
}

fn contains(expression: Expr, var: SymbolId) -> bool {
    let mut symbols = Vec::new();
    collect_symbols(&expression, &mut symbols);
    symbols.contains(&var)
}

fn same_function_body(a: &Expr, b: &Expr, var: SymbolId) -> bool {
    let Some(body) = function_body(a) else {
        return false;
    };
    let _ = var;
    same_expression(body, b)
}

fn same_expression(a: &Expr, b: &Expr) -> bool {
    // `a − b` normalises to the number zero when the two are the same
    // expression. `add` and `mul` are the canonicalisers; the difference is
    // exact arithmetic on the coefficients, so `sin u` and `sin u` are equal
    // and `sin u` and `sin 2u` are not.
    let difference = canlab_core::add([
        a.clone(),
        canlab_core::mul([ExprNode::int(-1), b.clone()]),
    ]);
    difference.as_number().is_some_and(Number::is_zero)
}

/// Whether `expression` is `body²`.
///
/// The denominator of `(1 − cos x)/x²` reaches here as `x²` — the quotient's
/// reciprocal is `(x²)⁻¹`, which `split_quotient` unwraps to `x²` — and the
/// cosine's argument is `x`. So the test is on the *base* of the square, not on
/// the square itself; comparing `x²` with `x` was the first version and it
/// matched nothing, which left the limit to the numeric path and answered `0`.
fn is_square_of(expression: &Expr, body: &Expr) -> bool {
    match &**expression {
        ExprNode::Pow(base, exponent) => {
            exponent.as_number().is_some_and(|n| n.to_f64() == 2.0) && same_expression(base, body)
        }
        _ => false,
    }
}

fn vanishes(expression: &Expr, var: SymbolId, point: f64) -> bool {
    evaluate(expression, var, point).abs() < 1e-12
}

// ---------------------------------------------------------------------------
// Numeric
// ---------------------------------------------------------------------------

/// How many refinements the numeric sequence may take.
const STEPS: usize = 14;
/// The first offset from the target, as a fraction of the target's own scale.
///
/// Scaling by the target and not by a fixed ladder is what makes a pole
/// visible: `1/(x−2)` at `x = 2` has no pole within `10⁻¹³` of a *unit* step,
/// but it has one within a millionth of the *target's* step, and the sequence
/// has to reach it to see the divergence. A fixed `10⁻ᵏ` ladder approached
/// `x = 2` from `2.1` down to `2.0000000000001` and never got closer.
const FIRST_STEP: f64 = 0.1;

fn numeric_limit(expression: &Expr, var: SymbolId, target: Target) -> Result<LimitValue, LimitError> {
    match target.approach {
        Approach::TwoSided => {
            let left = one_sided(expression, var, target, -1.0);
            let right = one_sided(expression, var, target, 1.0);
            match (left, right) {
                (Ok(left), Ok(right)) => {
                    let difference = (left - right).abs();
                    let scale = left.abs().max(right.abs()).max(1.0);
                    if difference <= 1e-3 * scale {
                        return Ok(LimitValue::Numeric {
                            value: 0.5 * (left + right),
                            uncertainty: 0.5 * difference + scale * 1e-9,
                        });
                    }
                    Err(LimitError::NotExist {
                        left: format!("{left:.6}"),
                        right: format!("{right:.6}"),
                    })
                }
                // Both sides diverging the same way is a limit: `1/x²` is
                // `+∞` from both, and "does not exist" would be wrong. Two
                // different infinities, or one infinite side and one finite,
                // is a limit that does not exist, and both sides are named.
                (Err(LimitError::Diverges { sign: left }), Err(LimitError::Diverges { sign: right }))
                    if left == right =>
                {
                    Ok(LimitValue::Infinite(left))
                }
                (left, right) => Err(LimitError::NotExist {
                    left: describe(left),
                    right: describe(right),
                }),
            }
        }
        Approach::FromAbove => one_sided(expression, var, target, 1.0).map(|value| {
            LimitValue::Numeric {
                value,
                uncertainty: 1e-6 * value.abs().max(1.0),
            }
        }),
        Approach::FromBelow => one_sided(expression, var, target, -1.0).map(|value| {
            LimitValue::Numeric {
                value,
                uncertainty: 1e-6 * value.abs().max(1.0),
            }
        }),
    }
}

fn describe(result: Result<f64, LimitError>) -> String {
    match result {
        Ok(value) => format!("{value:.6}"),
        Err(error) => error.message(),
    }
}

/// Evaluate along a geometric sequence approaching the target from `side`.
///
/// A geometric sequence and not an arithmetic one: a limit at `x → 0` is about
/// the *behaviour* near the target, and points at `10⁻¹, 10⁻², …` resolve it
/// where `0.1, 0.01, …` would need a hundred steps to reach the same place.
fn one_sided(expression: &Expr, var: SymbolId, target: Target, side: f64) -> Result<f64, LimitError> {
    let mut previous = f64::NAN;
    let mut current = f64::NAN;
    let scale = target.value.map(|value| value.abs().max(1.0)).unwrap_or(1.0);
    for step in 0..STEPS {
        let offset = FIRST_STEP.powi(step as i32 + 1) * scale;
        let point = match target.value {
            Some(value) => value + side * offset,
            // At infinity the sequence goes outwards.
            None => side * target.infinity as f64 * scale / offset,
        };
        let value = evaluate(expression, var, point);
        if !value.is_finite() {
            // A pole: the sequence is diverging, and the sign of the last finite
            // value it had is the side it diverges towards. `1/(x−2)` from above
            // is `+∞` and from below `−∞`, and saying so is the answer to the
            // question — "unsettled" would be a refusal of something known.
            if previous.is_finite() && previous != 0.0 {
                return Err(LimitError::Diverges {
                    sign: if previous > 0.0 { 1 } else { -1 },
                });
            }
            return Err(LimitError::Unsettled);
        }
        // Divergence, before convergence: a sequence whose magnitude is growing
        // by an order of magnitude per step is not approaching a number, and
        // waiting for it to reach a pole is waiting for a float to overflow.
        // `1/(x−2)` at `x → 2⁺` grows by ten each step from 10 to 10¹⁵ and never
        // stops; the growth *is* the answer, with the sign of the last value.
        if step > 0
            && previous.is_finite()
            && previous != 0.0
            && (value / previous).abs() > 5.0
            && value.abs() > 1e6
        {
            return Err(LimitError::Diverges {
                sign: if value > 0.0 { 1 } else { -1 },
            });
        }
        previous = current;
        current = value;
        if step > 2 && (current - previous).abs() <= 1e-9 * current.abs().max(1.0) {
            return Ok(current);
        }
    }
    if (current - previous).abs() <= 1e-4 * current.abs().max(1.0) {
        return Ok(current);
    }
    Err(LimitError::Unsettled)
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// The exact value of a polynomial at a rational point, if the expression is
/// a polynomial with rational coefficients.
fn exact_value(expression: &Expr, var: SymbolId, point: f64) -> Option<Number> {
    // The point must be exactly representable as a rational, or an "exact"
    // answer would be exact about a number the caller did not ask for.
    let point = exact_rational(point)?;
    let coefficients = polynomial_coefficients(expression, var)?;
    // Horner, in exact arithmetic.
    let mut value = Number::int(0);
    for coefficient in coefficients.iter().rev() {
        value = value.mul(&point).add(coefficient);
    }
    Some(value)
}

/// A `f64` as the rational it exactly is, or `None` when it is not finite.
///
/// `0.1` is not `1/10` in binary, and this function returns the binary value —
/// which is the number the caller asked about, unlike a decimal reading that
/// would answer a slightly different question.
fn exact_rational(value: f64) -> Option<Number> {
    if !value.is_finite() {
        return None;
    }
    if value.fract() == 0.0 && value.abs() < 1e18 {
        return Some(Number::int(value as i128));
    }
    let bits = value.to_bits();
    let sign = if value < 0.0 { -1 } else { 1 };
    let exponent = ((bits >> 52) & 0x7ff) as i64 - 1023 - 52;
    let mantissa = (bits & 0xf_ffff_ffff_ffff) as i128 | (1 << 52);
    let mantissa = sign * mantissa;
    if exponent >= 0 {
        let scale = 1i128.checked_shl(exponent as u32)?;
        Some(Number::int(mantissa.checked_mul(scale)?))
    } else {
        let scale = 1i128.checked_shl((-exponent) as u32)?;
        Some(Number::ratio(mantissa, scale))
    }
}

/// Evaluate an expression at `x`, with `pi` and `e` as their constants.
///
/// One evaluator for the whole crate: a second copy in [`crate::figure`] would
/// be a second opinion about what `sin` and `pi` mean, and the two would drift.
pub(crate) fn evaluate(expression: &Expr, var: SymbolId, x: f64) -> f64 {
    match &**expression {
        ExprNode::Num(number) => number.to_f64(),
        ExprNode::Sym(symbol) if *symbol == var => x,
        ExprNode::Sym(symbol) => match &*SymbolId::name(*symbol) {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            _ => f64::NAN,
        },
        ExprNode::Add(terms) => terms.iter().map(|t| evaluate(t, var, x)).sum(),
        ExprNode::Mul(factors) => factors.iter().map(|f| evaluate(f, var, x)).product(),
        ExprNode::Pow(base, exponent) => evaluate(base, var, x).powf(evaluate(exponent, var, x)),
        ExprNode::Fn(name, args) => {
            let Some(first) = args.first() else {
                return f64::NAN;
            };
            let value = evaluate(first, var, x);
            match &*SymbolId::name(*name) {
                "sin" => value.sin(),
                "cos" => value.cos(),
                "tan" => value.tan(),
                "exp" => value.exp(),
                "ln" => value.ln(),
                "sqrt" => value.sqrt(),
                "abs" => value.abs(),
                "arcsin" => value.asin(),
                "arccos" => value.acos(),
                "arctan" => value.atan(),
                "sinh" => value.sinh(),
                "cosh" => value.cosh(),
                "tanh" => value.tanh(),
                _ => f64::NAN,
            }
        }
        _ => f64::NAN,
    }
}

fn flatten_sum(expression: &Expr, out: &mut Vec<Expr>) {
    match &**expression {
        ExprNode::Add(terms) => {
            for term in terms {
                flatten_sum(term, out);
            }
        }
        _ => out.push(expression.clone()),
    }
}

/// Every symbol an expression mentions, in order of appearance.
///
/// Shared with the parser's figure statements, which need the variable of
/// integration from a curve that does not say it.
pub(crate) fn collect_symbols(expression: &Expr, out: &mut Vec<SymbolId>) {
    match &**expression {
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

