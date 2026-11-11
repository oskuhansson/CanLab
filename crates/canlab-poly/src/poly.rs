//! The polynomial itself: a map from monomials to coefficients.

use std::collections::BTreeMap;

use canlab_core::{Expr, ExprNode, Number, SymbolId};

use crate::{Coeff, Monomial};

/// A sparse polynomial in a fixed list of variables.
///
/// Equality is structural: two polynomials are equal when they have the same
/// variables in the same order and the same non-zero terms. That is exactly the
/// canonical form `to_expr` prints, so `to_expr(from_expr(e)) == normalize(e)` is
/// a meaningful test rather than a coincidence.
#[derive(Clone, Debug, PartialEq)]
pub struct Poly<C> {
    vars: Vec<SymbolId>,
    terms: BTreeMap<Monomial, C>,
}

impl<C: Coeff> Poly<C> {
    /// The zero polynomial in `vars`.
    pub fn zero(vars: Vec<SymbolId>) -> Self {
        Self {
            vars,
            terms: BTreeMap::new(),
        }
    }

    /// A constant.
    pub fn constant(vars: Vec<SymbolId>, coefficient: C) -> Self {
        let mut terms = BTreeMap::new();
        if !coefficient.is_zero() {
            terms.insert(Monomial::one(vars.len()), coefficient);
        }
        Self { vars, terms }
    }

    /// The `index`th variable, to the first power.
    pub fn variable(vars: Vec<SymbolId>, index: usize) -> Option<Self> {
        if index >= vars.len() {
            return None;
        }
        let mut terms = BTreeMap::new();
        terms.insert(Monomial::variable(index, vars.len()), C::one());
        Some(Self { vars, terms })
    }

    pub fn vars(&self) -> &[SymbolId] {
        &self.vars
    }

    pub fn terms(&self) -> impl Iterator<Item = (&Monomial, &C)> {
        self.terms.iter()
    }

    pub fn is_zero(&self) -> bool {
        self.terms.is_empty()
    }

    /// Total degree: the sum of the exponents of the highest term.
    pub fn degree(&self) -> u32 {
        self.terms
            .keys()
            .map(Monomial::total_degree)
            .max()
            .unwrap_or(0)
    }

    /// Degree in one variable, ignoring the others.
    pub fn degree_in(&self, index: usize) -> u32 {
        self.terms
            .keys()
            .filter_map(|monomial| monomial.0.get(index).copied())
            .max()
            .unwrap_or(0)
    }

    /// The coefficient of one monomial, or zero.
    pub fn coefficient(&self, monomial: &Monomial) -> C {
        self.terms
            .get(monomial)
            .cloned()
            .unwrap_or_else(C::zero)
    }

    /// Add `term` in place. Zero coefficients are dropped, so the map is sparse.
    pub fn push(&mut self, monomial: Monomial, coefficient: C) {
        if coefficient.is_zero() {
            return;
        }
        match self.terms.get_mut(&monomial) {
            Some(existing) => {
                let sum = existing.add(&coefficient);
                if sum.is_zero() {
                    self.terms.remove(&monomial);
                } else {
                    *existing = sum;
                }
            }
            None => {
                self.terms.insert(monomial, coefficient);
            }
        }
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        if self.vars != other.vars {
            return None;
        }
        let mut sum = self.clone();
        for (monomial, coefficient) in other.terms() {
            sum.push(monomial.clone(), coefficient.clone());
        }
        Some(sum)
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        if self.vars != other.vars {
            return None;
        }
        let mut difference = self.clone();
        for (monomial, coefficient) in other.terms() {
            difference.push(monomial.clone(), coefficient.neg());
        }
        Some(difference)
    }

    pub fn mul(&self, other: &Self) -> Option<Self> {
        if self.vars != other.vars {
            return None;
        }
        let mut product = Self::zero(self.vars.clone());
        for (left_monomial, left) in self.terms() {
            for (right_monomial, right) in other.terms() {
                product.push(
                    left_monomial.multiply(right_monomial),
                    left.mul(right),
                );
            }
        }
        Some(product)
    }

    /// Multiply every coefficient by a scalar.
    pub fn scale(&self, factor: &C) -> Self {
        if factor.is_zero() {
            return Self::zero(self.vars.clone());
        }
        let mut scaled = Self::zero(self.vars.clone());
        for (monomial, coefficient) in self.terms() {
            scaled.push(monomial.clone(), coefficient.mul(factor));
        }
        scaled
    }

    pub fn neg(&self) -> Self {
        self.scale(&C::one().neg())
    }

