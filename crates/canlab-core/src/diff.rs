//! Symbolic differentiation.
//!
//! The rules every calculus course uses: sums, the product rule, the power rule,
//! the chain rule for the functions this CAS knows, and the general
//! `base^exponent` rule for a variable in either place. A derivative of order
//! `n` is this applied `n` times — which is exactly what `f''` means, so higher
//! derivatives cost nothing extra here.
//!
//! What is *not* handled is a function this crate does not know how to
//! differentiate (`\arcsin`, say). Those return `None` rather than a wrong
//! answer; the caller decides whether that means "declined".

use crate::expr::{Expr, ExprNode};
use crate::normalize::{add, mul, pow};
use crate::symbol::SymbolId;

/// `d/dvar` of `expression`, or `None` when a rule is missing.
pub fn differentiate(expression: &Expr, var: SymbolId) -> Option<Expr> {
    match &**expression {
        ExprNode::Num(_) => Some(ExprNode::int(0)),

        ExprNode::Sym(symbol) => Some(ExprNode::int(if *symbol == var { 1 } else { 0 })),

        ExprNode::Add(terms) => {
            let derived: Option<Vec<Expr>> = terms
                .iter()
                .map(|term| differentiate(term, var))
                .collect();
            Some(add(derived?))
        }

        ExprNode::Mul(factors) => {
            // Product rule, n-ary: sum over each factor of its derivative times
            // the others. Written for n factors rather than two because `Mul` is
            // flattened.
            let mut terms = Vec::with_capacity(factors.len());
            for (index, factor) in factors.iter().enumerate() {
                let derived = differentiate(factor, var)?;
                if matches!(&*derived, ExprNode::Num(n) if n.is_zero()) {
                    continue;
                }
                let mut product: Vec<Expr> = Vec::with_capacity(factors.len());
                product.push(derived);
                for (other_index, other) in factors.iter().enumerate() {
                    if other_index != index {
                        product.push(other.clone());
                    }
                }
                terms.push(mul(product));
            }
            Some(if terms.is_empty() {
                ExprNode::int(0)
            } else {
                add(terms)
            })
        }

        ExprNode::Pow(base, exponent) => {
            let base_has_var = mentions(base, var);
            let exponent_has_var = mentions(exponent, var);
            match (base_has_var, exponent_has_var) {
                // A constant: the power rule.
                (true, false) => {
                    let inner = differentiate(base, var)?;
                    Some(mul([
                        exponent.clone(),
                        pow(base.clone(), add(vec![exponent.clone(), ExprNode::int(-1)])),
                        inner,
                    ]))
                }
                // An exponential: `a^u` is `a^u · ln a · u'`.
                (false, true) => {
                    let inner = differentiate(exponent, var)?;
                    Some(mul([
                        expression.clone(),
                        ExprNode::fn_raw(SymbolId::intern("ln"), vec![base.clone()]),
                        inner,
                    ]))
                }
                // Both: the general rule.
                (true, true) => {
                    let base_inner = differentiate(base, var)?;
                    let exponent_inner = differentiate(exponent, var)?;
                    let logarithm =
                        ExprNode::fn_raw(SymbolId::intern("ln"), vec![base.clone()]);
                    let first = mul([exponent_inner, logarithm]);
                    let second = mul([
                        exponent.clone(),
                        base_inner,
                        pow(base.clone(), ExprNode::int(-1)),
                    ]);
                    Some(mul([expression.clone(), add(vec![first, second])]))
                }
                (false, false) => Some(ExprNode::int(0)),
            }
        }

        ExprNode::Fn(name, args) => {
            // Only single-argument functions are differentiated. Everything else
            // is a rule this file does not have.
            let [argument] = args.as_slice() else {
                return None;
            };
            let inner = differentiate(argument, var)?;
            let name = name.as_str();
            let outer = match name.as_str() {
                "sin" => ExprNode::fn_raw(SymbolId::intern("cos"), vec![argument.clone()]),
                "cos" => mul([
                    ExprNode::int(-1),
                    ExprNode::fn_raw(SymbolId::intern("sin"), vec![argument.clone()]),
                ]),
                "tan" => pow(
                    ExprNode::fn_raw(SymbolId::intern("cos"), vec![argument.clone()]),
                    ExprNode::int(-2),
                ),
                "exp" => ExprNode::fn_raw(SymbolId::intern("exp"), vec![argument.clone()]),
                "ln" => pow(argument.clone(), ExprNode::int(-1)),
                _ => return None,
            };
            Some(mul([outer, inner]))
        }

        ExprNode::Hold(_) => None,
    }
}

