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

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, mul};
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
    if name.as_str() != "tan" {
        // `sin` and `cos` have two families; see the module comment.
        return None;
    }
    if !mentions(&argument, var) {
        return None;
    }
    let base = add(vec![arctangent(&right)]);
    Some(Solutions::Family {
        base,
        period: pi_times(1, 1),
        param: SymbolId::intern("k"),
    })
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
    fn a_sine_declines_because_it_needs_two_families() {
        // sin x = ½ has π/6 + 2kπ *and* 5π/6 + 2kπ, and one family cannot say it.
        let equation = add(vec![
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
            ExprNode::num(Number::ratio(-1, 2)),
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
