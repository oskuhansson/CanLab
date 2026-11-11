//! Trigonometric equations that have **one** family of solutions.
//!
//! `tan u = r` is the case where the answer is a single arithmetic progression:
//! the tangent repeats every half turn, so
//!
//! ```text
//! tan u = r   →   u = arctan r + kπ
//! ```
//!
//! and that is exactly what [`Solutions::Family`] holds. It is *not* true of sine
//! and cosine: `sin x = ½` has two progressions, `π/6 + 2kπ` and `5π/6 + 2kπ`,
//! and a single family cannot say that. W2.5 needs a union of families for those,
//! which is a change to `Solutions` (W2.1 owns the type), so those equations
//! decline here rather than answering half of their roots.
//!
//! The common values are kept exact: `tan x = 1` answers `π/4 + kπ`, not
//! `0.785… + kπ`. Everything else keeps `arctan` as a function, which is the exact
//! number and not an approximation of it.

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, int, mul};
use crate::Solutions;

/// `n/d · π`.
fn pi_times(numerator: i64, denominator: i64) -> Expr {
    mul([
        ExprNode::num(Number::ratio(numerator, denominator)),
        ExprNode::sym(SymbolId::intern("pi")),
    ])
}

/// `u = arctan(number)`, exact where a school value exists.
fn arctangent(number: &Number) -> Expr {
    if number.is_zero() {
        return ExprNode::int(0);
    }
    if let Some(integer) = match number {
        Number::Int(value) => Some(*value),
        _ => None,
    } {
        match integer {
            1 => return pi_times(1, 4),
            -1 => return pi_times(-1, 4),
            _ => {}
        }
    }
    ExprNode::fn_raw(SymbolId::intern("arctan"), vec![ExprNode::num(number.clone())])
}

/// The right-hand side of `f(u) = r`, when `equation` is that shape.
fn equals(equation: &Expr) -> Option<(SymbolId, Expr, Number)> {
    // `left − right` reaches here as `Add([<function>, Num(−r)])`.
    let terms: Vec<&Expr> = match &**equation {
        ExprNode::Add(terms) => terms.iter().collect(),
        _ => vec![equation],
    };
    let mut constant = Number::zero();
    let mut function: Option<(SymbolId, Expr)> = None;
    for term in terms {
        if let Some(number) = term.as_number() {
            constant = constant.add(number);
            continue;
        }
        let ExprNode::Fn(name, arguments) = &**term else {
            return None;
        };
        if function.is_some() || arguments.len() != 1 {
            return None;
        }
        function = Some((*name, arguments[0].clone()));
    }
    let (name, argument) = function?;
    Some((name, argument, constant.neg()))
}

/// Solve a trigonometric equation whose answer is a single family.
pub fn solve_trigonometric(equation: &Expr, var: SymbolId) -> Option<Solutions> {
    let (name, argument, right) = equals(equation)?;
    if !mentions(&argument, var) {
        return None;
    }
    let family = |base: Expr, period: Expr| Solutions::Family {
        base: add(vec![base]),
        period,
        param: SymbolId::intern("k"),
    };
    match name.as_str().as_str() {
        "tan" => Some(family(arctangent(&right), pi_times(1, 1))),
        // Sine and cosine have two branches. Where the branches coincide the
        // answer is one progression, and where they do not it is their union —
        // `sin x = ½` is `π/6 + 2kπ ∪ 5π/6 + 2kπ`, and answering only the first
        // would be half an answer.
        "sin" => {
            let alpha = arcsine(&right)?;
            match integer_of(&right) {
                Some(0) => Some(family(ExprNode::int(0), pi_times(1, 1))),
                Some(1) => Some(family(pi_times(1, 2), pi_times(2, 1))),
                Some(-1) => Some(family(pi_times(-1, 2), pi_times(2, 1))),
                _ => {
                    let second = add(vec![pi_times(1, 1), mul([int(-1), alpha.clone()])]);
                    Some(Solutions::Union(vec![
                        family(alpha, pi_times(2, 1)),
                        family(second, pi_times(2, 1)),
                    ]))
                }
            }
        }
        "cos" => {
            let alpha = arccosine(&right)?;
            match integer_of(&right) {
                Some(1) => Some(family(ExprNode::int(0), pi_times(2, 1))),
                Some(-1) => Some(family(pi_times(1, 1), pi_times(2, 1))),
                Some(0) => Some(family(pi_times(1, 2), pi_times(1, 1))),
                _ => {
                    let second = mul([int(-1), alpha.clone()]);
                    Some(Solutions::Union(vec![
                        family(alpha, pi_times(2, 1)),
                        family(second, pi_times(2, 1)),
                    ]))
                }
            }
        }
        _ => None,
    }
}

/// `arcsin r`, exact at the half-turn values a course asks for.
fn arcsine(number: &Number) -> Option<Expr> {
    if number == &Number::ratio(1, 2) {
        return Some(pi_times(1, 6));
    }
    if number == &Number::ratio(-1, 2) {
        return Some(pi_times(-1, 6));
    }
    inverse_arc(number, "arcsin")
}

