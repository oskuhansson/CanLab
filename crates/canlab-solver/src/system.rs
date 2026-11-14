//! Solving a *system*, not one equation at a time.
//!
//! `x = 1, y = x` has one answer in x and one in y, and solving the first
//! equation alone would silently drop `y`. Two methods cover the school cases:
//!
//! 1. **Substitution.** Any equation with a single unknown can be solved, and its
//!    value substituted into the others. Repeat. This handles triangular systems
//!    like the one above, and works for quadratics too.
//! 2. **Elimination.** What is left, if it is *linear* in its unknowns, goes into
//!    a matrix and is reduced by Gaussian elimination over the exact rationals.
//!    That is not special-cased to two unknowns: a 4×4 system reduces as happily
//!    as a 2×2 one, and a singular matrix is reported as such instead of producing
//!    a division by a determinant that happens to be zero.
//!
//! A linear system always has one of three answers — a point, no solution, or a
//! whole family — and all three are distinguished here. Anything non-linear that
//! substitution cannot peel apart is declined rather than half-answered.

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
            // An accent's body is an expression, so its symbols are the
            // document's symbols: `\vec{v}` mentions `v`.
            canlab_core::Hold::Accent { body, .. } => collect(body, out),
            canlab_core::Hold::Matrix { rows } => {
                for row in rows {
                    for cell in row {
                        collect(cell, out);
                    }
                }
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
        // Every equation must hold with the values just found. A leftover with no
        // variables is a number, and that number is either zero — the equation
        // holds — or not, which is a contradiction. Checking only "has no
        // variables" is how `x = 1, x = 2` used to return `x = 1`.
        for equation in &remaining {
            if !variables_of(equation).is_empty() {
                return Err(SolverError::NotImplemented);
            }
            match constant(equation) {
                Some(value) if value.is_zero() => {}
                Some(_) => return Ok(Vec::new()),
                // Not a number this backend can evaluate exactly — a `sin`, say.
                None => return Err(SolverError::NotImplemented),
            }
        }
        return Ok(assignments);
    }

    // Elimination, for whatever linear system is left.
    if !unknown.is_empty() && !remaining.is_empty() {
        match eliminate(&remaining, &unknown)? {
            Some(more) => {
                assignments.extend(more);
                return Ok(assignments);
            }
            None => {}
        }
    }

    Err(SolverError::NotImplemented)
}

/// The exact value of a constant expression, or `None` if it is not constant.
///
/// This is the same arithmetic the evaluator does, but over [`Number`] instead of
/// `f64`, so `1/3 + 1/6` is `1/2` and not `0.49999999999999994`. Gaussian
/// elimination divides, and a rounding error there becomes a wrong answer rather
/// than a slightly wrong one.
fn constant(expression: &Expr) -> Option<Number> {
    match &**expression {
        ExprNode::Num(number) => Some(number.clone()),
        ExprNode::Sym(symbol) => match symbol.as_str().as_str() {
            "pi" => Some(Number::float(std::f64::consts::PI)),
            "tau" => Some(Number::float(std::f64::consts::TAU)),
            "e" => Some(Number::float(std::f64::consts::E)),
            _ => None,
        },
        ExprNode::Add(terms) => {
            let mut total = Number::zero();
            for term in terms {
                total = total.add(&constant(term)?);
            }
            Some(total)
        }
        ExprNode::Mul(factors) => {
            let mut product = Number::one();
            for factor in factors {
                product = product.mul(&constant(factor)?);
            }
            Some(product)
        }
        ExprNode::Pow(base, exponent) => constant(base)?.pow(&constant(exponent)?),
        ExprNode::Fn(_, _) | ExprNode::Hold(_) => None,
    }
}

