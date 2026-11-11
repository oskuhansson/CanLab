//! Every shipped rule, checked numerically.
//!
//! A rewrite rule is the one kind of code where being wrong is invisible: the
//! result still looks like an answer. So each rule is exercised as an
//! *expression that contains it*, simplified, and compared with the original at
//! twenty random points — the simplified form must agree with the input wherever
//! both are defined.

use canlab_core::rewrite::{cost, simplify, simplify_with, Rule};
use canlab_core::{Expr, Program, SymbolId, add, expr::ExprNode, int, mul, pow, ratio, sym};

/// A deterministic stream, so a failure is reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A value in `range`, never exactly at an endpoint.
    fn value(&mut self, low: f64, high: f64) -> f64 {
        let unit = (self.next() % 10_000) as f64 / 10_000.0;
        low + (high - low) * (0.05 + 0.9 * unit)
    }
}

fn call(name: &str, argument: Expr) -> Expr {
    ExprNode::fn_raw(SymbolId::intern(name), vec![argument])
}

/// Compare `simplified` with `original` at twenty random points for each
/// variable, and require them to agree.
///
/// A `NaN` or a non-finite value on either side means that point is outside the
/// domain of one of them; those points are skipped rather than counted as a
/// failure, because `ln(-1)` is not an error in the rule, it is an error in the
/// sample.
fn agree(label: &str, original: &Expr, simplified: &Expr, ranges: &[(SymbolId, f64, f64)]) {
    let variables: Vec<SymbolId> = ranges.iter().map(|(variable, _, _)| *variable).collect();
    let before = Program::compile(original, &variables).expect("the original compiles");
    let after = Program::compile(simplified, &variables).expect("the simplification compiles");
    let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
    let mut compared = 0;
    for _ in 0..20 {
        let point: Vec<f64> = ranges
            .iter()
            .map(|(_, low, high)| rng.value(*low, *high))
            .collect();
        let left = before.eval(&point);
        let right = after.eval(&point);
        if !left.is_finite() || !right.is_finite() {
            continue;
        }
        let scale = left.abs().max(right.abs()).max(1.0);
        assert!(
            (left - right).abs() <= 1e-9 * scale,
            "{label}: at {point:?} the input is {left} and the rewrite is {right}"
        );
        compared += 1;
    }
    assert!(compared >= 10, "{label}: only {compared} points were comparable");
}

/// Simplify, and assert the result is smaller and agrees numerically.
fn check(label: &str, expression: Expr, ranges: &[(SymbolId, f64, f64)]) -> Expr {
    let simplified = simplify(&expression);
    assert!(
        cost(&simplified) < cost(&expression),
        "{label}: the rewrite did not make it smaller ({:?} -> {:?})",
        expression,
        simplified
    );
    agree(label, &expression, &simplified, ranges);
    simplified
}

fn no_variables() -> Vec<(SymbolId, f64, f64)> {
    Vec::new()
}

#[test]
fn the_logarithm_rules_agree_with_their_inputs() {
    let x = SymbolId::intern("x");
    let unit = vec![(x, 0.1, 3.0)];

    assert_eq!(check("ln of one", call("ln", int(1)), &no_variables()), int(0));
    assert_eq!(check("ln of e", call("ln", sym("e")), &no_variables()), int(1));
    assert_eq!(
        check("ln undoes exp", call("ln", call("exp", sym("x"))), &unit),
        sym("x")
    );
    assert_eq!(check("exp of zero", call("exp", int(0)), &no_variables()), int(1));
    assert_eq!(
        check("exp undoes ln", call("exp", call("ln", sym("x"))), &unit),
        sym("x")
    );
}

#[test]
fn the_root_and_absolute_value_rules_agree() {
    let x = SymbolId::intern("x");
    let both_signs = vec![(x, -3.0, 3.0)];
    let square_root = pow(pow(sym("x"), int(2)), ratio(1, 2));
    let simplified = check("root of a square", square_root, &both_signs);
    assert_eq!(simplified, call("abs", sym("x")), "not |x|");

    assert_eq!(check("absolute value of a number", call("abs", int(-3)), &no_variables()), int(3));
    check(
        "square of an absolute value",
        pow(call("abs", sym("x")), int(2)),
        &both_signs,
    );
}

#[test]
fn the_trigonometric_rules_agree() {
    assert_eq!(check("sine of zero", call("sin", int(0)), &no_variables()), int(0));
    assert_eq!(check("cosine of zero", call("cos", int(0)), &no_variables()), int(1));
    assert_eq!(check("tangent of zero", call("tan", int(0)), &no_variables()), int(0));
    assert_eq!(check("sine of pi", call("sin", sym("pi")), &no_variables()), int(0));
    assert_eq!(check("cosine of pi", call("cos", sym("pi")), &no_variables()), int(-1));
}

