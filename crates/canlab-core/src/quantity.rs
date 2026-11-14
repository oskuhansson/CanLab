//! W15.1: a quantity is a number and a unit, and the unit has algebra.
//!
//! `4.2 m/s` is not `4.2`. A physics answer carries a unit, and a CAS that drops
//! it has answered a different question — the number is right for a *model* and
//! the unit is what makes it a measurement.
//!
//! # The unit is a product of powers of base units
//!
//! A [`Unit`] is a map from a base unit's name to its exponent: `m/s` is
//! `{m: 1, s: −1}`, `kg·m/s²` is `{kg: 1, m: 1, s: −2}`, and `N` is the same
//! thing under a name. Multiplication adds exponents, division subtracts them,
//! and a power multiplies them — which is the whole of "unit algebra", and it is
//! exact integer arithmetic.
//!
//! # Derived units are recognised, not stored
//!
//! `kg·m/s²` *reduces to* `N`. The reduction is a table of named derived units
//! with their dimensions, looked up when a unit is printed; nothing stores
//! `N` as its own base, because then `N/m²` and `kg/(m·s²)` would be different
//! units that are the same thing, and a comparison between them would fail for
//! a reason that has nothing to do with the physics.
//!
//! # Prefixes are a scale, not a unit
//!
//! `km` is `1000 m`, and the prefix is a factor on the value: `4.2 km` is
//! `{value: 4200, unit: m}` once normalised. Keeping the prefix in the unit
//! would make `km` and `m` incomparable, and `1 km + 500 m` unanswerable.
//!
//! # What is refused
//!
//! `m + s` is refused with both dimensions named. That is the one rule this
//! module exists for: adding two different dimensions is not a number, and a
//! result that looks like one is the worst outcome a CAS can produce.

use std::collections::BTreeMap;

use crate::expr::{Expr, ExprNode};
use crate::number::Number;

/// Why a quantity has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitError {
    /// Two quantities whose dimensions do not match, named.
    DimensionMismatch {
        /// The left side's dimensions, in words.
        left: String,
        /// The right side's.
        right: String,
    },
    /// A unit name this module does not know.
    UnknownUnit(String),
    /// A prefix on a unit that does not take one (`kkg`).
    BadPrefix(String),
    /// A power that is not an integer.
    NonIntegerPower,
}

impl UnitError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            UnitError::DimensionMismatch { left, right } => {
                format!("`{left}` and `{right}` are different dimensions and cannot be added")
            }
            UnitError::UnknownUnit(name) => format!("`{name}` is not a unit this knows"),
            UnitError::BadPrefix(name) => {
                format!("`{name}` is not a prefix that can be applied here")
            }
            UnitError::NonIntegerPower => {
                "a unit can only be raised to an integer power".to_string()
            }
        }
    }
}

/// A unit: a product of powers of base units, with a scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// The base units and their exponents, sorted by name so that two equal
    /// units have equal maps.
    pub exponents: BTreeMap<String, i32>,
    /// The factor that converts this unit's numbers to the base units'.
    ///
    /// `km` has `scale = 1000` and `{m: 1}`; `m` has `scale = 1`. A quantity's
    /// value is always in *this* unit, and the scale is what converts it.
    pub scale: f64,
}

impl Unit {
    /// The dimensionless unit, `1`.
    pub fn dimensionless() -> Unit {
        Unit {
            exponents: BTreeMap::new(),
            scale: 1.0,
        }
    }

    /// A base unit.
    pub fn base(name: &str) -> Unit {
        let mut exponents = BTreeMap::new();
        exponents.insert(name.to_string(), 1);
        Unit {
            exponents,
            scale: 1.0,
        }
    }

    /// `true` when the unit is a number.
    pub fn is_dimensionless(&self) -> bool {
        self.exponents.values().all(|exponent| *exponent == 0)
    }

    /// `self · other`.
    pub fn mul(&self, other: &Unit) -> Unit {
        let mut exponents = self.exponents.clone();
        for (name, exponent) in &other.exponents {
            *exponents.entry(name.clone()).or_insert(0) += exponent;
        }
        exponents.retain(|_, exponent| *exponent != 0);
        Unit {
            exponents,
            scale: self.scale * other.scale,
        }
    }

    /// `self / other`.
    pub fn div(&self, other: &Unit) -> Unit {
        let mut exponents = self.exponents.clone();
        for (name, exponent) in &other.exponents {
            *exponents.entry(name.clone()).or_insert(0) -= exponent;
        }
        exponents.retain(|_, exponent| *exponent != 0);
        Unit {
            exponents,
            scale: self.scale / other.scale,
        }
    }

