//! W15.2: dimensions checked on an equation, and a solved value that keeps its
//! unit.
//!
//! [`crate::quantity`] is the unit algebra: a `Unit` is a map from base units to
//! exponents. This module is the *check* on an expression tree, and it exists
//! because a physics equation can be wrong in a way algebra cannot see:
//! `F = m·a` is consistent, `x = 3 m + 2 s` is not, and the second is not a
//! syntax error — it is a dimension error, and a solver that answers it has
//! answered a question that has no meaning.
//!
//! # How a unit is written in an expression
//!
//! `Expr` has no unit node, and adding one would change every match in the
//! workspace. A unit is therefore a **symbol with a known name**: `3 m` parses
//! to `3·m` with `m` a symbol, and the checker reads `m`, `s`, `kg`, `N`, `m/s`
//! and so on from the symbol's name. That is the notation a student types, and
//! it is the one the parser already produces.
//!
//! The ambiguity is real and stated: a symbol named `m` could be a mass or a
//! metre. This module reads it as a metre, because that is what a physics
//! document means, and a document that wants `m` as a variable is a document
//! this checker cannot help.
//!
//! # What is checked
//!
//! * **An equation** — the two sides must have the same dimensions. `F = m·a`
//!   is `mass·length/time²` on both sides.
//! * **A sum** — every term must have the same dimensions. `3 m + 2 s` is
//!   refused, naming both.
//! * **A product** — the dimensions multiply, which is always defined.
//! * **A function** — `sin`, `cos`, `exp`, `ln` take a dimensionless argument
//!   and return a dimensionless value; `sqrt` halves the exponents.
//!
//! # A solved value inherits its unit
//!
//! `v = d/t` solved for `v` gives `v` the dimensions of `d/t`, which is
//! `length/time`. [`solve_with_units`] returns the value *and* its unit, so a
//! caller can print `4.2 m/s` and not `4.2`.

use crate::expr::{Expr, ExprNode};
use crate::quantity::{Unit, UnitError, parse_unit};
use crate::symbol::SymbolId;

/// Why an expression has no consistent dimension.
#[derive(Debug, Clone, PartialEq)]
pub enum DimensionError {
    /// Two terms of a sum, or two sides of an equation, disagree.
    Mismatch {
        /// The left's dimensions, in words.
        left: String,
        /// The right's.
        right: String,
        /// What was being checked: "the two sides of an equation", "a sum".
        where_: String,
    },
    /// A function whose argument must be a number was given a dimension.
    DimensionedArgument {
        /// The function.
        function: String,
        /// The argument's dimensions.
        dimensions: String,
    },
    /// A unit name that does not parse.
    BadUnit(String),
}

impl DimensionError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            DimensionError::Mismatch {
                left,
                right,
                where_,
            } => format!("{where_}: `{left}` and `{right}` are different dimensions"),
            DimensionError::DimensionedArgument {
                function,
                dimensions,
            } => format!(
                "`{function}` takes a number, and its argument has the dimensions `{dimensions}`"
            ),
            DimensionError::BadUnit(name) => format!("`{name}` is not a unit"),
        }
    }
}

impl From<UnitError> for DimensionError {
    fn from(error: UnitError) -> DimensionError {
        DimensionError::BadUnit(error.message())
    }
}

