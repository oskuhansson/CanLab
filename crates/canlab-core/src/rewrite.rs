//! A small term-rewriting engine, and the identities the project ships with.
//!
//! The engine is deliberately not an e-graph and not a matcher. A rule is a
//! function from an expression to `Some(better)` or `None`, so writing a rule is
//! writing a `match` and reading one is reading a `match`. That is enough for the
//! identities a school exercise meets, and it keeps the whole thing auditable —
//! which matters more than reach here, because a wrong identity is invisible: the
//! answer looks like an answer.
//!
//! # It always terminates
//!
//! Three things guarantee it:
//!
//! * rules are applied one at a time, bottom-up, and each application must
//!   strictly **reduce the cost** or the walk stops;
//! * a **step budget** bounds a rule set with a cycle the cost function misses;
//! * a rewritten expression seen before ends the walk, so a cycle costs one step.
//!
//! # What a rule is allowed to assume
//!
//! Nothing. `√(u²) = |u|` and not `u`, because `u` may be negative.
//! `exp(ln u) = u` holds only where `ln u` exists, and the test samples only
//! there. Every rule below is checked numerically at random points by
//! `tests/rewrite.rs`; a rule that cannot be checked that way does not ship.

use crate::expr::{Expr, ExprNode};
use crate::normalize::normalize;
use crate::number::Number;
use crate::symbol::SymbolId;

/// One named rewrite.
///
/// `apply` sees one node. Returning `Some` replaces that node; `None` means the
/// rule does not match and the walk continues.
#[derive(Clone, Copy)]
pub struct Rule {
    pub name: &'static str,
    pub apply: fn(&Expr) -> Option<Expr>,
}

/// The rules that ship.
pub fn rules() -> &'static [Rule] {
    RULES
}

/// Simplify with the shipped rules under the default budget.
pub fn simplify(expression: &Expr) -> Expr {
    simplify_with(expression, rules(), DEFAULT_BUDGET)
}

/// How many rewrites a single call may perform.
pub const DEFAULT_BUDGET: usize = 256;

/// Simplify with a given rule set.
pub fn simplify_with(expression: &Expr, rules: &[Rule], budget: usize) -> Expr {
    let mut current = normalize(expression);
    let mut seen: Vec<Expr> = vec![current.clone()];
    for _ in 0..budget {
        let Some((_name, next)) = rewrite_step(&current, rules) else {
            break;
        };
        let next = normalize(&next);
        if seen.contains(&next) {
            break;
        }
        // Strictly bigger is refused; equal is allowed, because the exact value
        // of `sin(`π/4)` is `√2/2` and that is the same size as the sine. A
        // cycle is stopped by the repeat check rather than by the cost.
        if cost(&next) > cost(&current) {
            break;
        }
        seen.push(next.clone());
        current = next;
    }
    current
}

/// How expensive an expression is: one per node, two per function call.
///
/// A function costs more than the number it usually becomes, which is what makes
/// `ln e → 1` a reduction and `1 → ln e` not one.
pub fn cost(expression: &Expr) -> usize {
    match &**expression {
        ExprNode::Num(_) | ExprNode::Sym(_) => 1,
        ExprNode::Add(parts) | ExprNode::Mul(parts) => {
            1 + parts.iter().map(cost).sum::<usize>()
        }
        ExprNode::Pow(base, exponent) => 1 + cost(base) + cost(exponent),
        ExprNode::Fn(_, args) => 2 + args.iter().map(cost).sum::<usize>(),
        ExprNode::Hold(_) => usize::MAX / 2,
    }
}

/// One bottom-up rewrite: the first match in post-order wins.
fn rewrite_step(expression: &Expr, rules: &[Rule]) -> Option<(&'static str, Expr)> {
    // Children first, so `ln(exp x)` is simplified before the sum around it.
    let rewritten_child = match &**expression {
        ExprNode::Add(terms) => rebuild(terms, rules, ExprNode::add_raw),
        ExprNode::Mul(factors) => rebuild(factors, rules, ExprNode::mul_raw),
        ExprNode::Pow(base, exponent) => {
            let new_base = rewrite_step(base, rules).map(|(_, expression)| expression);
            let new_exponent = rewrite_step(exponent, rules).map(|(_, expression)| expression);
            if new_base.is_none() && new_exponent.is_none() {
                None
            } else {
                Some(ExprNode::pow_raw(
                    new_base.unwrap_or_else(|| base.clone()),
                    new_exponent.unwrap_or_else(|| exponent.clone()),
                ))
            }
        }
        _ => None,
    };
    if let Some(rewritten) = rewritten_child {
        return Some(("child", rewritten));
    }
    rules
        .iter()
        .find_map(|rule| (rule.apply)(expression).map(|rewritten| (rule.name, rewritten)))
}

