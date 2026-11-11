//! The properties the plan asks of a polynomial layer, plus the refusals.
//!
//! The randomised cases compare against a slow reference: a polynomial as a flat
//! list of terms multiplied out by hand. A fast implementation and a slow one
//! that share no code can only agree if both are right.

use canlab_core::{Expr, Number, SymbolId, expr::node, int, sym};
use canlab_poly::{Coeff, Monomial, Poly};

fn vars(names: &[&str]) -> Vec<SymbolId> {
    names.iter().map(|name| SymbolId::intern(name)).collect()
}

/// The zero polynomial in x and y.
fn zero2() -> Poly<Number> {
    Poly::zero(vars(&["x", "y"]))
}

/// `c·x^a·y^b` as a polynomial in x and y.
fn term2(c: i128, a: u32, b: u32) -> Poly<Number> {
    let mut poly = zero2();
    poly.push(Monomial(vec![a, b]), Number::int(c));
    poly
}

/// A slow reference: multiply out two lists of (exponent vector, coefficient).
fn reference_multiply(
    left: &[(Vec<u32>, Number)],
    right: &[(Vec<u32>, Number)],
) -> Vec<(Vec<u32>, Number)> {
    let mut out: Vec<(Vec<u32>, Number)> = Vec::new();
    for (left_exponents, left_coefficient) in left {
        for (right_exponents, right_coefficient) in right {
            let exponents: Vec<u32> = left_exponents
                .iter()
                .zip(right_exponents)
                .map(|(a, b)| a + b)
                .collect();
            let coefficient = left_coefficient.mul(right_coefficient);
            match out.iter_mut().find(|(seen, _)| *seen == exponents) {
                Some((_, existing)) => *existing = existing.add(&coefficient),
                None => out.push((exponents, coefficient)),
            }
        }
    }
    out.retain(|(_, coefficient)| !coefficient.is_zero());
    out
}

/// The non-zero terms of a polynomial as the reference wants them.
fn as_reference(poly: &Poly<Number>) -> Vec<(Vec<u32>, Number)> {
    poly.terms()
        .map(|(monomial, coefficient)| (monomial.0.clone(), coefficient.clone()))
        .collect()
}

#[test]
fn multiplication_agrees_with_a_slow_reference() {
    // A deterministic pseudo-random stream, so a failure is reproducible from
    // the seed rather than from luck.
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for case in 0..200 {
        let mut left = zero2();
        let mut right = zero2();
        for _ in 0..=(next() % 4) {
            left.push(
                Monomial(vec![(next() % 3) as u32, (next() % 3) as u32]),
                Number::int((next() % 11) as i128 - 5),
            );
        }
        for _ in 0..=(next() % 4) {
            right.push(
                Monomial(vec![(next() % 3) as u32, (next() % 3) as u32]),
                Number::int((next() % 11) as i128 - 5),
            );
        }
        let product = left.mul(&right).expect("same variables");
        let expected = reference_multiply(&as_reference(&left), &as_reference(&right));
        let mut actual = as_reference(&product);
        let mut expected_sorted = expected.clone();
        actual.sort_by(|a, b| a.0.cmp(&b.0));
        expected_sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(actual, expected_sorted, "case {case}");
    }
}

#[test]
fn a_polynomial_round_trips_through_an_expression() {
    let x = SymbolId::intern("x");
    let y = SymbolId::intern("y");
    let poly = term2(3, 2, 1)
        .add(&term2(-2, 0, 1))
        .and_then(|sum| sum.add(&term2(5, 0, 0)))
        .expect("same variables");
    let expression = poly.to_expr();
    let back = Poly::<Number>::from_expr(&expression, &vars(&["x", "y"])).expect("a polynomial");
    assert_eq!(poly, back, "expression: {expression:?}");
    // And it prints the way the printer would print it.
    assert_eq!(
        canlab_core::normalize(&expression),
        expression,
        "to_expr must already be canonical"
    );
    let _ = (x, y);
}

#[test]
fn a_function_call_is_not_a_polynomial() {
    let expression = canlab_core::expr::ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]);
    assert!(Poly::<Number>::from_expr(&expression, &vars(&["x"])).is_none());
}

#[test]
fn a_negative_power_is_refused() {
    let expression = canlab_core::pow(sym("x"), int(-2));
    assert!(Poly::<Number>::from_expr(&expression, &vars(&["x"])).is_none());
}

#[test]
fn a_fractional_power_is_refused() {
    let expression = canlab_core::pow(sym("x"), canlab_core::ratio(1, 2));
    assert!(Poly::<Number>::from_expr(&expression, &vars(&["x"])).is_none());
}

#[test]
fn a_variable_outside_the_list_is_refused() {
    let expression = canlab_core::add(vec![sym("x"), sym("z")]);
    assert!(Poly::<Number>::from_expr(&expression, &vars(&["x"])).is_none());
}

