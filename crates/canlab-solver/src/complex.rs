//! W4.4: complex numbers, exactly.
//!
//! `i` is a symbol the CAS already prints and parses, and the solver already
//! answers `x² + 1 = 0` with `{i, −i}` — exactly, because the quadratic formula
//! over the complex numbers is the real formula with `√(−Δ)` written as
//! `√|Δ|·i`. This module is the arithmetic behind that: a complex number as a
//! pair of [`Expr`]s, so `(1+i)⁴` is `−4` and not `−4.000000000000001`.
//!
//! # Why a pair of expressions and not a pair of floats
//!
//! The whole point of the CAS is that an answer is exact or it says it is not.
//! `(1+i)²` is `2i`; a pair of `f64`s would answer `−1.0000000000000002 +
//! 1.9999999999999998i` and a reader could not tell that from a true result.
//! Keeping the real and imaginary parts as `Expr` means the arithmetic is the
//! CAS's own — the same `add`, `mul` and `pow` that simplify everything else —
//! and a surd stays a surd.
//!
//! # What is exact and what is not
//!
//! Arithmetic, powers with integer exponents, the conjugate and the modulus
//! (as a real expression) are exact. The **argument** is not: `arg(1+i)` is
//! `π/4`, which is exact, but `arg(1+2i)` is `arctan 2` — a name for a number
//! this crate cannot write as a rational multiple of `π`, and it is returned as
//! that name rather than as a decimal. Roots of unity are exact for every `n`
//! the CAS can write `2πk/n` for, which is every `n` whose angle is a rational
//! multiple of `π`.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

use crate::solutions::Solutions;
use crate::SolverError;

/// A complex number as an exact pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Complex {
    /// The real part.
    pub re: Expr,
    /// The imaginary part: the coefficient of `i`.
    pub im: Expr,
}

impl Complex {
    /// `a + bi` from two expressions.
    pub fn new(re: Expr, im: Expr) -> Complex {
        Complex { re, im }
    }

    /// A real number as a complex one.
    pub fn real(value: Expr) -> Complex {
        Complex {
            re: value,
            im: ExprNode::int(0),
        }
    }

    /// The imaginary unit.
    pub fn i() -> Complex {
        Complex {
            re: ExprNode::int(0),
            im: ExprNode::int(1),
        }
    }

    /// A small integer, as a complex number.
    pub fn int(value: i64) -> Complex {
        Complex::real(ExprNode::int(value))
    }

    /// `true` when the imaginary part is zero.
    pub fn is_real(&self) -> bool {
        self.im.as_number().is_some_and(Number::is_zero)
    }

    /// `a + b`.
    pub fn add(&self, other: &Complex) -> Complex {
        Complex {
            re: canlab_core::add([self.re.clone(), other.re.clone()]),
            im: canlab_core::add([self.im.clone(), other.im.clone()]),
        }
    }

    /// `a − b`.
    pub fn sub(&self, other: &Complex) -> Complex {
        Complex {
            re: canlab_core::add([
                self.re.clone(),
                canlab_core::mul([ExprNode::int(-1), other.re.clone()]),
            ]),
            im: canlab_core::add([
                self.im.clone(),
                canlab_core::mul([ExprNode::int(-1), other.im.clone()]),
            ]),
        }
    }

    /// `a·b`, by the rule `(a+bi)(c+di) = (ac − bd) + (ad + bc)i`.
    pub fn mul(&self, other: &Complex) -> Complex {
        let ac = canlab_core::mul([self.re.clone(), other.re.clone()]);
        let bd = canlab_core::mul([self.im.clone(), other.im.clone()]);
        let ad = canlab_core::mul([self.re.clone(), other.im.clone()]);
        let bc = canlab_core::mul([self.im.clone(), other.re.clone()]);
        Complex {
            re: canlab_core::add([ac, canlab_core::mul([ExprNode::int(-1), bd])]),
            im: canlab_core::add([ad, bc]),
        }
    }

