//! Solving a *system*, not one equation at a time.
//!
//! `x = 1, y = x` has one answer in x and one in y, and solving the first
//! equation alone would silently drop `y`. Two methods cover the school cases:
//!
//! 1. **Substitution.** Any equation with a single unknown can be solved, and its
//!    value substituted into the others. Repeat. This handles triangular systems
//!    like the one above, and works for quadratics too.
//! 2. **Elimination.** If what is left is two linear equations in two unknowns,
//!    solve them exactly with determinants.
//!
//! Anything else — three unknowns, a non-linear remainder — is declined rather
//! than half-answered.

use std::collections::BTreeSet;

use canlab_core::{Expr, ExprNode, Number, SymbolId, add, mul};

use crate::{SolverError, native::NativeSolver};

/// Every variable the expression mentions.
pub fn variables_of(expression: &Expr) -> BTreeSet<SymbolId> {
    let mut out = BTreeSet::new();
    collect(expression, &mut out);
    out
}

fn collect(expression: &Expr, out: &mut BTreeSet<SymbolId>) {
    match &**expression {
        ExprNode::Num(_) => {}
        ExprNode::Sym(symbol) => {
            out.insert(*symbol);
        }
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            for term in terms {
                collect(term, out);
            }
        }
        ExprNode::Pow(base, exponent) => {
            collect(base, out);
            collect(exponent, out);
        }
        ExprNode::Fn(_, args) => {
            for arg in args {
                collect(arg, out);
            }
        }
        ExprNode::Hold(hold) => match &**hold {
            canlab_core::Hold::Integral { body, var } => {
                collect(body, out);
                out.insert(*var);
            }
        },
    }
}

/// Replace every occurrence of `var` with `value`.
pub fn substitute(expression: &Expr, var: SymbolId, value: &Expr) -> Expr {
    match &**expression {
        ExprNode::Num(_) => expression.clone(),
        ExprNode::Sym(symbol) if *symbol == var => value.clone(),
        ExprNode::Sym(_) => expression.clone(),
        ExprNode::Add(terms) => {
            let mapped: Vec<Expr> = terms.iter().map(|t| substitute(t, var, value)).collect();
            add(mapped)
        }
        ExprNode::Mul(factors) => {
            let mapped: Vec<Expr> = factors.iter().map(|f| substitute(f, var, value)).collect();
            mul(mapped)
        }
        ExprNode::Pow(base, exponent) => canlab_core::pow(
            substitute(base, var, value),
            substitute(exponent, var, value),
        ),
        ExprNode::Fn(_, _) | ExprNode::Hold(_) => expression.clone(),
    }
}

/// One assignment found by substitution.
type Assignments = Vec<(SymbolId, Expr)>;

/// Solve a system by the two methods in the module doc comment.
pub fn solve_system(
    solver: &NativeSolver,
    equations: &[Expr],
    vars: &[SymbolId],
) -> Result<Assignments, SolverError> {
    use crate::Solver as _;

    let mut remaining: Vec<Expr> = equations.to_vec();
    let mut assignments: Assignments = Vec::new();
    let mut unknown: Vec<SymbolId> = vars.to_vec();

    // Substitution: peel off equations that mention exactly one unknown.
    loop {
        let mut progress = false;
        for index in 0..remaining.len() {
            let mentioned: Vec<SymbolId> = variables_of(&remaining[index])
                .into_iter()
                .filter(|symbol| unknown.contains(symbol))
                .collect();
            if mentioned.len() != 1 {
                continue;
            }
            let variable = mentioned[0];
            let Ok(values) = solver.solve(&remaining[index], variable) else {
                continue;
            };
            // Exactly one value, or substitution would branch into cases this
            // backend has no way to express.
            if values.len() != 1 {
                continue;
            }
            let value = values[0].clone();
            unknown.retain(|symbol| *symbol != variable);
            assignments.push((variable, value.clone()));
            remaining.remove(index);
            for equation in &mut remaining {
                *equation = substitute(equation, variable, &value);
            }
            progress = true;
            break;
        }
        if !progress {
            break;
        }
    }

    if unknown.is_empty() {
        // Every equation must hold now; a leftover that does not is a
        // contradiction, which this backend does not report as a solution.
        for equation in &remaining {
            if !variables_of(equation).is_empty() {
                return Err(SolverError::NotImplemented);
            }
        }
        return Ok(assignments);
    }

    // Elimination, for the two-by-two linear remainder.
    if unknown.len() == 2 && remaining.len() == 2 {
        if let Some(more) = eliminate_two(&remaining, unknown[0], unknown[1])? {
            assignments.extend(more);
            return Ok(assignments);
        }
    }

    Err(SolverError::NotImplemented)
}

