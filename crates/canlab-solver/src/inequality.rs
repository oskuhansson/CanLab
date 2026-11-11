//! Solve an inequality with a sign chart over exact critical points.
//!
//! The comparison has to arrive here as a [`Relation`], not as a difference:
//! `|x − 1| < 2` and `|x − 1| > 2` are the same difference and different
//! answers. The chart is the standard school construction, with one deliberate
//! division of labour:
//!
//! * the **boundaries** are exact — roots of a numerator and poles of a
//!   denominator, found by the polynomial machinery the equation solver already
//!   has;
//! * the **sign between** two boundaries is sampled numerically at one interior
//!   point. The expression is continuous and has no root or pole inside such a
//!   segment, so one sample decides the whole segment.
//!
//! Absolute values are handled by enumerating their sign patterns: replacing
//! every `|u|` with `u` or `−u` turns the relation into a rational expression,
//! whose roots are exact. A candidate from a pattern that contradicts the sign
//! of `u` at that point is not a root of the original expression and is dropped
//! by checking the sign there. Four absolute values is the limit; beyond that
//! the enumeration stops being a small set of candidates and this backend
//! declines.

use canlab_core::{
    add, approx::value_of, mul, pow, Comparator, Expr, ExprNode, Number, Program, Relation,
    SymbolId,
};
use canlab_poly::{expand, Poly};

use crate::{Bound, NativeSolver, Solutions, Solver, SolverError};

/// How many absolute values one relation may contain. Four is sixteen sign
/// patterns; a school exercise with more is not a sign-chart exercise.
const MAX_ABSOLUTE_VALUES: usize = 4;

/// The largest integer power `fraction_form` will multiply out. A larger power
/// only turns a refusal into a slower refusal: the polynomial solver is not
/// going to finish that degree, and expanding it first is wasted work.
const MAX_POWER: u32 = 64;

/// The solution set of `relation` in `var`, as a union of intervals.
///
/// The comparison is normalised to `f > 0` or `f ≥ 0` once, at the top, so the
/// rest of the function only has to answer "where is `f` positive".
pub fn solve_relation(
    solver: &NativeSolver,
    relation: &Relation,
    var: SymbolId,
) -> Result<Solutions, SolverError> {
    let (difference, strict) = match relation.comparator {
        Comparator::Less => (difference(&relation.right, &relation.left), true),
        Comparator::LessOrEqual => (difference(&relation.right, &relation.left), false),
        Comparator::Greater => (difference(&relation.left, &relation.right), true),
        Comparator::GreaterOrEqual => (difference(&relation.left, &relation.right), false),
    };

    if difference.is_zero() {
        // Every point has `f = 0`, so a strict comparison is empty and a
        // non-strict one is the whole line. There is no boundary to chart, and
        // the answer type can say both without inventing an interval.
        return Ok(if strict {
            Solutions::None
        } else {
            Solutions::All
        });
    }

    let arguments = absolute_arguments(&difference);
    if arguments.len() > MAX_ABSOLUTE_VALUES {
        return Err(SolverError::NotImplemented);
    }
    // Compiled once: the sign check evaluates every absolute value's argument
    // at every candidate root, and a candidate is rejected or kept on that sign.
    let argument_programs: Vec<Program> = arguments
        .iter()
        .map(|argument| Program::compile(argument, &[var]))
        .collect::<Option<Vec<_>>>()
        .ok_or(SolverError::NotImplemented)?;

    let mut critical: Vec<Critical> = Vec::new();
    for pattern in 0..(1usize << arguments.len()) {
        let signs: Vec<i8> = (0..arguments.len())
            .map(|index| if pattern & (1 << index) == 0 { 1 } else { -1 })
            .collect();
        let mut cursor = 0usize;
        let substituted = substitute_absolute_values(&difference, &signs, &mut cursor);
        let (numerator, denominator) =
            fraction_form(&substituted, var).ok_or(SolverError::NotImplemented)?;
        let numerator =
            Poly::<Number>::from_expr(&expand(&numerator), &[var])
                .ok_or(SolverError::NotImplemented)?;
        let denominator =
            Poly::<Number>::from_expr(&expand(&denominator), &[var])
                .ok_or(SolverError::NotImplemented)?;
        if denominator.is_zero() {
            // The substituted expression divides by zero everywhere; there is
            // no function to chart.
            return Err(SolverError::NotImplemented);
        }

        // A zero numerator makes this pattern's equation true on a whole
        // region. Its isolated roots are the region's boundary, and continuity
        // puts those on another pattern's zero set; skipping the pattern here is
        // what keeps the critical set finite.
        if !numerator.is_zero() {
            for root in roots_of(solver, &numerator.to_expr(), var)? {
                if pattern_is_valid(&argument_programs, &signs, &root) {
                    push_critical(&mut critical, root, true, false)?;
                }
            }
        }
        for pole in roots_of(solver, &denominator.to_expr(), var)? {
            push_critical(&mut critical, pole, false, true)?;
        }
    }

    critical.sort_by(|left, right| left.number.total_cmp(&right.number));

    let program = Program::compile(&difference, &[var]).ok_or(SolverError::NotImplemented)?;
    let mut matched = Vec::with_capacity(critical.len() + 1);
    for segment in 0..=critical.len() {
        let probe = probe_point(&critical, segment);
        let value = program.eval(&[probe]);
        if !value.is_finite() {
            // The probe is between two boundaries, so a non-finite value is not
            // a pole of this expression; it is an evaluation this backend cannot
            // trust, and guessing the sign would be worse than declining.
            return Err(SolverError::NotImplemented);
        }
        // An interior probe that is exactly zero means the expression is zero
        // on the whole segment (a root would have split it). A strict
        // comparison excludes that segment, a non-strict one includes it.
        matched.push(if value > 0.0 {
            true
        } else if value == 0.0 {
            !strict
        } else {
            false
        });
    }

    Ok(Solutions::Inequality {
        var,
        intervals: intervals(&critical, &matched, strict),
    })
}

