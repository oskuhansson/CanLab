//! Outward-rounded bounds, so a quoted uncertainty is a guarantee.
//!
//! The detector study ends in numbers like "the ambiguity between the two
//! nearest hits is 11.7 ps". A number like that is only useful if the
//! simulation can say how much of it comes from the material constants being
//! uncertain and how much from the arithmetic. This type answers the second part
//! by rounding *outward* on every operation: if the bound comes back empty or
//! reversed, the code has drifted toward rounding error and the answer stops
//! being a guarantee.
//!
//! This is not a replacement for a proper interval library, and it does not
//! pretend to be: it covers the operations this project performs on a scalar
//! constant (add, subtract, multiply, divide, power, square root, trig), it
//! assumes no signed zero in the inputs, and it returns [`Interval::empty`] for
//! a division by an interval containing zero. Every one of those limits is
//! tested. A caller that needs gamma functions or interval Newton should reach
//! for a real library; a caller that needs to know whether 11.7 ps is bigger
//! than its own rounding error does not.
//!
//! # The rounding
//!
//! `next_down` and `next_up` are bit-neighbour operations on the IEEE-754
//! representation, written here with `to_bits`/`from_bits` so they cannot depend
//! on a compiler flag having been set.

use std::fmt;

/// A closed interval `[lo, hi]`, or the empty set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    lo: f64,
    hi: f64,
}

