//! Logarithmic equations: sums of logarithms over one base.
//!
//! `log₂ x + log₂(x−2) = 3` is the shape a course writes, and the method is the
//! student's: combine the logarithms into one, undo it, then check the answers
//! against the domain of the original equation.
//!
//! ```text
//! c·Σ ln uᵢ + d = 0,  c = 1/ln b   →   ∏uᵢ = b^{−d}
//! ```
//!
//! The check is not optional. `∏uᵢ = b^{−d}` is a *wider* equation than the one
//! that was asked: it accepts any values whose product is right, including ones
//! where a single logarithm has a non-positive argument. `log₂x + log₂(x−2) = 3`
//! has the roots `4` and `−2`, and `−2` is not a solution of the original
//! equation at all — `ln(−2)` is not a real number. So every candidate is
//! substituted back and the ones outside the domain are returned *separately*:
//! dropping them silently would be the same mistake as reporting them.
//!
//! What this does not do: different bases in one equation (`log₂x + log₃x = 1`),
//! different coefficients (`log₂x + 2log₂(x−1) = 3`), a logarithm of the unknown
//! on both sides of a product, and a right side that is not an integer power of
//! the base. All decline.

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, int, mul};
use crate::native::NativeSolver;
use crate::Solver as _;

fn natural_log(value: &Expr) -> Expr {
    ExprNode::fn_raw(SymbolId::intern("ln"), vec![value.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::pow;

    fn var() -> SymbolId {
        SymbolId::intern("x")
    }

    /// `log_base(argument)` in the form the field builds it: `ln u / ln b`.
    fn log_base(base: i64, argument: Expr) -> Expr {
        mul([
            natural_log(&argument),
            pow(natural_log(&int(base)), int(-1)),
        ])
    }

    #[test]
    fn a_sum_of_logarithms_combines() {
        // log₂x + log₂(x−2) = 3 → x(x−2) = 8 → x² − 2x − 8 = 0 → 4 and −2.
        let equation = add(vec![
            log_base(2, canlab_core::sym("x")),
            log_base(2, add(vec![canlab_core::sym("x"), int(-2)])),
            int(-3),
        ]);
        let (kept, rejected) = solve_logarithmic(&NativeSolver, &equation, var())
            .expect("a logarithmic equation");
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            canlab_core::approx::value_of(&kept[0]).map(|v| (v * 1e9).round() / 1e9),
            Some(4.0)
        );
        // −2 solves the *combined* equation and not the original one: ln(−2) is
        // not a real number. Dropping it silently would be the wrong repair.
        assert_eq!(rejected.len(), 1, "{rejected:?}");
        assert_eq!(
            canlab_core::approx::value_of(&rejected[0]).map(|v| (v * 1e9).round() / 1e9),
            Some(-2.0)
        );
    }

    #[test]
    fn one_logarithm_is_already_the_shape() {
        // log₂x = 3 → x = 8.
        let equation = add(vec![log_base(2, canlab_core::sym("x")), int(-3)]);
        let (kept, rejected) =
            solve_logarithmic(&NativeSolver, &equation, var()).expect("a logarithmic equation");
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(canlab_core::approx::value_of(&kept[0]), Some(8.0));
        assert!(rejected.is_empty(), "{rejected:?}");
    }

    #[test]
    fn a_surd_root_inside_the_domain_is_kept() {
        // log₂x + log₂(x−2) = 1 → x² − 2x − 2 = 0 → 1 ± √3. The positive one is
        // a solution, the other is outside the domain, so one of each.
        let equation = add(vec![
            log_base(2, canlab_core::sym("x")),
            log_base(2, add(vec![canlab_core::sym("x"), int(-2)])),
            int(-1),
        ]);
        let (kept, rejected) =
            solve_logarithmic(&NativeSolver, &equation, var()).expect("a logarithmic equation");
        assert_eq!((kept.len(), rejected.len()), (1, 1), "{kept:?} {rejected:?}");
    }

    #[test]
    fn two_bases_decline() {
        // log₂x + log₃x = 1 does not combine into one logarithm.
        let equation = add(vec![
            log_base(2, canlab_core::sym("x")),
            log_base(3, canlab_core::sym("x")),
            int(-1),
        ]);
        assert!(solve_logarithmic(&NativeSolver, &equation, var()).is_none());
    }
}

/// One term of the equation: `c·ln u` with `c = 1/ln b` for `log_b u`.
struct Logarithm {
    argument: Expr,
    base: Option<Number>,
    coefficient: Option<Number>,
}

/// Read `term` as `c·log_b(u)`.
fn logarithmic_parts(term: &Expr) -> Option<Logarithm> {
    // A bare `ln u` is a logarithm over `e` with coefficient one.
    if let ExprNode::Fn(name, arguments) = &**term
        && name.as_str() == "ln"
        && arguments.len() == 1
    {
        return Some(Logarithm {
            argument: arguments[0].clone(),
            base: None,
            coefficient: None,
        });
    }

    let factors: Vec<&Expr> = match &**term {
        ExprNode::Mul(factors) => factors.iter().collect(),
        _ => return None,
    };
    // One factor is `ln u`; another is `ln b` to the power −1. Anything else has
    // to be a number, and there may be no other logarithm.
    let mut argument: Option<Expr> = None;
    let mut base: Option<Number> = None;
    let mut coefficient: Option<Number> = None;
    for factor in factors {
        match &**factor {
            ExprNode::Fn(name, arguments)
                if name.as_str() == "ln" && arguments.len() == 1 =>
            {
                if argument.is_some() {
                    // `ln u · ln v` is not one logarithm.
                    return None;
                }
                argument = Some(arguments[0].clone());
            }
            ExprNode::Pow(inner, exponent) => {
                let ExprNode::Num(Number::Int(-1)) = &**exponent else {
                    return None;
                };
                let ExprNode::Fn(name, arguments) = &**inner else {
                    return None;
                };
                if name.as_str() != "ln" || arguments.len() != 1 {
                    return None;
                }
                let ExprNode::Num(number) = &*arguments[0] else {
                    return None;
                };
                if base.is_some() {
                    return None;
                }
                base = Some(number.clone());
            }
            ExprNode::Num(number) => coefficient = Some(number.clone()),
            _ => return None,
        }
    }
    Some(Logarithm {
        argument: argument?,
        base,
        coefficient,
    })
}

/// A candidate whose every logarithm has a positive argument.
fn inside_the_domain(
    roots: &[Expr],
    arguments: &[Expr],
    var: SymbolId,
) -> (Vec<Expr>, Vec<Expr>) {
    let mut kept = Vec::new();
    let mut rejected = Vec::new();
    for root in roots {
        let mut valid = true;
        for argument in arguments {
            let substituted = crate::system::substitute(argument, var, root);
            if let Some(value) = canlab_core::approx::value_of(&substituted)
                && value <= 0.0
            {
                valid = false;
                break;
            }
        }
        if valid {
            kept.push(root.clone());
        } else {
            rejected.push(root.clone());
        }
    }
    (kept, rejected)
}

/// `base^power` for a non-negative integer `power`.
fn integer_power(base: &Number, power: i128) -> Option<Number> {
    if power < 0 {
        return None;
    }
    let mut out = Number::int(1);
    for _ in 0..power {
        out = out.mul(base);
    }
    Some(out)
}

/// Solve a sum of logarithms over one base for `var`.
///
/// `None` when the equation is not that shape. `Some((kept, rejected))` when it
/// is: the kept roots solve the equation as written, the rejected ones solve the
/// combined equation it was turned into.
pub fn solve_logarithmic(
    solver: &NativeSolver,
    equation: &Expr,
    var: SymbolId,
) -> Option<(Vec<Expr>, Vec<Expr>)> {
    let terms: Vec<&Expr> = match &**equation {
        ExprNode::Add(terms) => terms.iter().collect(),
        _ => vec![equation],
    };
    let mut constant = Number::zero();
    let mut logs: Vec<Logarithm> = Vec::new();
    for term in terms {
        if let Some(number) = term.as_number() {
            constant = constant.add(number);
            continue;
        }
        match logarithmic_parts(term) {
            Some(parts) => logs.push(parts),
            None => return None,
        }
    }
    if logs.is_empty() {
        return None;
    }

    // One base for the whole equation, and one coefficient: `log₂x + log₂(x−2)`
    // combines, `log₂x + 2log₂(x−1)` does not.
    let base = logs[0].base.clone();
    let coefficient = logs[0].coefficient.clone();
    if logs
        .iter()
        .any(|log| log.base != base || log.coefficient != coefficient)
    {
        return None;
    }
    let coefficient = coefficient.unwrap_or_else(|| Number::int(1));
    if coefficient.is_zero() {
        return None;
    }

    // Without a numeric base the right side is `e^k`, which this does not carry.
    let base = base?;
    // `c·Σln u + d = 0` → `∏u = b^{−d/c}`. The exponent has to be an integer for
    // the product to be an exact coefficient.
    let rhs = constant.neg().div(&coefficient);
    let Number::Int(exponent) = rhs else {
        return None;
    };
    let target = integer_power(&base, i128::from(exponent))?;

    let arguments: Vec<Expr> = logs.iter().map(|log| log.argument.clone()).collect();
    let product = match arguments.len() {
        0 => return None,
        1 => arguments[0].clone(),
        _ => mul(arguments.clone()),
    };
    let expanded = add(vec![product, mul([int(-1), ExprNode::num(target)])]);
    let roots = solver.solve(&expanded, var).ok()?;
    Some(inside_the_domain(&roots, &arguments, var))
}