/// Read `expression` as `c + Σ aᵢ·varᵢ`: the constant and one coefficient per
/// variable, both exact.
///
/// The test for linearity is the second derivative: if `∂e/∂vᵢ` still mentions any
/// of the variables, the expression is not affine in them. That is not a
/// heuristic — it is the definition, checked symbolically.
fn affine(expression: &Expr, vars: &[SymbolId]) -> Option<(Number, Vec<Number>)> {
    let mut coefficients = Vec::with_capacity(vars.len());
    for var in vars {
        let derivative = canlab_core::differentiate(expression, *var)?;
        let mentioned = variables_of(&derivative);
        if mentioned.iter().any(|symbol| vars.contains(symbol)) {
            return None;
        }
        coefficients.push(constant(&derivative)?);
    }
    let mut constant_term = expression.clone();
    for var in vars {
        constant_term = substitute(&constant_term, *var, &ExprNode::int(0));
    }
    Some((constant(&constant_term)?, coefficients))
}

/// One row of the augmented matrix: the coefficients, then the right-hand side.
type MatrixRow = Vec<Number>;

/// What a reduced linear system says.
enum Reduced {
    /// Exactly one point.
    Unique(Vec<Number>),
    /// No point satisfies every equation.
    Contradictory,
    /// A whole family of points: fewer independent equations than unknowns.
    Underdetermined,
}

/// Gaussian elimination, exactly, with no special case for the size.
fn reduce(rows: &mut [MatrixRow], columns: usize) -> Reduced {
    let mut pivot_row = 0usize;
    for column in 0..columns {
        // Partial pivoting: any non-zero entry will do, because the arithmetic is
        // exact. Taking the first keeps the result deterministic.
        let Some(found) = (pivot_row..rows.len()).find(|row| !rows[*row][column].is_zero()) else {
            continue;
        };
        rows.swap(pivot_row, found);

        let pivot = rows[pivot_row][column].clone();
        for entry in rows[pivot_row].iter_mut() {
            *entry = entry.div(&pivot);
        }
        let normalised = rows[pivot_row].clone();
        for (index, row) in rows.iter_mut().enumerate() {
            if index == pivot_row || row[column].is_zero() {
                continue;
            }
            let factor = row[column].clone();
            for (slot, pivot_entry) in row.iter_mut().zip(&normalised) {
                *slot = slot.sub(&factor.mul(pivot_entry));
            }
        }
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }

    // A row of zero coefficients with a non-zero right-hand side is `0 = c`.
    for row in rows.iter() {
        let (coefficients, right) = row.split_at(columns);
        if coefficients.iter().all(Number::is_zero) && !right[0].is_zero() {
            return Reduced::Contradictory;
        }
    }
    if pivot_row < columns {
        return Reduced::Underdetermined;
    }
    let mut values = Vec::with_capacity(columns);
    for row in rows.iter().take(columns) {
        values.push(row[columns].clone());
    }
    Reduced::Unique(values)
}

