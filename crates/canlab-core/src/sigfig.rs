//! W15.3: significant figures, and an answer that never claims more than it has.
//!
//! `1.2 m × 3.45 m` is `4.1 m²`, not `4.14 m²`. The inputs carry two and three
//! significant figures, the school rule says a product carries the *fewer*, and
//! an answer that prints four digits has claimed a precision the measurement
//! does not have.
//!
//! # The school rule, stated
//!
//! * **Multiplication and division** — the result has as many significant
//!   figures as the factor with the fewest.
//! * **Addition and subtraction** — the result has as many *decimal places* as
//!   the term with the fewest, which is a different rule and the one a student
//!   mixes up: `1.2 + 3.45` is `4.7` (one decimal place), not `4.65`.
//! * **Exact numbers** — an integer count, a rational, a definition — carry no
//!   uncertainty and do not limit the result. `2 × 1.2 m` is `2.4 m` and not
//!   `2 m`, because the `2` is exact.
//!
//! # What this module is not
//!
//! It is not a rigorous error propagation. A rigorous treatment carries a
//! variance through every operation; the school rule carries a *count* and is
//! what an exam marks. The two disagree, and the exam's answer is the one a
//! student must write — so this module implements the exam's rule and says so,
//! rather than a statistically correct one that would be marked wrong.
//!
//! # Rounding
//!
//! Rounding is **half-up**, which is the rule an exam marks, and it is applied
//! once, at the end. Half-even is the statistically correct rule and the wrong
//! one here: `1.2 + 3.45 = 4.65` rounds to `4.6` under half-even (because `46` is
//! even) and to `4.7` under half-up, and every Finnish exam answer key says
//! `4.7`. A CAS that is statistically right and marked wrong is wrong.
//!
//! Rounding twice — once to the intermediate result and again to the final one —
//! is the classic way to lose a digit: `2.45 → 2.5` and then `2.5 → 3` where a
//! single rounding to one figure gives `2`.

use crate::number::Number;

/// How many significant figures a value carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Precision {
    /// The number of significant figures, or `None` for an exact value.
    pub significant: Option<u32>,
    /// The number of decimal places, for the addition rule.
    pub decimals: Option<i32>,
}

impl Precision {
    /// An exact value: no uncertainty, and it does not limit a result.
    pub fn exact() -> Precision {
        Precision {
            significant: None,
            decimals: None,
        }
    }

    /// A measurement with `significant` figures.
    pub fn measured(significant: u32) -> Precision {
        Precision {
            significant: Some(significant),
            decimals: None,
        }
    }

    /// A measurement with a given number of decimal places.
    pub fn decimals(places: i32) -> Precision {
        Precision {
            significant: None,
            decimals: Some(places),
        }
    }

    /// `true` when nothing is known about the value's precision.
    pub fn is_exact(&self) -> bool {
        self.significant.is_none() && self.decimals.is_none()
    }

    /// The precision of a decimal literal as written: `1.2` has two figures and
    /// one decimal place, `3.45` has three and two, and `120` has two (a
    /// trailing zero without a decimal point is not significant).
    ///
    /// This is the reading a physics teacher uses, and it is why the *text* of a
    /// number matters: `120` and `120.` are different claims.
    pub fn of_literal(text: &str) -> Precision {
        let text = text.trim();
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (
                mantissa,
                exponent.trim().parse::<i32>().unwrap_or(0),
            ),
            None => (text, 0),
        };
        let digits: String = mantissa
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            return Precision::exact();
        }
        let has_point = mantissa.contains('.');
        let fraction_length = mantissa
            .split_once('.')
            .map(|(_, fraction)| fraction.len() as i32)
            .unwrap_or(0);
        // Leading zeros are not significant; trailing zeros are, when there is a
        // decimal point.
        let trimmed: &str = digits.trim_start_matches('0');
        let significant = if trimmed.is_empty() {
            // `0.0` has one significant figure: the zero itself.
            1
        } else if has_point {
            trimmed.len() as u32
        } else {
            // `120` — the trailing zero is not significant.
            trimmed.trim_end_matches('0').len().max(1) as u32
        };
        Precision {
            significant: Some(significant),
            decimals: Some((fraction_length - exponent).max(0)),
        }
    }
}