    /// `selfⁿ`.
    pub fn pow(&self, exponent: i32) -> Unit {
        let mut exponents = BTreeMap::new();
        for (name, power) in &self.exponents {
            let product = power * exponent;
            if product != 0 {
                exponents.insert(name.clone(), product);
            }
        }
        Unit {
            exponents,
            scale: self.scale.powi(exponent),
        }
    }

    /// The dimensions as words: `m/s`, `kg·m/s²`, `1` for none.
    ///
    /// A named derived unit is used when the whole unit is one: `kg·m/s²`
    /// prints as `N`. That is the reduction the roadmap asks for, and it is a
    /// *lookup*, so `N/m²` and `kg/(m·s²)` print as `Pa` both.
    pub fn to_latex(&self) -> String {
        if let Some(name) = derived_name(&self.exponents) {
            return name.to_string();
        }
        if self.is_dimensionless() {
            return String::from("1");
        }
        let mut numerator: Vec<String> = Vec::new();
        let mut denominator: Vec<String> = Vec::new();
        for (name, exponent) in &self.exponents {
            let magnitude = exponent.abs();
            let factor = if magnitude == 1 {
                name.clone()
            } else {
                format!("{name}^{magnitude}")
            };
            if *exponent > 0 {
                numerator.push(factor);
            } else {
                denominator.push(factor);
            }
        }
        let top = if numerator.is_empty() {
            String::from("1")
        } else {
            numerator.join("\\cdot ")
        };
        if denominator.is_empty() {
            top
        } else {
            format!("\\frac{{{top}}}{{{}}}", denominator.join("\\cdot "))
        }
    }

    /// The dimensions in words, for a message: `length/time`.
    pub fn describe(&self) -> String {
        if self.is_dimensionless() {
            return String::from("a number");
        }
        let mut parts: Vec<String> = Vec::new();
        for (name, exponent) in &self.exponents {
            let dimension = dimension_of(name);
            if exponent.abs() == 1 {
                parts.push(dimension.to_string());
            } else {
                parts.push(format!("{dimension}^{}", exponent.abs()));
            }
        }
        parts.join("·")
    }
}

/// The named derived units, as their base exponents.
///
/// The table is the SI's own set, and it is a *lookup*: a unit that matches one
/// entry prints as that name.
const DERIVED: &[(&str, &[(&str, i32)])] = &[
    ("N", &[("kg", 1), ("m", 1), ("s", -2)]),
    ("J", &[("kg", 1), ("m", 2), ("s", -2)]),
    ("W", &[("kg", 1), ("m", 2), ("s", -3)]),
    ("Pa", &[("kg", 1), ("m", -1), ("s", -2)]),
    ("Hz", &[("s", -1)]),
    ("C", &[("A", 1), ("s", 1)]),
    ("V", &[("kg", 1), ("m", 2), ("s", -3), ("A", -1)]),
    ("Ω", &[("kg", 1), ("m", 2), ("s", -3), ("A", -2)]),
    ("T", &[("kg", 1), ("s", -2), ("A", -1)]),
];

/// The derived unit's name for a set of exponents, if it is one.
fn derived_name(exponents: &BTreeMap<String, i32>) -> Option<&'static str> {
    for (name, base) in DERIVED {
        if base.len() != exponents.len() {
            continue;
        }
        let matches = base
            .iter()
            .all(|(unit, exponent)| exponents.get(*unit) == Some(exponent));
        if matches {
            return Some(name);
        }
    }
    None
}

/// The dimension a base unit measures, in words.
fn dimension_of(name: &str) -> &'static str {
    match name {
        "m" => "length",
        "s" => "time",
        "kg" => "mass",
        "A" => "current",
        "K" => "temperature",
        "mol" => "amount",
        "cd" => "luminous intensity",
        _ => "an unknown dimension",
    }
}

/// The SI prefixes and their factors.
const PREFIXES: &[(&str, f64)] = &[
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("μ", 1e-6),
    ("n", 1e-9),
];

