//! Exponential equations: one exponential term and one constant.
//!
//! `2^x = 8`, `3·e^{2x} = 12`, `5^{x−1} = 1` — the shape a course writes as "a
//! power equals a number". The method is the one a student uses: isolate the
//! power, then take the logarithm of both sides.
//!
//! ```text
//! c·a^u + d = 0   →   a^u = −d/c   →   u = log_a(−d/c)
//! ```
//!
//! The logarithm is kept **exact** when the right side is a power of the base
//! (`2^u = 8` is `u = 3`, not `u = 2.079…`), and written as `ln r / ln a` when it
//! is not (`2^x = 7` is `\frac{\ln 7}{\ln 2}`, which is the exact number). A
//! negative or zero right side has no real solution at all, because a positive
//! base to a real power is positive — that is an answer, not a refusal.
//!
//! What this does *not* do: two exponential terms (`2^x + 3^x = 5`), a power of a
//! power (`2^{x^2} = 8` is handled only through the linear/quadratic solver on the
//! exponent, so it needs the exponent to be polynomial), and equations that mix a
//! power with the variable outside it (`x·2^x = 8`). All of those decline.

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, int, mul, pow};
use crate::native::NativeSolver;
use crate::Solver as _;

/// `base`, `exponent` and the numeric coefficient in front, when `term` is
/// `c·a^u` with `a` free of `var`.
fn exponential_parts(term: &Expr, var: SymbolId) -> Option<(Expr, Expr, Number)> {
    let (coefficient, rest) = match &**term {
        ExprNode::Mul(factors) if factors.len() == 2 => match &*factors[0] {
            ExprNode::Num(number) => (number.clone(), factors[1].clone()),
            _ => (Number::int(1), term.clone()),
        },
        ExprNode::Num(_) => return None,
        _ => (Number::int(1), term.clone()),
    };
    let ExprNode::Pow(base, exponent) = &*rest else {
        return None;
    };
    // The base is a number or `e`; a base that mentions the unknown is not an
    // exponential this can isolate.
    if mentions(base, var) {
        return None;
    }
    let known_base = base.as_number().is_some() || base.as_symbol().is_some_and(|s| s.as_str() == "e");
    if !known_base {
        return None;
    }
    if !mentions(exponent, var) {
        return None;
    }
    Some((base.clone(), exponent.clone(), coefficient))
}

fn mentions(expression: &Expr, var: SymbolId) -> bool {
    match &**expression {
        ExprNode::Num(_) => false,
        ExprNode::Sym(symbol) => *symbol == var,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => terms.iter().any(|term| mentions(term, var)),
        ExprNode::Pow(base, exponent) => mentions(base, var) || mentions(exponent, var),
        ExprNode::Fn(_, arguments) => arguments.iter().any(|argument| mentions(argument, var)),
        // A held expression is not something this reader opens; decline it.
        ExprNode::Hold(_) => true,
    }
}

/// `ln(value)` as an expression.
fn natural_log(value: &Expr) -> Expr {
    ExprNode::fn_raw(SymbolId::intern("ln"), vec![value.clone()])
}

/// The exact `k` with `base^k = value`, if there is one in a small range.
///
/// `2^3 = 8` must answer `3`, not `2.079…`: an exact answer is the reason to have
/// a CAS. A range of ±64 keeps the search finite, and a power beyond it is still
/// answerable as a logarithm.
fn exact_power(base: &Number, value: &Number) -> Option<i128> {
    if base.is_zero() || base.to_f64() <= 0.0 || (base.to_f64() - 1.0).abs() < 1e-12 {
        return None;
    }
    let mut power = Number::int(1);
    for exponent in 0..=64 {
        if power == *value {
            return Some(exponent);
        }
        power = power.mul(base);
    }
    let mut power = Number::int(1);
    for exponent in 1..=64 {
        power = power.div(base);
        if power == *value {
            return Some(-exponent);
        }
    }
    None
}

/// `u = log_a(r)`, exact when `r` is a power of `a`.
fn logarithm(base: &Expr, value: &Expr) -> Option<Expr> {
    if let (Some(a), Some(r)) = (base.as_number(), value.as_number())
        && let Some(exponent) = exact_power(a, r)
    {
        return Some(int(i64::try_from(exponent).ok()?));
    }
    // `ln r / ln a`, which is the exact logarithm for every other case. `e` is
    // its own logarithm, so `e^x = 5` answers `\ln 5` and not `\ln 5 / \ln e`.
    let upper = natural_log(value);
    if base.as_symbol().is_some_and(|symbol| symbol.as_str() == "e") {
        return Some(upper);
    }
    let lower = natural_log(base);
    Some(mul([upper, pow(lower, int(-1))]))
}

