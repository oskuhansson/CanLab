//! The answer to an equation, as a set rather than a list of roots.
//!
//! A `Vec<Expr>` can only say "these are the solutions". Three ordinary exam
//! answers are not lists: an equation may have none, every number may solve it,
//! the solutions may be a family (`x = π/6 + 2kπ`), or they may be a union of
//! intervals (`x > 2`). Forcing those into a vector leaves an empty vector that
//! means both "nothing works" and "I could not find anything", which is how a
//! CAS starts lying.
//!
//! The variants keep the distinction the backend actually made. [`Solutions::None`]
//! and [`Solutions::Values`] with an empty vector are the same set written two
//! ways; only the backend knows which of the two it meant, so the type keeps
//! both instead of collapsing one into the other. [`Solutions::is_empty`] is
//! the question a display should ask when it only needs "does anything work".
//!
//! Latitude is deliberate here: a family and an inequality are *sets*, not
//! single expressions, so [`Solutions::to_latex`] prints them as sets. A caller
//! that knows the variable can prefix `x \in …` itself; the type does not carry
//! the variable for the value-like variants, and inventing one would be a
//! second, hidden answer.

use canlab_core::{Expr, ExprNode, Number, SymbolId};

/// One end of an interval in [`Solutions::Inequality`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    /// Unbounded below: `-∞`. An infinite end is never included, so it is
    /// always written with a round bracket.
    NegInfinity,
    /// A finite endpoint.
    Finite {
        /// The endpoint, exact like everything else the solver returns.
        value: Expr,
        /// `true` writes `(` / `)`; `false` writes `[` / `]`.
        open: bool,
    },
    /// Unbounded above: `∞`. Like [`Bound::NegInfinity`], always open.
    PosInfinity,
}

/// Everything a backend can say about the solutions of one equation.
///
/// See the module docs for why this is not a `Vec<Expr>`.
// No `Eq`: an [`Solutions::Approximation`] carries an `f64` residual, and a
// float is not an equivalence class (`NaN` would break the reflexivity `Eq`
// promises). `PartialEq` is what comparing two answers actually needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Solutions {
    /// The equation has no solutions at all.
    None,
    /// Every value of the variable solves the equation (`0 = 0`).
    All,
    /// A finite list of exact solutions, in the order the backend found them.
    Values(Vec<Expr>),
    /// One real root found numerically, with the evidence that it is one.
    ///
    /// This variant exists **because** a numeric root must never be presented
    /// as exact. `x^4 - x - 1 = 0` has roots no exact path this solver has can
    /// name, and an `f64` is not a number the CAS can prove anything about: it
    /// is a point where the function is small, not a point where it is zero.
    /// Keeping the residual beside the value is what lets a reader judge the
    /// answer instead of trusting it, and it is why this is not a member of
    /// [`Solutions::Values`]: a value in that list is exact by construction.
    Approximation {
        /// The root, as the number the scan found. It is a `Number::Float`, so
        /// the tree itself already says "not exact".
        value: Expr,
        /// `|f(value)|` relative to the function's scale — the rule is
        /// `canlab-num`'s, so two roots from one scan are comparable. A true
        /// root lands far below `1e-10`; a sign change at a pole does not.
        residual: f64,
    },
    /// `base + k·period` for every integer `k`, where `param` is the name of
    /// `k`. For example `π/6 + 2kπ`.
    Family {
        /// The phase every solution is offset from.
        base: Expr,
        /// One full turn: the difference between consecutive solutions.
        period: Expr,
        /// The symbol written for the integer parameter (`k` here).
        param: SymbolId,
    },
    /// The solution set is a union of intervals on the line of `var`.
    ///
    /// The intervals are stored in the order the backend produced them; a
    /// renderer that wants them ascending sorts them itself, because ordering
    /// two exact expressions is a comparison this type does not promise.
    Inequality {
        /// The variable the intervals are intervals of.
        var: SymbolId,
        /// The union, each interval `(lower, upper)`.
        intervals: Vec<(Bound, Bound)>,
    },
    /// The union of several answer sets.
    ///
    /// `sin x = ½` is two progressions, `π/6 + 2kπ` and `5π/6 + 2kπ`, and no
    /// single [`Solutions::Family`] can say that. The members are kept whole, so
    /// each one still renders and serialises the way it does alone; a union with
    /// one member is that member.
    Union(Vec<Solutions>),
}