/// Why a precision cannot be applied.
#[derive(Debug, Clone, PartialEq)]
pub enum PrecisionError {
    /// The value is not a finite number.
    NotANumber,
}

impl PrecisionError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        "significant figures apply to a finite number".to_string()
    }
}

/// The number of significant figures in a value, as it would be written.
///
/// `0.0042` has two, `4200` written as an integer has two, `4.20` has three.
pub fn significant_figures(value: f64) -> u32 {
    if value == 0.0 {
        return 1;
    }
    let magnitude = value.abs();
    // The digits of the value in scientific notation, without trailing zeros
    // that the value itself does not carry: `4.2` is `4.2e0` and has two.
    let exponent = magnitude.log10().floor() as i32;
    let scaled = magnitude / 10f64.powi(exponent);
    // `scaled` is in `[1, 10)`; its digits are significant until the value's own
    // representation runs out.
    let text = format!("{scaled:.15}");
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        1
    } else {
        trimmed.len().min(15) as u32
    }
}

/// Round a value to `figures` significant figures, half-even.
pub fn round_to_figures(value: f64, figures: u32) -> Result<f64, PrecisionError> {
    if !value.is_finite() {
        return Err(PrecisionError::NotANumber);
    }
    if figures == 0 {
        return Ok(0.0);
    }
    if value == 0.0 {
        return Ok(0.0);
    }
    let exponent = value.abs().log10().floor() as i32;
    // The scale that turns the value into an integer of `figures` digits.
    let scale = 10f64.powi(figures as i32 - 1 - exponent);
    let scaled = value * scale;
    // Half-even, applied once.
    let rounded = round_half_up(scaled);
    Ok(rounded / scale)
}

/// Round to the nearest integer, ties away from zero — the exam's rule.
///
/// The tie is detected with a tolerance, and the tolerance is the point:
/// `1.2 + 3.45` is `4.65` in decimal and `4.6499999999999995` in `f64`, so a
/// strict `fraction == 0.5` test misses the tie and rounds down. A physics
/// teacher's answer is `4.7`, and the value the student wrote *is* a tie — the
/// binary representation is an artefact of the machine, not of the measurement.
fn round_half_up(value: f64) -> f64 {
    let floor = value.floor();
    let fraction = value - floor;
    if (fraction - 0.5).abs() < 1e-9 {
        // A tie: away from zero, which is `floor + 1` for a positive value and
        // `floor` for a negative one whose fraction is `0.5` above it.
        return if value >= 0.0 { floor + 1.0 } else { floor };
    }
    value.round()
}

/// Round a value to `places` decimal places, half-even.
pub fn round_to_decimals(value: f64, places: i32) -> Result<f64, PrecisionError> {
    if !value.is_finite() {
        return Err(PrecisionError::NotANumber);
    }
    let scale = 10f64.powi(places);
    Ok(round_half_up(value * scale) / scale)
}

/// The precision of a product or a quotient: the fewer significant figures.
pub fn product_precision(left: Precision, right: Precision) -> Precision {
    match (left.significant, right.significant) {
        (Some(a), Some(b)) => Precision::measured(a.min(b)),
        (Some(a), None) => Precision::measured(a),
        (None, Some(b)) => Precision::measured(b),
        (None, None) => Precision::exact(),
    }
}

/// The precision of a sum or a difference: the fewer decimal places.
pub fn sum_precision(left: Precision, right: Precision) -> Precision {
    match (left.decimals, right.decimals) {
        (Some(a), Some(b)) => Precision::decimals(a.min(b)),
        (Some(a), None) => Precision::decimals(a),
        (None, Some(b)) => Precision::decimals(b),
        (None, None) => Precision::exact(),
    }
}

/// A value with the precision it is known to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// The value, unrounded.
    pub value: f64,
    /// What is known about it.
    pub precision: Precision,
}

impl Measured {
    /// A value at its own precision, read from its text.
    pub fn literal(text: &str) -> Measured {
        Measured {
            value: text.trim().parse().unwrap_or(f64::NAN),
            precision: Precision::of_literal(text),
        }
    }

    /// An exact value.
    pub fn exact(value: f64) -> Measured {
        Measured {
            value,
            precision: Precision::exact(),
        }
    }