/// Solve the linear part of a system by elimination.
///
/// `Ok(None)` means "this is not a linear system", which is not a failure: the
/// caller declines with its own reason. `Ok(Some(vec![]))` means the equations
/// contradict each other.
fn eliminate(equations: &[Expr], unknown: &[SymbolId]) -> Result<Option<Assignments>, SolverError> {
    // Only the unknowns that actually appear; an irrelevant variable must not make
    // the matrix wider than it needs to be.
    let mut mentioned: Vec<SymbolId> = Vec::new();
    for equation in equations {
        for variable in variables_of(equation) {
            if unknown.contains(&variable) && !mentioned.contains(&variable) {
                mentioned.push(variable);
            }
        }
    }
    if mentioned.is_empty() {
        return Ok(None);
    }

    let mut rows: Vec<MatrixRow> = Vec::new();
    for equation in equations {
        let Some((constant_term, coefficients)) = affine(equation, &mentioned) else {
            return Ok(None);
        };
        let mut row: MatrixRow = coefficients;
        // `c + Σ aᵢxᵢ = 0` is `Σ aᵢxᵢ = -c`.
        row.push(constant_term.neg());
        rows.push(row);
    }

    // A square system has as many equations as unknowns. Extra equations are
    // redundant information, not a reason to refuse: elimination copes with a
    // rectangular matrix, and a contradictory row is detected above.
    match reduce(&mut rows, mentioned.len()) {
        Reduced::Unique(values) => Ok(Some(
            mentioned
                .iter()
                .zip(values)
                .map(|(variable, value)| (*variable, ExprNode::num(value)))
                .collect(),
        )),
        Reduced::Contradictory => Ok(Some(Vec::new())),
        Reduced::Underdetermined => Err(SolverError::NotImplemented),
    }
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

    /// `x + y + z = 6`, `x − y = 0`, `y − z = 0`  =>  all three are 2.
    fn three_by_three() -> (Vec<Expr>, Vec<SymbolId>) {
        let equations = vec![
            canlab_core::add(vec![x(), y(), sym("z"), int(-6)]),
            canlab_core::add(vec![x(), mul(vec![int(-1), y()])]),
            canlab_core::add(vec![y(), mul(vec![int(-1), sym("z")])]),
        ];
        let vars = vec![
            SymbolId::intern("x"),
            SymbolId::intern("y"),
            SymbolId::intern("z"),
        ];
        (equations, vars)
    }

    #[test]
    fn elimination_is_not_special_cased_to_two_unknowns() {
        // The 2x2 case was a determinant; this is the same code path at 3x3.
        let (equations, vars) = three_by_three();
        let assignments = NativeSolver
            .solve_system(&equations, &vars)
            .expect("a 3x3 linear system is solvable");
        assert_eq!(assignments.len(), 3, "{assignments:?}");
        for (variable, value) in assignments {
            assert_eq!(value.to_string(), "2", "{}", variable.as_str());
        }
    }

    #[test]
    fn elimination_keeps_fractions_exact() {
        // 2x + y = 1, x + 3y = 2  =>  x = 1/5, y = 3/5. In floating point this is
        // where a "did it work?" check gets fuzzy; here it is exact.
        let equations = vec![
            canlab_core::add(vec![mul(vec![int(2), x()]), y(), int(-1)]),
            canlab_core::add(vec![x(), mul(vec![int(3), y()]), int(-2)]),
        ];
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        let assignments = NativeSolver.solve_system(&equations, &vars).expect("solvable");
        let value = |name: &str| {
            assignments
                .iter()
                .find(|(var, _)| var.as_str() == name)
                .map(|(_, value)| value.to_string())
        };
        assert_eq!(value("x").as_deref(), Some("1/5"));
        assert_eq!(value("y").as_deref(), Some("3/5"));
    }

    #[test]
    fn a_contradiction_is_an_empty_answer_not_a_decline() {
        // x + y = 1 and x + y = 2 cannot both hold. That is an answer — "no
        // solutions" — and returning it as a decline would be a different claim.
        let equations = vec![
            canlab_core::add(vec![x(), y(), int(-1)]),
            canlab_core::add(vec![x(), y(), int(-2)]),
        ];
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        assert_eq!(
            NativeSolver.solve_system(&equations, &vars),
            Ok(Vec::new())
        );
    }

    #[test]
    fn a_family_of_solutions_is_declined_rather_than_narrowed() {
        // One equation, two unknowns: an infinite set of points. Reporting one of
        // them would be a lie, so this is a decline.
        let equations = vec![canlab_core::add(vec![x(), y(), int(-1)])];
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        assert_eq!(
            NativeSolver.solve_system(&equations, &vars),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn a_four_by_four_system_solves() {
        // Each equation gives one unknown away to substitution only if it is
        // alone; here none are, so all four go through the matrix.
        // a+b+c+d=10, a-b=0, b-c=0, c-d=0  =>  all 2.5
        let (a, b, c, d) = (sym("a"), sym("b"), sym("c"), sym("d"));
        let negative = |e: Expr| mul(vec![int(-1), e]);
        let equations = vec![
            canlab_core::add(vec![a.clone(), b.clone(), c.clone(), d.clone(), int(-10)]),
            canlab_core::add(vec![a.clone(), negative(b.clone())]),
            canlab_core::add(vec![b.clone(), negative(c.clone())]),
            canlab_core::add(vec![c.clone(), negative(d.clone())]),
        ];
        let vars = [
            SymbolId::intern("a"),
            SymbolId::intern("b"),
            SymbolId::intern("c"),
            SymbolId::intern("d"),
        ];
        let assignments = NativeSolver.solve_system(&equations, &vars).expect("solvable");
        assert_eq!(assignments.len(), 4);
        for (variable, value) in assignments {
            assert_eq!(value.to_string(), "5/2", "{}", variable.as_str());
        }
    }

    #[test]
    fn a_contradiction_reached_through_substitution_is_not_a_solution() {
        // `x = 1` and `x = 2` both have a single unknown, so substitution takes
        // both and no matrix is ever built. The check for a leftover equation
        // used to ask only "has it any variables?" — so this returned `x = 1`.
        let equations = vec![
            canlab_core::add(vec![x(), int(-1)]),
            canlab_core::add(vec![x(), int(-2)]),
        ];
        let vars = [SymbolId::intern("x")];
        assert_eq!(NativeSolver.solve_system(&equations, &vars), Ok(Vec::new()));
    }

    #[test]
    fn a_contradiction_in_an_overdetermined_system_is_found_too() {
        // Substitution peels `2x = 5` first, then `x + y = 3`, and the third
        // equation only reveals the contradiction once it is a number.
        let equations = vec![
            canlab_core::add(vec![mul(vec![int(2), x()]), int(-5)]),
            canlab_core::add(vec![x(), y(), int(-3)]),
            canlab_core::add(vec![x(), mul(vec![int(-1), y()]), int(-1)]),
        ];
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        assert_eq!(NativeSolver.solve_system(&equations, &vars), Ok(Vec::new()));
    }

    #[test]
    fn a_leftover_that_does_hold_is_a_solution() {
        // The same overdetermined shape as the contradiction above, but
        // consistent: 2x = 4, x + y = 3, x − y = 1 is x = 2, y = 1.
        let equations = vec![
            canlab_core::add(vec![mul(vec![int(2), x()]), int(-4)]),
            canlab_core::add(vec![x(), y(), int(-3)]),
            canlab_core::add(vec![x(), mul(vec![int(-1), y()]), int(-1)]),
        ];
        let vars = [SymbolId::intern("x"), SymbolId::intern("y")];
        let assignments = NativeSolver
            .solve_system(&equations, &vars)
            .expect("a consistent overdetermined system");
        assert_eq!(assignments.len(), 2, "{assignments:?}");
        for (variable, value) in assignments {
            let expected = if variable.as_str() == "x" { "2" } else { "1" };
            assert_eq!(value.to_string(), expected, "{}", variable.as_str());
        }
    }

    #[test]
    fn a_leftover_with_a_function_is_declined_not_guessed() {
        // `cos x` at the found value is not a number this backend can evaluate
        // exactly, so the honest answer is a decline, not "0 = cos 1".
        let cosine = canlab_core::ExprNode::fn_raw(SymbolId::intern("cos"), vec![x()]);
        let equations = vec![
            canlab_core::add(vec![x(), int(-1)]),
            canlab_core::add(vec![cosine, canlab_core::ratio(-1, 2)]),
        ];
        let vars = [SymbolId::intern("x")];
        assert_eq!(
            NativeSolver.solve_system(&equations, &vars),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn a_constant_expression_is_read_exactly() {
        // `pi` has no exact rational value, but it is a number, not a symbol.
        assert!(constant(&sym("pi")).is_some());
        assert!(constant(&sym("x")).is_none());
        let third = mul(vec![canlab_core::ratio(1, 3), int(3)]);
        assert_eq!(constant(&third).map(|n| n.to_string()).as_deref(), Some("1"));
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