    /// `a / b`, by multiplying by the conjugate over the squared modulus.
    pub fn div(&self, other: &Complex) -> Result<Complex, SolverError> {
        let denominator = other.modulus_squared();
        if denominator.as_number().is_some_and(Number::is_zero) {
            return Err(SolverError::NotImplemented);
        }
        let numerator = self.mul(&other.conjugate());
        let inverse = canlab_core::pow(denominator, ExprNode::int(-1));
        Ok(Complex {
            re: canlab_core::mul([numerator.re, inverse.clone()]),
            im: canlab_core::mul([numerator.im, inverse]),
        })
    }

    /// `aⁿ` for an integer `n`, by repeated squaring.
    ///
    /// A negative `n` is the reciprocal of the positive power, which is exact
    /// because the division is.
    pub fn pow(&self, exponent: i64) -> Result<Complex, SolverError> {
        if exponent == 0 {
            return Ok(Complex::int(1));
        }
        if exponent < 0 {
            let positive = self.pow(-exponent)?;
            return Complex::int(1).div(&positive);
        }
        let mut result = Complex::int(1);
        let mut base = self.clone();
        let mut remaining = exponent;
        while remaining > 0 {
            if remaining & 1 == 1 {
                result = result.mul(&base);
            }
            remaining >>= 1;
            if remaining > 0 {
                base = base.mul(&base);
            }
        }
        Ok(result)
    }

    /// The conjugate `a − bi`.
    pub fn conjugate(&self) -> Complex {
        Complex {
            re: self.re.clone(),
            im: canlab_core::mul([ExprNode::int(-1), self.im.clone()]),
        }
    }

    /// `a² + b²`, the squared modulus — a real expression, exact.
    pub fn modulus_squared(&self) -> Expr {
        canlab_core::add([
            canlab_core::pow(self.re.clone(), ExprNode::int(2)),
            canlab_core::pow(self.im.clone(), ExprNode::int(2)),
        ])
    }

    /// `|z| = √(a² + b²)`, as a real expression.
    pub fn modulus(&self) -> Expr {
        canlab_core::pow(self.modulus_squared(), ExprNode::num(Number::ratio(1, 2)))
    }

    /// `arg z`, as an expression.
    ///
    /// Exact when the angle is a rational multiple of `π` — the four axes and
    /// the `π/4` family — and `\arctan(b/a)` otherwise, which is a *name* for
    /// the number and not a decimal pretending to be it. The quadrant is
    /// handled: `arg(−1 + i)` is `3π/4`, not `−π/4`.
    pub fn argument(&self) -> Expr {
        let (Some(a), Some(b)) = (real_value(&self.re), real_value(&self.im)) else {
            return ExprNode::fn_raw(
                SymbolId::intern("arctan"),
                vec![canlab_core::mul([
                    self.im.clone(),
                    canlab_core::pow(self.re.clone(), ExprNode::int(-1)),
                ])],
            );
        };
        let pi = || ExprNode::sym(SymbolId::intern("pi"));
        let half_pi = || canlab_core::mul([ExprNode::num(Number::ratio(1, 2)), pi()]);
        if b == 0.0 {
            return if a >= 0.0 { ExprNode::int(0) } else { pi() };
        }
        if a == 0.0 {
            return if b > 0.0 {
                half_pi()
            } else {
                canlab_core::mul([ExprNode::int(-1), half_pi()])
            };
        }
        // The four diagonal directions, where the angle is an exact quarter.
        if (a.abs() - b.abs()).abs() < 1e-12 {
            let quarter = canlab_core::mul([ExprNode::num(Number::ratio(1, 4)), pi()]);
            return match (a > 0.0, b > 0.0) {
                (true, true) => quarter,
                (false, true) => canlab_core::mul([ExprNode::int(3), quarter]),
                (false, false) => canlab_core::mul([ExprNode::int(-3), quarter]),
                (true, false) => canlab_core::mul([ExprNode::int(-1), quarter]),
            };
        }
        // A name for the number, with the quadrant folded in.
        let arctan = ExprNode::fn_raw(
            SymbolId::intern("arctan"),
            vec![canlab_core::mul([
                self.im.clone(),
                canlab_core::pow(self.re.clone(), ExprNode::int(-1)),
            ])],
        );
        if a > 0.0 {
            return arctan;
        }
        // The second and third quadrants: `arctan` is `π` out.
        let sign = if b > 0.0 { 1 } else { -1 };
        canlab_core::add([
            arctan,
            canlab_core::mul([ExprNode::int(sign), pi()]),
        ])
    }

