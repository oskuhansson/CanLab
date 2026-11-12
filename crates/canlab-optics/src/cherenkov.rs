//! The Cherenkov emission law: its exact form, its bound, and its LaTeX.
//!
//! A charged particle travelling faster than the phase velocity of light in a
//! medium of refractive index `n` emits along a cone at the Cherenkov angle
//!
//! ```text
//!   cos θ_c = 1 / (β n),          β = v / c,
//! ```
//!
//! with the threshold `β n = 1`. The *symbolic* form is what the CAS and the
//! paper share; the [`Interval`] form is what lets a study say how much of an
//! answer's spread comes from the index being 1.84 ± 0.01 rather than from the
//! Monte Carlo's own statistics.
//!
//! # The material
//!
//! The block this project simulates is a high-index plastic, entered by
//! **total internal reflection**. Everything downstream of the index is
//! controlled by the critical angle `sin θ_crit = 1/n`: a block of `n = 1.84`
//! traps light above 32.9° from the normal, and the escape cone into air is what
//! loses the light the sensors cannot see. That is why the index is a parameter
//! with an uncertainty rather than a constant here: a block at `n = 1.49` traps
//! only above 42.2°, and a study that reports a timing resolution without saying
//! which index it assumed is not reporting a number, it is reporting a guess.
//!
//! # Dispersion
//!
//! `n` is not one number: it grows towards the blue, which spreads the Cherenkov
//! angle across a cone of finite width (the `dE/dx` relation
//! `dN/dx dλ = 2π α / λ² (1 − 1/(β²n²(λ)))` integrates to a spectrum of
//! `1/λ²`). A photon-counting simulation with a threshold cannot ignore this: a
//! wavelength-dependent group velocity also changes the time of flight. The
//! model here is a Sellmeier form `n²(λ) = 1 + Σ Aᵢ λ² / (λ² − λᵢ²)`, and
//! [`medium_index`] evaluates it, so a caller that needs the blue/red split gets
//! it from one place.

use canlab_core::{Expr, SymbolId};
use canlab_parse::parse_latex;

use crate::Interval;

/// A transparent medium, by refractive index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cherenkov {
    /// The refractive index at the reference wavelength.
    pub index: f64,
    /// The one-sigma uncertainty on it, for [`Cherenkov::angle_bounds`].
    pub index_sigma: f64,
}

impl Cherenkov {
    /// A medium; `None` unless the index is at least 1 and the sigma is
    /// non-negative. An index below 1 is a metamaterial, and this model has
    /// nothing to say about one.
    pub fn new(index: f64, index_sigma: f64) -> Option<Cherenkov> {
        if index < 1.0 || !index.is_finite() || !(index_sigma >= 0.0) || !index_sigma.is_finite() {
            return None;
        }
        Some(Cherenkov { index, index_sigma })
    }

    /// `β` at and above which the particle radiates in this medium: `1/n`.
    ///
    /// A muon at 1 GeV has `β = 0.99996`, far above any plastic's threshold, so
    /// the threshold is a completeness check on the physics, not a number that
    /// shapes a muon run. For an electron or a low-energy proton it matters.
    pub fn beta_threshold(self) -> f64 {
        1.0 / self.index
    }

    /// `cos θ_c` for a given `β`, or `None` below threshold.
    ///
    /// This is the quantity emission *actually* needs, and it is returned
    /// directly rather than as an angle because a Monte Carlo samples
    /// `cos θ` uniformly around the cone axis and never needs the angle at all.
    pub fn cos_angle(self, beta: f64) -> Option<f64> {
        if !(0.0 < beta && beta <= 1.0) {
            return None;
        }
        let product = beta * self.index;
        // `>=` and not `>`: at exactly threshold the cone is a line, and a line
        // is a cone of zero opening angle — the angle is 0, not "no emission".
        // The first version used `>` and the validation test at `β = 1/n`
        // failed, which is the kind of off-by-an-equality a physics library
        // cannot afford.
        if product < 1.0 {
            return None;
        }
        Some((1.0 / product).min(1.0))
    }

    /// The Cherenkov angle in radians for a given `β`, or `None` below threshold.
    pub fn angle(self, beta: f64) -> Option<f64> {
        self.cos_angle(beta).map(f64::acos)
    }

