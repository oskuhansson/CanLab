//! Exact rational numbers with an explicit approximate variant.
//!
//! Zero dependencies: the exact arithmetic is `i128`-based rather than
//! arbitrary-precision. Every operation is checked, and on overflow the result
//! degrades to [`Number::Float`] instead of wrapping or panicking. For the
//! scale this project operates at (school-level through early-undergraduate
//! algebra) that trade is invisible; it is documented rather than hidden.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

/// A numeric leaf.
///
/// `Int` and `Rat` are exact; `Float` is approximate and never compares equal
/// to an exact value of the same magnitude. That asymmetry is deliberate: a
/// CAS that silently turns `1/3` into `0.333…` has already lost.
#[derive(Clone, Debug)]
pub enum Number {
    Int(i128),
    /// Always normalized: `den > 0`, `gcd(|num|, den) == 1`, `den != 1`.
    Rat(i128, i128),
    Float(f64),
}

impl Number {
    pub fn int<T: Into<i128>>(value: T) -> Self {
        Number::Int(value.into())
    }

    /// Build an exact rational, collapsing to `Int` when possible.
    pub fn ratio<T: Into<i128>>(num: T, den: T) -> Self {
        normalize_ratio(num.into(), den.into())
    }

    pub fn float(f: f64) -> Self {
        Number::Float(f)
    }

    pub fn zero() -> Self {
        Number::Int(0)
    }