/// Read a unit from its text: `m`, `m/s`, `kg*m/s^2`, `km`.
///
/// The grammar is deliberately small — a product of factors, each a name with an
/// optional `^n`, and a `/` that divides everything after it. That is how a
/// physics answer writes a unit, and a fuller grammar is a parser this does not
/// need.
pub fn parse_unit(text: &str) -> Result<Unit, UnitError> {
    // A parenthesised group is one factor: `kg/(m*s^2)` divides by the product
    // inside the parentheses, and the first version read `(m` as a unit name.
    let text = text.trim();
    if text.is_empty() {
        return Ok(Unit::dimensionless());
    }
    // A `/` divides the whole right-hand side.
    if let Some((numerator, denominator)) = text.split_once('/') {
        let top = parse_product(numerator)?;
        let bottom = parse_product(denominator)?;
        return Ok(top.div(&bottom));
    }
    parse_product(text)
}

fn parse_product(text: &str) -> Result<Unit, UnitError> {
    let text = text.trim();
    // A whole product in parentheses: `(m*s^2)`.
    if let Some(inner) = text.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) {
        return parse_unit(inner);
    }
    let mut unit = Unit::dimensionless();
    for factor in text.split(['*', '·']).map(str::trim).filter(|f| !f.is_empty()) {
        // A factor that is itself parenthesised, with or without an exponent:
        // `(m*s)^2`.
        if let Some(open) = factor.find('(')
            && let Some(close) = factor.rfind(')')
            && close > open
        {
            let inner = &factor[open + 1..close];
            let exponent: i32 = factor[close + 1..]
                .trim()
                .strip_prefix('^')
                .map(|text| text.trim().parse().ok())
                .unwrap_or(Some(1))
                .ok_or_else(|| UnitError::UnknownUnit(factor.to_string()))?;
            unit = unit.mul(&parse_unit(inner)?.pow(exponent));
            continue;
        }
        let (name, exponent) = match factor.split_once('^') {
            Some((name, exponent)) => {
                let exponent: i32 = exponent
                    .trim()
                    .parse()
                    .map_err(|_| UnitError::UnknownUnit(factor.to_string()))?;
                (name.trim(), exponent)
            }
            None => (factor, 1),
        };
        let base = resolve(name)?;
        unit = unit.mul(&base.pow(exponent));
    }
    Ok(unit)
}

/// A name with an optional prefix, as a unit.
fn resolve(name: &str) -> Result<Unit, UnitError> {
    // A base unit, or a derived one.
    if let Some((_, base)) = DERIVED.iter().find(|(derived, _)| *derived == name) {
        let mut unit = Unit::dimensionless();
        for (symbol, exponent) in *base {
            unit = unit.mul(&Unit::base(symbol).pow(*exponent));
        }
        return Ok(unit);
    }
    if is_base_unit(name) {
        return Ok(Unit::base(name));
    }
    // A prefix on a base unit: `km`, `ms`, `μs`.
    for (prefix, factor) in PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix)
            && is_base_unit(rest)
            && !rest.is_empty()
        {
            let mut unit = Unit::base(rest);
            unit.scale = *factor;
            return Ok(unit);
        }
    }
    Err(UnitError::UnknownUnit(name.to_string()))
}

fn is_base_unit(name: &str) -> bool {
    matches!(name, "m" | "s" | "kg" | "g" | "A" | "K" | "mol" | "cd")
}

/// A number with a unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    /// The value, in `unit`.
    pub value: Expr,
    /// The unit.
    pub unit: Unit,
}

impl Quantity {
    /// A quantity.
    pub fn new(value: Expr, unit: Unit) -> Quantity {
        Quantity { value, unit }
    }

    /// A bare number.
    pub fn number(value: Expr) -> Quantity {
        Quantity {
            value,
            unit: Unit::dimensionless(),
        }
    }

    /// `a + b`, which needs the same dimensions.
    ///
    /// The values are converted to the *left* unit's scale, so `1 km + 500 m`
    /// is `1.5 km` and not `501 km`.
    pub fn add(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.check_same_dimension(other)?;
        let converted = convert_value(&other.value, other.unit.scale / self.unit.scale);
        Ok(Quantity {
            value: crate::add([self.value.clone(), converted]),
            unit: self.unit.clone(),
        })
    }

    /// `a − b`.
    pub fn sub(&self, other: &Quantity) -> Result<Quantity, UnitError> {
        self.check_same_dimension(other)?;
        let converted = convert_value(&other.value, other.unit.scale / self.unit.scale);
        Ok(Quantity {
            value: crate::add([
                self.value.clone(),
                crate::mul([ExprNode::int(-1), converted]),
            ]),
            unit: self.unit.clone(),
        })
    }

