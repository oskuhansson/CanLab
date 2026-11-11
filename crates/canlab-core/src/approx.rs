//! The decimal beside an exact answer.
//!
//! An exam answer is often read as a decimal — `1/3 = 0.333`, `√2 = 1.414` — and
//! a CAS that prints only the exact form leaves the reader to do arithmetic it
//! already did. This module produces the decimal, and does it in the one way that
//! cannot lie: it evaluates the expression only when it has no free variables, and
//! it never replaces the exact form with the decimal.

use crate::expr::{Expr, ExprNode};
use crate::number::Number;
use crate::symbol::SymbolId;
use crate::eval::Program;

/// The value of an expression with no free variables.
///
/// `None` when there is a variable, when the expression cannot be evaluated
/// numerically (a held integral), or when the result is not finite. A parametric
/// answer has no single value, and this says so rather than evaluating at 1.
pub fn value_of(expression: &Expr) -> Option<f64> {
    let mut symbols = Vec::new();
    free_symbols(expression, &mut symbols);
    if symbols.iter().any(|symbol| !is_constant(*symbol)) {
        return None;
    }
    let program = Program::compile(expression, &[])?;
    let value = program.eval(&[]);
    value.is_finite().then_some(value)
}

/// `pi`, `e` and `tau` are constants the evaluator folds; anything else is a
/// variable.
fn is_constant(symbol: SymbolId) -> bool {
    matches!(symbol.as_str().as_str(), "pi" | "e" | "tau")
}

fn free_symbols(expression: &Expr, out: &mut Vec<SymbolId>) {
    match &**expression {
        ExprNode::Sym(symbol) => {
            if !out.contains(symbol) {
                out.push(*symbol);
            }
        }
        ExprNode::Num(_) => {}
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            for term in terms {
                free_symbols(term, out);
            }
        }
        ExprNode::Pow(base, exponent) => {
            free_symbols(base, out);
            free_symbols(exponent, out);
        }
        ExprNode::Fn(_, args) => {
            for argument in args {
                free_symbols(argument, out);
            }
        }
        ExprNode::Hold(_) => {}
    }
}

/// A decimal with `digits` significant figures, trailing zeros trimmed.
///
/// Three significant figures is the school convention, and this is what that
/// looks like: `2/3 → "0.667"`, `√2 → "1.41"`, `12.0 → "12"`.
pub fn significant(value: f64, digits: u32) -> String {
    if !value.is_finite() {
        return String::from("—");
    }
    if value == 0.0 {
        return String::from("0");
    }
    let digits = digits.max(1) as i32;
    let exponent = value.abs().log10().floor() as i32;
    let decimals = (digits - 1 - exponent).max(0) as usize;
    let text = format!("{value:.decimals$}");
    let trimmed = match text.split_once('.') {
        Some((whole, fraction)) => {
            let fraction = fraction.trim_end_matches('0');
            if fraction.is_empty() {
                String::from(whole)
            } else {
                format!("{whole}.{fraction}")
            }
        }
        None => text,
    };
    // `-0` is a formatting artefact, not a value.
    if trimmed == "-0" {
        String::from("0")
    } else {
        trimmed
    }
}

/// The decimal for an expression, when it has one, at the given precision.
pub fn decimal_of(expression: &Expr, digits: u32) -> Option<String> {
    value_of(expression).map(|value| significant(value, digits))
}

/// Whether the exact form is already a decimal — an integer or a rational that
/// prints exactly — in which case a second, approximate form would be noise.
pub fn is_exact_decimal(expression: &Expr) -> bool {
    matches!(expression.as_number(), Some(Number::Int(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{int, ratio, sym};

    #[test]
    fn three_significant_figures_is_the_school_convention() {
        assert_eq!(significant(2.0 / 3.0, 3), "0.667");
        assert_eq!(significant(2.0_f64.sqrt(), 3), "1.41");
        assert_eq!(significant(12.0, 3), "12");
        assert_eq!(significant(0.0012345, 3), "0.00123");
        assert_eq!(significant(0.0, 3), "0");
        assert_eq!(significant(-0.0001, 3), "-0.0001");
    }

    #[test]
    fn a_constant_expression_has_a_value() {
        let expression = crate::pow(int(2), ratio(1, 2));
        let value = value_of(&expression).expect("√2 is a number");
        assert!((value - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(decimal_of(&expression, 3).as_deref(), Some("1.41"));
    }

    #[test]
    fn an_expression_with_a_variable_has_no_single_value() {
        let expression = crate::add(vec![sym("x"), int(1)]);
        assert!(value_of(&expression).is_none(), "x is not a number");
    }

    #[test]
    fn a_held_integral_has_no_value() {
        let expression = ExprNode::hold(crate::expr::Hold::Integral {
            body: crate::pow(sym("x"), int(2)),
            var: sym("x").as_symbol().expect("a symbol"),
        });
        assert!(value_of(&expression).is_none());
    }

    #[test]
    fn an_integer_needs_no_second_form() {
        assert!(is_exact_decimal(&int(7)));
        assert!(!is_exact_decimal(&ratio(1, 3)));
    }
}
