//! Root finding for a [`canlab_core::Expr`], through the compiled evaluator.
//!
//! This is the bridge the solver and the future integration work (W3.1) need:
//! the numerical crate itself knows nothing about expressions, and the
//! expression side knows nothing about brackets. `Program::compile` is the one
//! place that turns a tree into a function of `x`, and it already refuses the
//! constructs that have no numerical value. Reusing it keeps this crate from
//! growing a second partial evaluator.
//!
//! A held integral, a second variable, or any function the evaluator does not
//! implement is [`RootError::UnsupportedExpression`] — a refusal, not a guess.

use canlab_core::{Expr, Program, SymbolId};

use crate::{DEFAULT_GRID, Root, RootError, scan_with_grid};

/// All roots of `expression` in `interval`, treating `var` as the variable.
///
/// This is [`crate::scan`] with the evaluation step supplied by
/// [`Program::compile`]. As with `scan`, the interval may be reversed and a
/// touching root is found by minimising `|f|` inside a grid cell.
pub fn scan_expression(
    expression: &Expr,
    var: SymbolId,
    interval: (f64, f64),
    tolerance: f64,
) -> Result<Vec<Root>, RootError> {
    scan_expression_with_grid(expression, var, interval, tolerance, DEFAULT_GRID)
}

/// [`scan_expression`] with an explicit number of grid cells.
pub fn scan_expression_with_grid(
    expression: &Expr,
    var: SymbolId,
    interval: (f64, f64),
    tolerance: f64,
    grid: usize,
) -> Result<Vec<Root>, RootError> {
    let program = Program::compile(expression, &[var]).ok_or(RootError::UnsupportedExpression)?;
    scan_with_grid(|x| program.eval(&[x]), interval, tolerance, grid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{ExprNode, Hold, add, int, mul, sym};

    fn x() -> Expr {
        sym("x")
    }

    #[test]
    fn an_expression_is_scanned_through_the_compiled_program() {
        let expression = mul([add([x(), int(2)]), add([x(), int(-1)]), add([x(), int(-3)])]);
        let roots = scan_expression(&expression, SymbolId::intern("x"), (-4.0, 4.0), 1e-12)
            .expect("the expression compiles");
        assert_eq!(roots.len(), 3, "found {roots:?}");
        for (root, expected) in roots.iter().zip([-2.0, 1.0, 3.0]) {
            assert!(
                (root.value - expected).abs() <= 1e-8,
                "{} is not {expected}",
                root.value
            );
            assert!(root.residual <= 1e-12, "{root:?}");
        }
    }

    #[test]
    fn an_expression_the_evaluator_cannot_compile_is_refused() {
        let expression = ExprNode::hold(Hold::Integral {
            body: x(),
            var: SymbolId::intern("x"),
        });
        let error = scan_expression(&expression, SymbolId::intern("x"), (-1.0, 1.0), 1e-12)
            .expect_err("a held integral has no numerical value");
        assert_eq!(error, RootError::UnsupportedExpression);
        assert!(error.to_string().contains("cannot be compiled"));
    }
}
