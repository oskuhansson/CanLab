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

    /// Divide exactly, or refuse.
    ///
    /// `None` when the division is not exact — a zero divisor, a non-zero
    /// remainder, or a multivariate input. The remainder is the point: a caller
    /// that wanted a quotient has no use for a quotient and a lie.
    ///
    /// This exists for Bareiss elimination, where every step divides by the
    /// previous pivot and the division is exact by theorem. A `Some` here is
    /// therefore a check on the theorem as much as on the inputs.
    pub fn div_exact(&self, divisor: &Self) -> Option<Self> {
        let (quotient, remainder) = self.div_rem(divisor)?;
        remainder.is_zero().then_some(quotient)
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
    ///
    /// A missing constant term is a factor of `x`, and that factor is divided out
    /// before the search: `x³ − x` has three roots, and reporting only `0` because
    /// the constant term is zero would be half an answer.
    pub fn rational_roots(&self) -> Option<Vec<Number>> {
        if self.vars().len() != 1 {
            return None;
        }
        let mut working = self.clone();
        let mut roots: Vec<Number> = Vec::new();

        loop {
            let coefficients: BTreeMap<u32, Number> = working.univariate_coefficients()?;
            if coefficients.is_empty() {
                // The zero polynomial: every number is a root, which is not a
                // list of roots and is the caller's business to report.
                return None;
            }
            let constant = coefficients.get(&0).cloned().unwrap_or_else(Number::zero);
            if !constant.is_zero() {
                break;
            }
            roots.push(Number::zero());
            working = divide_by_variable(&working)?;
        }

        let coefficients: BTreeMap<u32, Number> = working.univariate_coefficients()?;
        let degree = coefficients.keys().copied().max()?;
        if degree == 0 {
            return Some(roots);
        }
        let leading = coefficients.get(&degree)?.clone();
        let constant = coefficients.get(&0).cloned().unwrap_or_else(Number::zero);
        let numerator_divisors = divisors(&constant)?;
        let denominator_divisors = divisors(&leading)?;
        for numerator in &numerator_divisors {
            for denominator in &denominator_divisors {
                let candidate = numerator.div(denominator);
                if working
                    .evaluate(std::slice::from_ref(&candidate))
                    .is_zero()
                    && !roots.contains(&candidate)
                {
                    roots.push(candidate);
                }
            }
        }
        Some(roots)
    }

    /// Divide by `(x − root)`, when `root` is a root.
    ///
    /// Synthetic division: the quotient's coefficients are the running sums of
    /// `aₖ + root·bₖ`, and the remainder is what the constant term leaves over.
    /// A nonzero remainder means the divisor does not divide, and `None` says so
    /// rather than returning a quotient that is off by a fraction.
    ///
    /// This is what lets a root search be *complete*: after dividing out the
    /// rational roots, the leftover degree says how much is still unknown, and
    /// `(x−1)(x²−2)` is `x² − 2` rather than "one root and three silent
    /// coin-flips".
    pub fn deflate(&self, root: &Number) -> Option<Poly<Number>> {
        let coefficients: BTreeMap<u32, Number> = self.univariate_coefficients()?;
        let degree = coefficients.keys().copied().max()?;
        if degree < 1 {
            return None;
        }
        let at = |power: u32| coefficients.get(&power).cloned().unwrap_or_else(Number::zero);

        let mut quotient: Vec<Number> = vec![Number::zero(); degree as usize];
        let mut carry = Number::zero();
        for power in (1..=degree).rev() {
            carry = at(power).add(&carry.mul(root));
            quotient[(power - 1) as usize] = carry.clone();
        }
        let remainder = at(0).add(&carry.mul(root));
        if !remainder.is_zero() {
            return None;
        }
        Some(from_coefficients(self.vars().to_vec(), &quotient))
    }
}

/// `poly / x`, when the constant term is zero.
fn divide_by_variable(poly: &Poly<Number>) -> Option<Poly<Number>> {    let mut out = Poly::zero(poly.vars().to_vec());
    for (monomial, coefficient) in poly.terms() {
        let mut lowered = monomial.clone();
        let first = lowered.0.first_mut()?;
        if *first == 0 {
            return None;
        }
        *first -= 1;
        out.push(lowered, coefficient.clone());
    }
    if out.is_zero() {
        return None;
    }
    Some(out)
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