/// One point of a system's answer set: every unknown pinned at once.
pub type Point = Vec<(SymbolId, Expr)>;

/// What a **system** of equations answers.
///
/// Deliberately not a [`Solutions`]: that type describes the values of one
/// variable. A nonlinear system meeting in two places is two *assignments*, and
/// saying it as a set of numbers for `x` would lose which `y` goes with which
/// `x` — the mistake a single assignment list makes when it reports one point.
#[derive(Clone, Debug, PartialEq)]
pub enum SystemAnswer {
    /// The equations contradict each other.
    None,
    /// Every value satisfies them (a degenerate system).
    All,
    /// The set of points where all the equations hold at once.
    Points(Vec<Point>),
}

impl SystemAnswer {
    /// `true` when the system has no solution.
    pub fn is_empty(&self) -> bool {
        match self {
            SystemAnswer::None => true,
            SystemAnswer::All => false,
            SystemAnswer::Points(points) => points.is_empty(),
        }
    }
}

impl Solutions {
    /// `true` when no value of the variable solves the equation.
    ///
    /// [`Solutions::None`] and an empty [`Solutions::Values`] are both empty:
    /// the variant records how the backend said it, but the answer is the same.
    /// An [`Solutions::Inequality`] with no intervals is empty too. A family is
    /// never empty; it has infinitely many members by construction. An
    /// approximation is never empty either: a root was found, however
    /// approximate it is.
    pub fn is_empty(&self) -> bool {
        match self {
            Solutions::None => true,
            Solutions::All => false,
            Solutions::Values(values) => values.is_empty(),
            // A numeric root is a member of the set, not a missing answer.
            Solutions::Approximation { .. } => false,
            Solutions::Family { .. } => false,
            Solutions::Inequality { intervals, .. } => intervals.is_empty(),
            // A union is empty exactly when every member is.
            Solutions::Union(members) => members.iter().all(Solutions::is_empty),
        }
    }

    /// Render the answer as LaTeX math, without surrounding delimiters.
    ///
    /// Each variant prints as the set it denotes: `\varnothing` for no
    /// solutions, `\mathbb{R}` for all of them, `\left\{…\right\}` for a finite
    /// list, `\frac{\pi}{6} + 2kπ` for a family and `(-1, 3]` for an interval.
    /// An [`Solutions::Approximation`] prints its value and nothing else: the
    /// `\approx` belongs to the caller, which is the layer that knows whether
    /// the number is being presented as an answer or compared with another.
    /// The variable is part of the interval notation only through its endpoints;
    /// writing `x \in …` is the caller's job, because only the caller knew
    /// which variable was asked about.
    pub fn to_latex(&self) -> String {
        match self {
            Solutions::None => String::from("\\varnothing"),
            Solutions::All => String::from("\\mathbb{R}"),
            Solutions::Values(values) if values.is_empty() => String::from("\\varnothing"),
            Solutions::Values(values) => {
                let mut out = String::from("\\left\\{");
                for (index, value) in ascending(values).iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&canlab_tex::to_latex(value));
                }
                out.push_str("\\right\\}");
                out
            }
            // The value alone. `\approx` is the caller's decision, because only
            // the caller knows whether it is writing `x ≈ 1.2207` or comparing
            // two answers; printing the relation here would decide for it.
            Solutions::Approximation { value, .. } => canlab_tex::to_latex(value),
            Solutions::Family {
                base,
                period,
                param,
            } => {
                // `base + k·period` with the zero phase left out: `x = kπ` is
                // not written `x = 0 + kπ`.
                if base.is_zero() {
                    return period_latex(period, *param);
                }
                let mut out = phase_latex(base);
                out.push_str(" + ");
                out.push_str(&period_latex(period, *param));
                out
            }
            // `π/6 + 2kπ \cup 5π/6 + 2kπ`. An empty member is not part of the
            // union, and a union that keeps one member prints as that member.
            Solutions::Union(members) => members
                .iter()
                .filter(|member| !member.is_empty())
                .map(Solutions::to_latex)
                .collect::<Vec<String>>()
                .join(" \\cup "),
            Solutions::Inequality { intervals, .. } => {
                if intervals.is_empty() {
                    return String::from("\\varnothing");
                }
                let mut out = String::new();
                for (index, (lower, upper)) in intervals.iter().enumerate() {
                    if index > 0 {
                        out.push_str(" \\cup ");
                    }
                    out.push(if lower.is_open() { '(' } else { '[' });
                    out.push_str(&bound_latex(lower));
                    out.push_str(", ");
                    out.push_str(&bound_latex(upper));
                    out.push(if upper.is_open() { ')' } else { ']' });
                }
                out
            }
        }
    }
}

