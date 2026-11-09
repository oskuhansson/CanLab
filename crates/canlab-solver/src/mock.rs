//! A backend with no backend behind it.
//!
//! The CLI, the UI and the snapshot tests need the full pipeline — trait
//! object, error handling, unevaluated-integral rendering — to run with no
//! Python installed. [`MockSolver`] provides exactly that, and almost nothing
//! else: it integrates `x^n` and declines everything else. That is deliberate.
//! A mock that pretends to be smart hides the bugs the real backend has, and a
//! mock that returns `NotImplemented` everywhere would never exercise the
//! success path. One honest trick is the useful middle.

use canlab_core::{Expr, ExprNode, Number, SymbolId, mul, pow};

use crate::{Solver, SolverError};

/// A solver that knows one integral and says so.
///
/// [`Solver::integrate`] handles `x^n` for integer `n >= 0` (and `x` itself,
/// which is `x^1`); every other operation returns
/// [`SolverError::NotImplemented`].
pub struct MockSolver;

/// The exponent `n` if `e` is exactly `var^n` with `n >= 0`.
fn exponent_of(e: &Expr, var: SymbolId) -> Option<i128> {
    match &**e {
        ExprNode::Sym(symbol) if *symbol == var => Some(1),
        ExprNode::Pow(base, exp) => match (&**base, &**exp) {
            (ExprNode::Sym(symbol), ExprNode::Num(Number::Int(n))) if *symbol == var && *n >= 0 => {
                Some(*n)
            }
            _ => None,
        },
        _ => None,
    }
}

impl Solver for MockSolver {
    fn simplify(&self, _e: &Expr) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }

    fn diff(&self, _e: &Expr, _var: SymbolId) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }

    fn integrate(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        let exponent = exponent_of(e, var).ok_or(SolverError::NotImplemented)?;
        let raised = exponent.checked_add(1).ok_or(SolverError::NotImplemented)?;
        // x^(n+1) / (n+1), built through the normalizing constructors so the
        // answer is canonical before anyone prints it.
        Ok(mul([
            pow(ExprNode::sym(var), ExprNode::num(Number::int(raised))),
            ExprNode::num(Number::ratio(1, raised)),
        ]))
    }

    fn solve(&self, _eq: &Expr, _var: SymbolId) -> Result<Vec<Expr>, SolverError> {
        Err(SolverError::NotImplemented)
    }

    fn limit(&self, _e: &Expr, _var: SymbolId, _at: &Expr) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }

    fn series(
        &self,
        _e: &Expr,
        _var: SymbolId,
        _at: &Expr,
        _order: usize,
    ) -> Result<Expr, SolverError> {
        Err(SolverError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, normalize, ratio, sym};
    use std::sync::Arc;

    fn x() -> Expr {
        sym("x")
    }

    #[test]
    fn integrates_x_cubed() {
        let antiderivative = MockSolver
            .integrate(&pow(x(), int(3)), SymbolId::intern("x"))
            .expect("x^3 is in the mock's repertoire");
        assert_eq!(antiderivative.to_string(), "1/4*x^4");
    }

    #[test]
    fn integrates_the_variable_itself() {
        let antiderivative = MockSolver
            .integrate(&x(), SymbolId::intern("x"))
            .expect("x is x^1");
        assert_eq!(antiderivative.to_string(), "1/2*x^2");
    }

    #[test]
    fn integrates_a_higher_power_in_another_variable() {
        let t = sym("t");
        let antiderivative = MockSolver
            .integrate(&pow(t, int(5)), SymbolId::intern("t"))
            .expect("t^5");
        assert_eq!(antiderivative.to_string(), "1/6*t^6");
    }

    #[test]
    fn declines_functions() {
        let e = normalize(&ExprNode::fn_raw(SymbolId::intern("sin"), vec![x()]));
        assert_eq!(
            MockSolver.integrate(&e, SymbolId::intern("x")),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn declines_negative_and_fractional_powers() {
        let reciprocal = pow(x(), int(-1));
        let root = pow(x(), ratio(1, 2));
        for e in [reciprocal, root] {
            assert_eq!(
                MockSolver.integrate(&e, SymbolId::intern("x")),
                Err(SolverError::NotImplemented)
            );
        }
    }

    #[test]
    fn declines_a_different_variable() {
        assert_eq!(
            MockSolver.integrate(&pow(x(), int(2)), SymbolId::intern("y")),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn declines_everything_else() {
        let x = x();
        let var = SymbolId::intern("x");
        assert_eq!(MockSolver.simplify(&x), Err(SolverError::NotImplemented));
        assert_eq!(MockSolver.diff(&x, var), Err(SolverError::NotImplemented));
        assert_eq!(MockSolver.solve(&x, var), Err(SolverError::NotImplemented));
        assert_eq!(
            MockSolver.limit(&x, var, &int(0)),
            Err(SolverError::NotImplemented)
        );
        assert_eq!(
            MockSolver.series(&x, var, &int(0), 4),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn works_behind_a_shared_trait_object() {
        let solver: Arc<dyn Solver> = Arc::new(MockSolver);
        let clone = Arc::clone(&solver);
        let handle =
            std::thread::spawn(move || clone.integrate(&pow(x(), int(2)), SymbolId::intern("x")));
        let result = handle.join().expect("thread finishes").expect("x^2");
        assert_eq!(result.to_string(), "1/3*x^3");
    }
}