/// Solve one equation of the form `c·a^u + d = 0` for `var`.
///
/// `None` when the equation is not that shape. `Some(vec![])` when it is and the
/// right side is not positive: that is "no real solution", which is an answer.
pub fn solve_exponential(solver: &NativeSolver, equation: &Expr, var: SymbolId) -> Option<Vec<Expr>> {
    let terms: Vec<&Expr> = match &**equation {
        ExprNode::Add(terms) => terms.iter().collect(),
        _ => vec![equation],
    };
    let mut exponential: Option<(Expr, Expr, Number)> = None;
    let mut constant = Number::zero();
    for term in terms {
        if let Some(parts) = exponential_parts(term, var) {
            if exponential.is_some() {
                // Two exponential terms: `2^x + 3^x = 5` is not this method.
                return None;
            }
            exponential = Some(parts);
        } else if let Some(number) = term.as_number() {
            constant = constant.add(number);
        } else {
            return None;
        }
    }

    let (base, exponent, coefficient) = exponential?;
    if coefficient.is_zero() {
        return None;
    }
    // `c·a^u + d = 0` → `a^u = −d/c`.
    let right = constant.neg().div(&coefficient);
    let right = ExprNode::num(right);
    // A positive base to a real power is positive, so a non-positive right side
    // has no real solution — and that is an answer, not a decline.
    if !right.as_number().is_some_and(|number| number.to_f64() > 0.0) {
        return Some(Vec::new());
    }

    let logarithm = logarithm(&base, &right)?;
    // `u = log_a(r)`; the exponent may itself be a polynomial in `var`.
    let reduced = add(vec![
        exponent.clone(),
        mul([int(-1), logarithm.clone()]),
    ]);
    if let Ok(values) = solver.solve(&reduced, var) {
        return Some(values);
    }
    // The exponent is the unknown itself: `a^x = r` reads straight off as
    // `x = log_a r`, and when that logarithm is not a number — `2^x = 7` — the
    // polynomial solver cannot carry it, but the exact answer is still this.
    if exponent == ExprNode::sym(var) {
        return Some(vec![logarithm]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{Number, sym};

    fn var() -> SymbolId {
        SymbolId::intern("x")
    }

    /// `2^x = 8` as `2^x − 8`.
    fn bracket(base: i64, exponent: Expr) -> Expr {
        pow(int(base), exponent)
    }

    #[test]
    fn a_power_that_matches_is_exact() {
        // 2^x = 8: 8 is 2³, so the answer is 3 and not 2.079…
        let equation = add(vec![bracket(2, sym("x")), int(-8)]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert_eq!(roots.len(), 1, "{roots:?}");
        assert_eq!(roots[0].as_number().map(Number::to_f64), Some(3.0));
    }

    #[test]
    fn a_power_that_does_not_match_is_a_logarithm() {
        // 2^x = 7 has no rational answer; `ln 7 / ln 2` is the exact one.
        let equation = add(vec![bracket(2, sym("x")), int(-7)]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert_eq!(roots.len(), 1, "{roots:?}");
        let value = canlab_core::approx::value_of(&roots[0]).expect("a number");
        assert!((value - 7f64.log2()).abs() < 1e-12, "{value}");
        assert!(
            canlab_tex::to_latex(&roots[0]).contains("ln"),
            "the exact answer keeps the logarithm: {}",
            canlab_tex::to_latex(&roots[0])
        );
    }

    #[test]
    fn a_coefficient_is_divided_out_first() {
        // 3·2^x = 12 → 2^x = 4 → x = 2.
        let equation = add(vec![
            mul([int(3), bracket(2, sym("x"))]),
            int(-12),
        ]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert_eq!(roots.len(), 1, "{roots:?}");
        assert_eq!(roots[0].as_number().map(Number::to_f64), Some(2.0));
    }

    #[test]
    fn a_power_with_no_real_solution_says_so() {
        // 2^x > 0 for every real x, so 2^x = −4 has no solution. That is an
        // answer, not a refusal: the caller reports `Solutions::None`.
        let equation = add(vec![bracket(2, sym("x")), int(4)]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert!(roots.is_empty(), "{roots:?}");
    }

    #[test]
    fn the_exponent_may_be_a_polynomial() {
        // 2^{x²} = 8 → x² = 3, so the answers are ±√3.
        let equation = add(vec![bracket(2, pow(sym("x"), int(2))), int(-8)]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert_eq!(roots.len(), 2, "{roots:?}");
        let mut values: Vec<f64> = roots
            .iter()
            .filter_map(canlab_core::approx::value_of)
            .collect();
        values.sort_by(f64::total_cmp);
        assert!((values[0] + 3f64.sqrt()).abs() < 1e-12, "{values:?}");
        assert!((values[1] - 3f64.sqrt()).abs() < 1e-12, "{values:?}");
    }

    #[test]
    fn a_base_of_e_keeps_its_own_logarithm() {
        // e^x = 5 → ln 5, not ln 5 / ln e.
        let equation = add(vec![pow(sym("e"), sym("x")), int(-5)]);
        let roots = solve_exponential(&NativeSolver, &equation, var()).expect("an exponential");
        assert_eq!(roots.len(), 1, "{roots:?}");
        let latex = canlab_tex::to_latex(&roots[0]);
        assert_eq!(latex.matches("ln").count(), 1, "{latex}");
    }

    #[test]
    fn the_variable_outside_the_power_declines() {
        // x·2^x = 8 is not this method, and a wrong answer is worse than a
        // refusal.
        let equation = add(vec![mul([sym("x"), bracket(2, sym("x"))]), int(-8)]);
        assert!(solve_exponential(&NativeSolver, &equation, var()).is_none());
    }

    #[test]
    fn two_powers_decline() {
        // 2^x + 3^x = 5.
        let equation = add(vec![
            bracket(2, sym("x")),
            bracket(3, sym("x")),
            int(-5),
        ]);
        assert!(solve_exponential(&NativeSolver, &equation, var()).is_none());
    }
}