/// The floating-point value immediately below `value`.
fn next_down(value: f64) -> f64 {
    if value.is_nan() || value == f64::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f64::MIN_POSITIVE;
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

/// The floating-point value immediately above `value`.
fn next_up(value: f64) -> f64 {
    if value.is_nan() || value == f64::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f64::MIN_POSITIVE;
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

impl Interval {
    /// The empty set: the answer does not exist.
    pub const fn empty() -> Interval {
        Interval { lo: f64::NAN, hi: f64::NAN }
    }

    /// The interval containing only `value`.
    pub fn point(value: f64) -> Interval {
        Interval { lo: value, hi: value }
    }

    /// `[lo, hi]`, or [`Interval::empty`] when `lo > hi` or either end is `NaN`.
    pub fn new(lo: f64, hi: f64) -> Interval {
        if lo.is_nan() || hi.is_nan() || lo > hi {
            return Interval::empty();
        }
        Interval { lo, hi }
    }

    /// The lower end.
    pub fn lo(self) -> f64 {
        self.lo
    }

    /// The upper end.
    pub fn hi(self) -> f64 {
        self.hi
    }

    /// `true` when the set is empty.
    pub fn is_empty(self) -> bool {
        self.lo.is_nan() || self.hi.is_nan() || self.lo > self.hi
    }

    /// The widest possible error: half-width plus half an ulp at each end.
    ///
    /// The point of the type: a caller asking "is this number bigger than the
    /// noise" gets an answer that includes the rounding.
    pub fn radius(self) -> f64 {
        if self.is_empty() {
            return f64::NAN;
        }
        0.5 * (self.hi - self.lo).abs().max(self.lo.abs().max(self.hi.abs()) * f64::EPSILON)
            + 0.5 * (self.hi - self.lo).abs()
    }

    /// `[lo, hi] + [lo, hi]`, rounded outward.
    pub fn add(self, other: Interval) -> Interval {
        if self.is_empty() || other.is_empty() {
            return Interval::empty();
        }
        Interval::new(next_down(self.lo + other.lo), next_up(self.hi + other.hi))
    }

    /// `self − other`, rounded outward.
    pub fn sub(self, other: Interval) -> Interval {
        if self.is_empty() || other.is_empty() {
            return Interval::empty();
        }
        Interval::new(next_down(self.lo - other.hi), next_up(self.hi - other.lo))
    }

    /// The product, by the four corner products (which is all that is needed
    /// once both ends of both factors are ordered).
    pub fn mul(self, other: Interval) -> Interval {
        if self.is_empty() || other.is_empty() {
            return Interval::empty();
        }
        let corners = [
            self.lo * other.lo,
            self.lo * other.hi,
            self.hi * other.lo,
            self.hi * other.hi,
        ];
        if corners.iter().any(|c| c.is_nan()) {
            return Interval::empty();
        }
        let lo = corners.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Interval::new(next_down(lo), next_up(hi))
    }

    /// The quotient, or [`Interval::empty`] when the divisor contains zero.
    pub fn div(self, other: Interval) -> Interval {
        if self.is_empty() || other.is_empty() {
            return Interval::empty();
        }
        if other.lo <= 0.0 && other.hi >= 0.0 {
            return Interval::empty();
        }
        let corners = [
            self.lo / other.lo,
            self.lo / other.hi,
            self.hi / other.lo,
            self.hi / other.hi,
        ];
        if corners.iter().any(|c| c.is_nan()) {
            return Interval::empty();
        }
        let lo = corners.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Interval::new(next_down(lo), next_up(hi))
    }

    /// `lo^n` and `hi^n` for an integer `n`, rounded outward.
    ///
    /// An even power of an interval spanning zero starts at zero, not at the
    /// power of the smaller endpoint: `[−2, 3]² = [0, 9]`, and getting that wrong
    /// is the classic interval mistake.
    pub fn powi(self, n: i32) -> Interval {
        if self.is_empty() || n == 0 {
            return Interval::point(1.0);
        }
        if n < 0 {
            return Interval::point(1.0).div(self.powi(-n));
        }
        if n % 2 == 0 && self.lo <= 0.0 && self.hi >= 0.0 {
            let hi = self.lo.abs().max(self.hi.abs()).powi(n);
            return Interval::new(0.0, next_up(hi));
        }
        let a = self.lo.powi(n);
        let b = self.hi.powi(n);
        Interval::new(next_down(a.min(b)), next_up(a.max(b)))
    }

    /// The square root, or empty when either end is negative.
    pub fn sqrt(self) -> Interval {
        if self.is_empty() || self.lo < 0.0 {
            return Interval::empty();
        }
        Interval::new(next_down(self.lo.sqrt()), next_up(self.hi.sqrt()))
    }

    /// The sine, over an interval whose ends are rounded outward by the extreme
    /// values the function can reach.
    ///
    /// Only the pruning that matters is done: if the interval contains a peak
    /// (`pi/2 + 2k·pi`) the upper end is 1, and if it contains a trough
    /// (`-pi/2 + 2k·pi`) the lower end is −1. `k` is found from the ends rather
    /// than iterated.
    pub fn sin(self) -> Interval {
        if self.is_empty() {
            return Interval::empty();
        }
        let a = self.lo.sin();
        let b = self.hi.sin();
        let (mut lo, mut hi) = (a.min(b), a.max(b));
        let pi = std::f64::consts::PI;
        // A peak is at pi/2 + 2k*pi; walk the k values the interval can contain.
        let k_lo = ((self.lo - pi / 2.0) / (2.0 * pi)).ceil() as i64 - 1;
        let k_hi = ((self.hi - pi / 2.0) / (2.0 * pi)).floor() as i64 + 1;
        for k in k_lo..=k_hi {
            let k = k as f64;
            let peak = pi / 2.0 + 2.0 * k * pi;
            let trough = -pi / 2.0 + 2.0 * k * pi;
            if self.lo <= peak && peak <= self.hi {
                hi = 1.0;
            }
            if self.lo <= trough && trough <= self.hi {
                lo = -1.0;
            }
        }
        Interval::new(next_down(lo), next_up(hi).min(1.0 + f64::EPSILON))
    }

    /// The cosine, from the sine shifted by a quarter turn.
    pub fn cos(self) -> Interval {
        // cos x = sin(x + pi/2). The shift is itself an interval operation, but
        // the only error it introduces is one ulp of pi/2, which the outward
        // rounding below absorbs.
        let half_pi = Interval::new(
            next_down(std::f64::consts::FRAC_PI_2),
            next_up(std::f64::consts::FRAC_PI_2),
        );
        self.add(half_pi).sin()
    }

    /// The arc sine, empty outside `[-1, 1]`.
    pub fn asin(self) -> Interval {
        if self.is_empty() || self.lo < -1.0 - 1e-15 || self.hi > 1.0 + 1e-15 {
            return Interval::empty();
        }
        let lo = self.lo.clamp(-1.0, 1.0).asin();
        let hi = self.hi.clamp(-1.0, 1.0).asin();
        Interval::new(next_down(lo), next_up(hi))
    }

    /// The arc cosine, empty outside `[-1, 1]`.
    pub fn acos(self) -> Interval {
        if self.is_empty() || self.lo < -1.0 - 1e-15 || self.hi > 1.0 + 1e-15 {
            return Interval::empty();
        }
        let lo = self.hi.clamp(-1.0, 1.0).acos();
        let hi = self.lo.clamp(-1.0, 1.0).acos();
        Interval::new(next_down(lo), next_up(hi))
    }

    /// Whether every possible value is positive.
    pub fn is_positive(self) -> bool {
        !self.is_empty() && self.lo > 0.0
    }

    /// Whether the interval contains `value`.
    pub fn contains(self, value: f64) -> bool {
        !self.is_empty() && self.lo <= value && value <= self.hi
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            write!(f, "empty")
        } else {
            write!(f, "[{:.9e}, {:.9e}]", self.lo, self.hi)
        }
    }
}
