//! The two one-line acoustics formulas the W6.4 scenarios are checked against.
//!
//! They live in the crate and not in the test so that the test is a test of the
//! crate's physics: a formula written inside the test and then asserted passes
//! on a tree with the crate deleted, which is not a test of anything.

/// The speed of sound in air at `temperature_celsius`, in metres per second.
///
/// `c = 331.3·√(1 + T/273.15)`, the ideal-gas form with the sea-level value at
/// `0 °C`. It is the textbook formula and is good to a few tenths of a metre per
/// second over the range a classroom uses; it is not a humid-air model. Below
/// absolute zero the square root's argument is negative and the result is `NaN`.
pub fn speed_of_sound(temperature_celsius: f64) -> f64 {
    331.3 * (1.0 + temperature_celsius / 273.15).sqrt()
}

/// The frequency an observer at rest hears from a source moving towards it at
/// `speed`: `f' = f·c/(c − v)`.
///
/// `c` is the speed of sound and `v` the source's speed towards the observer.
/// The formula is the non-relativistic Doppler shift for a moving source and a
/// stationary observer; it assumes `v < c`, because at `v = c` the denominator
/// is zero and the shift is unbounded.
pub fn doppler_observed(source_hz: f64, c: f64, v: f64) -> f64 {
    source_hz * c / (c - v)
}

/// The frequency an observer at rest hears from a source moving away at
/// `speed`: `f' = f·c/(c + v)`.
///
/// The receding form is not the mirror of [`doppler_observed`]: a source at
/// `v = c/10` is heard at `10/9` of its frequency approaching and `10/11`
/// receding, and the two shifts differ by more than their average.
pub fn doppler_receding(source_hz: f64, c: f64, v: f64) -> f64 {
    source_hz * c / (c + v)
}
