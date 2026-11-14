//! W5.1: combinatorics, exactly until the exact type runs out.
//!
//! `n!`, `C(n, k)` and `P(n, k)` are integers, and an exam's answer to "how many
//! ways" is an integer. This module computes them in [`Number`]'s exact
//! arithmetic — `i128` — so `20! = 2432902008176640000` is exact and not
//! `2.43290200817664e18`.
//!
//! # Where exactness ends, and what happens there
//!
//! `i128` holds every factorial up to `33!` (`3.68·10³⁷`) and every binomial
//! coefficient whose partial products fit. Beyond that, [`Number`] degrades to a
//! `Float` *by design* — the arithmetic is checked and the result says it is
//! approximate rather than wrapping or panicking. This module does not pretend
//! otherwise: a value that comes back as a `Float` is a number this crate cannot
//! state exactly, and the caller can see that from the type.
//!
//! The roadmap's bound is `10³⁸`, and `33!` is `3.68·10³⁷` — so the whole range
//! it asks for is exact, and `34!` is the first factorial that is not. The
//! boundary was measured, not assumed: the first version of this comment said
//! `34!` fits, and the test that asserted its value failed to compile because it
//! does not.
//!
//! # Why the definitions and not the formulas
//!
//! `C(n, k)` is computed by the product `∏ (n−i)/(i+1)`, which stays small
//! until the end and is exact at every step because each partial product is an
//! integer (the binomial coefficients are). Computing `n!/(k!(n−k)!)` directly
//! overflows `i128` for a `C(60, 30)` that the product computes exactly, and a
//! formula that overflows where the definition does not is the wrong formula.

use canlab_core::number::Number;

/// Why a combinatorial count has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum CountError {
    /// A negative argument.
    Negative,
    /// `k > n`: there is no way to choose more than there is.
    TooMany,
}

impl CountError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            CountError::Negative => {
                "a factorial or a count is not defined for a negative number".to_string()
            }
            CountError::TooMany => {
                "choosing more items than there are has no count".to_string()
            }
        }
    }
}

/// `n!`, exactly while `n!` fits in `i128`.
///
/// `0! = 1`, which is the empty product and not a special case the code needs
/// to know about beyond the loop not running.
pub fn factorial(n: u32) -> Number {
    // `33!` is the largest factorial that fits in an `i128`; beyond it the
    // product becomes a `Float` through `Number`'s checked arithmetic, and the
    // returned type is the visible signal that the answer stopped being exact.
    let mut result = Number::int(1);
    for value in 1..=n {
        result = result.mul(&Number::int(value as i128));
    }
    result
}

/// `n!` for a signed `n`, refusing a negative one.
pub fn checked_factorial(n: i64) -> Result<Number, CountError> {
    if n < 0 {
        return Err(CountError::Negative);
    }
    Ok(factorial(n as u32))
}

/// `C(n, k)`, the number of `k`-element subsets of `n`.
///
/// By the product `∏_{i=0}^{k−1} (n − i)/(i + 1)`, which is exact at every step
/// because each partial product is a binomial coefficient. `C(n, 0) = C(n, n) =
/// 1`, and `C(n, k) = C(n, n−k)` holds by construction because the loop runs
/// over the smaller of `k` and `n − k`.
pub fn binomial(n: u32, k: u32) -> Result<Number, CountError> {
    if k > n {
        return Err(CountError::TooMany);
    }
    // The smaller half: `C(n, k) = C(n, n−k)`, and the smaller loop is both
    // faster and less likely to overflow.
    let k = k.min(n - k);
    let mut result = Number::int(1);
    for index in 0..k {
        // `result · (n − index) / (index + 1)`, in that order: the product is an
        // integer at every step, so the division is exact.
        //
        // *Exact* is the word that matters, and `Number::div` does not give it:
        // it takes the reciprocal and multiplies, which builds a rational whose
        // numerator and denominator can each overflow `i128` before they cancel.
        // `C(60, 30)` came back as `118264581564861424` — the first fifteen
        // digits of the right answer — because of it. The division here is
        // integer division on an `Int`, which is exact by construction and
        // cannot overflow a value that already fits.
        let numerator = result.mul(&Number::int((n - index) as i128));
        let divisor = (index + 1) as i128;
        result = match numerator {
            Number::Int(value) if value % divisor == 0 => Number::Int(value / divisor),
            // A non-integer numerator means the partial product overflowed and
            // became a float; the division is then whatever the float says, and
            // the value is a `Float` the caller can see.
            other => other.div(&Number::int(divisor)),
        };
    }
    Ok(result)
}