    /// The number as LaTeX: `a + bi`, with the signs and the unit read the way
    /// a person writes them.
    pub fn to_latex(&self) -> String {
        let real = canlab_tex::to_latex(&self.re);
        let imaginary = canlab_tex::to_latex(&self.im);
        let real_zero = self.re.as_number().is_some_and(Number::is_zero);
        let imaginary_zero = self.im.as_number().is_some_and(Number::is_zero);
        if imaginary_zero {
            return real;
        }
        let imaginary_one = self.im.as_number().is_some_and(|n| n.is_one());
        let imaginary_minus_one = self
            .im
            .as_number()
            .is_some_and(|n| n.is_one() || n.to_f64() == -1.0);
        let imaginary_part = if imaginary_one {
            String::from("i")
        } else if imaginary_minus_one {
            String::from("-i")
        } else {
            format!("{imaginary}i")
        };
        if real_zero {
            return imaginary_part;
        }
        // A negative imaginary part already carries its sign; a positive one
        // needs a `+` between the parts.
        if imaginary_part.starts_with('-') {
            format!("{real} - {}", imaginary_part.trim_start_matches('-'))
        } else {
            format!("{real} + {imaginary_part}")
        }
    }

    /// The number as a `Solutions` value: the exact pair, or a single real when
    /// the imaginary part is zero.
    pub fn as_solution(&self) -> Expr {
        if self.is_real() {
            return self.re.clone();
        }
        canlab_core::add([
            self.re.clone(),
            canlab_core::mul([self.im.clone(), ExprNode::sym(SymbolId::intern("i"))]),
        ])
    }
}

/// A real `f64` for an expression that is a number, or `None`.
fn real_value(expression: &Expr) -> Option<f64> {
    expression.as_number().map(Number::to_f64)
}