/// Rebuild a n-ary node with every child rewritten.
fn rebuild(
    children: &[Expr],
    rules: &[Rule],
    build: fn(Vec<Expr>) -> Expr,
) -> Option<Expr> {
    let mut changed = false;
    let mut new_children = Vec::with_capacity(children.len());
    for child in children {
        match rewrite_step(child, rules) {
            Some((_, rewritten)) => {
                changed = true;
                new_children.push(rewritten);
            }
            None => new_children.push(child.clone()),
        }
    }
    changed.then(|| build(new_children))
}

// ---------------------------------------------------------------- the rules

static RULES: &[Rule] = &[
    Rule {
        name: "ln of one",
        apply: |expression| match_function(expression, "ln", |argument| {
            is_one(argument).then(|| ExprNode::int(0))
        }),
    },
    Rule {
        name: "ln of e",
        apply: |expression| match_function(expression, "ln", |argument| {
            is_symbol(argument, "e").then(|| ExprNode::int(1))
        }),
    },
    Rule {
        name: "ln undoes exp",
        apply: |expression| match_function(expression, "ln", |argument| {
            inner_call(argument, "exp").map(|inner| inner[0].clone())
        }),
    },
    Rule {
        name: "exp of zero",
        apply: |expression| match_function(expression, "exp", |argument| {
            is_zero(argument).then(|| ExprNode::int(1))
        }),
    },
    Rule {
        name: "exp undoes ln",
        apply: |expression| match_function(expression, "exp", |argument| {
            inner_call(argument, "ln").map(|inner| inner[0].clone())
        }),
    },
    Rule {
        name: "root of a square is the absolute value",
        apply: |expression| match &**expression {
            ExprNode::Pow(base, exponent) if is_half(exponent) => match &**base {
                ExprNode::Pow(inner, power) if is_two(power) => {
                    Some(call("abs", inner.clone()))
                }
                _ => None,
            },
            _ => None,
        },
    },
    Rule {
        name: "absolute value of a number",
        apply: |expression| match_function(expression, "abs", |argument| {
            argument.as_number().map(|number| ExprNode::num(number.abs()))
        }),
    },
    Rule {
        name: "square of an absolute value",
        apply: |expression| match &**expression {
            ExprNode::Pow(base, exponent) if is_two(exponent) => match &**base {
                ExprNode::Fn(name, arguments) if same(name, "abs") && arguments.len() == 1 => {
                    Some(ExprNode::pow_raw(arguments[0].clone(), ExprNode::int(2)))
                }
                _ => None,
            },
            _ => None,
        },
    },
    Rule {
        name: "sine of zero",
        apply: |expression| trig_of_zero(expression, "sin", 0),
    },
    Rule {
        name: "cosine of zero",
        apply: |expression| trig_of_zero(expression, "cos", 1),
    },
    Rule {
        name: "tangent of zero",
        apply: |expression| trig_of_zero(expression, "tan", 0),
    },
    Rule {
        name: "sine of pi",
        apply: |expression| trig_of_pi(expression, "sin", 0),
    },
    Rule {
        name: "cosine of pi",
        apply: |expression| trig_of_pi(expression, "cos", -1),
    },
    Rule {
        name: "sine squared plus cosine squared",
        apply: |expression| {
            let ExprNode::Add(terms) = &**expression else {
                return None;
            };
            for (index, term) in terms.iter().enumerate() {
                let Some((first, first_argument)) = square_of_call(term) else {
                    continue;
                };
                for (other_index, other) in terms.iter().enumerate() {
                    if other_index == index {
                        continue;
                    }
                    let Some((second, second_argument)) = square_of_call(other) else {
                        continue;
                    };
                    let paired = (same(&first, "sin") && same(&second, "cos"))
                        || (same(&first, "cos") && same(&second, "sin"));
                    if !paired || first_argument != second_argument {
                        continue;
                    }
                    let mut rest: Vec<Expr> = terms
                        .iter()
                        .enumerate()
                        .filter(|(position, _)| *position != index && *position != other_index)
                        .map(|(_, term)| term.clone())
                        .collect();
                    rest.push(ExprNode::int(1));
                    return Some(ExprNode::add_raw(rest));
                }
            }
            None
        },
    },
    Rule {
        name: "a square root times itself",
        apply: |expression| {
            let ExprNode::Mul(factors) = &**expression else {
                return None;
            };
            for (index, left) in factors.iter().enumerate() {
                let Some(left_argument) = square_root_argument(left) else {
                    continue;
                };
                for (other_index, right) in factors.iter().enumerate() {
                    if other_index <= index {
                        continue;
                    }
                    let Some(right_argument) = square_root_argument(right) else {
                        continue;
                    };
                    if left_argument != right_argument {
                        continue;
                    }
                    // √u · √u = u, and the rest of the product is untouched.
                    let mut rest: Vec<Expr> = factors
                        .iter()
                        .enumerate()
                        .filter(|(position, _)| *position != index && *position != other_index)
                        .map(|(_, factor)| factor.clone())
                        .collect();
                    rest.push(left_argument.clone());
                    return Some(ExprNode::mul_raw(rest));
                }
            }
            None
        },
    },
    Rule {
        name: "trigonometric value at a standard angle",
        apply: |expression| {
            for name in ["sin", "cos", "tan"] {
                if let Some(value) = trig_exact(expression, name) {
                    return Some(value);
                }
            }
            None
        },
    },
];