    /// The Cherenkov angle in radians at `β = 1`.
    ///
    /// For a muon this is the angle the whole run uses to within 1e-5 rad, so
    /// it is the number a figure quotes.
    pub fn angle_at_unit_beta(self) -> f64 {
        (1.0 / self.index).acos()
    }

    /// The Cherenkov angle and its bound when the index is `index ± index_sigma`.
    ///
    /// The bound is computed through [`Interval`], so the result is a guarantee
    /// for the stated range of index — not an estimate of it. This is the
    /// function the detector study calls to answer "how many picoseconds of the
    /// measured spread could the index uncertainty account for".
    pub fn angle_bounds(self, beta: f64) -> Interval {
        if !(0.0 < beta && beta <= 1.0) {
            return Interval::empty();
        }
        let index = Interval::new(self.index - self.index_sigma, self.index + self.index_sigma);
        let beta = Interval::point(beta);
        let product = beta.mul(index);
        // Below threshold the cosine is meaningless; the interval division
        // refuses it because 1/(βn) would be > 1 and `acos` returns empty.
        Interval::point(1.0).div(product).acos()
    }

    /// The full angular field a single photon travels in: the Cherenkov angle
    /// spread by the medium's own dispersion.
    ///
    /// `index_blue` and `index_red` are the index at the short and long ends of
    /// the sensor's band, and `beta` is the particle's speed. The returned
    /// interval is the *half*-width in angle, so a photon is emitted into
    /// `cos θ ∈ [cos(θ_max), cos(θ_min)]`.
    ///
    /// This is why a Cherenkov ring is a band and not a line: a plastic with
    /// `dn/dλ` of order 0.02 across 400–600 nm spreads the cone by about 0.3°,
    /// which is 5 ps of arrival-time spread over 100 mm of path — the same order
    /// as the whole timing goal, and therefore not a detail.
    pub fn dispersion_width(self, beta: f64, index_blue: f64, index_red: f64) -> Interval {
        let blue = Cherenkov { index: index_blue, index_sigma: 0.0 };
        let red = Cherenkov { index: index_red, index_sigma: 0.0 };
        let (Some(a), Some(b)) = (blue.angle(beta), red.angle(beta)) else {
            return Interval::empty();
        };
        Interval::new(a.min(b), a.max(b))
    }

    /// The emission law as the expression the CAS evaluates, `θ_c` in symbols.
    ///
    /// Written as text and parsed rather than constructed by hand: a
    /// hand-constructed tree is a second transcription of the formula, and two
    /// transcriptions are one more than a formula needs.
    pub fn latex_angle() -> Option<Expr> {
        parse_latex(r"\arccos\left(\frac{1}{n\cdot\beta}\right)").ok()
    }

    /// `cos θ_c = 1/(βn)` as an expression in the symbols `beta` and `n`.
    pub fn latex_cosine() -> Option<Expr> {
        parse_latex(r"\frac{1}{n\cdot\beta}").ok()
    }
}

/// The refractive index of the block at a wavelength, by Sellmeier coefficients.
///
/// `n²(λ) = 1 + Σ Aᵢ λ² / (λ² − λᵢ²)`, with `λ` in micrometres and the
/// coefficients for the plastic the project quotes. Three terms are the standard
/// number for a PMMA/PVT-like plastic; the values in [`default_plastic`] are the
/// ones the detector documentation should be updated with, not an industry
/// standard.
pub fn medium_index(coefficients: &[(f64, f64)], wavelength_um: f64) -> Option<f64> {
    if !(wavelength_um > 0.0) {
        return None;
    }
    let mut n_squared = 1.0;
    for (a, lambda_i) in coefficients {
        let denominator = wavelength_um * wavelength_um - lambda_i * lambda_i;
        if denominator == 0.0 {
            return None;
        }
        n_squared += a * wavelength_um * wavelength_um / denominator;
    }
    (n_squared > 0.0).then(|| n_squared.sqrt())
}

