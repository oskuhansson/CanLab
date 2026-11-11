//! Expand, collect, divide, cancel and factor, on expressions rather than on
//! polynomials.
//!
//! The comparisons are structural: `expand` returns a canonical expression, so two
//! expanded forms of the same polynomial are `==` and a test needs no printer.

use canlab_core::{Expr, int, sym};
use canlab_poly::{FactorError, cancel, collect, divide, expand, factor};

/// `x + n` as an expression.
fn linear(n: i64) -> Expr {
    canlab_core::add(vec![sym("x"), int(n)])
}

#[test]
fn a_product_of_sums_expands() {
    let product = canlab_core::mul(vec![linear(1), linear(2)]);
    let expanded = expand(&product);
    let expected = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        canlab_core::mul(vec![int(3), sym("x")]),
        int(2),
    ]);
    assert_eq!(expanded, expected);
}

#[test]
fn a_square_expands() {
    let square = canlab_core::pow(linear(1), int(2));
    let expanded = expand(&square);
    let expected = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        canlab_core::mul(vec![int(2), sym("x")]),
        int(1),
    ]);
    assert_eq!(expanded, expected);
}

#[test]
fn a_function_survives_expansion() {
    // `(x + 1)·sin x` is not a polynomial, so it is distributed structurally and
    // the `sin` stays where it is.
    let sine = canlab_core::expr::ExprNode::fn_raw(canlab_core::SymbolId::intern("sin"), vec![sym("x")]);
    let product = canlab_core::mul(vec![linear(1), sine.clone()]);
    let expanded = expand(&product);
    let expected = canlab_core::add(vec![
        canlab_core::mul(vec![sym("x"), sine.clone()]),
        sine,
    ]);
    assert_eq!(expanded, expected);
}

#[test]
fn a_negative_power_is_not_expanded_into_a_polynomial() {
    let expression = canlab_core::pow(linear(1), int(-2));
    let expanded = expand(&expression);
    assert_eq!(expanded, expression, "an inverse is not a polynomial");
}

#[test]
fn collect_groups_by_one_variable_and_keeps_the_rest_as_coefficients() {
    let a = sym("a");
    let b = sym("b");
    let expression = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        canlab_core::mul(vec![a.clone(), sym("x")]),
        b.clone(),
    ]);
    let collected = collect(&expression, canlab_core::SymbolId::intern("x"));
    assert_eq!(collected, expression);
}

#[test]
fn division_returns_a_quotient_and_a_remainder() {
    let dividend = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        int(-1),
    ]);
    let (quotient, remainder) = divide(&dividend, &linear(-1), canlab_core::SymbolId::intern("x"))
        .expect("x² - 1 is divisible by x - 1");
    assert_eq!(remainder, int(0));
    assert_eq!(quotient, linear(1));
}

#[test]
fn cancelling_a_common_factor_leaves_the_rest() {
    let numerator = canlab_core::add(vec![canlab_core::pow(sym("x"), int(2)), int(-1)]);
    let denominator = linear(-1);
    let (top, bottom) =
        cancel(&numerator, &denominator, canlab_core::SymbolId::intern("x")).expect("cancellable");
    assert_eq!(top, linear(1));
    assert_eq!(bottom, int(1));
}

#[test]
fn a_quadratic_with_rational_roots_factors() {
    let quadratic = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        canlab_core::mul(vec![int(-3), sym("x")]),
        int(2),
    ]);
    let factored = factor(&quadratic).expect("(x-1)(x-2)");
    assert_eq!(expand(&factored), quadratic, "the factors must multiply back");
}

#[test]
fn factorisation_refuses_what_it_cannot_do() {
    let no_rational_roots = canlab_core::add(vec![
        canlab_core::pow(sym("x"), int(2)),
        int(1),
    ]);
    assert_eq!(factor(&no_rational_roots), Err(FactorError::NoFactors));

    let sine = canlab_core::expr::ExprNode::fn_raw(canlab_core::SymbolId::intern("sin"), vec![sym("x")]);
    assert_eq!(factor(&sine), Err(FactorError::NotAPolynomial));

    let two_variables = canlab_core::add(vec![sym("x"), sym("y")]);
    assert_eq!(factor(&two_variables), Err(FactorError::NotAPolynomial));
}

#[test]
fn cancelling_by_a_non_polynomial_is_refused() {
    let sine = canlab_core::expr::ExprNode::fn_raw(canlab_core::SymbolId::intern("sin"), vec![sym("x")]);
    assert!(cancel(&sine, &linear(1), canlab_core::SymbolId::intern("x")).is_none());
}