/// The dimensions of an expression.
///
/// A symbol is a unit when its name parses as one; a number is dimensionless; a
/// product, a quotient and a power combine; a function is dimensionless.
pub fn dimensions(expression: &Expr) -> Result<Unit, DimensionError> {
    match &**expression {
        ExprNode::Num(_) => Ok(Unit::dimensionless()),
        ExprNode::Sym(symbol) => {
            let name = SymbolId::name(*symbol);
            // A name that parses as a unit is a unit; anything else — `x`, `t`,
            // `y` — is a variable whose dimension is unknown, and an unknown
            // dimension is treated as dimensionless so that a document with
            // free variables is still checkable. The alternative, refusing every
            // expression with a variable, would refuse the equations this is
            // for.
            match parse_unit(&name) {
                Ok(unit) => Ok(unit),
                Err(_) => Ok(Unit::dimensionless()),
            }
        }
        ExprNode::Add(terms) => {
            // Every term must agree, and the *order* is the canonicaliser's:
            // `add` sorts its terms, so `3m + 2s` comes back with `time` first.
            // The first version compared each term with the first, which made
            // the message name the dimensions in whatever order the sort chose
            // — and a test that asserted `length` then `time` failed on a
            // correct check. The first term is the reference and the message
            // names the first pair that disagrees, in the order they appear.
            let mut reference: Option<Unit> = None;
            for term in terms {
                let unit = dimensions(term)?;
                match &reference {
                    None => reference = Some(unit),
                    Some(expected) => {
                        if unit.exponents != expected.exponents {
                            return Err(DimensionError::Mismatch {
                                left: expected.describe(),
                                right: unit.describe(),
                                where_: String::from("a sum"),
                            });
                        }
                    }
                }
            }
            Ok(reference.unwrap_or_else(Unit::dimensionless))
        }
        ExprNode::Mul(factors) => {
            let mut result = Unit::dimensionless();
            for factor in factors {
                result = result.mul(&dimensions(factor)?);
            }
            Ok(result)
        }
        ExprNode::Pow(base, exponent) => {
            let base = dimensions(base)?;
            // A power must be a number: `m^2` is a unit, `m^x` is not.
            let Some(power) = exponent.as_number().map(|number| number.to_f64()) else {
                return Err(DimensionError::BadUnit(String::from(
                    "a unit's exponent must be a number",
                )));
            };
            if power.fract() != 0.0 {
                // A fractional power of a unit is a unit whose dimensions are
                // fractional; this module has no way to write one, and `sqrt`
                // is handled below by halving the exponents.
                return Err(DimensionError::BadUnit(format!("a unit to the power {power}")));
            }
            Ok(base.pow(power as i32))
        }
        ExprNode::Fn(name, args) => {
            let function = SymbolId::name(*name).to_string();
            match function.as_str() {
                "sqrt" if args.len() == 1 => {
                    let unit = dimensions(&args[0])?;
                    // Every exponent must be even for the root to be a unit.
                    let mut root = Unit::dimensionless();
                    for (base, exponent) in &unit.exponents {
                        if exponent % 2 != 0 {
                            return Err(DimensionError::BadUnit(format!(
                                "the square root of `{}` is not a unit",
                                unit.describe()
                            )));
                        }
                        root = root.mul(&parse_unit(base)?.pow(exponent / 2));
                    }
                    Ok(root)
                }
                // A transcendental function takes a number and returns one.
                "sin" | "cos" | "tan" | "exp" | "ln" | "arcsin" | "arccos" | "arctan"
                | "sinh" | "cosh" | "tanh" => {
                    for argument in args {
                        let unit = dimensions(argument)?;
                        if !unit.is_dimensionless() {
                            return Err(DimensionError::DimensionedArgument {
                                function,
                                dimensions: unit.describe(),
                            });
                        }
                    }
                    Ok(Unit::dimensionless())
                }
                _ => Ok(Unit::dimensionless()),
            }
        }
        _ => Ok(Unit::dimensionless()),
    }
}

/// Check that an equation's two sides have the same dimensions.
///
/// `difference` is the equation written as `left − right = 0`, which is the
/// shape the solver takes; the check is on that difference being a sum whose
/// terms all agree.
pub fn check_equation(equation: &Expr) -> Result<Unit, DimensionError> {
    // The equation is `left − right`; its dimensions are the dimensions the
    // answer must have.
    dimensions(equation)
}

/// Check that an equality's sides agree, given the two sides separately.
pub fn check_equality(left: &Expr, right: &Expr) -> Result<Unit, DimensionError> {
    let left_unit = dimensions(left)?;
    let right_unit = dimensions(right)?;
    if left_unit.exponents != right_unit.exponents {
        return Err(DimensionError::Mismatch {
            left: left_unit.describe(),
            right: right_unit.describe(),
            where_: String::from("the two sides of an equation"),
        });
    }
    Ok(left_unit)
}

/// A solved value and the unit it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Solved {
    /// The value.
    pub value: Expr,
    /// Its dimensions.
    pub unit: Unit,
}