/// `P(n, k) = n!/(n−k)!`, the number of `k`-permutations of `n`.
pub fn permutations(n: u32, k: u32) -> Result<Number, CountError> {
    if k > n {
        return Err(CountError::TooMany);
    }
    // The falling factorial `n·(n−1)·…·(n−k+1)`, which is the definition and
    // does not build either factorial.
    let mut result = Number::int(1);
    for index in 0..k {
        result = result.mul(&Number::int((n - index) as i128));
    }
    Ok(result)
}

/// `C(n, k)` for signed arguments, refusing the ones with no meaning.
pub fn checked_binomial(n: i64, k: i64) -> Result<Number, CountError> {
    if n < 0 || k < 0 {
        return Err(CountError::Negative);
    }
    binomial(n as u32, k as u32)
}

/// `P(n, k)` for signed arguments.
pub fn checked_permutations(n: i64, k: i64) -> Result<Number, CountError> {
    if n < 0 || k < 0 {
        return Err(CountError::Negative);
    }
    permutations(n as u32, k as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factorials_are_exact_through_the_roadmaps_bound() {
        // `10! = 3628800`, `20! = 2432902008176640000` — the "20-element draw"
        // the roadmap names — and `33!` is the last one `i128` holds; `34!` is
        // the first that does not.
        assert_eq!(factorial(0), Number::int(1));
        assert_eq!(factorial(1), Number::int(1));
        assert_eq!(factorial(10), Number::int(3_628_800));
        assert_eq!(factorial(20), Number::int(2_432_902_008_176_640_000i128));
        // `33!` is the last one `i128` holds: `3.68·10³⁷`.
        assert_eq!(
            factorial(33),
            Number::int(8_683_317_618_811_886_495_518_194_401_280_000_000i128)
        );
        // Every one of them is an `Int`, not a `Float`: the type says exact.
        for n in 0..=33 {
            assert!(
                matches!(factorial(n), Number::Int(_)),
                "{n}! is not exact"
            );
        }
    }

    #[test]
    fn pascals_identity_holds() {
        // The roadmap's first check: `C(n, k) = C(n−1, k−1) + C(n−1, k)`.
        for n in 1..=20u32 {
            for k in 1..n {
                let left = binomial(n, k).expect("a binomial");
                let right = binomial(n - 1, k - 1)
                    .expect("a binomial")
                    .add(&binomial(n - 1, k).expect("a binomial"));
                assert_eq!(left, right, "C({n},{k})");
            }
        }
    }

    #[test]
    fn the_symmetry_holds() {
        // The roadmap's second check: `C(n, k) = C(n, n−k)`.
        for n in 0..=30u32 {
            for k in 0..=n {
                assert_eq!(
                    binomial(n, k).expect("a binomial"),
                    binomial(n, n - k).expect("a binomial"),
                    "C({n},{k})"
                );
            }
        }
    }

    #[test]
    fn a_twenty_element_draw_is_exact() {
        // The roadmap's third check. `C(20, 5) = 15504` and `P(20, 5) =
        // 1860480`, both exact integers.
        assert_eq!(binomial(20, 5).expect("a binomial"), Number::int(15_504));
        assert_eq!(
            permutations(20, 5).expect("a permutation"),
            Number::int(1_860_480)
        );
        // And the relation between them: `P(n,k) = C(n,k)·k!`.
        let product = binomial(20, 5)
            .expect("a binomial")
            .mul(&factorial(5));
        assert_eq!(product, permutations(20, 5).expect("a permutation"));
    }

    #[test]
    fn the_product_formula_is_exact_where_the_factorial_formula_overflows() {
        // `C(60, 30)` fits in `i128`, but `60!/(30!·30!)` would not: the
        // factorial formula builds two numbers of 10⁸² before dividing them.
        // The product formula never builds a number larger than the answer, so
        // it stays exact.
        let value = binomial(60, 30).expect("a binomial");
        assert!(
            matches!(value, Number::Int(_)),
            "C(60,30) should be exact, got {value:?}"
        );
        // And it is the number `math.comb(60, 30)` gives. The first version of
        // this assertion had a hand-typed value three times too long — the
        // *code* was right and the test was wrong, which is why the value is
        // quoted from an independent implementation.
        assert_eq!(value, Number::int(118_264_581_564_861_424i128));
    }

    #[test]
    fn the_edges_are_one_and_the_impossible_is_refused() {
        for n in 0..=10u32 {
            assert_eq!(binomial(n, 0).expect("a binomial"), Number::int(1));
            assert_eq!(binomial(n, n).expect("a binomial"), Number::int(1));
        }
        assert_eq!(binomial(5, 6), Err(CountError::TooMany));
        assert_eq!(checked_factorial(-1), Err(CountError::Negative));
        assert_eq!(checked_binomial(-1, 0), Err(CountError::Negative));
        assert_eq!(checked_permutations(3, -1), Err(CountError::Negative));
    }
}
