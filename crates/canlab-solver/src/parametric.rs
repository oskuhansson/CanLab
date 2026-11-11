//! Solving one equation for one variable, with every other symbol left standing.
//!
//! The numeric solver answers `x² = 4` with `±2`. This answers `x² = y` with
//! `±√y` — the same quadratic formula, applied where the coefficients are
//! expressions instead of numbers. It is what the editor's autofill needs: type
//! `x =` in a document that contains `x² + y² = 1` and the answer is
//! `±√(1 − y²)`, not a refusal about two variables.
//!
//! # What it will not do
//!
//! Degrees above two, and anything that is not a polynomial in the variable, are
//! `None`. A square root of a symbolic discriminant is left as a square root:
//! the sign of `1 − y²` depends on `y`, so a solver that "simplified" `√(y²)` to
//! `y` would be wrong half the time.

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, mul, pow};
use canlab_poly::Poly;

/// Solve `equation = 0` for `variable`, leaving the other symbols as parameters.
pub fn solve_parametric(equation: &Expr, variable: SymbolId) -> Option<Vec<Expr>> {
    let vars = vec![variable];
    let polynomial = Poly::<Expr>::from_expr(equation, &vars)?;
    let coefficients = polynomial.univariate_coefficients()?;
    let degree = coefficients.keys().copied().max().unwrap_or(0);
    let coefficient = |power: u32| {
        coefficients
            .get(&power)
            .cloned()
            .unwrap_or_else(|| ExprNode::int(0))
    };

    match degree {
        0 => None,
        1 => {
            // `b·x + c = 0` gives `x = -c/b`. A zero `b` is the degenerate case
            // the numeric path already reports as a contradiction.
            let b = coefficient(1);
            if b.is_zero() {
                return None;
            }
            Some(vec![divide(&negate(&coefficient(0)), &b)])
        }
        2 => {
            let a = coefficient(2);
            let b = coefficient(1);
            let c = coefficient(0);
            // `b² − 4ac`, then pull a rational square out of it so that
            // `4 − 4y²` becomes `2√(1 − y²)` instead of `√(4 − 4y²)`.
            let discriminant = add(vec![
                mul(vec![b.clone(), b.clone()]),
                mul(vec![ExprNode::int(-4), a.clone(), c]),
            ]);
            let root = sqrt_of(&discriminant);
            let denominator = mul(vec![ExprNode::int(2), a]);
            Some(vec![
                divide(&add(vec![negate(&b), root.clone()]), &denominator),
                divide(&add(vec![negate(&b), negate(&root)]), &denominator),
            ])
        }
        _ => None,
    }
}

fn negate(expression: &Expr) -> Expr {
    mul(vec![ExprNode::int(-1), expression.clone()])
}

/// `left / right`, as an expression rather than as a division node.
fn divide(left: &Expr, right: &Expr) -> Expr {
    mul(vec![left.clone(), pow(right.clone(), ExprNode::int(-1))])
}

/// `√expression`, with any rational square factor taken out of it.
///
/// `√(4 − 4y²)` is `2√(1 − y²)`, and that is the form a reader expects to see.
/// Only the numeric content is extracted: pulling a square out of a symbolic
/// factor needs a sign assumption that this solver refuses to make.
fn sqrt_of(expression: &Expr) -> Expr {
    let variables = canlab_poly::free_symbols(expression);
    let Some(poly) = Poly::<Number>::from_expr(expression, &variables) else {
        return pow(expression.clone(), ratio_half());
    };
    let Some(content) = integer_content(&poly) else {
        return pow(expression.clone(), ratio_half());
    };
    let Some(root) = exact_sqrt(&content) else {
        return pow(expression.clone(), ratio_half());
    };
    if root == Number::one() {
        return pow(expression.clone(), ratio_half());
    }
    // Divide the *polynomial* by the square, not the expression: scaling every
    // coefficient is exact and keeps the result a polynomial, while dividing the
    // printed expression leaves `(1/4)·(4 − 4y²)` sitting under a square root.
    let Number::Int(content) = content else {
        return pow(expression.clone(), ratio_half());
    };
    let reduced = poly.scale(&Number::ratio(1, content)).to_expr();
    mul(vec![ExprNode::num(root), pow(reduced, ratio_half())])
}

fn ratio_half() -> Expr {
    canlab_core::ratio(1, 2)
}

