//! Euclidean division, greatest common divisors, resultants and rational roots.
//!
//! All of this is univariate on purpose. A multivariate quotient is only defined
//! relative to a monomial order and a divisor set, and a wrong quotient is worse
//! than a refusal — so the multivariate cases return `None`.

use std::collections::BTreeMap;

use canlab_core::{Number, SymbolId};

use crate::{Coeff, Monomial, Poly};

impl<C: Coeff> Poly<C> {
    /// Divide by another polynomial in the same single variable.
    ///
    /// Returns `(quotient, remainder)`, or `None` when either polynomial is
    /// multivariate, when the divisor is zero, or when a coefficient cannot be
    /// divided (a ring without inverses).
    pub fn div_rem(&self, divisor: &Self) -> Option<(Self, Self)> {
        if self.vars() != divisor.vars() || self.vars().len() != 1 {
            return None;
        }
        if divisor.is_zero() {
            return None;
        }
        let mut remainder = self.clone();
        let mut quotient = Self::zero(self.vars().to_vec());
        let divisor_degree = divisor.degree_in(0);
        let divisor_leading = leading_coefficient(divisor)?;

        while !remainder.is_zero() && remainder.degree_in(0) >= divisor_degree {
            let remainder_leading = leading_coefficient(&remainder)?;
            let factor = remainder_leading.try_div(&divisor_leading)?;
            let shift = Monomial(vec![remainder.degree_in(0) - divisor_degree]);
            quotient.push(shift.clone(), factor.clone());
            let subtracted = shift_exponents(&divisor.scale(&factor), &shift);
            remainder = remainder.sub(&subtracted)?;
        }

        Some((quotient, remainder))
    }

    /// The greatest common divisor, monic when the ring allows it.
    ///
    /// Euclid's algorithm. Two zero polynomials have no divisor to return, and a
    /// multivariate input is refused rather than guessed at.
    pub fn gcd(&self, other: &Self) -> Option<Self> {
        if self.vars() != other.vars() || self.vars().len() != 1 {
            return None;
        }
        let mut a = self.clone();
        let mut b = other.clone();
        while !b.is_zero() {
            let (_, remainder) = a.div_rem(&b)?;
            a = b;
            b = remainder;
        }
        if a.is_zero() {
            return None;
        }
        let leading = leading_coefficient(&a)?;
        let (quotient, _) = a.div_rem(&Poly::constant(a.vars().to_vec(), leading))?;
        Some(quotient)
    }

    /// The resultant of two univariate polynomials, as the determinant of their
    /// Sylvester matrix.
    ///
    /// Zero exactly when they share a root, which is what makes it useful for
    /// eliminating a variable from a system.
    pub fn resultant(&self, other: &Self) -> Option<C> {
        if self.vars() != other.vars() || self.vars().len() != 1 {
            return None;
        }
        let m = self.degree_in(0) as usize;
        let n = other.degree_in(0) as usize;
        let size = m + n;
        if size == 0 {
            return Some(C::one());
        }
        let self_coefficients = coefficients_descending(self, m);
        let other_coefficients = coefficients_descending(other, n);

        let mut matrix = vec![vec![C::zero(); size]; size];
        for row in 0..n {
            for (offset, coefficient) in self_coefficients.iter().enumerate() {
                if row + offset < size {
                    matrix[row][row + offset] = coefficient.clone();
                }
            }
        }
        for row in 0..m {
            for (offset, coefficient) in other_coefficients.iter().enumerate() {
                // The second block shifts `g` the same way, starting at column
                // `row` — not at `n + row`. Getting that wrong still produces a
                // determinant, just not the resultant.
                if row + offset < size {
                    matrix[n + row][row + offset] = coefficient.clone();
                }
            }
        }
        Some(determinant(&mut matrix))
    }
}