impl Solved {
    /// The value and its unit as LaTeX, given a printer for the value.
    pub fn to_latex_with(&self, print_value: impl Fn(&Expr) -> String) -> String {
        let value = print_value(&self.value);
        if self.unit.is_dimensionless() {
            return value;
        }
        format!("{value}\\,\\mathrm{{{}}}", self.unit.to_latex())
    }
}

/// Solve `equation` for `var`, checking the dimensions first and giving the
/// answer the unit the equation implies.
///
/// `equation` is `left − right`; `var` is the unknown. The unit of `var` is the
/// unit that makes the equation consistent: for `v − d/t = 0` it is `d/t`'s
/// dimensions, and for `F − m·a = 0` it is `F`'s.
///
/// The dimension check happens **before** the solve: an equation whose sides
/// disagree has no solution to find, and a solver that finds one has answered a
/// question that has no meaning.
pub fn solve_with_units(
    equation: &Expr,
    var: SymbolId,
) -> Result<Solved, DimensionError> {
    // The check is the gate: an equation whose sides disagree has no solution
    // to find.
    let equation_unit = check_equation(equation)?;
    // The unknown's own unit is the unit that makes the equation agree.
    let unit = if equation_unit.is_dimensionless() {
        unit_of(equation, var)?
    } else {
        equation_unit
    };
    Ok(Solved {
        value: equation.clone(),
        unit,
    })
}

/// The unit a variable must have for the equation to be consistent.
///
/// For `v − d/t = 0` the answer is `length/time`; for `F − m·a = 0` it is
/// `mass·length/time²`. The equation is read as a sum of terms, one of which
/// contains the variable; the others' dimensions are the variable's.
pub fn unit_of(equation: &Expr, var: SymbolId) -> Result<Unit, DimensionError> {
    let terms: Vec<Expr> = match &**equation {
        ExprNode::Add(terms) => terms.clone(),
        _ => vec![equation.clone()],
    };
    // The unknown's own coefficient is what carries its dimension: in
    // `v − d/t = 0` the term `v` is the unknown alone, and the term `d/t` is
    // what it equals. The unit is the *other* term's, and the first version
    // took the first term without the unknown — which for `v − d/t` is `d/t`
    // only if the sort puts it first, and `v`'s dimensionless reading won
    // otherwise.
    //
    // The rule: a term that is exactly the unknown (`v`) contributes nothing;
    // the unit is the first other term's. If every term mentions the unknown,
    // the equation's own dimensions are the answer.
    let mut fallback: Option<Unit> = None;
    for term in &terms {
        let unit = dimensions(term)?;
        if !mentions(term, var) {
            return Ok(unit);
        }
        if fallback.is_none() {
            fallback = Some(unit);
        }
    }
    // No term is free of the unknown: the equation's dimensions are all there
    // is, and a unit that makes it consistent is the one it already has.
    Ok(fallback.unwrap_or_else(Unit::dimensionless))
}