/// `arccos r`, exact at ±½.
fn arccosine(number: &Number) -> Option<Expr> {
    if number == &Number::ratio(1, 2) {
        return Some(pi_times(1, 3));
    }
    if number == &Number::ratio(-1, 2) {
        return Some(pi_times(2, 3));
    }
    inverse_arc(number, "arccos")
}

/// The exact function value, or a refusal when the number is outside `[−1, 1]`
/// and the equation therefore has no real solution at all.
fn inverse_arc(number: &Number, name: &str) -> Option<Expr> {
    let value = number.to_f64();
    if !(-1.0..=1.0).contains(&value) {
        return None;
    }
    Some(ExprNode::fn_raw(
        SymbolId::intern(name),
        vec![ExprNode::num(number.clone())],
    ))
}

/// `-1`, `0` or `1` as an integer, and nothing else.
fn integer_of(number: &Number) -> Option<i64> {
    match number {
        Number::Int(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

fn mentions(expression: &Expr, var: SymbolId) -> bool {
    match &**expression {
        ExprNode::Num(_) => false,
        ExprNode::Sym(symbol) => *symbol == var,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => terms.iter().any(|term| mentions(term, var)),
        ExprNode::Pow(base, exponent) => mentions(base, var) || mentions(exponent, var),
        ExprNode::Fn(_, arguments) => arguments.iter().any(|argument| mentions(argument, var)),
        ExprNode::Hold(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, sym};

    fn var() -> SymbolId {
        SymbolId::intern("x")
    }

    /// `tan x − r`.
    fn tangent_of_x_minus(r: Number) -> Expr {
        add(vec![
            ExprNode::fn_raw(SymbolId::intern("tan"), vec![sym("x")]),
            ExprNode::num(r),
        ])
    }

    #[test]
    fn tangent_one_is_a_quarter_turn() {
        // tan x = 1 → π/4 + kπ, and the π/4 is exact.
        let solutions = solve_trigonometric(&tangent_of_x_minus(Number::int(-1)), var())
            .expect("a tangent equation");
        let latex = solutions.to_latex();
        assert!(latex.contains("\\pi k"), "{latex}");
        assert!(latex.contains("\\pi"), "{latex}");
        assert!(!latex.contains("arctan"), "the common angle is exact: {latex}");
    }

    #[test]
    fn tangent_zero_is_the_half_turn_itself() {
        // tan x = 0 → kπ, without a `0 +` in front of it.
        let solutions = solve_trigonometric(&tangent_of_x_minus(Number::int(0)), var())
            .expect("a tangent equation");
        let latex = solutions.to_latex();
        assert!(latex.contains("\\pi k"), "{latex}");
        assert!(!latex.starts_with("0"), "{latex}");
    }

    #[test]
    fn an_awkward_value_keeps_its_arctangent() {
        // tan x = 2 → arctan 2 + kπ: exact, and the only honest form.
        let solutions = solve_trigonometric(&tangent_of_x_minus(Number::int(-2)), var())
            .expect("a tangent equation");
        assert!(solutions.to_latex().contains("arctan"), "{}", solutions.to_latex());
    }

    #[test]
    fn a_sine_at_its_extreme_is_one_family() {
        // sin x = 1 → π/2 + 2kπ: the two branches coincide, so this is complete.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
            int(-1),
        ]);
        let solutions = solve_trigonometric(&equation, var()).expect("one family");
        let latex = solutions.to_latex();
        assert!(latex.contains("\\frac{\\pi}{2}"), "{latex}");
        assert!(latex.contains("2"), "the period is a full turn: {latex}");
    }

    #[test]
    fn a_cosine_at_zero_is_one_half_turn_family() {
        // cos x = 0 → π/2 + kπ.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("cos"), vec![sym("x")]),
        ]);
        let solutions = solve_trigonometric(&equation, var()).expect("one family");
        assert!(solutions.to_latex().contains("\\pi k"), "{}", solutions.to_latex());
    }

    #[test]
    fn a_sine_of_a_half_is_two_families_and_they_are_both_there() {
        // sin x = ½ → π/6 + 2kπ ∪ 5π/6 + 2kπ. One family would be half the answer.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
            ExprNode::num(Number::ratio(-1, 2)),
        ]);
        let solutions = solve_trigonometric(&equation, var()).expect("two families");
        let latex = solutions.to_latex();
        assert!(latex.contains("\\cup"), "both branches: {latex}");
        assert!(latex.contains("\\frac{\\pi}{6}"), "{latex}");
    }

    #[test]
    fn a_sine_outside_the_range_has_no_solution() {
        // sin x = 2 has none, and that is a refusal here rather than a family.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
            int(-2),
        ]);
        assert!(solve_trigonometric(&equation, var()).is_none());
    }

    #[test]
    fn a_tangent_of_a_constant_is_not_about_the_unknown() {
        // tan 2 = 0 is not an equation in x.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("tan"), vec![int(2)]),
            int(-1),
        ]);
        assert!(solve_trigonometric(&equation, var()).is_none());
    }
}