impl From<Vec<Expr>> for Solutions {
    /// The historical answer — a plain list of roots — as a set of values.
    ///
    /// An empty vector stays [`Solutions::Values`] rather than becoming
    /// [`Solutions::None`]: this conversion does not ask the backend anything,
    /// so it must not reinterpret the answer it was handed.
    fn from(values: Vec<Expr>) -> Self {
        Solutions::Values(values)
    }
}

impl Bound {
    /// `true` when this end of the interval is excluded.
    ///
    /// Crate-internal because it exists for the two renderers (LaTeX here, JSON
    /// in `json`); a caller outside the crate matches on the variant, which is
    /// already public and carries the flag.
    pub(crate) fn is_open(&self) -> bool {
        match self {
            Bound::NegInfinity | Bound::PosInfinity => true,
            Bound::Finite { open, .. } => *open,
        }
    }
}

/// The values in ascending order, when they are all real numbers.
///
/// A finite answer set is a *set*: `\left\{1, -1, 2, -2\right\}` is the order the
/// search found them in, and a reader has to sort it in their head to check it.
/// When a value is not a number — a parameter, a radical the evaluator cannot
/// reduce — the backend's order is kept, because ordering two exact expressions
/// is a comparison this renderer does not promise. Ties (a repeated root) keep
/// their order and contrast; the sort is stable.
fn ascending(values: &[Expr]) -> Vec<&Expr> {
    let numbers: Option<Vec<f64>> = values
        .iter()
        .map(canlab_core::approx::value_of)
        .collect();
    match numbers {
        Some(numbers) if numbers.iter().all(|value| value.is_finite()) => {
            let mut paired: Vec<(f64, &Expr)> = numbers.into_iter().zip(values).collect();
            paired.sort_by(|left, right| left.0.total_cmp(&right.0));
            paired.into_iter().map(|(_, value)| value).collect()
        }
        _ => values.iter().collect(),
    }
}

/// The text of one endpoint: `-\infty`, a rendered expression, or `\infty`.
fn bound_latex(bound: &Bound) -> String {
    match bound {
        Bound::NegInfinity => String::from("-\\infty"),
        Bound::PosInfinity => String::from("\\infty"),
        Bound::Finite { value, .. } => canlab_tex::to_latex(value),
    }
}

/// Split a canonical expression into its numeric coefficient and the rest.
///
/// Canonical form puts a `Num` first in a `Mul`; that invariant is the only
/// thing this relies on. `5` -> `(5, 1)`, `-2x` -> `(-2, x)`, `x` -> `(None, x)`.
fn coefficient_and_rest(e: &Expr) -> (Option<Number>, Expr) {
    match &**e {
        ExprNode::Num(number) => (Some(number.clone()), ExprNode::int(1)),
        ExprNode::Mul(factors) if !factors.is_empty() => match &*factors[0] {
            ExprNode::Num(number) => {
                let rest = &factors[1..];
                let base = match rest.len() {
                    0 => ExprNode::int(1),
                    1 => rest[0].clone(),
                    _ => ExprNode::mul_raw(rest.to_vec()),
                };
                (Some(number.clone()), base)
            }
            _ => (None, e.clone()),
        },
        _ => (None, e.clone()),
    }
}