    /// `self^n`, by repeated squaring.
    pub fn pow(&self, mut exponent: u32) -> Self {
        let mut result = Self::constant(self.vars.clone(), C::one());
        let mut base = self.clone();
        while exponent > 0 {
            if exponent & 1 == 1 {
                result = result.mul(&base).expect("same variables");
            }
            exponent >>= 1;
            if exponent > 0 {
                base = base.mul(&base).expect("same variables");
            }
        }
        result
    }

    /// Substitute a value for every variable.
    pub fn evaluate(&self, values: &[C]) -> C {
        let mut total = C::zero();
        for (monomial, coefficient) in self.terms() {
            let mut term = coefficient.clone();
            for (index, exponent) in monomial.0.iter().enumerate() {
                let value = values.get(index).cloned().unwrap_or_else(C::zero);
                for _ in 0..*exponent {
                    term = term.mul(&value);
                }
            }
            total = total.add(&term);
        }
        total
    }

    /// The formal derivative with respect to one variable.
    pub fn derivative(&self, index: usize) -> Self {
        let mut result = Self::zero(self.vars.clone());
        for (monomial, coefficient) in self.terms() {
            let Some(exponent) = monomial.0.get(index).copied() else {
                continue;
            };
            if exponent == 0 {
                continue;
            }
            let mut lowered = monomial.clone();
            lowered.0[index] = exponent - 1;
            // The coefficient is an arbitrary ring element, so `k·c` is repeated
            // addition rather than a multiplication by a number.
            let mut scaled = C::zero();
            for _ in 0..exponent {
                scaled = scaled.add(coefficient);
            }
            result.push(lowered, scaled);
        }
        result
    }

    /// Print the polynomial as an [`Expr`], deterministically.
    pub fn to_expr(&self) -> Expr {
        let mut terms: Vec<Expr> = Vec::new();
        for (monomial, coefficient) in self.terms() {
            terms.push(self.term_to_expr(monomial, coefficient));
        }
        canlab_core::add(terms)
    }

    /// Read an expression as a polynomial in `vars`.
    ///
    /// `None` for anything that is not a polynomial in those variables: a
    /// function call, a held integral, a variable outside the list, a negative or
    /// fractional power. A document can contain all of those, so this is a
    /// refusal and not a panic.
    pub fn from_expr(expression: &Expr, vars: &[SymbolId]) -> Option<Self> {
        match &**expression {
            ExprNode::Num(number) => Some(Self::constant(vars.to_vec(), C::from_number(number)?)),
            ExprNode::Sym(symbol) => {
                let index = vars.iter().position(|variable| variable == symbol)?;
                Self::variable(vars.to_vec(), index)
            }
            ExprNode::Add(terms) => {
                let mut sum = Self::zero(vars.to_vec());
                for term in terms {
                    sum = sum.add(&Self::from_expr(term, vars)?)?;
                }
                Some(sum)
            }
            ExprNode::Mul(factors) => {
                let mut product = Self::constant(vars.to_vec(), C::one());
                for factor in factors {
                    product = product.mul(&Self::from_expr(factor, vars)?)?;
                }
                Some(product)
            }
            ExprNode::Pow(base, exponent) => {
                let Number::Int(power) = exponent.as_number()? else {
                    return None;
                };
                if *power < 0 || *power > i128::from(u32::MAX) {
                    return None;
                }
                let base = Self::from_expr(base, vars)?;
                Some(base.pow(u32::try_from(*power).ok()?))
            }
            ExprNode::Fn(_, _) | ExprNode::Hold(_) => None,
        }
    }

    /// Every coefficient of one variable, keyed by its power.
    ///
    /// `None` when the polynomial is not univariate, or when a coefficient is not
    /// a constant in the remaining variables — that case needs polynomial
    /// coefficients and a different algorithm.
    pub fn univariate_coefficients(&self) -> Option<BTreeMap<u32, C>> {
        if self.vars.len() != 1 {
            return None;
        }
        let mut coefficients = BTreeMap::new();
        for (monomial, coefficient) in self.terms() {
            let power = monomial.0.first().copied()?;
            coefficients.insert(power, coefficient.clone());
        }
        Some(coefficients)
    }

    fn term_to_expr(&self, monomial: &Monomial, coefficient: &C) -> Expr {
        let mut factors: Vec<Expr> = Vec::new();
        let unit = C::one();
        if *coefficient != unit {
            factors.push(coefficient.to_expr());
        }
        for (index, exponent) in monomial.0.iter().enumerate() {
            if *exponent == 0 {
                continue;
            }
            let variable = ExprNode::sym(self.vars[index]);
            let powered = if *exponent == 1 {
                variable
            } else {
                canlab_core::pow(variable, ExprNode::int(i64::from(*exponent)))
            };
            factors.push(powered);
        }
        if factors.is_empty() {
            canlab_core::add(vec![ExprNode::int(1)])
        } else {
            canlab_core::mul(factors)
        }
    }
}