/// The u of √u, when the factor is a square root.
fn square_root_argument(expression: &Expr) -> Option<&Expr> {
    let ExprNode::Pow(base, exponent) = &**expression else {
        return None;
    };
    is_half(exponent).then_some(base)
}

/// Apply `rule` to a call of `name` with exactly one argument.
fn match_function(
    expression: &Expr,
    name: &str,
    rule: impl FnOnce(&Expr) -> Option<Expr>,
) -> Option<Expr> {
    let ExprNode::Fn(function, arguments) = &**expression else {
        return None;
    };
    if !same(function, name) || arguments.len() != 1 {
        return None;
    }
    rule(&arguments[0])
}

/// The arguments of `expression` when it is a call of `name`.
fn inner_call<'a>(expression: &'a Expr, name: &str) -> Option<&'a [Expr]> {
    match &**expression {
        ExprNode::Fn(function, arguments) if same(function, name) => Some(arguments),
        _ => None,
    }
}

/// `(name, argument)` when `term` is `name(argument)²`.
fn square_of_call(term: &Expr) -> Option<(SymbolId, &Expr)> {
    let ExprNode::Pow(base, exponent) = &**term else {
        return None;
    };
    if !is_two(exponent) {
        return None;
    }
    let ExprNode::Fn(name, arguments) = &**base else {
        return None;
    };
    if arguments.len() != 1 {
        return None;
    }
    Some((*name, &arguments[0]))
}

fn trig_of_zero(expression: &Expr, name: &str, exact: i64) -> Option<Expr> {
    match_function(expression, name, |argument| {
        is_zero(argument).then(|| ExprNode::int(exact))
    })
}

fn trig_of_pi(expression: &Expr, name: &str, exact: i64) -> Option<Expr> {
    match_function(expression, name, |argument| {
        is_symbol(argument, "pi").then(|| ExprNode::int(exact))
    })
}

/// A rational multiple of π, when the argument is one.
///
/// `π/6` reaches the rule as `Mul([Rat(1,6), π])`; `π` alone as the symbol. Any
/// other shape is not a standard angle and the rule declines.
fn pi_multiple(expression: &Expr) -> Option<Number> {
    match &**expression {
        ExprNode::Sym(symbol) if same(symbol, "pi") => Some(Number::one()),
        ExprNode::Mul(factors) => {
            let mut multiplier = Number::one();
            let mut pies = 0;
            for factor in factors {
                match &**factor {
                    ExprNode::Sym(symbol) if same(symbol, "pi") => pies += 1,
                    _ => {
                        multiplier = multiplier.mul(factor.as_number()?);
                    }
                }
            }
            (pies == 1).then_some(multiplier)
        }
        _ => None,
    }
}