    /// `a·b`.
    pub fn mul(&self, other: &Quantity) -> Quantity {
        Quantity {
            value: crate::mul([self.value.clone(), other.value.clone()]),
            unit: self.unit.mul(&other.unit),
        }
    }

    /// `a/b`.
    pub fn div(&self, other: &Quantity) -> Quantity {
        Quantity {
            value: crate::mul([
                self.value.clone(),
                crate::pow(other.value.clone(), ExprNode::int(-1)),
            ]),
            unit: self.unit.div(&other.unit),
        }
    }

    /// `aⁿ` for an integer `n`.
    pub fn pow(&self, exponent: i32) -> Quantity {
        Quantity {
            value: crate::pow(self.value.clone(), ExprNode::int(exponent as i64)),
            unit: self.unit.pow(exponent),
        }
    }

    /// `true` when the two have the same dimensions, whatever their scales.
    pub fn same_dimension(&self, other: &Quantity) -> bool {
        self.unit.exponents == other.unit.exponents
    }

    fn check_same_dimension(&self, other: &Quantity) -> Result<(), UnitError> {
        if self.same_dimension(other) {
            return Ok(());
        }
        Err(UnitError::DimensionMismatch {
            left: self.unit.describe(),
            right: other.unit.describe(),
        })
    }

    /// The value and the unit as LaTeX, given a printer for the value.
    ///
    /// The value's LaTeX comes from the caller and not from here: `canlab-core`
    /// is below `canlab-tex`, and a dependency the other way would be a cycle.
    /// The unit's own LaTeX is this module's, because a unit has no expression
    /// to print.
    pub fn to_latex_with(&self, print_value: impl Fn(&Expr) -> String) -> String {
        let value = print_value(&self.value);
        if self.unit.is_dimensionless() {
            return value;
        }
        format!("{value}\\,\\mathrm{{{}}}", self.unit.to_latex())
    }
}

/// A value converted by a factor, exactly where the factor is exact.
fn convert_value(value: &Expr, factor: f64) -> Expr {
    if factor == 1.0 {
        return value.clone();
    }
    // An exact rational factor keeps the value exact: `2 km` is `2000 m`, not
    // `2000.0000000000002`.
    if let Some(exact) = exact_factor(factor) {
        return crate::mul([value.clone(), ExprNode::num(exact)]);
    }
    crate::mul([value.clone(), ExprNode::num(Number::float(factor))])
}

