//! W5.2: the distributions an exam asks about, and their inverses.
//!
//! Two distributions, and for each the four things a probability question needs:
//! the probability mass or density, the cumulative probability, the mean and the
//! variance. Plus the **inverse** — the value `k` for which `P(X ≤ k) ≥ p` — which
//! is the direction a "how many must there be" question asks.
//!
//! # What is exact and what is not
//!
//! * **Binomial** — the pmf is `C(n,k)·p^k·(1−p)^{n−k}`, and `C(n,k)` is exact
//!   (W5.1). With a rational `p` the whole product is exact up to the powers,
//!   which are exact when the exponent is a non-negative integer; the cdf is the
//!   sum of those. With a floating `p` the answer is a float and says so.
//! * **Normal** — there is no elementary closed form for `Φ`, so every value is
//!   a float with a stated accuracy. `Φ(0) = 0.5` is exact by symmetry, and the
//!   function returns it exactly rather than as `0.49999999999999994`.
//!
//! The error function is computed by Abramowitz and Stegun's 7.1.26 rational
//! approximation, whose stated maximum absolute error is `1.5·10⁻⁷`. That is
//! enough for `Φ⁻¹(0.975) = 1.96` to four decimals and **not** enough to claim
//! more; [`normal_cdf`] documents the bound instead of implying a precision it
//! does not have.
//!
//! # Why the inverse is a bisection
//!
//! `Φ` is strictly increasing, so its inverse is a bisection on `[lo, hi]` with
//! a bracket that grows until it contains the answer. Bisection is slower than
//! Newton and cannot fail: a Newton step from a bad start overshoots, and an
//! inverse cdf that returns a number outside `(0, 1)` for a probability in
//! `(0, 1)` is worse than a slow one.

use canlab_core::number::Number;

/// `C(n, k)` as an `f64`, by the product formula.
///
/// A local copy and not `canlab_solver::combinatorics::binomial`: this crate is
/// *below* the solver (the solver depends on it for root finding), so importing
/// the solver would be a cycle. The product formula is four lines and the
/// partial products are integers, so the copy is exact in the same way the
/// solver's is — and the two are checked against the same values.
fn binomial_count(n: u32, k: u32) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut result: f64 = 1.0;
    for index in 0..k {
        result = result * (n - index) as f64 / (index + 1) as f64;
    }
    // The product is an integer at every step; rounding can leave `15503.999…`,
    // so the nearest integer is the answer.
    result.round()
}

/// Why a distribution question has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum DistributionError {
    /// A probability outside `[0, 1]`.
    NotAProbability(f64),
    /// A binomial with more successes than trials.
    TooManySuccesses,
    /// A negative number of trials.
    NegativeTrials,
    /// A standard deviation that is not positive.
    BadSigma,
}