/// `sin`, `cos` or `tan` at a standard angle, exactly.
///
/// The values a school exercise is expected to know: the first-quadrant angles,
/// the axes, and `π`. Anything else — `π/7`, `2π/3` — is left to the numeric
/// path rather than given a decimal dressed up as an exact answer.
fn trig_exact(expression: &Expr, name: &str) -> Option<Expr> {
    let ExprNode::Fn(function, arguments) = &**expression else {
        return None;
    };
    if !same(function, name) || arguments.len() != 1 {
        return None;
    }
    let multiple = pi_multiple(&arguments[0])?;
    let key = (name, multiple.clone());
    let value = match key {
        _ if name == "sin" && is_ratio(&multiple, 1, 6) => Some(ratio_half()),
        _ if name == "sin" && is_ratio(&multiple, 1, 4) => Some(root_over(2, 2)),
        _ if name == "sin" && is_ratio(&multiple, 1, 3) => Some(root_over(3, 2)),
        _ if name == "sin" && is_ratio(&multiple, 1, 2) => Some(ExprNode::int(1)),
        _ if name == "cos" && is_ratio(&multiple, 1, 6) => Some(root_over(3, 2)),
        _ if name == "cos" && is_ratio(&multiple, 1, 4) => Some(root_over(2, 2)),
        _ if name == "cos" && is_ratio(&multiple, 1, 3) => Some(ratio_half()),
        _ if name == "cos" && is_ratio(&multiple, 1, 2) => Some(ExprNode::int(0)),
        _ if name == "tan" && is_ratio(&multiple, 1, 6) => Some(root_over(3, 3)),
        _ if name == "tan" && is_ratio(&multiple, 1, 4) => Some(ExprNode::int(1)),
        _ if name == "tan" && is_ratio(&multiple, 1, 3) => {
            Some(ExprNode::pow_raw(ExprNode::int(3), ExprNode::num(Number::ratio(1, 2))))
        }
        _ => None,
    };
    value
}

fn is_ratio(number: &Number, numerator: i128, denominator: i128) -> bool {
    matches!(number, Number::Rat(n, d) if *n == numerator && *d == denominator)
}

fn ratio_half() -> Expr {
    ExprNode::num(Number::ratio(1, 2))
}

/// `√root / denominator`.
fn root_over(root: i64, denominator: i64) -> Expr {
    ExprNode::mul_raw(vec![
        ExprNode::pow_raw(ExprNode::int(root), ExprNode::num(Number::ratio(1, 2))),
        ExprNode::num(Number::ratio(1, denominator)),
    ])
}


fn call(name: &str, argument: Expr) -> Expr {
    ExprNode::fn_raw(SymbolId::intern(name), vec![argument])
}

fn same(symbol: &SymbolId, name: &str) -> bool {
    symbol.as_str().as_str() == name
}

fn is_zero(expression: &Expr) -> bool {
    expression.is_zero()
}

fn is_one(expression: &Expr) -> bool {
    matches!(expression.as_number(), Some(number) if number.is_one())
}

fn is_two(expression: &Expr) -> bool {
    matches!(expression.as_number(), Some(Number::Int(2)))
}

fn is_half(expression: &Expr) -> bool {
    matches!(expression.as_number(), Some(Number::Rat(1, 2)))
}

fn is_symbol(expression: &Expr, name: &str) -> bool {
    matches!(&**expression, ExprNode::Sym(symbol) if same(symbol, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Hold;

    #[test]
    fn a_hold_costs_more_than_anything_that_can_be_computed() {
        let held = ExprNode::hold(Hold::Integral {
            body: ExprNode::int(1),
            var: SymbolId::intern("x"),
        });
        assert!(cost(&held) > 1000);
    }

    #[test]
    fn a_rule_set_that_never_matches_returns_the_input() {
        static NONE: &[Rule] = &[Rule {
            name: "never",
            apply: |_| None,
        }];
        let expression = ExprNode::int(1);
        assert_eq!(simplify_with(&expression, NONE, 8), normalize(&expression));
    }
}