/// Coefficients of a linear expression: one per variable, plus the constant.
///
/// `None` when the expression is not linear in exactly those variables.
fn linear_form(
    expression: &Expr,
    vars: [SymbolId; 2],
) -> Option<(Number, Number, Number)> {
    let terms: Vec<Expr> = match &**expression {
        ExprNode::Add(terms) => terms.clone(),
        _ => vec![expression.clone()],
    };
    let mut left = Number::zero();
    let mut right = Number::zero();
    let mut constant = Number::zero();
    for term in terms {
        let factors: Vec<Expr> = match &*term {
            ExprNode::Mul(factors) => factors.clone(),
            _ => vec![term.clone()],
        };
        let mut coefficient = Number::one();
        let mut seen: Option<SymbolId> = None;
        for factor in factors {
            match &*factor {
                ExprNode::Num(number) => coefficient = coefficient.mul(number),
                ExprNode::Sym(symbol) => {
                    if seen.is_some() || !vars.contains(symbol) {
                        return None;
                    }
                    seen = Some(*symbol);
                }
                _ => return None,
            }
        }
        match seen {
            Some(symbol) if symbol == vars[0] => left = left.add(&coefficient),
            Some(symbol) if symbol == vars[1] => right = right.add(&coefficient),
            Some(_) => return None,
            None => constant = constant.add(&coefficient),
        }
    }
    Some((left, right, constant))
}

/// Cramer's rule for `a·x + b·y + c = 0` twice over.
fn eliminate_two(
    equations: &[Expr],
    first: SymbolId,
    second: SymbolId,
) -> Result<Option<Assignments>, SolverError> {
    let (a1, b1, c1) = match linear_form(&equations[0], [first, second]) {
        Some(form) => form,
        None => return Ok(None),
    };
    let (a2, b2, c2) = match linear_form(&equations[1], [first, second]) {
        Some(form) => form,
        None => return Ok(None),
    };
    let determinant = a1.mul(&b2).sub(&a2.mul(&b1));
    if determinant.is_zero() {
        // Parallel or identical lines: no unique answer to report.
        return Ok(None);
    }
    // `a·x + b·y = -c`, so the right-hand sides carry the sign.
    let d1 = c1.neg();
    let d2 = c2.neg();
    let x = d1.mul(&b2).sub(&d2.mul(&b1)).div(&determinant);
    let y = a1.mul(&d2).sub(&a2.mul(&d1)).div(&determinant);
    Ok(Some(vec![
        (first, ExprNode::num(x)),
        (second, ExprNode::num(y)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, mul, sym};
    use crate::Solver;

    fn x() -> Expr {
        sym("x")
    }

    fn y() -> Expr {
        sym("y")
    }

    #[test]
    fn substitution_solves_a_triangular_system() {
        // x = 1 and y = x. Solving the first alone would drop y entirely, which
        // is the bug this exists for.
        let first = canlab_core::add(vec![x(), int(-1)]);
        let second = canlab_core::add(vec![y(), mul(vec![int(-1), x()])]);
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        let assignments = NativeSolver.solve_system(&[first, second], &vars).expect("solvable");
        assert_eq!(assignments.len(), 2);
        let value = |name: &str| {
            assignments
                .iter()
                .find(|(var, _)| var.as_str() == name)
                .map(|(_, value)| value.to_string())
        };
        assert_eq!(value("x").as_deref(), Some("1"));
        assert_eq!(value("y").as_deref(), Some("1"));
    }

    #[test]
    fn elimination_solves_two_linear_equations() {
        // x + y = 3 and 2x - y = 0  =>  x = 1, y = 2
        let first = canlab_core::add(vec![x(), y(), int(-3)]);
        let second = canlab_core::add(vec![mul(vec![int(2), x()]), mul(vec![int(-1), y()])]);
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        let assignments = NativeSolver.solve_system(&[first, second], &vars).expect("solvable");
        let value = |name: &str| {
            assignments
                .iter()
                .find(|(var, _)| var.as_str() == name)
                .map(|(_, value)| value.to_string())
        };
        assert_eq!(value("x").as_deref(), Some("1"));
        assert_eq!(value("y").as_deref(), Some("2"));
    }

    #[test]
    fn a_system_it_cannot_solve_is_declined() {
        // Three unknowns, three quadratics: outside both methods.
        let equations = vec![
            canlab_core::add(vec![mul(vec![x(), y()]), int(-1)]),
            canlab_core::add(vec![mul(vec![y(), sym("z")]), int(-1)]),
            canlab_core::add(vec![mul(vec![x(), sym("z")]), int(-1)]),
        ];
        let vars = [
            SymbolId::intern("x"),
            SymbolId::intern("y"),
            SymbolId::intern("z"),
        ];
        assert_eq!(
            NativeSolver.solve_system(&equations, &vars),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn substitution_replaces_everywhere() {
        let expression = canlab_core::add(vec![x(), mul(vec![int(2), x()])]);
        let replaced = substitute(&expression, SymbolId::intern("x"), &int(5));
        assert_eq!(replaced.to_string(), "15");
    }
}