/// One boundary of the sign chart.
struct Critical {
    /// The exact value, which is what an endpoint is printed as.
    value: Expr,
    /// The same value as a float, for ordering and for probing between points.
    number: f64,
    /// The expression is zero here, so a non-strict answer may include it.
    is_root: bool,
    /// The expression has a pole here, so no answer may include it.
    is_pole: bool,
}

/// `left − right`, canonical.
fn difference(left: &Expr, right: &Expr) -> Expr {
    add([
        left.clone(),
        mul([ExprNode::int(-1), right.clone()]),
    ])
}

fn is_absolute(name: SymbolId) -> bool {
    name.as_str().as_str() == "abs"
}

/// Every `|u|` in the expression, outermost first, as the argument `u`.
///
/// The order is the order [`substitute_absolute_values`] assigns signs in; the
/// two functions walk the tree the same way, which is what keeps a sign attached
/// to the absolute value it was chosen for.
fn absolute_arguments(expression: &Expr) -> Vec<Expr> {
    fn walk(expression: &Expr, out: &mut Vec<Expr>) {
        match &**expression {
            ExprNode::Fn(name, arguments) if is_absolute(*name) && arguments.len() == 1 => {
                out.push(arguments[0].clone());
                walk(&arguments[0], out);
            }
            ExprNode::Add(terms) | ExprNode::Mul(terms) => {
                for term in terms {
                    walk(term, out);
                }
            }
            ExprNode::Pow(base, exponent) => {
                walk(base, out);
                walk(exponent, out);
            }
            ExprNode::Fn(_, arguments) => {
                for argument in arguments {
                    walk(argument, out);
                }
            }
            ExprNode::Num(_) | ExprNode::Sym(_) | ExprNode::Hold(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(expression, &mut out);
    out
}

/// Replace every `|u|` with `±u` according to `signs`.
fn substitute_absolute_values(expression: &Expr, signs: &[i8], cursor: &mut usize) -> Expr {
    match &**expression {
        ExprNode::Fn(name, arguments) if is_absolute(*name) && arguments.len() == 1 => {
            let index = *cursor;
            *cursor += 1;
            let inner = substitute_absolute_values(&arguments[0], signs, cursor);
            let sign = signs.get(index).copied().unwrap_or(1);
            mul([ExprNode::int(i64::from(sign)), inner])
        }
        ExprNode::Add(terms) => add(
            terms
                .iter()
                .map(|term| substitute_absolute_values(term, signs, cursor))
                .collect::<Vec<_>>(),
        ),
        ExprNode::Mul(factors) => mul(
            factors
                .iter()
                .map(|factor| substitute_absolute_values(factor, signs, cursor))
                .collect::<Vec<_>>(),
        ),
        ExprNode::Pow(base, exponent) => pow(
            substitute_absolute_values(base, signs, cursor),
            substitute_absolute_values(exponent, signs, cursor),
        ),
        ExprNode::Fn(name, arguments) => ExprNode::fn_raw(
            *name,
            arguments
                .iter()
                .map(|argument| substitute_absolute_values(argument, signs, cursor))
                .collect::<Vec<_>>(),
        ),
        ExprNode::Num(_) | ExprNode::Sym(_) | ExprNode::Hold(_) => expression.clone(),
    }
}

/// `expression` as `numerator / denominator`, both polynomial-shaped.
///
/// Only numbers, the variable and integer powers are accepted. A second symbol
/// or a function is a refusal: a polynomial with a symbolic coefficient has no
/// roots this backend can list, and `sin x < 0` is not a sign chart over
/// polynomial boundaries.
fn fraction_form(expression: &Expr, var: SymbolId) -> Option<(Expr, Expr)> {
    match &**expression {
        ExprNode::Num(number) => Some((ExprNode::num(number.clone()), ExprNode::int(1))),
        ExprNode::Sym(symbol) if *symbol == var => Some((expression.clone(), ExprNode::int(1))),
        ExprNode::Sym(_) => None,
        ExprNode::Add(terms) => {
            let mut numerator = ExprNode::int(0);
            let mut denominator = ExprNode::int(1);
            for term in terms {
                let (term_numerator, term_denominator) = fraction_form(term, var)?;
                numerator = add([
                    mul([numerator, term_denominator.clone()]),
                    mul([term_numerator, denominator.clone()]),
                ]);
                denominator = mul([denominator, term_denominator]);
            }
            Some((numerator, denominator))
        }
        ExprNode::Mul(factors) => {
            let mut numerator = ExprNode::int(1);
            let mut denominator = ExprNode::int(1);
            for factor in factors {
                let (factor_numerator, factor_denominator) = fraction_form(factor, var)?;
                numerator = mul([numerator, factor_numerator]);
                denominator = mul([denominator, factor_denominator]);
            }
            Some((numerator, denominator))
        }
        ExprNode::Pow(base, exponent) => {
            let Number::Int(power) = exponent.as_number()? else {
                return None;
            };
            let magnitude = u32::try_from(power.unsigned_abs()).ok()?;
            if magnitude > MAX_POWER {
                return None;
            }
            let negative = *power < 0;
            let (numerator, denominator) = fraction_form(base, var)?;
            let power = ExprNode::int(i64::from(magnitude));
            if negative {
                // `(n/d)^-k` is `(d/n)^k`: the denominator's roots become poles
                // of the whole expression, which is exactly what the chart needs
                // to know.
                Some((pow(denominator, power.clone()), pow(numerator, power)))
            } else {
                Some((pow(numerator, power.clone()), pow(denominator, power)))
            }
        }
        ExprNode::Fn(_, _) | ExprNode::Hold(_) => None,
    }
}

/// The exact real roots of a polynomial with numeric coefficients.
///
/// The polynomial solver is the one W2.3 built: rational roots, deflation and a
/// leftover linear or quadratic factor. A polynomial it cannot finish is a
/// refusal, not a partial list — a missed root would show up as a missing
/// interval boundary.
fn roots_of(
    solver: &NativeSolver,
    polynomial: &Expr,
    var: SymbolId,
) -> Result<Vec<Expr>, SolverError> {
    match solver.solve_all(polynomial, var)? {
        Solutions::Values(values) => Ok(values),
        Solutions::None => Ok(Vec::new()),
        // "Every number is a root" is not a finite set of boundaries. The
        // numerator case is handled before this is called; a zero denominator
        // is refused by the caller.
        _ => Err(SolverError::NotImplemented),
    }
}

/// Is this candidate a root of the *original* expression, not just of one sign
/// pattern?
///
/// A pattern is valid at the point when every absolute value's argument has the
/// sign that pattern assumed. At an argument of zero both signs are valid, so
/// the tolerance only has to absorb the rounding of evaluating the argument.
fn pattern_is_valid(programs: &[Program], signs: &[i8], root: &Expr) -> bool {
    let Some(value) = value_of(root) else {
        return false;
    };
    for (program, sign) in programs.iter().zip(signs) {
        let argument = program.eval(&[value]);
        if !argument.is_finite() {
            return false;
        }
        let tolerance = 1e-9 * argument.abs().max(1.0);
        let agrees = if *sign > 0 {
            argument >= -tolerance
        } else {
            argument <= tolerance
        };
        if !agrees {
            return false;
        }
    }
    true
}

/// Add one boundary, merging it with an equal one already found.
///
/// Two patterns can name the same point with differently shaped expressions, so
/// the comparison is on their numeric values to floating-point precision rather
/// than on the expressions themselves. A pole wins the stored expression
/// because it is the reason an endpoint is excluded.
fn push_critical(
    critical: &mut Vec<Critical>,
    value: Expr,
    is_root: bool,
    is_pole: bool,
) -> Result<(), SolverError> {
    let number = value_of(&value).ok_or(SolverError::NotImplemented)?;
    if !number.is_finite() {
        return Err(SolverError::NotImplemented);
    }
    if let Some(existing) = critical.iter_mut().find(|existing| {
        (existing.number - number).abs() <= 1e-12 * number.abs().max(1.0)
    }) {
        existing.is_root |= is_root;
        existing.is_pole |= is_pole;
        if is_pole {
            existing.value = value;
        }
        return Ok(());
    }
    critical.push(Critical {
        value,
        number,
        is_root,
        is_pole,
    });
    Ok(())
}

/// One point inside segment `segment` of the chart.
fn probe_point(critical: &[Critical], segment: usize) -> f64 {
    if critical.is_empty() {
        return 0.0;
    }
    if segment == 0 {
        let first = critical[0].number;
        return first - (1.0 + first.abs());
    }
    if segment >= critical.len() {
        let last = critical[critical.len() - 1].number;
        return last + (1.0 + last.abs());
    }
    let lower = critical[segment - 1].number;
    let upper = critical[segment].number;
    lower + (upper - lower) / 2.0
}

/// The union of the segments whose sign matched, with each endpoint's bracket.
fn intervals(critical: &[Critical], matched: &[bool], strict: bool) -> Vec<(Bound, Bound)> {
    let included = |index: usize| !strict && critical[index].is_root && !critical[index].is_pole;
    let endpoint = |index: usize| Bound::Finite {
        value: critical[index].value.clone(),
        open: !included(index),
    };

    // The atoms are already in order: a segment, then the boundary after it
    // (when a non-strict answer includes that boundary), then the next segment.
    let mut atoms: Vec<(Bound, Bound)> = Vec::new();
    if matched[0] {
        let upper = if critical.is_empty() {
            Bound::PosInfinity
        } else {
            endpoint(0)
        };
        atoms.push((Bound::NegInfinity, upper));
    }
    for index in 0..critical.len() {
        if included(index) {
            // An isolated solution: `x² ≤ 0` is `[0, 0]`, and dropping it
            // because no segment matched would answer the empty set.
            atoms.push((endpoint(index), endpoint(index)));
        }
        if matched[index + 1] {
            let lower = endpoint(index);
            let upper = if index + 1 == critical.len() {
                Bound::PosInfinity
            } else {
                endpoint(index + 1)
            };
            atoms.push((lower, upper));
        }
    }

    merge(atoms)
}

/// Join atoms that share a point the answer includes.
///
/// `(−∞, 2) ∪ [2, ∞)` is `(−∞, ∞)`, and leaving it as two intervals would be a
/// correct set written in a way the reader has to simplify. Two atoms that meet
/// at a point neither includes stay apart: `(−∞, 2) ∪ (2, ∞)` really is missing
/// `2`.
fn merge(atoms: Vec<(Bound, Bound)>) -> Vec<(Bound, Bound)> {
    let mut out: Vec<(Bound, Bound)> = Vec::new();
    for (lower, upper) in atoms {
        if let Some((_, previous_upper)) = out.last_mut()
            && touches(previous_upper, &lower)
        {
            *previous_upper = upper;
            continue;
        }
        out.push((lower, upper));
    }
    out
}

fn touches(upper: &Bound, lower: &Bound) -> bool {
    match (upper, lower) {
        (
            Bound::Finite {
                value: a,
                open: a_open,
            },
            Bound::Finite {
                value: b,
                open: b_open,
            },
        ) => a == b && !(*a_open && *b_open),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, sym};

    fn x() -> Expr {
        sym("x")
    }

    fn var() -> SymbolId {
        SymbolId::intern("x")
    }

    fn absolute(inner: Expr) -> Expr {
        ExprNode::fn_raw(SymbolId::intern("abs"), vec![inner])
    }

    fn finite(value: i64, open: bool) -> Bound {
        Bound::Finite {
            value: int(value),
            open,
        }
    }

    fn relation(left: Expr, right: Expr, comparator: Comparator) -> Relation {
        Relation::new(left, right, comparator)
    }

    /// `|x − 1|` and a constant, the left side shared by the pair that proves
    /// the comparator survives.
    fn absolute_shift() -> Expr {
        absolute(add([x(), int(-1)]))
    }

    #[test]
    fn the_same_left_side_answers_differently_for_less_and_greater() {
        // This is the test the design note requires: a hard-coded "less than"
        // passes the first assertion and fails the second.
        let less = NativeSolver
            .solve_relation(&relation(absolute_shift(), int(2), Comparator::Less), var())
            .expect("|x-1| < 2 is charted");
        assert_eq!(
            less,
            Solutions::Inequality {
                var: var(),
                intervals: vec![(finite(-1, true), finite(3, true))],
            }
        );

        let greater = NativeSolver
            .solve_relation(
                &relation(absolute_shift(), int(2), Comparator::Greater),
                var(),
            )
            .expect("|x-1| > 2 is charted");
        assert_eq!(
            greater,
            Solutions::Inequality {
                var: var(),
                intervals: vec![
                    (Bound::NegInfinity, finite(-1, true)),
                    (finite(3, true), Bound::PosInfinity),
                ],
            }
        );
    }

    #[test]
    fn an_absolute_value_inequality_answers_the_open_interval() {
        let solutions = NativeSolver
            .solve_relation(&relation(absolute_shift(), int(2), Comparator::Less), var())
            .expect("|x-1| < 2");
        assert_eq!(
            solutions,
            Solutions::Inequality {
                var: var(),
                intervals: vec![(finite(-1, true), finite(3, true))],
            }
        );
        assert_eq!(solutions.to_latex(), "(-1, 3)");
    }

    #[test]
    fn a_rational_inequality_excludes_the_denominators_root() {
        // (x−1)/(x+2) ≥ 0. The numerator's root is a solution and the
        // denominator's root is not, which is the whole reason the two ends of
        // the answer have different brackets.
        let ratio = mul([
            add([x(), int(-1)]),
            pow(add([x(), int(2)]), int(-1)),
        ]);
        let solutions = NativeSolver
            .solve_relation(
                &relation(ratio, int(0), Comparator::GreaterOrEqual),
                var(),
            )
            .expect("(x-1)/(x+2) >= 0 is charted");
        assert_eq!(
            solutions,
            Solutions::Inequality {
                var: var(),
                intervals: vec![
                    (Bound::NegInfinity, finite(-2, true)),
                    (finite(1, false), Bound::PosInfinity),
                ],
            }
        );
        assert_eq!(
            solutions.to_latex(),
            "(-\\infty, -2) \\cup [1, \\infty)"
        );
    }

    #[test]
    fn an_inequality_the_backend_cannot_chart_is_declined() {
        // x⁴ − x − 1 has real roots and no rational one; the polynomial solver
        // refuses it, so the chart has no trustworthy boundary and must not
        // guess one. A decline is a normal answer, not a failure.
        let polynomial = add([pow(x(), int(4)), mul([int(-1), x()]), int(-1)]);
        let error = NativeSolver
            .solve_relation(
                &relation(polynomial, int(0), Comparator::Greater),
                var(),
            )
            .expect_err("no exact critical points");
        assert_eq!(error, SolverError::NotImplemented);
        assert!(error.is_declined());
    }
}