/// The greatest positive integer that divides every coefficient, when all of them
/// are integers.
fn integer_content(poly: &Poly<Number>) -> Option<Number> {
    let mut content: i128 = 0;
    for (_, coefficient) in poly.terms() {
        let value = match coefficient {
            Number::Int(value) => *value,
            _ => return None,
        };
        content = gcd(content, value);
    }
    if content == 0 {
        None
    } else {
        Some(Number::int(content))
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

/// The exact square root of a perfect square, if it is one.
fn exact_sqrt(value: &Number) -> Option<Number> {
    let Number::Int(value) = value else {
        return None;
    };
    if *value <= 0 {
        return None;
    }
    let mut candidate: i128 = 1;
    while candidate * candidate < *value {
        candidate += 1;
    }
    if candidate * candidate == *value {
        Some(Number::int(candidate))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{Program, int, sym};

    fn x() -> SymbolId {
        SymbolId::intern("x")
    }

    /// The numeric value of `expression` with every symbol replaced by `values`.
    fn value(expression: &Expr, substitutions: &[(SymbolId, Number)]) -> f64 {
        let mut reduced = expression.clone();
        for (symbol, number) in substitutions {
            reduced = crate::system::substitute(&reduced, *symbol, &ExprNode::num(number.clone()));
        }
        let program = Program::compile(&reduced, &[]).expect("numeric after substitution");
        program.eval(&[])
    }

    #[test]
    fn a_linear_equation_answers_with_a_parameter() {
        // `2x + y = 3` gives `x = (3 - y)/2`.
        let equation = add(vec![mul(vec![int(2), sym("x")]), sym("y"), int(-3)]);
        let solutions = solve_parametric(&equation, x()).expect("linear");
        assert_eq!(solutions.len(), 1);
        assert!((value(&solutions[0], &[(SymbolId::intern("y"), Number::int(1))]) - 1.0).abs() < 1e-12);
        assert!((value(&solutions[0], &[(SymbolId::intern("y"), Number::int(5))]) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_circle_answers_with_two_branches() {
        // `x² + y² = 1` gives `x = ±√(1 − y²)`.
        let equation = add(vec![
            pow(sym("x"), int(2)),
            pow(sym("y"), int(2)),
            int(-1),
        ]);
        let solutions = solve_parametric(&equation, x()).expect("quadratic");
        assert_eq!(solutions.len(), 2, "a circle has two branches in x");
        let y = SymbolId::intern("y");
        let plus = value(&solutions[0], &[(y, Number::ratio(3, 5))]);
        let minus = value(&solutions[1], &[(y, Number::ratio(3, 5))]);
        assert!((plus - 0.8).abs() < 1e-12, "plus branch: {plus}");
        assert!((minus + 0.8).abs() < 1e-12, "minus branch: {minus}");
    }

    #[test]
    fn a_symbolic_leading_coefficient_stays_symbolic() {
        // `a·x = 1` gives `x = 1/a`, with `a` untouched.
        let equation = add(vec![mul(vec![sym("a"), sym("x")]), int(-1)]);
        let solutions = solve_parametric(&equation, x()).expect("linear");
        assert_eq!(solutions.len(), 1);
        let a = SymbolId::intern("a");
        assert!((value(&solutions[0], &[(a, Number::int(4))]) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn the_content_of_a_numeric_polynomial_is_its_integer_gcd() {
        // The piece that turns `√(4 - 4y²)` into `2√(1 - y²)`, tested where it
        // is exact rather than through a rendering.
        let y = vec![SymbolId::intern("y")];
        let expression = add(vec![int(4), mul(vec![int(-4), pow(sym("y"), int(2))])]);
        let poly = Poly::<Number>::from_expr(&expression, &y).expect("a polynomial");
        assert_eq!(integer_content(&poly), Some(Number::int(4)));
        assert_eq!(exact_sqrt(&Number::int(4)), Some(Number::int(2)));
        assert_eq!(exact_sqrt(&Number::int(8)), None);
        assert_eq!(exact_sqrt(&Number::int(-4)), None);
    }

    #[test]
    fn a_cubic_is_refused_rather_than_half_solved() {
        let equation = add(vec![pow(sym("x"), int(3)), int(-1)]);
        assert!(solve_parametric(&equation, x()).is_none());
    }

    #[test]
    fn a_transcendental_equation_is_refused() {
        let sine = ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]);
        assert!(solve_parametric(&sine, x()).is_none());
    }
}
