//! Sparse multivariate polynomials, over a coefficient type the caller chooses.
//!
//! The algebra engine needs three things this crate provides, and it needs them
//! exact:
//!
//! * **A canonical form.** `to_expr` is deterministic, so two polynomials that are
//!   equal print the same way and a test can compare them without a normalizer.
//! * **Coefficients that are not numbers.** Parametric solving asks "solve
//!   `x² + y² = 1` for `x`", and there the coefficients are `1`, `0` and
//!   `y² − 1` — expressions. `Poly<Expr>` is that case, and it is why the type is
//!   generic from the start rather than being `Poly<Number>` with a second
//!   implementation bolted on later.
//! * **Refusal rather than guessing.** A polynomial in the given variables is a
//!   `Some`; a function call, a negative power or a variable outside the list is
//!   a `None`. Nothing here panics on input from a document.
//!
//! # What a polynomial is not
//!
//! There is no division by a polynomial except in the univariate case, where it is
//! Euclidean and exact. Multivariate division needs a monomial order and a
//! remainder basis to mean anything, and nothing in the plan asks for it yet; a
//! wrong quotient is worse than a refusal.

pub mod expr;
pub mod poly;
pub mod univariate;

pub use expr::{FactorError, cancel, collect, divide, expand, factor, free_symbols};
pub use poly::Poly;

use canlab_core::{Expr, ExprNode, Number, SymbolId};

/// Everything a sparse polynomial needs from its coefficients.
///
/// Two implementations are expected: [`Number`] for exact arithmetic and [`Expr`]
/// for coefficients that are themselves symbolic. Nothing here assumes a field —
/// `try_div` returns `None` when it cannot divide, which is what lets the same
/// code be honest about a ring that has no inverses.
pub trait Coeff: Clone + PartialEq + std::fmt::Debug {
    fn zero() -> Self;
    fn one() -> Self;
    fn is_zero(&self) -> bool;

    fn add(&self, other: &Self) -> Self;
    fn sub(&self, other: &Self) -> Self;
    fn mul(&self, other: &Self) -> Self;
    fn neg(&self) -> Self;

    /// Exact division. `None` when the divisor is zero, or when the ring has no
    /// inverse for it.
    fn try_div(&self, other: &Self) -> Option<Self>;

    /// A `Number` as a coefficient, or `None` if this ring cannot hold it.
    fn from_number(number: &Number) -> Option<Self>;

    /// A symbol that is not one of the polynomial's variables, taken as a
    /// coefficient.
    ///
    /// `Expr` can hold it; `Number` cannot, and saying so is the difference
    /// between `collect(x² + a·x, x)` working with symbolic coefficients and it
    /// being silently refused.
    fn from_symbol(symbol: SymbolId) -> Option<Self>;

    /// Print the coefficient as an expression. A ring that cannot say what its
    /// elements mean has no business being a coefficient of a printed polynomial.
    fn to_expr(&self) -> Expr;
}

impl Coeff for Number {
    fn zero() -> Self {
        Number::zero()
    }

    fn one() -> Self {
        Number::one()
    }

    fn is_zero(&self) -> bool {
        Number::is_zero(self)
    }

    fn add(&self, other: &Self) -> Self {
        Number::add(self, other)
    }

    fn sub(&self, other: &Self) -> Self {
        Number::sub(self, other)
    }

    fn mul(&self, other: &Self) -> Self {
        Number::mul(self, other)
    }

    fn neg(&self) -> Self {
        Number::neg(self)
    }

    fn try_div(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            None
        } else {
            Some(Number::div(self, other))
        }
    }

    fn from_number(number: &Number) -> Option<Self> {
        Some(number.clone())
    }

    fn from_symbol(_symbol: SymbolId) -> Option<Self> {
        // A number cannot absorb `a` in `a·x`.
        None
    }

    fn to_expr(&self) -> Expr {
        ExprNode::num(self.clone())
    }
}

impl Coeff for Expr {
    fn zero() -> Self {
        ExprNode::int(0)
    }

    fn one() -> Self {
        ExprNode::int(1)
    }

    fn is_zero(&self) -> bool {
        // Explicit, not `self.is_zero()`: `Expr` has its own convenience method
        // and an inherent method wins over a trait method, so the obvious spelling
        // would call itself.
        matches!(&**self, ExprNode::Num(number) if number.is_zero())
    }

    fn add(&self, other: &Self) -> Self {
        canlab_core::add(vec![self.clone(), other.clone()])
    }

    fn sub(&self, other: &Self) -> Self {
        canlab_core::add(vec![
            self.clone(),
            canlab_core::mul(vec![ExprNode::int(-1), other.clone()]),
        ])
    }

    fn mul(&self, other: &Self) -> Self {
        canlab_core::mul(vec![self.clone(), other.clone()])
    }

    fn neg(&self) -> Self {
        canlab_core::mul(vec![ExprNode::int(-1), self.clone()])
    }

    fn try_div(&self, other: &Self) -> Option<Self> {
        // An expression has no general inverse. `x / 2` is a coefficient, but
        // `x / y` is not, and guessing turns an answer into a wrong answer.
        if let Some(number) = other.as_number() {
            return Some(canlab_core::mul(vec![
                self.clone(),
                canlab_core::pow(ExprNode::num(number.clone()), ExprNode::int(-1)),
            ]));
        }
        None
    }

    fn from_number(number: &Number) -> Option<Self> {
        Some(ExprNode::num(number.clone()))
    }

    fn from_symbol(symbol: SymbolId) -> Option<Self> {
        Some(ExprNode::sym(symbol))
    }

    fn to_expr(&self) -> Expr {
        self.clone()
    }
}

/// One monomial: an exponent for each variable, in the order the polynomial was
/// built with.
///
/// The derived `Ord` is lexicographic on the exponent vector, which is a total
/// order and therefore enough to make `to_expr` deterministic. It is not the
/// order a Gröbner basis would want; nothing here computes one.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Monomial(pub Vec<u32>);

impl Monomial {
    /// The monomial of a single variable to the first power.
    pub fn variable(index: usize, variables: usize) -> Self {
        let mut exponents = vec![0; variables];
        exponents[index] = 1;
        Self(exponents)
    }

    /// The constant monomial.
    pub fn one(variables: usize) -> Self {
        Self(vec![0; variables])
    }

    pub fn total_degree(&self) -> u32 {
        self.0.iter().sum()
    }

    pub fn multiply(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .zip(&other.0)
                .map(|(a, b)| a + b)
                .collect(),
        )
    }

    pub fn divides(&self, other: &Self) -> bool {
        self.0.iter().zip(&other.0).all(|(a, b)| a <= b)
    }

    /// This divided by `other`, when it is exact.
    pub fn quot_rem(&self, other: &Self) -> Option<Self> {
        if !other.divides(self) {
            return None;
        }
        Some(Self(
            self.0
                .iter()
                .zip(&other.0)
                .map(|(a, b)| a - b)
                .collect(),
        ))
    }
}