#[test]
fn division_reconstructs_the_dividend() {
    let x = vars(&["x"]);
    let divisor = Poly::<Number>::from_expr(
        &canlab_core::add(vec![sym("x"), int(-2)]),
        &x,
    )
    .expect("x - 2");
    let dividend = Poly::<Number>::from_expr(
        &canlab_core::mul(vec![
            canlab_core::add(vec![sym("x"), int(-2)]),
            canlab_core::add(vec![sym("x"), int(3)]),
        ]),
        &x,
    )
    .expect("(x - 2)(x + 3)");
    let (quotient, remainder) = dividend.div_rem(&divisor).expect("exact division");
    assert!(remainder.is_zero(), "remainder: {remainder:?}");
    let rebuilt = quotient
        .mul(&divisor)
        .and_then(|product| product.add(&remainder))
        .expect("same variables");
    assert_eq!(dividend, rebuilt);
}

#[test]
fn dividing_by_zero_is_refused() {
    let x = vars(&["x"]);
    let dividend = Poly::<Number>::from_expr(&sym("x"), &x).expect("x");
    let zero = Poly::<Number>::zero(x);
    assert!(dividend.div_rem(&zero).is_none());
}

#[test]
fn the_gcd_of_two_products_divides_both() {
    let x = vars(&["x"]);
    let make = |terms: &[i64]| {
        let mut expression = int(1);
        for term in terms {
            expression = canlab_core::mul(vec![expression, canlab_core::add(vec![sym("x"), int(*term)])]);
        }
        Poly::<Number>::from_expr(&expression, &x).expect("a product of linear factors")
    };
    // p, q and r are pairwise coprime, so the common factor of `pq` and `pr` is
    // `p` itself and nothing else. (A first version of this test gave every
    // factor the same `(x + 1)`, and the gcd it produced — `(x+1)²(x+2)` — was
    // right while the assertion was wrong.)
    let p = make(&[1, 2]);
    let q = make(&[3, 4]);
    let r = make(&[5, 6]);
    let pq = p.mul(&q).expect("same variables");
    let pr = p.mul(&r).expect("same variables");
    let gcd = pq.gcd(&pr).expect("a common factor");
    for factor in [&pq, &pr] {
        let (_, remainder) = factor.div_rem(&gcd).expect("divisible");
        assert!(remainder.is_zero(), "the gcd must divide both");
    }
    // The gcd is p up to a constant, and monic, so it *is* p here.
    assert_eq!(gcd, p, "gcd: {gcd:?}");
}

#[test]
fn the_resultant_vanishes_exactly_when_roots_are_shared() {
    let x = vars(&["x"]);
    let make = |roots: &[i64]| {
        let mut expression = int(1);
        for root in roots {
            expression = canlab_core::mul(vec![expression, canlab_core::add(vec![sym("x"), int(-*root)])]);
        }
        Poly::<Number>::from_expr(&expression, &x).expect("a product of linear factors")
    };
    let a = make(&[1, 2]);
    let b = make(&[2, 3]);
    let c = make(&[3, 4]);
    assert!(a.resultant(&b).expect("same variables").is_zero());
    assert!(!a.resultant(&c).expect("same variables").is_zero());
}

#[test]
fn rational_roots_are_found_exactly() {
    let x = vars(&["x"]);
    let poly = Poly::<Number>::from_expr(
        &canlab_core::mul(vec![
            canlab_core::add(vec![sym("x"), int(-1)]),
            canlab_core::add(vec![sym("x"), int(-3)]),
        ]),
        &x,
    )
    .expect("(x-1)(x-3)");
    let mut roots: Vec<f64> = poly
        .rational_roots()
        .expect("a univariate polynomial")
        .iter()
        .map(Number::to_f64)
        .collect();
    roots.sort_by(f64::total_cmp);
    assert_eq!(roots, vec![1.0, 3.0]);
}

#[test]
fn a_multivariate_gcd_is_refused_rather_than_guessed() {
    let a = term2(1, 1, 0);
    let b = term2(1, 0, 1);
    assert!(a.gcd(&b).is_none(), "multivariate gcd is not implemented");
}

#[test]
fn an_expression_coefficient_ring_works_too() {
    // This is the case parametric solving needs: coefficients that are
    // themselves expressions in the other variables.
    let x = vars(&["x"]);
    let y = sym("y");
    let mut poly: Poly<Expr> = Poly::zero(x.clone());
    poly.push(Monomial(vec![2]), Expr::from_number(&Number::one()).expect("one"));
    poly.push(
        Monomial(vec![0]),
        canlab_core::add(vec![canlab_core::mul(vec![y.clone(), y]), int(-1)]),
    );
    let printed = poly.to_expr();
    // x² + y² - 1, in some order.
    let text = format!("{printed:?}");
    assert!(text.contains("Pow"), "expected a square in {text}");
    let _ = node(&printed);
}
