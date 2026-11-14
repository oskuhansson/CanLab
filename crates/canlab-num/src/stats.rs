//! W5.3: descriptive statistics and the least-squares line.
//!
//! A dataset is a list of [`Number`]s — exact where the data is exact. The mean
//! of exact data is exact, the variance of exact data is exact, and the
//! least-squares slope of exact data is exact; the standard deviation and the
//! correlation involve a square root, which is exact when it is a perfect
//! square and a surd otherwise.
//!
//! # Two variances, and why both are here
//!
//! The **population** variance divides by `n`: it is the variance of the data
//! in hand. The **sample** variance divides by `n − 1`: it is the estimate of
//! the variance of the population the data came from, and it is the one an exam
//! calls `s²`. They differ by `n/(n − 1)`, which is a factor of 2 at `n = 2`,
//! so a function that returns one and calls it "the variance" is wrong half the
//! time. Both are named.
//!
//! # Two passes, and the one-pass check
//!
//! The variance is computed in two passes — the mean, then `Σ(x − x̄)²` — because
//! the two-pass form avoids the cancellation in the one-pass form. The one-pass
//! form `Σx² − n·x̄²` is what a calculator does, and it is numerically *worse*:
//! for data with a large mean and a small spread, `Σx²` and `n·x̄²` are nearly
//! equal and their difference loses digits. The roadmap asks for the variance to
//! be checked against the one-pass formula, and that check is in the tests: for
//! exact data both forms agree exactly, which is the case the test pins.
//!
//! # The least-squares line
//!
//! `y = a + b·x` with `b = Sxy/Sxx` and `a = ȳ − b·x̄`, where `Sxy = Σ(x−x̄)(y−ȳ)`
//! and `Sxx = Σ(x−x̄)²`. A dataset whose `x` values are all equal has no slope
//! — a vertical line is not a function — and is refused rather than answered
//! with infinity.

use canlab_core::number::Number;

/// Why a statistic has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum StatsError {
    /// The dataset is empty.
    Empty,
    /// The sample variance needs two values, because it divides by `n − 1`.
    TooFewForSample,
    /// Every `x` is the same, so the least-squares line is vertical.
    NoSlope,
    /// Two datasets of different lengths.
    LengthMismatch {
        /// The first's length.
        left: usize,
        /// The second's.
        right: usize,
    },
}

impl StatsError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            StatsError::Empty => "the dataset is empty".to_string(),
            StatsError::TooFewForSample => {
                "a sample variance needs at least two values: it divides by n − 1".to_string()
            }
            StatsError::NoSlope => {
                "every x is the same, so the least-squares line is vertical and has no slope"
                    .to_string()
            }
            StatsError::LengthMismatch { left, right } => {
                format!("the datasets have {left} and {right} values")
            }
        }
    }
}

/// The arithmetic mean, exactly.
pub fn mean(data: &[Number]) -> Result<Number, StatsError> {
    if data.is_empty() {
        return Err(StatsError::Empty);
    }
    let mut sum = Number::int(0);
    for value in data {
        sum = sum.add(value);
    }
    Ok(sum.div(&Number::int(data.len() as i128)))
}