impl Poly<Number> {
    /// The exact rational roots of a univariate polynomial.
    ///
    /// The rational root theorem: a root `p/q` in lowest terms has `p` dividing
    /// the constant term and `q` dividing the leading coefficient. Each distinct
    /// root appears once, which is what "the roots are" means.
    pub fn rational_roots(&self) -> Option<Vec<Number>> {
        if self.vars().len() != 1 {
            return None;
        }
        let coefficients: BTreeMap<u32, Number> = self.univariate_coefficients()?;
        let degree = coefficients.keys().copied().max()?;
        if degree == 0 {
            return None;
        }
        let leading = coefficients.get(&degree)?.clone();
        let constant = coefficients.get(&0).cloned().unwrap_or_else(Number::zero);
        if constant.is_zero() {
            return Some(vec![Number::zero()]);
        }
        let numerator_divisors = divisors(&constant)?;
        let denominator_divisors = divisors(&leading)?;
        let mut roots: Vec<Number> = Vec::new();
        for numerator in &numerator_divisors {
            for denominator in &denominator_divisors {
                let candidate = numerator.div(denominator);
                if self.evaluate(std::slice::from_ref(&candidate)).is_zero()
                    && !roots.contains(&candidate)
                {
                    roots.push(candidate);
                }
            }
        }
        Some(roots)
    }
}

/// The coefficient of the highest power present.
fn leading_coefficient<C: Coeff>(poly: &Poly<C>) -> Option<C> {
    let degree = poly.degree_in(0);
    poly.terms()
        .find(|(monomial, _)| monomial.0.first().copied() == Some(degree))
        .map(|(_, coefficient)| coefficient.clone())
}

/// Multiply every monomial by `monomial`.
fn shift_exponents<C: Coeff>(poly: &Poly<C>, monomial: &Monomial) -> Poly<C> {
    let mut shifted = Poly::zero(poly.vars().to_vec());
    for (term, coefficient) in poly.terms() {
        shifted.push(term.multiply(monomial), coefficient.clone());
    }
    shifted
}

/// Coefficients from the highest power down, filling missing powers with zero.
fn coefficients_descending<C: Coeff>(poly: &Poly<C>, degree: usize) -> Vec<C> {
    let mut coefficients = vec![C::zero(); degree + 1];
    for (monomial, coefficient) in poly.terms() {
        if let Some(power) = monomial.0.first().copied() {
            let index = degree.saturating_sub(power as usize);
            if index < coefficients.len() {
                coefficients[index] = coefficient.clone();
            }
        }
    }
    coefficients
}

/// Determinant by Gaussian elimination over a field.
///
/// A singular matrix has determinant zero, which is an answer; only a ring that
/// cannot divide is a `None`.
fn determinant<C: Coeff>(matrix: &mut [Vec<C>]) -> C {
    let size = matrix.len();
    let mut result = C::one();
    for column in 0..size {
        let Some(pivot) = (column..size).find(|row| !matrix[*row][column].is_zero()) else {
            return C::zero();
        };
        if pivot != column {
            matrix.swap(pivot, column);
            result = result.neg();
        }
        let pivot_value = matrix[column][column].clone();
        result = result.mul(&pivot_value);
        for row in (column + 1)..size {
            let Some(factor) = matrix[row][column].try_div(&pivot_value) else {
                // Not a field: leave the remaining rows alone. The result is then
                // only meaningful when the caller's ring divides, which the
                // univariate path guarantees for `Number`.
                break;
            };
            if factor.is_zero() {
                continue;
            }
            for inner in column..size {
                let reduce = factor.mul(&matrix[column][inner]);
                matrix[row][inner] = matrix[row][inner].sub(&reduce);
            }
        }
    }
    result
}

/// Every divisor of a rational number, positive and negative.
fn divisors(number: &Number) -> Option<Vec<Number>> {    let value = number.to_f64();
    if !value.is_finite() || value == 0.0 {
        return None;
    }
    let magnitude = value.abs();
    // Only honest for small integers. A float coefficient has no divisors, and a
    // guess would be worse than nothing.
    if (magnitude - magnitude.round()).abs() > 1e-9 || magnitude > 1e6 {
        return None;
    }
    let limit = magnitude.round() as i128;
    let mut out = Vec::new();
    for candidate in 1..=limit {
        if limit % candidate == 0 {
            out.push(Number::int(candidate));
            out.push(Number::int(-candidate));
        }
    }
    Some(out)
}

/// Build a univariate polynomial from its coefficients, lowest power first.
///
/// The variables list must have exactly one entry; anything else is not a
/// univariate polynomial and gets an empty polynomial rather than a panic, which
/// the callers treat as a refusal.
pub fn from_coefficients<C: Coeff>(vars: Vec<SymbolId>, coefficients: &[C]) -> Poly<C> {
    let mut poly = Poly::zero(vars);
    if poly.vars().len() != 1 {
        return poly;
    }
    for (power, coefficient) in coefficients.iter().enumerate() {
        poly.push(Monomial(vec![power as u32]), coefficient.clone());
    }
    poly
}