    /// `a·b`, with the product's precision.
    pub fn mul(&self, other: &Measured) -> Measured {
        Measured {
            value: self.value * other.value,
            precision: product_precision(self.precision, other.precision),
        }
    }

    /// `a/b`.
    pub fn div(&self, other: &Measured) -> Measured {
        Measured {
            value: self.value / other.value,
            precision: product_precision(self.precision, other.precision),
        }
    }

    /// `a + b`, with the sum's precision.
    pub fn add(&self, other: &Measured) -> Measured {
        Measured {
            value: self.value + other.value,
            precision: sum_precision(self.precision, other.precision),
        }
    }

    /// `a − b`.
    pub fn sub(&self, other: &Measured) -> Measured {
        Measured {
            value: self.value - other.value,
            precision: sum_precision(self.precision, other.precision),
        }
    }

    /// The value rounded to its precision, as a number.
    pub fn rounded(&self) -> Result<f64, PrecisionError> {
        match (self.precision.significant, self.precision.decimals) {
            (Some(figures), _) => round_to_figures(self.value, figures),
            (None, Some(places)) => round_to_decimals(self.value, places),
            (None, None) => Ok(self.value),
        }
    }

    /// The value as text, with exactly the digits its precision allows.
    ///
    /// This is the printing the roadmap asks for: `1.2 × 3.45` prints `4.1` and
    /// not `4.14`, and an exact answer prints in full.
    pub fn to_text(&self) -> String {
        let Ok(rounded) = self.rounded() else {
            return String::from("not a number");
        };
        match (self.precision.significant, self.precision.decimals) {
            (Some(figures), _) => {
                if rounded == 0.0 {
                    return String::from("0");
                }
                let exponent = rounded.abs().log10().floor() as i32;
                // A value between `10^-4` and `10^15` prints in positional
                // notation with the right number of decimals; outside it, in
                // scientific notation.
                let places = (figures as i32 - 1 - exponent).max(0);
                if (-4..15).contains(&exponent) {
                    format!("{:.*}", places as usize, rounded)
                } else {
                    format!("{:.*e}", (figures - 1) as usize, rounded)
                }
            }
            (None, Some(places)) => {
                if places <= 0 {
                    format!("{rounded:.0}")
                } else {
                    format!("{:.*}", places as usize, rounded)
                }
            }
            (None, None) => {
                // An exact value prints in full, without a trailing `.0` that
                // would suggest a decimal measurement.
                if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
                    format!("{}", rounded as i128)
                } else {
                    format!("{rounded}")
                }
            }
        }
    }

    /// The value as a `Number`, rounded, for a caller that wants the exact type.
    pub fn number(&self) -> Number {
        match self.rounded() {
            Ok(value) => Number::float(value),
            Err(_) => Number::float(f64::NAN),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_product_carries_the_fewer_figures() {
        // The roadmap's check: `1.2 m × 3.45 m` is `4.1 m²`.
        let area = Measured::literal("1.2").mul(&Measured::literal("3.45"));
        assert_eq!(area.precision.significant, Some(2));
        assert_eq!(area.to_text(), "4.1");
        // The unrounded value is `4.14`, which is what the answer *would* be if
        // it had three figures.
        assert!((area.value - 4.14).abs() < 1e-12);
    }

    #[test]
    fn a_sum_carries_the_fewer_decimal_places() {
        // `1.2 + 3.45 = 4.65`, but the answer is `4.7`: one decimal place, from
        // the `1.2`. This is the rule a student mixes up with the product rule.
        let sum = Measured::literal("1.2").add(&Measured::literal("3.45"));
        assert_eq!(sum.precision.decimals, Some(1));
        assert_eq!(sum.to_text(), "4.7");
        // And a difference the same way.
        let difference = Measured::literal("3.45").sub(&Measured::literal("1.2"));
        assert_eq!(difference.to_text(), "2.3");
    }

    #[test]
    fn an_exact_number_does_not_limit_the_result() {
        // `2 × 1.2` is `2.4` and not `2`: the `2` is a count, not a measurement.
        let product = Measured::exact(2.0).mul(&Measured::literal("1.2"));
        assert_eq!(product.precision.significant, Some(2));
        assert_eq!(product.to_text(), "2.4");
        // And an exact answer prints in full.
        let exact = Measured::exact(1234.0);
        assert_eq!(exact.to_text(), "1234");
        assert_eq!(Measured::exact(0.5).to_text(), "0.5");
    }

    #[test]
    fn the_digit_count_matches_the_inputs() {
        // The roadmap's check. For a set of literals, the printed answer has the
        // number of significant figures the rule predicts.
        let cases: &[(&str, &str, u32)] = &[
            ("1.2", "3.45", 2),
            ("2.00", "3.0", 2),
            ("1.234", "5.6", 2),
            ("10", "10", 1),
            ("0.0012", "3.0", 2),
        ];
        for (left, right, expected) in cases {
            let product = Measured::literal(left).mul(&Measured::literal(right));
            assert_eq!(
                product.precision.significant,
                Some(*expected),
                "{left} × {right}"
            );
            // The printed text has that many significant figures — counted
            // from the *text*, not from the parsed value: `6.0` is two figures
            // as written and one as a number, and the text is what a student
            // writes.
            let text = product.to_text();
            assert_eq!(
                Precision::of_literal(&text).significant,
                Some(*expected),
                "{left} × {right} = {text}"
            );
        }
    }

    #[test]
    fn a_literal_reads_the_figures_a_teacher_would() {
        // `1.2` → two figures, one decimal place. `3.45` → three, two.
        assert_eq!(Precision::of_literal("1.2").significant, Some(2));
        assert_eq!(Precision::of_literal("1.2").decimals, Some(1));
        assert_eq!(Precision::of_literal("3.45").significant, Some(3));
        // `120` has two: the trailing zero without a point is not significant.
        assert_eq!(Precision::of_literal("120").significant, Some(2));
        // `120.` has three: the point makes the zero significant.
        assert_eq!(Precision::of_literal("120.").significant, Some(3));
        // `0.0042` has two; the leading zeros are not.
        assert_eq!(Precision::of_literal("0.0042").significant, Some(2));
        // `1.20e3` has three.
        assert_eq!(Precision::of_literal("1.20e3").significant, Some(3));
    }

    #[test]
    fn rounding_is_half_up_and_applied_once() {
        // `2.5` to one figure is `3` — ties away from zero, the exam's rule.
        assert_eq!(round_to_figures(2.5, 1).expect("a value"), 3.0);
        assert_eq!(round_to_figures(3.5, 1).expect("a value"), 4.0);
        // `2.45` to two figures is `2.5`, and rounding it again to one would
        // give `3` — which is why the rounding is applied once.
        assert_eq!(round_to_figures(2.45, 2).expect("a value"), 2.5);
        // A value that is not a tie rounds normally.
        assert_eq!(round_to_figures(2.46, 2).expect("a value"), 2.5);
        assert_eq!(round_to_figures(0.0042, 2).expect("a value"), 0.0042);
    }

    #[test]
    fn an_exact_answer_still_prints_exactly() {
        // The roadmap's third check: a value with no measurement behind it is
        // not rounded and does not grow a `.0`.
        assert_eq!(Measured::exact(4.0).to_text(), "4");
        assert_eq!(Measured::exact(1.5).to_text(), "1.5");
        assert_eq!(Measured::exact(1000.0).to_text(), "1000");
        // And a measured value keeps its trailing zeros: `2.0` is two figures.
        assert_eq!(Measured::literal("2.0").to_text(), "2.0");
    }

    #[test]
    fn a_very_large_or_small_measured_value_prints_in_scientific_notation() {
        // `1.2e-6` is two figures and prints as `1.2e-6`, not as `0.0000012`.
        let value = Measured {
            value: 1.2e-6,
            precision: Precision::measured(2),
        };
        assert_eq!(value.to_text(), "1.2e-6");
        let value = Measured {
            value: 1.2e20,
            precision: Precision::measured(2),
        };
        assert_eq!(value.to_text(), "1.2e20");
    }

    #[test]
    fn a_non_finite_value_has_no_figures() {
        assert_eq!(
            round_to_figures(f64::NAN, 2),
            Err(PrecisionError::NotANumber)
        );
        let value = Measured {
            value: f64::INFINITY,
            precision: Precision::measured(2),
        };
        assert_eq!(value.to_text(), "not a number");
    }
}