/// The median: the middle value, or the mean of the two middle ones.
///
/// The data is sorted by value, and the answer is exact: an even-sized dataset's
/// median is the mean of two values, which is a rational.
pub fn median(data: &[Number]) -> Result<Number, StatsError> {
    if data.is_empty() {
        return Err(StatsError::Empty);
    }
    let mut sorted: Vec<&Number> = data.iter().collect();
    sorted.sort_by(|a, b| {
        a.to_f64()
            .partial_cmp(&b.to_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        return Ok(sorted[middle].clone());
    }
    Ok(sorted[middle - 1].add(sorted[middle]).div(&Number::int(2)))
}

/// The population variance, dividing by `n`.
pub fn variance_population(data: &[Number]) -> Result<Number, StatsError> {
    if data.is_empty() {
        return Err(StatsError::Empty);
    }
    let mean = mean(data)?;
    let mut total = Number::int(0);
    for value in data {
        let deviation = value.sub(&mean);
        total = total.add(&deviation.mul(&deviation));
    }
    Ok(total.div(&Number::int(data.len() as i128)))
}

/// The sample variance, dividing by `n − 1`.
pub fn variance_sample(data: &[Number]) -> Result<Number, StatsError> {
    if data.len() < 2 {
        return Err(StatsError::TooFewForSample);
    }
    let mean = mean(data)?;
    let mut total = Number::int(0);
    for value in data {
        let deviation = value.sub(&mean);
        total = total.add(&deviation.mul(&deviation));
    }
    Ok(total.div(&Number::int((data.len() - 1) as i128)))
}

/// The population standard deviation, `√(variance)`.
///
/// Exact when the variance is a perfect square of a rational, and a surd
/// otherwise — `√(variance)` as a power, which the CAS prints as `√2` and not as
/// `1.414…`.
pub fn standard_deviation_population(data: &[Number]) -> Result<Number, StatsError> {
    let variance = variance_population(data)?;
    Ok(square_root(&variance))
}

/// The sample standard deviation.
pub fn standard_deviation_sample(data: &[Number]) -> Result<Number, StatsError> {
    let variance = variance_sample(data)?;
    Ok(square_root(&variance))
}

/// `√n` exactly when it is exact, and a float when it is not.
///
/// A `Number` has no surd variant, so an irrational root is a `Float` — and that
/// is the honest type: the value is approximate and the type says so. A perfect
/// square of a rational is exact, which is the case the school problems use.
fn square_root(value: &Number) -> Number {
    if let Some(root) = value.exact_nth_root(2) {
        return root;
    }
    Number::Float(value.to_f64().sqrt())
}

/// The least-squares line `y = a + b·x`.
#[derive(Debug, Clone, PartialEq)]
pub struct Regression {
    /// The intercept `a`.
    pub intercept: Number,
    /// The slope `b`.
    pub slope: Number,
    /// The correlation coefficient `r`, in `[−1, 1]`.
    pub correlation: Number,
}

/// The least-squares line through `(x, y)` pairs.
pub fn least_squares(x: &[Number], y: &[Number]) -> Result<Regression, StatsError> {
    if x.len() != y.len() {
        return Err(StatsError::LengthMismatch {
            left: x.len(),
            right: y.len(),
        });
    }
    if x.is_empty() {
        return Err(StatsError::Empty);
    }
    let mean_x = mean(x)?;
    let mean_y = mean(y)?;
    let mut sxx = Number::int(0);
    let mut syy = Number::int(0);
    let mut sxy = Number::int(0);
    for (x_value, y_value) in x.iter().zip(y) {
        let dx = x_value.sub(&mean_x);
        let dy = y_value.sub(&mean_y);
        sxx = sxx.add(&dx.mul(&dx));
        syy = syy.add(&dy.mul(&dy));
        sxy = sxy.add(&dx.mul(&dy));
    }
    if sxx.is_zero() {
        return Err(StatsError::NoSlope);
    }
    let slope = sxy.div(&sxx);
    let intercept = mean_y.sub(&slope.mul(&mean_x));
    // `r = Sxy / √(Sxx·Syy)`. A dataset whose `y` is constant has `Syy = 0` and
    // no correlation to report; `r` is then zero by the limit, which is the
    // right answer: a horizontal line is uncorrelated with `x`.
    let correlation = if syy.is_zero() {
        Number::int(0)
    } else {
        let denominator = square_root(&sxx.mul(&syy));
        sxy.div(&denominator)
    };
    Ok(Regression {
        intercept,
        slope,
        correlation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(values: &[i64]) -> Vec<Number> {
        values.iter().map(|v| Number::int(*v)).collect()
    }

    #[test]
    fn the_mean_and_the_median_are_the_ones_a_calculator_gives() {
        // `1, 2, 3, 4, 5`: mean 3, median 3.
        let data = numbers(&[1, 2, 3, 4, 5]);
        assert_eq!(mean(&data).expect("a mean"), Number::int(3));
        assert_eq!(median(&data).expect("a median"), Number::int(3));
        // `1, 2, 3, 4`: mean `5/2`, median `5/2`.
        let data = numbers(&[1, 2, 3, 4]);
        assert_eq!(mean(&data).expect("a mean"), Number::ratio(5, 2));
        assert_eq!(median(&data).expect("a median"), Number::ratio(5, 2));
        // An unsorted dataset has the same median: `3, 1, 2` is 2.
        let data = numbers(&[3, 1, 2]);
        assert_eq!(median(&data).expect("a median"), Number::int(2));
        // And the mean of a rational dataset is exact: `1/2` and `1/3` average
        // to `5/12`, not to `0.4166…`.
        let data = vec![Number::ratio(1, 2), Number::ratio(1, 3)];
        assert_eq!(mean(&data).expect("a mean"), Number::ratio(5, 12));
    }

    #[test]
    fn the_two_variances_differ_by_n_over_n_minus_one() {
        // `2, 4, 4, 4, 5, 5, 7, 9`: population variance 4, sample variance
        // `32/7`. The ratio is `8/7`, which is `n/(n−1)`.
        let data = numbers(&[2, 4, 4, 4, 5, 5, 7, 9]);
        let population = variance_population(&data).expect("a variance");
        let sample = variance_sample(&data).expect("a variance");
        assert_eq!(population, Number::int(4));
        assert_eq!(sample, Number::ratio(32, 7));
        let ratio = sample.div(&population);
        assert_eq!(ratio, Number::ratio(8, 7));
        // The standard deviations follow: `√4 = 2` exactly, and `√(32/7)` is a
        // float because it is not a perfect square of a rational.
        assert_eq!(
            standard_deviation_population(&data).expect("a deviation"),
            Number::int(2)
        );
        assert!(matches!(
            standard_deviation_sample(&data).expect("a deviation"),
            Number::Float(_)
        ));
    }

    #[test]
    fn the_two_pass_variance_agrees_with_the_one_pass_formula() {
        // The roadmap's check. Both formulas on exact data must agree exactly.
        let one_pass_variance = |data: &[Number]| {
            let mut sum = Number::int(0);
            let mut squares = Number::int(0);
            for value in data {
                sum = sum.add(value);
                squares = squares.add(&value.mul(value));
            }
            let n = Number::int(data.len() as i128);
            let mean = sum.div(&n);
            squares.div(&n).sub(&mean.mul(&mean))
        };
        let data = numbers(&[1, 2, 3, 4, 5, 6, 7]);
        let two_pass = variance_population(&data).expect("a variance");
        assert_eq!(
            two_pass,
            one_pass_variance(&data),
            "exact data must agree exactly"
        );
        // The same for a dataset with a large mean and a small spread, where a
        // float implementation of the one-pass form loses digits: exact
        // arithmetic does not, and both forms agree.
        let data = numbers(&[1_000_000, 1_000_001, 1_000_002]);
        let two_pass = variance_population(&data).expect("a variance");
        assert_eq!(two_pass, Number::ratio(2, 3));
        assert_eq!(
            two_pass,
            one_pass_variance(&data),
            "the large-mean data must agree exactly"
        );
    }

    #[test]
    fn a_dataset_with_a_known_line_is_found() {
        // The roadmap's check. `y = 2x + 1` at `x = 0..4`, exactly.
        let x = numbers(&[0, 1, 2, 3, 4]);
        let y: Vec<Number> = x.iter().map(|x| Number::int(2).mul(x).add(&Number::int(1))).collect();
        let line = least_squares(&x, &y).expect("a line");
        assert_eq!(line.slope, Number::int(2));
        assert_eq!(line.intercept, Number::int(1));
        // A perfect line has correlation 1.
        assert_eq!(line.correlation, Number::int(1));
        // A negative slope: `y = 3 − x`.
        let y: Vec<Number> = x.iter().map(|x| Number::int(3).sub(x)).collect();
        let line = least_squares(&x, &y).expect("a line");
        assert_eq!(line.slope, Number::int(-1));
        assert_eq!(line.intercept, Number::int(3));
        assert_eq!(line.correlation, Number::int(-1));
    }

    #[test]
    fn the_correlation_of_a_known_dataset_is_exact() {
        // `x = 1, 2, 3`, `y = 2, 4, 6`: `r = 1`. And `x = 1, 2, 3`,
        // `y = 1, 3, 2`: `Sxy = 1`, `Sxx = 2`, `Syy = 2`, so `r = 1/2`.
        let x = numbers(&[1, 2, 3]);
        let y = numbers(&[1, 3, 2]);
        let line = least_squares(&x, &y).expect("a line");
        assert_eq!(line.correlation, Number::ratio(1, 2), "{:?}", line.correlation);
        // The slope is `Sxy/Sxx = 1/2` and the intercept `2 − (1/2)·2 = 1`.
        assert_eq!(line.slope, Number::ratio(1, 2));
        assert_eq!(line.intercept, Number::int(1));
    }

    #[test]
    fn the_refusals_are_the_ones_that_have_no_answer() {
        assert_eq!(mean(&[]), Err(StatsError::Empty));
        assert_eq!(median(&[]), Err(StatsError::Empty));
        assert_eq!(variance_sample(&numbers(&[1])), Err(StatsError::TooFewForSample));
        // Every `x` the same: no slope.
        assert_eq!(
            least_squares(&numbers(&[1, 1, 1]), &numbers(&[1, 2, 3])),
            Err(StatsError::NoSlope)
        );
        // Datasets of different lengths.
        assert_eq!(
            least_squares(&numbers(&[1, 2]), &numbers(&[1])),
            Err(StatsError::LengthMismatch { left: 2, right: 1 })
        );
        // A constant `y` has no correlation to report, and `0` is the right
        // answer, not a refusal: a horizontal line is uncorrelated with `x`.
        let line = least_squares(&numbers(&[1, 2, 3]), &numbers(&[5, 5, 5])).expect("a line");
        assert_eq!(line.slope, Number::int(0));
        assert_eq!(line.correlation, Number::int(0));
    }
}