    pub fn one() -> Self {
        Number::Int(1)
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Number::Int(i) => *i == 0,
            Number::Rat(n, _) => *n == 0,
            Number::Float(f) => *f == 0.0,
        }
    }

    pub fn is_one(&self) -> bool {
        match self {
            Number::Int(i) => *i == 1,
            Number::Rat(..) => false,
            Number::Float(f) => *f == 1.0,
        }
    }

    /// The magnitude: `|self|`.
    ///
    /// A method and not `abs()` on an `f64` conversion, because the exact
    /// variants must stay exact: `|−4/15|` is `4/15`, not `0.2666…`.
    pub fn absolute(&self) -> Number {
        match self {
            Number::Int(value) => Number::Int(value.abs()),
            Number::Rat(numerator, denominator) => Number::Rat(numerator.abs(), *denominator),
            Number::Float(value) => Number::Float(value.abs()),
        }
    }

    pub fn is_negative(&self) -> bool {
        match self {
            Number::Int(i) => *i < 0,
            Number::Rat(n, _) => *n < 0,
            Number::Float(f) => *f < 0.0,
        }
    }

    pub fn is_integer(&self) -> bool {
        match self {
            Number::Int(_) => true,
            Number::Rat(..) => false,
            Number::Float(f) => f.fract() == 0.0,
        }
    }

    pub fn to_f64(&self) -> f64 {
        match self {
            Number::Int(i) => *i as f64,
            Number::Rat(n, d) => *n as f64 / *d as f64,
            Number::Float(f) => *f,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Number::Int(_) => 0,
            Number::Rat(..) => 1,
            Number::Float(_) => 2,
        }
    }

    pub fn add(&self, other: &Number) -> Number {
        match (self, other) {
            (Number::Int(a), Number::Int(b)) => match a.checked_add(*b) {
                Some(v) => Number::Int(v),
                None => Number::Float(*a as f64 + *b as f64),
            },
            (Number::Float(_), _) | (_, Number::Float(_)) => {
                Number::Float(self.to_f64() + other.to_f64())
            }
            _ => {
                let (an, ad) = self.parts();
                let (bn, bd) = other.parts();
                let g = gcd(ad, bd);
                let lhs = an.checked_mul(bd / g);
                let rhs = bn.checked_mul(ad / g);
                let denom = ad.checked_mul(bd / g);
                match (lhs, rhs, denom) {
                    (Some(l), Some(r), Some(d)) => match l.checked_add(r) {
                        Some(n) => normalize_ratio(n, d),
                        None => Number::Float(self.to_f64() + other.to_f64()),
                    },
                    _ => Number::Float(self.to_f64() + other.to_f64()),
                }
            }
        }
    }

    pub fn sub(&self, other: &Number) -> Number {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Number) -> Number {
        match (self, other) {
            (Number::Int(a), Number::Int(b)) => match a.checked_mul(*b) {
                Some(v) => Number::Int(v),
                None => Number::Float(*a as f64 * *b as f64),
            },
            (Number::Float(_), _) | (_, Number::Float(_)) => {
                Number::Float(self.to_f64() * other.to_f64())
            }
            _ => {
                let (an, ad) = self.parts();
                let (bn, bd) = other.parts();
                // Cross-reduce first; this is what keeps i128 from overflowing
                // on inputs an arbitrary-precision CAS would shrug at.
                let g1 = gcd(an, bd);
                let g2 = gcd(bn, ad);
                let n = (an / g1).checked_mul(bn / g2);
                let d = (ad / g2).checked_mul(bd / g1);
                match (n, d) {
                    (Some(n), Some(d)) => normalize_ratio(n, d),
                    _ => Number::Float(self.to_f64() * other.to_f64()),
                }
            }
        }
    }

    pub fn div(&self, other: &Number) -> Number {
        if other.is_zero() {
            return Number::Float(f64::NAN);
        }
        self.mul(&other.recip())
    }

    pub fn recip(&self) -> Number {
        match self {
            Number::Int(i) => {
                if *i == 0 {
                    Number::Float(f64::NAN)
                } else {
                    normalize_ratio(1, *i)
                }
            }
            Number::Rat(n, d) => normalize_ratio(*d, *n),
            Number::Float(f) => Number::Float(1.0 / f),
        }
    }

    pub fn neg(&self) -> Number {
        match self {
            Number::Int(i) => match i.checked_neg() {
                Some(v) => Number::Int(v),
                None => Number::Float(-(*i as f64)),
            },
            Number::Rat(n, d) => Number::Rat(-n, *d),
            Number::Float(f) => Number::Float(-f),
        }
    }

    pub fn abs(&self) -> Number {
        if self.is_negative() { self.neg() } else { self.clone() }
    }

    fn parts(&self) -> (i128, i128) {
        match self {
            Number::Int(i) => (*i, 1),
            Number::Rat(n, d) => (*n, *d),
            Number::Float(f) => (f.to_bits() as i128, 1),
        }
    }

    /// Exact powers: integer exponents always; `p/q` exponents when the exact
    /// `q`-th root exists (`4^(1/2) = 2`, but `2^(1/2)` is left symbolic).
    pub fn pow(&self, exponent: &Number) -> Option<Number> {
        match exponent {
            Number::Int(e) => self.pow_int(*e),
            Number::Rat(p, q) => self.pow_rational(*p, *q),
            Number::Float(f) => Some(Number::Float(self.to_f64().powf(*f))),
        }
    }

    fn pow_int(&self, e: i128) -> Option<Number> {
        if e == 0 {
            return Some(Number::one());
        }
        if e > 4096 || e < -4096 {
            return Some(Number::Float(self.to_f64().powi(e as i32)));
        }
        let negative = e < 0;
        let magnitude = e.unsigned_abs();
        let base = if negative { self.recip() } else { self.clone() };

        let mut acc = Number::one();
        let mut factor = base;
        let mut remaining = magnitude;
        while remaining > 0 {
            if remaining & 1 == 1 {
                acc = acc.mul(&factor);
                if matches!(acc, Number::Float(_)) {
                    return Some(acc);
                }
            }
            remaining >>= 1;
            if remaining > 0 {
                factor = factor.mul(&factor);
                if matches!(factor, Number::Float(_)) {
                    return Some(Number::Float(self.to_f64().powi(e as i32)));
                }
            }
        }
        Some(acc)
    }

    fn pow_rational(&self, p: i128, q: i128) -> Option<Number> {
        if q <= 0 || q > 64 {
            return None;
        }
        let q = q as u32;
        let root = self.exact_nth_root(q)?;
        let magnitude = p.unsigned_abs();
        let powered = root.pow_int(magnitude as i128)?;
        if p < 0 {
            Some(powered.recip())
        } else {
            Some(powered)
        }
    }

    /// `self^(1/q)` when that root is exact.
    pub fn exact_nth_root(&self, q: u32) -> Option<Number> {
        match self {
            Number::Int(i) => {
                if *i < 0 {
                    if q % 2 == 0 {
                        return None;
                    }
                    let inner = integer_nth_root((*i).unsigned_abs(), q)?;
                    Some(Number::Int(-(inner as i128)))
                } else {
                    let root = integer_nth_root(*i as u128, q)?;
                    Some(Number::Int(root as i128))
                }
            }
            Number::Rat(n, d) => {
                if *n < 0 {
                    return None;
                }
                let rn = integer_nth_root(*n as u128, q)?;
                let rd = integer_nth_root(*d as u128, q)?;
                Some(normalize_ratio(rn as i128, rd as i128))
            }
            Number::Float(f) => {
                if *f < 0.0 {
                    return None;
                }
                let root = f.powf(1.0 / q as f64);
                if (root.powi(q as i32) - f).abs() <= 1e-9 * f.abs().max(1.0) {
                    Some(Number::Float(root))
                } else {
                    None
                }
            }
        }
    }
}