/// A family's phase, in the form the exercise writes it: `\frac{\pi}{6}`.
///
/// The printer renders `π/6` as it stores it, `\frac{1}{6}\pi`, which is
/// correct but not how a solution is written by hand. A family is already a
/// presentation construct — the answer is *the form the exercise wants* — so a
/// rational coefficient over a symbolic part prints as a fraction of that part.
/// Every other phase goes through the ordinary printer unchanged.
fn phase_latex(phase: &Expr) -> String {
    let (coefficient, rest) = coefficient_and_rest(phase);
    let Some(coefficient) = coefficient else {
        return canlab_tex::to_latex(phase);
    };
    let Number::Rat(numerator, denominator) = coefficient else {
        return canlab_tex::to_latex(phase);
    };
    if rest.is_one() {
        return canlab_tex::to_latex(phase);
    }
    let sign = if numerator < 0 { "-" } else { "" };
    let magnitude = numerator.abs();
    let numerator = if magnitude == 1 {
        canlab_tex::to_latex(&rest)
    } else {
        canlab_tex::to_latex(&canlab_core::mul([
            ExprNode::num(Number::int(magnitude)),
            rest,
        ]))
    };
    format!("{sign}\\frac{{{numerator}}}{{{denominator}}}")
}

/// `2k\pi` for a period of `2\pi`: the parameter goes inside the period's
/// numeric coefficient, where a general solution is read aloud.
///
/// When the period has no exact numeric coefficient to split, the ordinary
/// printer multiplies it by the parameter instead. That also keeps the result
/// lexically safe: `\pi` followed by `k` needs a separator, or TeX reads one
/// control word `\pik`.
fn period_latex(period: &Expr, param: SymbolId) -> String {
    let parameter = ExprNode::sym(param);
    // The fallback: let the canonical printer do the multiplication.
    let multiplied =
        || canlab_tex::to_latex(&canlab_core::mul([period.clone(), parameter.clone()]));

    let (coefficient, rest) = coefficient_and_rest(period);
    let Some(coefficient) = coefficient else {
        return multiplied();
    };
    if rest.is_one() {
        return multiplied();
    }

    let parameter_latex = canlab_tex::to_latex(&parameter);
    let rest_latex = canlab_tex::to_latex(&rest);
    match coefficient {
        Number::Int(value) => {
            let sign = if value < 0 { "-" } else { "" };
            let magnitude = value.abs();
            if magnitude == 1 {
                format!("{sign}{parameter_latex}{rest_latex}")
            } else {
                format!("{sign}{magnitude}{parameter_latex}{rest_latex}")
            }
        }
        Number::Rat(numerator, denominator) => {
            let sign = if numerator < 0 { "-" } else { "" };
            let magnitude = numerator.abs();
            let numerator = if magnitude == 1 {
                format!("{parameter_latex}{rest_latex}")
            } else {
                format!("{magnitude}{parameter_latex}{rest_latex}")
            };
            format!("{sign}\\frac{{{numerator}}}{{{denominator}}}")
        }
        // An approximate period has no exact coefficient to split; the
        // printer's own form is honest about that.
        Number::Float(_) => multiplied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, ratio, sym};

    fn k() -> SymbolId {
        SymbolId::intern("k")
    }

    fn pi() -> Expr {
        sym("pi")
    }

    #[test]
    fn a_family_prints_as_the_exam_answer() {
        let family = Solutions::Family {
            base: canlab_core::mul([ratio(1, 6), pi()]),
            period: canlab_core::mul([int(2), pi()]),
            param: k(),
        };
        assert_eq!(family.to_latex(), "\\frac{\\pi}{6} + 2k\\pi");

        // A zero phase is not written at all. The factor order of `π·k` is
        // whatever the interner handed out, so the assertion is on what the
        // family must contain, not on the exact spacing.
        let through_origin = Solutions::Family {
            base: int(0),
            period: pi(),
            param: k(),
        };
        let latex = through_origin.to_latex();
        assert!(latex.contains('k'), "the parameter went missing: {latex}");
        assert!(!latex.contains("0 +"), "a zero phase was printed: {latex}");
    }

    #[test]
    fn an_inequality_uses_the_bracket_style_of_each_endpoint() {
        let finite = |value: i64, open: bool| Bound::Finite {
            value: int(value),
            open,
        };
        let single = Solutions::Inequality {
            var: SymbolId::intern("x"),
            intervals: vec![(finite(-1, true), finite(3, true))],
        };
        assert_eq!(single.to_latex(), "(-1, 3)");

        let mixed = Solutions::Inequality {
            var: SymbolId::intern("x"),
            intervals: vec![
                (finite(-1, true), finite(3, false)),
                (finite(4, false), Bound::PosInfinity),
            ],
        };
        assert_eq!(mixed.to_latex(), "(-1, 3] \\cup [4, \\infty)");

        let unbounded = Solutions::Inequality {
            var: SymbolId::intern("x"),
            intervals: vec![(Bound::NegInfinity, finite(2, false))],
        };
        assert_eq!(unbounded.to_latex(), "(-\\infty, 2]");
    }

    #[test]
    fn all_and_none_print_as_sets() {
        assert_eq!(Solutions::None.to_latex(), "\\varnothing");
        assert_eq!(Solutions::All.to_latex(), "\\mathbb{R}");
        // The two spellings of "nothing works" print the same.
        assert_eq!(Solutions::Values(Vec::new()).to_latex(), "\\varnothing");
    }

    #[test]
    fn values_print_as_an_ascending_set() {
        // The search finds roots in whatever order the deflation loop pops them.
        // A set is a set either way, but `{-2, 2}` is the one a reader can check.
        assert_eq!(
            Solutions::Values(vec![int(2), int(-2)]).to_latex(),
            "\\left\\{-2, 2\\right\\}"
        );
        assert_eq!(
            Solutions::Values(vec![int(3), int(-1), int(0), int(-1)]).to_latex(),
            "\\left\\{-1, -1, 0, 3\\right\\}"
        );
    }

    #[test]
    fn a_value_that_is_not_a_number_keeps_the_backend_order() {
        // `1 + y` and `1 - y` are a parametric pair; which is "smaller" depends
        // on `y`, so the renderer must not invent an order.
        let y = ExprNode::sym(SymbolId::intern("y"));
        let plus = canlab_core::add(vec![int(1), y.clone()]);
        let minus = canlab_core::add(vec![int(1), canlab_core::mul(vec![int(-1), y])]);
        let expected = format!(
            "\\left\\{{{}, {}\\right\\}}",
            canlab_tex::to_latex(&plus),
            canlab_tex::to_latex(&minus)
        );
        assert_eq!(
            Solutions::Values(vec![plus, minus]).to_latex(),
            expected
        );
    }

    #[test]
    fn is_empty_says_which_answers_have_no_members() {
        assert!(Solutions::None.is_empty());
        assert!(Solutions::Values(Vec::new()).is_empty());
        assert!(
            Solutions::Inequality {
                var: SymbolId::intern("x"),
                intervals: Vec::new(),
            }
            .is_empty()
        );

        assert!(!Solutions::All.is_empty());
        assert!(!Solutions::Values(vec![int(0)]).is_empty());
        assert!(
            !Solutions::Family {
                base: int(0),
                period: int(2),
                param: k(),
            }
            .is_empty()
        );
    }

    #[test]
    fn a_vector_becomes_a_set_of_values() {
        let values = vec![int(2), int(-2)];
        assert_eq!(Solutions::from(values.clone()), Solutions::Values(values));
    }

    #[test]
    fn an_approximation_prints_only_its_value() {
        // The `\approx` is the caller's decision: this string may be written
        // `x ≈ …` or compared with another answer, and the relation is not part
        // of the value.
        let approximate = Solutions::Approximation {
            value: ExprNode::num(Number::float(1.25)),
            residual: 1e-16,
        };
        assert_eq!(approximate.to_latex(), "1.25");
        assert!(
            !approximate.to_latex().contains("\\approx"),
            "the sign belongs to the caller"
        );
    }

    #[test]
    fn an_approximation_is_never_empty() {
        // A numeric root is a member of the answer set; `is_empty` asks whether
        // anything solves the equation, and something does.
        assert!(
            !Solutions::Approximation {
                value: ExprNode::num(Number::float(1.25)),
                residual: 1e-16,
            }
            .is_empty()
        );
    }
}