#[test]
fn the_pythagorean_identity_agrees_and_survives_other_terms() {
    let x = SymbolId::intern("x");
    let anywhere = vec![(x, -3.0, 3.0)];
    let identity = add(vec![
        pow(call("sin", sym("x")), int(2)),
        pow(call("cos", sym("x")), int(2)),
    ]);
    assert_eq!(check("sin² + cos²", identity, &anywhere), int(1));

    // Inside a larger sum, so the rule has to fire in the middle of an `Add`.
    let with_constant = add(vec![
        pow(call("sin", sym("x")), int(2)),
        pow(call("cos", sym("x")), int(2)),
        int(5),
    ]);
    let simplified = simplify(&with_constant);
    agree("sin² + cos² + 5", &with_constant, &simplified, &anywhere);
    assert_eq!(simplified, int(6), "the identity did not fire inside the sum");
}

#[test]
fn a_rule_that_would_grow_the_expression_is_not_applied() {
    // `ln e → 1` is a reduction; the reverse is not, and the engine must prefer
    // the number. An expression that is already a number stays one.
    assert_eq!(simplify(&int(7)), int(7));
    let nested = add(vec![call("ln", sym("e")), int(1)]);
    assert_eq!(simplify(&nested), int(2));
}

#[test]
fn the_walk_terminates_on_a_rule_set_with_a_cycle() {
    // `x + 0 → x` and `x → x + 0` together are a cycle. The cost function stops
    // it on the first step that would not shrink; the budget is the backstop.
    fn add_zero(expression: &Expr) -> Option<Expr> {
        Some(add(vec![expression.clone(), int(0)]))
    }
    static CYCLIC: &[Rule] = &[Rule {
        name: "grow by zero",
        apply: add_zero,
    }];
    let expression = sym("x");
    let simplified = simplify_with(&expression, CYCLIC, 4);
    // normalize drops `+ 0`, so the cycle is broken by canonicalization itself.
    assert_eq!(simplified, sym("x"));
}

#[test]
fn simplify_is_idempotent() {
    let expression = add(vec![
        call("ln", sym("e")),
        call("sin", int(0)),
        mul(vec![int(3), pow(call("abs", mul(vec![int(-1), sym("x")])), int(2))]),
    ]);
    let once = simplify(&expression);
    let twice = simplify(&once);
    assert_eq!(once, twice, "a second pass must not find anything to do");
}

#[test]
fn a_square_root_times_itself_is_the_number() {
    // √2 · √2 = 2. The rule matches the two factors structurally, so the rest
    // of the product is untouched.
    let root_two = pow(int(2), ratio(1, 2));
    let product = mul(vec![int(3), root_two.clone(), root_two]);
    let simplified = check("√2 · √2", product, &no_variables());
    assert_eq!(simplified, int(6), "3·√2·√2 is 6");
}

#[test]
fn the_standard_angles_are_exact() {
    let pi = sym("pi");
    let sixth = mul(vec![ratio(1, 6), pi.clone()]);
    let quarter = mul(vec![ratio(1, 4), pi.clone()]);
    let third = mul(vec![ratio(1, 3), pi.clone()]);
    let half_turn = mul(vec![ratio(1, 2), pi.clone()]);

    assert_eq!(simplify(&call("sin", sixth.clone())), ratio(1, 2));
    assert_eq!(simplify(&call("cos", third.clone())), ratio(1, 2));
    assert_eq!(simplify(&call("cos", half_turn)), int(0));
    assert_eq!(simplify(&call("tan", quarter.clone())), int(1));
    // √2/2 and √3/3 are equal in size to the sine and tangent they replace,
    // which is why the walk accepts a rewrite that does not shrink.
    assert_eq!(
        simplify(&call("sin", quarter)),
        mul(vec![pow(int(2), ratio(1, 2)), ratio(1, 2)])
    );
    assert_eq!(
        simplify(&call("tan", sixth)),
        mul(vec![pow(int(3), ratio(1, 2)), ratio(1, 3)])
    );

    // Numerically, against the true values.
    agree("sin π/6", &call("sin", mul(vec![ratio(1, 6), pi.clone()])), &ratio(1, 2), &no_variables());
}

#[test]
fn an_angle_that_is_not_standard_is_left_alone() {
    let argument = mul(vec![ratio(1, 7), sym("pi")]);
    let expression = call("sin", argument);
    assert_eq!(simplify(&expression), expression, "π/7 has no exact value here");
}