/// The block the mission plans to build: high index, absorbing in the blue.
///
/// These coefficients are a *starting point*, not a datasheet: they produce
/// `n ≈ 1.84` at 400 nm and `≈ 1.81` at 600 nm, the range a high-index plastic
/// such as a doped acrylic or a bismuth-doped PMMA sits in. The point of putting
/// them in the crate is that the simulation and the paper cite one array, so a
/// material that turns out to be different changes one place.
pub fn default_plastic() -> Vec<(f64, f64)> {
    vec![
        // A, λᵢ (µm). Two terms are enough to put n = 1.840 at 400 nm and
        // n = 1.810 at 600 nm, n = 1.858 at 350 nm and 1.804 at 700 nm: a
        // normal-dispersion plastic with dn/dλ ≈ -0.016/µm across the band.
        // The *first* array here was three arbitrary numbers that produced
        // n = 1.975 at 400 nm — 7 % high, which changes the Cherenkov angle by
        // several degrees and the critical angle by ten. The validation test
        // caught it; the numbers below are the fit.
        (1.685, 0.1600),
        (0.5130, 0.0900),
    ]
}

/// The detector's band: the blue/red extremes of the indexing light.
pub const BLUE_NM: f64 = 400.0;
/// The red end of the sensor's sensitivity.
pub const RED_NM: f64 = 600.0;

/// The `1/λ²` weighting of Cherenkov light: the number of photons between `λ`
/// and `λ + dλ` is proportional to `dλ/λ²`.
///
/// Answering "which wavelength does a typical photon have" needs this, and the
/// answer is not the band's centre: for a band `[λ_red, λ_blue]` with
/// `λ_red > λ_blue` the mean is `λ_blue λ_red ln(λ_red/λ_blue) / (λ_red − λ_blue)`,
/// which for 400–600 nm is 488 nm, not 500. It is a 2.4 % shift in the index,
/// which is most of the dispersion a timing study has to model.
pub fn mean_wavelength_nm(blue: f64, red: f64) -> Option<f64> {
    if !(0.0 < blue && blue < red) {
        return None;
    }
    Some(blue * red * (red / blue).ln() / (red - blue))
}

/// The number of Cherenkov photons per millimetre of path, per unit of
/// `1 − 1/(β²n²)`, in the band.
///
/// `dN/dx = 2π α z² (1 − 1/(β²n²)) ∫ dλ/λ²`, with `λ` in the same unit as `dx`.
/// Evaluated with `dx` in mm and `λ` in nm, the integral is
/// `(1/λ_blue − 1/λ_red)` per nm, and the constant carries the `1e6` between nm
/// and mm. The result is photons/mm, which is the unit a 10 cm block's yield is
/// quoted in.
pub fn photons_per_mm(beta: f64, index: f64, blue_nm: f64, red_nm: f64) -> Option<f64> {
    if !(0.0 < beta && beta <= 1.0) || !(0.0 < blue_nm && blue_nm < red_nm) {
        return None;
    }
    let product = beta * index;
    let cone = 1.0 - 1.0 / (product * product);
    if cone <= 0.0 {
        return Some(0.0);
    }
    let alpha = 1.0 / 137.035_999_084_f64;
    // 2π α (1/λ_blue − 1/λ_red) with nm → mm: 1/nm = 1e6/mm.
    let band = 1e6 * (1.0 / blue_nm - 1.0 / red_nm);
    Some(2.0 * std::f64::consts::PI * alpha * cone * band)
}

/// The identity that makes the two Cherenkov equations one: this is a compile-
/// time documentation hook, proving the symbolic form and the float form agree.
///
/// Returns `true` when the CAS's parsed `1/(βn)` evaluates, at numbers, to the
/// same value as [`Cherenkov::cos_angle`] — within one part in 1e12. If the two
/// ever disagree, one of them has drifted, and the simulation would be using a
/// law the CAS no longer states.
#[allow(dead_code)]
fn symbolic_agrees_with_numeric(beta: f64, index: f64) -> bool {
    let Some(expr) = Cherenkov::latex_cosine() else {
        return false;
    };
    let Some(program) = canlab_core::Program::compile(
        &expr,
        &[SymbolId::intern("n"), SymbolId::intern(r"\beta")],
    ) else {
        return false;
    };
    let value = program.eval(&[index, beta]);
    let Some(medium) = Cherenkov::new(index, 0.0) else {
        return false;
    };
    let Some(numeric) = medium.cos_angle(beta) else {
        return false;
    };
    (value - numeric).abs() <= 1e-12 * numeric.abs().max(1e-300)
}