fn normalize_ratio(num: i128, den: i128) -> Number {
    if den == 0 {
        return Number::Float(f64::NAN);
    }
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    let g = gcd(num, den);
    let (num, den) = (num / g, den / g);
    if den == 1 {
        Number::Int(num)
    } else {
        Number::Rat(num, den)
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    let mut a = a.unsigned_abs();
    let mut b = b.unsigned_abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 { 1 } else { a as i128 }
}

fn integer_nth_root(value: u128, q: u32) -> Option<u128> {
    if q == 0 {
        return None;
    }
    if q == 1 || value <= 1 {
        return Some(value);
    }
    let mut lo: u128 = 0;
    let mut hi: u128 = value;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        match checked_pow_u128(mid, q) {
            Some(p) if p <= value => lo = mid,
            _ => hi = mid - 1,
        }
    }
    if checked_pow_u128(lo, q) == Some(value) {
        Some(lo)
    } else {
        None
    }
}

fn checked_pow_u128(base: u128, exp: u32) -> Option<u128> {
    let mut acc: u128 = 1;
    for _ in 0..exp {
        acc = acc.checked_mul(base)?;
    }
    Some(acc)
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Number {}

impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Number::Int(a), Number::Int(b)) => a.cmp(b),
            (Number::Rat(an, ad), Number::Rat(bn, bd)) => match (an.checked_mul(*bd), bn.checked_mul(*ad)) {
                (Some(l), Some(r)) => l.cmp(&r),
                _ => self.to_f64().total_cmp(&other.to_f64()),
            },
            (Number::Float(a), Number::Float(b)) => a.total_cmp(b),
            // Different exactness classes have different ranks, so exact and
            // approximate values of the same magnitude are never `Equal`.
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rank().hash(state);
        match self {
            Number::Int(i) => i.hash(state),
            Number::Rat(n, d) => {
                n.hash(state);
                d.hash(state);
            }
            Number::Float(f) => f.to_bits().hash(state),
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Number::Int(i) => write!(f, "{i}"),
            Number::Rat(n, d) => write!(f, "{n}/{d}"),
            Number::Float(x) => write!(f, "{x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationals_stay_exact() {
        let third = Number::ratio(1, 3);
        let sixth = Number::ratio(1, 6);
        assert_eq!(third.add(&sixth), Number::ratio(1, 2));
        assert_eq!(third.add(&sixth).to_string(), "1/2");
    }

    #[test]
    fn integer_denominators_collapse() {
        assert!(matches!(Number::ratio(4, 2), Number::Int(_)));
        assert_eq!(Number::ratio(4, 2), Number::int(2));
    }

    #[test]
    fn negative_denominators_normalize() {
        assert_eq!(Number::ratio(1, -2), Number::ratio(-1, 2));
        assert_eq!(Number::ratio(1, -2).to_string(), "-1/2");
    }

    #[test]
    fn exact_beats_approximate() {
        assert_ne!(Number::int(2), Number::float(2.0));
        assert_eq!(Number::float(2.0), Number::float(2.0));
    }

    #[test]
    fn exact_roots() {
        assert_eq!(Number::int(4).pow(&Number::ratio(1, 2)), Some(Number::int(2)));
        assert_eq!(Number::int(8).pow(&Number::ratio(1, 3)), Some(Number::int(2)));
        assert_eq!(Number::int(2).pow(&Number::ratio(1, 2)), None);
        assert_eq!(Number::ratio(9, 4).pow(&Number::ratio(1, 2)), Some(Number::ratio(3, 2)));
    }

    #[test]
    fn negative_exponents() {
        assert_eq!(Number::int(2).pow(&Number::int(-2)), Some(Number::ratio(1, 4)));
    }

    #[test]
    fn overflow_degrades_to_float_instead_of_panicking() {
        let big = Number::int(i128::MAX);
        let doubled = big.add(&big);
        assert!(matches!(doubled, Number::Float(_)));
    }

    #[test]
    fn division_by_zero_is_nan() {
        assert!(Number::int(1).div(&Number::zero()).to_f64().is_nan());
    }

    #[test]
    fn mul_cross_reduces() {
        // (6/35) * (49/27) = 294/945 = 14/45
        let a = Number::ratio(6, 35);
        let b = Number::ratio(49, 27);
        assert_eq!(a.mul(&b).to_string(), "14/45");
    }
}