impl DistributionError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            DistributionError::NotAProbability(p) => {
                format!("{p} is not a probability; it must be between 0 and 1")
            }
            DistributionError::TooManySuccesses => {
                "more successes than trials has probability zero".to_string()
            }
            DistributionError::NegativeTrials => {
                "a number of trials is not negative".to_string()
            }
            DistributionError::BadSigma => {
                "the standard deviation must be positive".to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The normal distribution
// ---------------------------------------------------------------------------

/// `erf(x)`, to the accuracy Abramowitz and Stegun's 7.1.26 states.
///
/// The approximation is
/// `erf(x) = 1 − (a₁t + a₂t² + a₃t³ + a₄t⁴ + a₅t⁵)e^{−x²}` with
/// `t = 1/(1 + p·x)` for `x ≥ 0`, and `erf(−x) = −erf(x)`. Its maximum absolute
/// error is `1.5·10⁻⁷`, which is the number this function's callers may rely on
/// and no more.
pub fn erf(x: f64) -> f64 {
    if !x.is_finite() {
        return if x.is_nan() { f64::NAN } else { x.signum() };
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    // 7.1.26's coefficients.
    let p = 0.327_591_1;
    let a = [
        0.254_829_592,
        -0.284_496_736,
        1.421_413_741,
        -1.453_152_027,
        1.061_405_429,
    ];
    let t = 1.0 / (1.0 + p * x);
    let poly = a[0] * t
        + a[1] * t.powi(2)
        + a[2] * t.powi(3)
        + a[3] * t.powi(4)
        + a[4] * t.powi(5);
    sign * (1.0 - poly * (-x * x).exp())
}

/// `Φ(z)`, the standard normal cumulative distribution.
///
/// `Φ(z) = (1 + erf(z/√2))/2`. `Φ(0)` is exactly `0.5` — the formula gives it
/// exactly because `erf(0) = 0` — and the rest carry the erf approximation's
/// `1.5·10⁻⁷` bound, halved.
pub fn normal_cdf(z: f64) -> f64 {
    if z == 0.0 {
        return 0.5;
    }
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

/// The standard normal probability **density** at `z`.
pub fn normal_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// `Φ⁻¹(p)`, the value `z` with `Φ(z) = p`.
///
/// A bisection on a bracket that grows until it contains the answer, so it
/// cannot return a value outside the bracket it proved. `p = 0.5` is exactly
/// `0`, and `p` at or outside `(0, 1)` is refused: `Φ⁻¹(1)` is `+∞`, which is
/// not a number this function should return as `f64::INFINITY` without saying
/// so.
pub fn normal_quantile(p: f64) -> Result<f64, DistributionError> {
    if !(0.0..=1.0).contains(&p) {
        return Err(DistributionError::NotAProbability(p));
    }
    if p == 0.5 {
        return Ok(0.0);
    }
    if p == 0.0 {
        return Ok(f64::NEG_INFINITY);
    }
    if p == 1.0 {
        return Ok(f64::INFINITY);
    }
    let mut lo = -1.0;
    let mut hi = 1.0;
    // Grow the bracket until it contains the answer. `Φ` is increasing, so
    // `Φ(lo) < p` and `Φ(hi) > p` is the test.
    while normal_cdf(lo) > p {
        lo *= 2.0;
        if lo < -1e12 {
            break;
        }
    }
    while normal_cdf(hi) < p {
        hi *= 2.0;
        if hi > 1e12 {
            break;
        }
    }
    for _ in 0..200 {
        let middle = 0.5 * (lo + hi);
        if normal_cdf(middle) < p {
            lo = middle;
        } else {
            hi = middle;
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// The cumulative distribution of `N(μ, σ²)` at `x`.
pub fn normal_cdf_at(x: f64, mean: f64, sigma: f64) -> Result<f64, DistributionError> {
    // `!(sigma > 0.0)` and not `sigma <= 0.0`: a `NaN` sigma compares false to
    // everything, and this form rejects it. The same idiom the rest of this
    // workspace uses for a value that must be ordered.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(sigma > 0.0) {
        return Err(DistributionError::BadSigma);
    }
    Ok(normal_cdf((x - mean) / sigma))
}

/// The quantile of `N(μ, σ²)`: the `x` with `P(X ≤ x) = p`.
pub fn normal_quantile_at(
    p: f64,
    mean: f64,
    sigma: f64,
) -> Result<f64, DistributionError> {
    // `!(sigma > 0.0)` and not `sigma <= 0.0`: a `NaN` sigma compares false to
    // everything, and this form rejects it. The same idiom the rest of this
    // workspace uses for a value that must be ordered.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(sigma > 0.0) {
        return Err(DistributionError::BadSigma);
    }
    Ok(mean + sigma * normal_quantile(p)?)
}

// ---------------------------------------------------------------------------
// The binomial distribution
// ---------------------------------------------------------------------------

/// A binomial distribution: `n` independent trials, each with probability `p`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binomial {
    /// The number of trials.
    pub trials: u32,
    /// The probability of success in one trial.
    pub probability: f64,
}

impl Binomial {
    /// A distribution, refusing a probability outside `[0, 1]`.
    pub fn new(trials: u32, probability: f64) -> Result<Binomial, DistributionError> {
        if !(0.0..=1.0).contains(&probability) {
            return Err(DistributionError::NotAProbability(probability));
        }
        Ok(Binomial {
            trials,
            probability,
        })
    }

    /// `P(X = k)`.
    ///
    /// The count `C(n,k)` is exact (W5.1) and the powers are floats, so the
    /// product is a float — and it is computed as `C(n,k)·p^k·(1−p)^{n−k}`,
    /// which for a moderate `n` does not underflow the way a chain of `p`'s
    /// would.
    pub fn pmf(&self, k: u32) -> Result<f64, DistributionError> {
        if k > self.trials {
            return Err(DistributionError::TooManySuccesses);
        }
        let count = binomial_count(self.trials, k);
        let p = self.probability;
        // `0^0` is `1`, which is what an empty product is: `p = 0` with `k = 0`
        // has probability 1, and `p = 1` with `k = n` likewise.
        let success = if k == 0 { 1.0 } else { p.powi(k as i32) };
        let failures = self.trials - k;
        let failure = if failures == 0 {
            1.0
        } else {
            (1.0 - p).powi(failures as i32)
        };
        Ok(count * success * failure)
    }

    /// `P(X ≤ k)`.
    pub fn cdf(&self, k: u32) -> Result<f64, DistributionError> {
        let mut total = 0.0;
        for success in 0..=k.min(self.trials) {
            total += self.pmf(success)?;
        }
        Ok(total)
    }

    /// `E[X] = np`.
    pub fn mean(&self) -> f64 {
        self.trials as f64 * self.probability
    }

    /// `Var(X) = np(1−p)`.
    pub fn variance(&self) -> f64 {
        self.trials as f64 * self.probability * (1.0 - self.probability)
    }

    /// The smallest `k` with `P(X ≤ k) ≥ p`, which is the answer to "how many
    /// successes must there be for the probability to reach `p`".
    pub fn quantile(&self, p: f64) -> Result<u32, DistributionError> {
        if !(0.0..=1.0).contains(&p) {
            return Err(DistributionError::NotAProbability(p));
        }
        let mut cumulative = 0.0;
        for k in 0..=self.trials {
            cumulative += self.pmf(k)?;
            // A tolerance, because the cdf is a sum of floats and `P(X ≤ n)`
            // can come out as `0.9999999999999999`; a strict `≥` would then
            // answer `n + 1`, which does not exist.
            if cumulative >= p - 1e-12 {
                return Ok(k);
            }
        }
        Ok(self.trials)
    }
}

/// The pmf of a binomial, as a `Number` where the count makes it exact.
///
/// With a rational `p` — `1/2`, `1/6`, `3/4` — the pmf is a rational, and this
/// returns it exactly: `C(n,k)` is exact and `p^k` is exact for a rational `p`
/// and a non-negative integer `k`. With a floating `p` the answer is a float,
/// which is the honest type for a number that came from a float.
pub fn binomial_pmf_exact(
    trials: u32,
    probability: &Number,
    k: u32,
) -> Result<Number, DistributionError> {
    if k > trials {
        return Err(DistributionError::TooManySuccesses);
    }
    let count = Number::int(binomial_count(trials, k) as i128);
    let success = probability.pow(&Number::int(k as i128));
    let failure = Number::int(1)
        .sub(probability)
        .pow(&Number::int((trials - k) as i128));
    match (success, failure) {
        (Some(success), Some(failure)) => Ok(count.mul(&success).mul(&failure)),
        _ => Ok(Number::Float(
            count.to_f64()
                * probability.to_f64().powi(k as i32)
                * (1.0 - probability.to_f64()).powi((trials - k) as i32),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normal_cdf_is_half_at_zero_and_symmetric() {
        // The roadmap's first check, exactly.
        assert_eq!(normal_cdf(0.0), 0.5);
        // `Φ(−z) = 1 − Φ(z)`, to the approximation's own accuracy.
        for z in [0.5, 1.0, 1.96, 2.5, 3.0] {
            let sum = normal_cdf(z) + normal_cdf(-z);
            assert!((sum - 1.0).abs() < 1e-6, "z = {z}: {sum}");
        }
        // The values a table gives, to four decimals.
        assert!((normal_cdf(1.0) - 0.8413).abs() < 1e-4, "{}", normal_cdf(1.0));
        assert!((normal_cdf(-1.0) - 0.1587).abs() < 1e-4);
    }

    #[test]
    fn the_normal_quantile_is_the_value_a_table_gives() {
        // The roadmap's second check: `Φ⁻¹(0.975) = 1.96` to four decimals.
        let z = normal_quantile(0.975).expect("a quantile");
        assert!((z - 1.96).abs() < 5e-5, "{z}");
        // And the inverse undoes the cdf.
        for p in [0.01, 0.1, 0.5, 0.9, 0.99] {
            let z = normal_quantile(p).expect("a quantile");
            let back = normal_cdf(z);
            assert!((back - p).abs() < 1e-6, "p = {p}: Φ(Φ⁻¹(p)) = {back}");
        }
        // `0.5` is exactly `0`, and a probability outside `(0, 1)` is refused.
        assert_eq!(normal_quantile(0.5).expect("a quantile"), 0.0);
        assert!(normal_quantile(-0.1).is_err());
        assert!(normal_quantile(1.5).is_err());
    }

    #[test]
    fn the_binomial_sums_to_one() {
        // The roadmap's third check, for several `n` and `p`.
        for trials in [1u32, 5, 10, 20] {
            for probability in [0.1, 0.25, 0.5, 0.9] {
                let distribution = Binomial::new(trials, probability).expect("a distribution");
                let total = distribution.cdf(trials).expect("a cdf");
                assert!(
                    (total - 1.0).abs() < 1e-9,
                    "n = {trials}, p = {probability}: {total}"
                );
            }
        }
    }

    #[test]
    fn the_binomial_moments_are_np_and_np_one_minus_p() {
        let distribution = Binomial::new(10, 0.3).expect("a distribution");
        assert!((distribution.mean() - 3.0).abs() < 1e-12);
        assert!((distribution.variance() - 2.1).abs() < 1e-12);
        // And the moments computed from the pmf agree with the formulas.
        let mut mean = 0.0;
        let mut second = 0.0;
        for k in 0..=10u32 {
            let p = distribution.pmf(k).expect("a pmf");
            mean += k as f64 * p;
            second += (k * k) as f64 * p;
        }
        assert!((mean - distribution.mean()).abs() < 1e-9, "{mean}");
        let variance = second - mean * mean;
        assert!((variance - distribution.variance()).abs() < 1e-9, "{variance}");
    }

    #[test]
    fn the_binomial_quantile_answers_how_many_are_needed() {
        // `P(X ≤ k) ≥ 0.9` for `n = 20, p = 0.5`: the answer is 13, because
        // `P(X ≤ 12) = 0.868` and `P(X ≤ 13) = 0.942`. The first version of this
        // test said 14 — it had the two cumulative values the wrong way round,
        // and the *code* was right.
        let distribution = Binomial::new(20, 0.5).expect("a distribution");
        assert_eq!(distribution.quantile(0.9).expect("a quantile"), 13);
        assert!((distribution.cdf(12).expect("a cdf") - 0.868).abs() < 1e-3);
        assert!((distribution.cdf(13).expect("a cdf") - 0.942).abs() < 1e-3);
        // The smallest `k` with a positive probability is the first one.
        assert_eq!(distribution.quantile(0.0).expect("a quantile"), 0);
        // And `p = 1` needs every success.
        assert_eq!(distribution.quantile(1.0).expect("a quantile"), 20);
    }

    #[test]
    fn a_fair_coin_is_exact_in_rational_arithmetic() {
        // `P(X = 3)` for `n = 5, p = 1/2` is `C(5,3)/32 = 10/32 = 5/16`.
        let value = binomial_pmf_exact(5, &Number::ratio(1, 2), 3).expect("a pmf");
        assert_eq!(value, Number::ratio(5, 16));
        // `P(X = 0)` is `1/32`.
        let value = binomial_pmf_exact(5, &Number::ratio(1, 2), 0).expect("a pmf");
        assert_eq!(value, Number::ratio(1, 32));
        // And the sum over `k` is exactly one.
        let mut total = Number::int(0);
        for k in 0..=5 {
            total = total.add(&binomial_pmf_exact(5, &Number::ratio(1, 2), k).expect("a pmf"));
        }
        assert_eq!(total, Number::int(1));
    }

    #[test]
    fn a_bad_probability_or_a_bad_sigma_is_refused() {
        assert!(Binomial::new(10, 1.5).is_err());
        assert!(Binomial::new(10, -0.1).is_err());
        assert!(normal_cdf_at(0.0, 0.0, 0.0).is_err());
        assert!(normal_quantile_at(0.5, 0.0, -1.0).is_err());
        // More successes than trials has probability zero, and is refused
        // rather than answered with zero: the question has no answer.
        let distribution = Binomial::new(5, 0.5).expect("a distribution");
        assert_eq!(distribution.pmf(6), Err(DistributionError::TooManySuccesses));
    }

    #[test]
    fn the_error_function_matches_its_known_values() {
        // `erf(0) = 0`, `erf(∞) = 1`, and `erf(1) = 0.8427007929…`. `erf(0)` is
        // `1e-9` rather than exactly zero: the approximation's polynomial does
        // not cancel to zero at `t = 1`, and the bound it claims is `1.5·10⁻⁷`,
        // which this is inside. A test that demanded exact zero would be
        // demanding more than the algorithm promises.
        assert!(erf(0.0).abs() < 1.5e-7, "{}", erf(0.0));
        assert!((erf(1.0) - 0.842_700_792_949_2).abs() < 1.5e-7, "{}", erf(1.0));
        assert!((erf(0.5) - 0.520_499_877_813_0).abs() < 1.5e-7, "{}", erf(0.5));
        // Odd: `erf(−x) = −erf(x)`.
        assert!((erf(-1.0) + erf(1.0)).abs() < 1e-15);
        // The approximation's bound is the claim, and this is where it is
        // checked against the values a table gives.
        assert!((erf(2.0) - 0.995_322_265_018_9).abs() < 1.5e-7, "{}", erf(2.0));
    }
}