/// `dⁿ/dvarⁿ`, by applying [`differentiate`] `n` times.
pub fn differentiate_n(expression: &Expr, var: SymbolId, order: u32) -> Option<Expr> {
    let mut current = expression.clone();
    for _ in 0..order {
        current = differentiate(&current, var)?;
    }
    Some(current)
}

/// Does `expression` mention `var` anywhere?
fn mentions(expression: &Expr, var: SymbolId) -> bool {
    match &**expression {
        ExprNode::Num(_) => false,
        ExprNode::Sym(symbol) => *symbol == var,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            terms.iter().any(|term| mentions(term, var))
        }
        ExprNode::Pow(base, exponent) => mentions(base, var) || mentions(exponent, var),
        ExprNode::Fn(_, args) => args.iter().any(|arg| mentions(arg, var)),
        ExprNode::Hold(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{int, sym};

    fn x() -> Expr {
        sym("x")
    }

    fn dx(expression: Expr) -> String {
        differentiate(&expression, SymbolId::intern("x"))
            .expect("differentiable")
            .to_string()
    }

    #[test]
    fn the_power_rule() {
        assert_eq!(dx(pow(x(), int(2))), "2*x");
        assert_eq!(dx(pow(x(), int(3))), "3*x^2");
        assert_eq!(dx(x()), "1");
    }

    #[test]
    fn constants_and_other_variables_die() {
        assert_eq!(dx(int(7)), "0");
        assert_eq!(dx(sym("y")), "0");
    }

    #[test]
    fn sums_differentiate_term_by_term() {
        let expression = add(vec![pow(x(), int(2)), mul(vec![int(3), x()]), int(4)]);
        assert_eq!(dx(expression), "3 + 2*x");
    }

    #[test]
    fn the_product_rule() {
        // x · x = x², so the derivative is 2x.
        assert_eq!(dx(mul(vec![x(), x()])), "2*x");
        // 3x² → 6x, through the coefficient as well.
        assert_eq!(dx(mul(vec![int(3), pow(x(), int(2))])), "6*x");
    }

    #[test]
    fn the_chain_rule_through_a_power() {
        // (x² + 1)³ → 3(x² + 1)² · 2x
        let inner = add(vec![pow(x(), int(2)), int(1)]);
        let derived = differentiate(&pow(inner, int(3)), SymbolId::intern("x")).expect("chain");
        assert!(derived.to_string().contains("6*x"), "got {derived}");
    }

    #[test]
    fn the_trig_and_log_rules() {
        let sin = ExprNode::fn_raw(SymbolId::intern("sin"), vec![x()]);
        assert_eq!(
            differentiate(&sin, SymbolId::intern("x")).unwrap().to_string(),
            "cos(x)"
        );
        let ln = ExprNode::fn_raw(SymbolId::intern("ln"), vec![x()]);
        assert_eq!(
            differentiate(&ln, SymbolId::intern("x")).unwrap().to_string(),
            "x^-1"
        );
    }

    #[test]
    fn the_general_exponential_rule() {
        // x^x → x^x (ln x + 1)
        let expression = pow(x(), x());
        let derived = differentiate(&expression, SymbolId::intern("x")).expect("general rule");
        assert!(derived.to_string().contains("ln"), "got {derived}");
    }

    #[test]
    fn higher_orders_repeat_the_rule() {
        // d²/dx² x³ = 6x, d³/dx³ x³ = 6, d⁴/dx⁴ x³ = 0
        let cubic = pow(x(), int(3));
        let var = SymbolId::intern("x");
        assert_eq!(differentiate_n(&cubic, var, 2).unwrap().to_string(), "6*x");
        assert_eq!(differentiate_n(&cubic, var, 3).unwrap().to_string(), "6");
        assert_eq!(differentiate_n(&cubic, var, 4).unwrap().to_string(), "0");
    }

    #[test]
    fn an_unknown_function_is_declined_not_guessed() {
        let arcsin = ExprNode::fn_raw(SymbolId::intern("arcsin"), vec![x()]);
        assert!(differentiate(&arcsin, SymbolId::intern("x")).is_none());
    }
}