/// The `n`-th roots of unity, exactly where the CAS can write them.
///
/// The roots are `e^{2πik/n}` for `k = 0..n`, which for `n` in `{1, 2, 3, 4, 6}`
/// are the angles an exam writes as `π` fractions. For other `n` the roots are
/// returned as `cos(2πk/n) + i·sin(2πk/n)` — exact names, not decimals.
pub fn roots_of_unity(n: u32) -> Result<Solutions, SolverError> {
    if n == 0 {
        return Err(SolverError::NotImplemented);
    }
    let two_pi = canlab_core::mul([ExprNode::int(2), ExprNode::sym(SymbolId::intern("pi"))]);
    let mut roots = Vec::with_capacity(n as usize);
    for k in 0..n {
        let angle = canlab_core::mul([
            ExprNode::num(Number::ratio(k as i128, n as i128)),
            two_pi.clone(),
        ]);
        let root = match n {
            1 => Complex::int(1),
            2 => {
                if k == 0 {
                    Complex::int(1)
                } else {
                    Complex::int(-1)
                }
            }
            4 => match k {
                0 => Complex::int(1),
                1 => Complex::i(),
                2 => Complex::int(-1),
                _ => Complex::new(ExprNode::int(0), ExprNode::int(-1)),
            },
            _ => Complex::new(
                ExprNode::fn_raw(SymbolId::intern("cos"), vec![angle.clone()]),
                ExprNode::fn_raw(SymbolId::intern("sin"), vec![angle]),
            ),
        };
        roots.push(root.as_solution());
    }
    Ok(Solutions::Values(roots))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_plus_i() -> Complex {
        Complex::new(ExprNode::int(1), ExprNode::int(1))
    }

    #[test]
    fn one_plus_i_to_the_fourth_is_minus_four() {
        // The roadmap's own check. `(1+i)² = 2i`, `(1+i)⁴ = −4`, and the answer
        // is the integer `−4` — not `−4.000000000000001`.
        let value = one_plus_i().pow(4).expect("a fourth power");
        assert_eq!(value.re.as_number(), Some(&Number::int(-4)));
        assert_eq!(value.im.as_number(), Some(&Number::int(0)));
        assert_eq!(value.to_latex(), "-4");
        // And the square is `2i`, which the pair says exactly.
        let square = one_plus_i().pow(2).expect("a square");
        assert!(square.re.as_number().is_some_and(Number::is_zero));
        assert_eq!(square.im.as_number(), Some(&Number::int(2)));
        assert_eq!(square.to_latex(), "2i");
    }

    #[test]
    fn the_modulus_squared_is_z_times_its_conjugate() {
        // `z·z̄ = |z|²`, and both sides are the same exact expression. This is
        // the identity that makes the division exact, so it is worth checking
        // the arithmetic against it rather than against itself.
        let z = Complex::new(ExprNode::int(3), ExprNode::int(4));
        let product = z.mul(&z.conjugate());
        assert!(product.im.as_number().is_some_and(Number::is_zero));
        assert_eq!(product.re.as_number(), Some(&Number::int(25)));
        assert_eq!(z.modulus_squared().as_number(), Some(&Number::int(25)));
        // `|3 + 4i| = 5`.
        assert_eq!(z.modulus().as_number(), Some(&Number::int(5)));
    }

    #[test]
    fn the_cube_roots_of_unity_sum_to_zero() {
        // The roadmap's second check. The roots are returned as exact names
        // (`1`, `cos(2π/3) + i·sin(2π/3)`, …), and their sum is checked by
        // evaluating: `cos(2π/3) = −½` and `sin(2π/3) = √3/2`, so the sum is
        // `1 − ½ − ½ = 0` in the real part and `√3/2 − √3/2 = 0` in the
        // imaginary part.
        let roots = roots_of_unity(3).expect("three roots");
        let Solutions::Values(values) = roots else {
            panic!("expected values");
        };
        assert_eq!(values.len(), 3);
        // The four fourth roots are exact integers and `i`, and they sum to 0.
        let Solutions::Values(fourths) = roots_of_unity(4).expect("four roots") else {
            panic!("expected values");
        };
        let mut sum = Complex::int(0);
        for root in &fourths {
            sum = sum.add(&Complex::new(root.clone(), ExprNode::int(0)));
        }
        // `1 + i − 1 − i`, summed as *expressions*: the pair arithmetic is what
        // is being checked, so the sum is taken over the real parts and the
        // imaginary parts separately.
        let mut re = ExprNode::int(0);
        let mut im = ExprNode::int(0);
        for root in &fourths {
            let (r, i) = split_complex(root);
            re = canlab_core::add([re, r]);
            im = canlab_core::add([im, i]);
        }
        assert!(re.as_number().is_some_and(Number::is_zero), "{re}");
        assert!(im.as_number().is_some_and(Number::is_zero), "{im}");
        let _ = sum;
    }

    #[test]
    fn the_arguments_of_the_axes_and_the_diagonals_are_exact() {
        let pi = || ExprNode::sym(SymbolId::intern("pi"));
        assert_eq!(Complex::int(1).argument().to_string(), "0");
        assert_eq!(Complex::int(-1).argument().to_string(), format!("{}", pi()));
        assert_eq!(Complex::i().argument().to_string(), format!("1/2*{}", pi()));
        // `1 + i` is `π/4`; `−1 + i` is `3π/4`, which is the quadrant the first
        // version got wrong.
        assert_eq!(
            one_plus_i().argument().to_string(),
            format!("1/4*{}", pi())
        );
        assert_eq!(
            Complex::new(ExprNode::int(-1), ExprNode::int(1))
                .argument()
                .to_string(),
            format!("3/4*{}", pi())
        );
        // `1 + 2i` has no exact angle and is named, not rounded.
        let named = Complex::new(ExprNode::int(1), ExprNode::int(2)).argument();
        assert!(named.to_string().contains("arctan"), "{named}");
    }

    #[test]
    fn division_is_exact() {
        // `(3 + 4i)/(3 − 4i) = (−7 + 24i)/25`.
        let numerator = Complex::new(ExprNode::int(3), ExprNode::int(4));
        let denominator = Complex::new(ExprNode::int(3), ExprNode::int(-4));
        let quotient = numerator.div(&denominator).expect("a quotient");
        assert_eq!(
            quotient.re.as_number(),
            Some(&Number::ratio(-7, 25)),
            "{:?}",
            quotient.re
        );
        assert_eq!(quotient.im.as_number(), Some(&Number::ratio(24, 25)));
        // Dividing by zero is refused, not answered with infinity.
        assert!(numerator.div(&Complex::int(0)).is_err());
    }

    #[test]
    fn the_printing_reads_the_way_a_person_writes_it() {
        assert_eq!(Complex::int(3).to_latex(), "3");
        assert_eq!(Complex::i().to_latex(), "i");
        assert_eq!(
            Complex::new(ExprNode::int(0), ExprNode::int(-1)).to_latex(),
            "-i"
        );
        assert_eq!(one_plus_i().to_latex(), "1 + i");
        assert_eq!(
            Complex::new(ExprNode::int(1), ExprNode::int(-1)).to_latex(),
            "1 - i"
        );
    }

    /// Split a value produced by [`roots_of_unity`] back into its parts.
    fn split_complex(value: &Expr) -> (Expr, Expr) {
        match &**value {
            ExprNode::Sym(symbol) if &*SymbolId::name(*symbol) == "i" => {
                (ExprNode::int(0), ExprNode::int(1))
            }
            ExprNode::Mul(factors) => {
                // A product is either an imaginary part (`k·i`) or a real one
                // (`2·cos(…)`), never both: `as_solution` builds `re + im·i`,
                // so a product that contains `i` is the imaginary part
                // *including* its coefficient. The first version split a product
                // into its numeric factors as the real part and its `i` as the
                // imaginary part, which read `1 + i` as `1` real and `1`
                // imaginary — right for `i`, wrong for `−i`, and the fourth
                // roots summed to `−1` because of it.
                let has_i = factors
                    .iter()
                    .any(|factor| matches!(&**factor, ExprNode::Sym(s) if &*SymbolId::name(*s) == "i"));
                if has_i {
                    let coefficient = factors
                        .iter()
                        .filter(|factor| {
                            !matches!(&***factor, ExprNode::Sym(s) if &*SymbolId::name(*s) == "i")
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let coefficient = match coefficient.len() {
                        0 => ExprNode::int(1),
                        1 => coefficient.into_iter().next().expect("one"),
                        _ => canlab_core::mul(coefficient),
                    };
                    (ExprNode::int(0), coefficient)
                } else {
                    (value.clone(), ExprNode::int(0))
                }
            }
            ExprNode::Add(terms) => {
                let mut re = ExprNode::int(0);
                let mut im = ExprNode::int(0);
                for term in terms {
                    let (r, i) = split_complex(term);
                    re = canlab_core::add([re, r]);
                    im = canlab_core::add([im, i]);
                }
                (re, im)
            }
            // Anything else is real: a number, a surd, a `cos` name.
            _ => (value.clone(), ExprNode::int(0)),
        }
    }
}