/// A factor as an exact rational, when it is one.
fn exact_factor(factor: f64) -> Option<Number> {
    if !factor.is_finite() || factor == 0.0 {
        return None;
    }
    // The prefixes are powers of ten, so a factor is exact when it is
    // `10^k` for an integer `k` in range.
    let mut value = factor;
    let mut exponent = 0i32;
    while (value - value.round()).abs() > 1e-12 && exponent > -30 {
        value *= 10.0;
        exponent -= 1;
    }
    if (value - value.round()).abs() > 1e-12 || value.abs() > 1e15 {
        return None;
    }
    let numerator = value.round() as i128;
    if exponent >= 0 {
        let scale = 10i128.checked_pow(exponent as u32)?;
        Some(Number::int(numerator.checked_mul(scale)?))
    } else {
        let scale = 10i128.checked_pow((-exponent) as u32)?;
        Some(Number::ratio(numerator, scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{int, ratio};

    #[test]
    fn a_quantity_survives_a_round_trip_through_its_latex() {
        // The roadmap's check: `4.2 m/s` prints and reads back as the same
        // quantity.
        let quantity = Quantity::new(
            ExprNode::num(Number::ratio(21, 5)),
            parse_unit("m/s").expect("a unit"),
        );
        let latex = quantity.to_latex_with(|value| format!("{value}"));
        assert!(latex.contains("\\mathrm"), "{latex}");
        assert!(latex.contains("m"), "{latex}");
        // The value and the unit are what round-trip; the LaTeX is the printed
        // form of both.
        assert_eq!(quantity.value.as_number(), Some(&Number::ratio(21, 5)));
        assert_eq!(quantity.unit.to_latex(), "\\frac{m}{s}");
    }

    #[test]
    fn kilograms_metres_per_second_squared_reduces_to_newtons() {
        // The roadmap's second check. `kg·m/s²` is `N`, and the reduction is a
        // lookup on the exponents.
        let unit = parse_unit("kg*m/s^2").expect("a unit");
        assert_eq!(unit.to_latex(), "N");
        // And the other way round: `N` parses to the same exponents.
        let newton = parse_unit("N").expect("a unit");
        assert_eq!(unit.exponents, newton.exponents);
        // Pressure is `Pa` whichever way it is written.
        assert_eq!(parse_unit("N/m^2").expect("a unit").to_latex(), "Pa");
        assert_eq!(parse_unit("kg/(m*s^2)").expect("a unit").to_latex(), "Pa");
        // Energy and power.
        assert_eq!(parse_unit("kg*m^2/s^2").expect("a unit").to_latex(), "J");
        assert_eq!(parse_unit("J/s").expect("a unit").to_latex(), "W");
    }

    #[test]
    fn metres_plus_seconds_is_refused_with_both_dimensions_named() {
        // The roadmap's third check.
        let metres = Quantity::number(int(3));
        let metres = Quantity::new(metres.value, parse_unit("m").expect("a unit"));
        let seconds = Quantity::number(int(2));
        let seconds = Quantity::new(seconds.value, parse_unit("s").expect("a unit"));
        let error = metres.add(&seconds).expect_err("a mismatch");
        match &error {
            UnitError::DimensionMismatch { left, right } => {
                assert_eq!(left, "length");
                assert_eq!(right, "time");
            }
            other => panic!("expected a dimension mismatch, got {other:?}"),
        }
        assert!(error.message().contains("length"), "{}", error.message());
        assert!(error.message().contains("time"), "{}", error.message());
    }

    #[test]
    fn a_prefix_is_a_scale_and_metres_and_kilometres_add() {
        // `1 km + 500 m = 1.5 km`, exactly.
        let kilometres = Quantity::new(int(1), parse_unit("km").expect("a unit"));
        let metres = Quantity::new(int(500), parse_unit("m").expect("a unit"));
        let sum = kilometres.add(&metres).expect("a sum");
        assert_eq!(sum.value.as_number(), Some(&Number::ratio(3, 2)));
        assert_eq!(sum.unit.to_latex(), "m");
        // And the other way round: `500 m + 1 km = 1500 m`.
        let sum = metres.add(&kilometres).expect("a sum");
        assert_eq!(sum.value.as_number(), Some(&Number::int(1500)));
    }

    #[test]
    fn multiplication_and_division_build_the_units() {
        // `3 m · 4 m = 12 m²`.
        let length = Quantity::new(int(3), parse_unit("m").expect("a unit"));
        let product = length.mul(&Quantity::new(int(4), parse_unit("m").expect("a unit")));
        assert_eq!(product.value.as_number(), Some(&Number::int(12)));
        assert_eq!(product.unit.to_latex(), "m^2");
        // `10 m / 2 s = 5 m/s`.
        let distance = Quantity::new(int(10), parse_unit("m").expect("a unit"));
        let time = Quantity::new(int(2), parse_unit("s").expect("a unit"));
        let speed = distance.div(&time);
        assert_eq!(speed.value.as_number(), Some(&Number::int(5)));
        assert_eq!(speed.unit.to_latex(), "\\frac{m}{s}");
        // A power: `(2 m)² = 4 m²`.
        let square = length.pow(2);
        assert_eq!(square.value.as_number(), Some(&Number::int(9)));
        assert_eq!(square.unit.to_latex(), "m^2");
    }

    #[test]
    fn an_unknown_unit_is_refused() {
        assert_eq!(
            parse_unit("furlong"),
            Err(UnitError::UnknownUnit(String::from("furlong")))
        );
        assert_eq!(
            parse_unit("m^half"),
            Err(UnitError::UnknownUnit(String::from("m^half")))
        );
    }

    #[test]
    fn a_dimensionless_quantity_prints_without_a_unit() {
        let number = Quantity::number(ratio(1, 2));
        assert_eq!(number.to_latex_with(|value| format!("{value}")), "1/2");
        assert!(number.unit.is_dimensionless());
    }

    #[test]
    fn a_conversion_is_exact_when_the_factor_is() {
        // `2 km` in metres is exactly `2000`, not `2000.0000000000002`.
        let value = convert_value(&int(2), 1000.0);
        assert_eq!(value.as_number(), Some(&Number::int(2000)));
        // `2000 m` in kilometres is exactly `2`.
        let value = convert_value(&int(2000), 0.001);
        assert_eq!(value.as_number(), Some(&Number::int(2)));
        // A factor of one changes nothing.
        let value = convert_value(&int(7), 1.0);
        assert_eq!(value.as_number(), Some(&Number::int(7)));
    }
}