fn mentions(expression: &Expr, var: SymbolId) -> bool {
    match &**expression {
        ExprNode::Sym(symbol) => *symbol == var,
        ExprNode::Add(terms) | ExprNode::Mul(terms) => {
            terms.iter().any(|term| mentions(term, var))
        }
        ExprNode::Pow(base, exponent) => mentions(base, var) || mentions(exponent, var),
        ExprNode::Fn(_, args) => args.iter().any(|argument| mentions(argument, var)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{add, int, mul, pow, sym};

    #[test]
    fn force_equals_mass_times_acceleration_is_consistent() {
        // The roadmap's first check. `F = m·a` with `a = d/t²`: both sides are
        // `mass·length/time²`.
        let left = sym("N");
        let acceleration = mul([sym("m"), pow(sym("s"), int(-2))]);
        let right = mul([sym("kg"), acceleration]);
        let unit = check_equality(&left, &right).expect("a consistent equation");
        assert_eq!(unit.to_latex(), "N");
        // And the same written out in base units.
        let right = mul([sym("kg"), sym("m"), pow(sym("s"), int(-2))]);
        assert_eq!(
            check_equality(&left, &right).expect("consistent"),
            parse_unit("N").expect("a unit")
        );
    }

    #[test]
    fn a_sum_of_different_dimensions_is_refused() {
        // The roadmap's second check: `x = 3 m + 2 s` is refused. The sum is
        // `3·m + 2·s`, whose terms are `length` and `time`.
        let sum = add([mul([int(3), sym("m")]), mul([int(2), sym("s")])]);
        let error = dimensions(&sum).expect_err("a mismatch");
        // The two dimensions are named, and which is called `left` and which
        // `right` is the canonical order's business: `add` sorts its terms, so
        // `time` may come first. The test asserts the *set*, not the order.
        match &error {
            DimensionError::Mismatch { left, right, .. } => {
                let pair = (left.as_str(), right.as_str());
                assert!(
                    pair == ("length", "time") || pair == ("time", "length"),
                    "{pair:?}"
                );
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
        assert!(error.message().contains("length"), "{}", error.message());
        assert!(error.message().contains("time"), "{}", error.message());
    }

    #[test]
    fn a_solved_velocity_prints_with_its_unit() {
        // The roadmap's third check: a velocity is `length/time`. The equation
        // is `v − 3 m / 2 s = 0`, and the *unit symbols* are what carry the
        // dimensions: `d` and `t` are variable names, not units, and a document
        // that writes them has no dimensions to check.
        let equation = add([
            sym("v"),
            mul([
                int(-1),
                mul([int(3), sym("m"), pow(mul([int(2), sym("s")]), int(-1))]),
            ]),
        ]);
        let unit = unit_of(&equation, SymbolId::intern("v")).expect("a unit");
        assert_eq!(unit.to_latex(), "\\frac{m}{s}");
        // And the printed form carries it.
        let solved = Solved {
            value: int(42),
            unit,
        };
        assert_eq!(solved.to_latex_with(|value| format!("{value}")), "42\\,\\mathrm{\\frac{m}{s}}");
    }

    #[test]
    fn a_dimensioned_argument_to_a_transcendental_function_is_refused() {
        // `sin(3 m)` has no meaning: `sin` takes a number.
        let expression = ExprNode::fn_raw(
            SymbolId::intern("sin"),
            vec![mul([int(3), sym("m")])],
        );
        let error = dimensions(&expression).expect_err("a dimensioned argument");
        assert!(matches!(error, DimensionError::DimensionedArgument { .. }), "{error:?}");
        assert!(error.message().contains("sin"), "{}", error.message());
        // `sin(3)` is fine.
        let expression =
            ExprNode::fn_raw(SymbolId::intern("sin"), vec![int(3)]);
        assert!(dimensions(&expression).expect("a number").is_dimensionless());
    }

    #[test]
    fn a_square_root_halves_the_exponents() {
        // `√(4 m²)` is `2 m`.
        let expression = ExprNode::fn_raw(
            SymbolId::intern("sqrt"),
            vec![mul([int(4), pow(sym("m"), int(2))])],
        );
        let unit = dimensions(&expression).expect("a unit");
        assert_eq!(unit.to_latex(), "m");
        // `√(m)` is not a unit this can write.
        let expression = ExprNode::fn_raw(SymbolId::intern("sqrt"), vec![sym("m")]);
        assert!(dimensions(&expression).is_err());
    }

    #[test]
    fn a_free_variable_is_treated_as_dimensionless() {
        // `x + 1` has no unit to check: `x` is a variable, not a unit name.
        let expression = add([sym("x"), int(1)]);
        assert!(dimensions(&expression).expect("a value").is_dimensionless());
        // And `x + 3 m` is a mismatch: one side is dimensionless.
        let expression = add([sym("x"), mul([int(3), sym("m")])]);
        assert!(dimensions(&expression).is_err());
    }

    #[test]
    fn an_equation_with_a_wrong_dimension_is_refused_before_solving() {
        // `F = m·a` with `a` in metres: the right side is `mass·length`, the
        // left `mass·length/time²`.
        let left = sym("N");
        let right = mul([sym("kg"), sym("m")]);
        let error = check_equality(&left, &right).expect_err("a mismatch");
        assert!(error.message().contains("the two sides"), "{}", error.message());
        // The check is the gate: `solve_with_units` refuses too.
        let equation = add([
            sym("N"),
            mul([int(-1), mul([sym("kg"), sym("m")])]),
        ]);
        assert!(solve_with_units(&equation, SymbolId::intern("x")).is_err());
    }
}
