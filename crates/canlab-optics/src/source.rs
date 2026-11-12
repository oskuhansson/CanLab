//! Where the photons come from: the muon Cherenkov source.
//!
//! The transport traces one photon; something has to decide how many there are,
//! which wavelengths, and which direction. That is this module, and the number it
//! is built around is the Cherenkov yield — how many photons a muon of length
//! `L` leaves in a band — because that decides whether four SiPMs see ten photons
//! or ten thousand.
//!
//! # The yield
//!
//! `dN/dx dλ = 2π α z² / λ² · (1 − 1/(β²n²))`, integrated over the band, gives
//!
//! ```text
//!   dN/dx = 2π α (1 − 1/(β²n²)) (1/λ_min − 1/λ_max)
//! ```
//!
//! with `λ` in the same length unit as `x`. For a muon at β ≈ 1 in a plastic with
//! `n = 1.84` over 400–600 nm that is about 26.9 photons per millimetre — a
//! ten-millimetre-thick crossing sees a few hundred photons, of which the
//! transport measures how many reach a face.
//!
//! The *direction* is the cone, `cos θ = 1/(βn)`, uniform in azimuth. The
//! *wavelength* is `1/λ²` weighted, not uniform — which matters because `n(λ)`
//! is not flat, and a simulation that sampled wavelengths uniformly would put
//! too many red photons in and get the dispersion, and therefore the timing,
//! slightly wrong.
//!
//! # What this module is not
//!
//! It does not model the muon's own energy loss, delta rays or secondary
//! electrons. A photon count from [`expected_photons`] is the *mean* of a
//! Poisson process, and a caller that wants per-event fluctuations samples that
//! Poisson rather than treating the mean as the count.

use crate::cherenkov::{BLUE_NM, RED_NM, default_plastic, medium_index};

/// The Cherenkov photon yield of a path length, photons per millimetre.
///
/// `beta` is the muon's speed over `c` (0.99996 for a 4 GeV muon) and `index`
/// the block's refractive index, over the band [`BLUE_NM`]–[`RED_NM`].
///
/// `None` below the Cherenkov threshold, where there is no yield rather than a
/// zero one — the difference between a muon that cannot radiate and one that
/// happened to radiate nothing in this band.
pub fn photons_per_mm(beta: f64, index: f64) -> Option<f64> {
    if !(0.0 < beta && beta <= 1.0) || index < 1.0 {
        return None;
    }
    let product = beta * index;
    let cone = 1.0 - 1.0 / (product * product);
    if cone <= 0.0 {
        return None;
    }
    let alpha = 1.0 / 137.035_999_084;
    // λ in nanometres -> per millimetre: 1/nm is 1e6/mm.
    let band = 1e6 * (1.0 / BLUE_NM - 1.0 / RED_NM);
    Some(std::f64::consts::TAU * alpha * cone * band)
}

/// The expected number of photons from a path of `length_mm`.
pub fn expected_photons(length_mm: f64, beta: f64, index: f64) -> Option<f64> {
    if !(length_mm >= 0.0) {
        return None;
    }
    photons_per_mm(beta, index).map(|per_mm| per_mm * length_mm)
}

/// A wavelength in the band, weighted by `1/λ²`, from a uniform draw in `[0, 1)`.
///
/// The `1/λ²` distribution's CDF is
/// `(1/λ_blue − 1/λ)/(1/λ_blue − 1/λ_red)`, and its inverse is
/// `λ = 1/(1/λ_blue + u·(1/λ_red − 1/λ_blue))` — one divide, no rejection,
/// exact.
pub fn wavelength_nm(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    let low = 1.0 / BLUE_NM;
    let high = 1.0 / RED_NM;
    1.0 / (low + u * (high - low))
}

/// The refractive index at a wavelength, by the project's Sellmeier model.
pub fn index_at(wavelength_nm: f64) -> Option<f64> {
    medium_index(&default_plastic(), wavelength_nm / 1000.0)
}

/// The Cherenkov cone angle for a photon of this wavelength, radians from the
/// muon's heading.
///
/// The cone is not one angle: it spreads over the band by `arccos(1/n(λ))`. A
/// simulation that used one `n` would put every photon on a circle; this is
/// where the honest spread comes from.
pub fn cone_angle(beta: f64, wavelength_nm: f64) -> Option<f64> {
    let index = index_at(wavelength_nm)?;
    if beta * index <= 1.0 {
        return None;
    }
    Some((1.0 / (beta * index)).acos())
}

/// One photon's birth, as the transport needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Photon {
    /// Its wavelength, nanometres.
    pub wavelength_nm: f64,
    /// The angle from the muon's heading, radians.
    pub theta_rad: f64,
    /// The azimuth around the muon's heading, radians.
    pub phi_rad: f64,
}

impl Photon {
    /// A photon drawn from the cone.
    ///
    /// `azimuth` is uniform in `[0, 2π)` and `wavelength_draw` uniform in
    /// `[0, 1)`; the caller owns the generator and the seed.
    pub fn sample(beta: f64, azimuth: f64, wavelength_draw: f64) -> Option<Photon> {
        let wavelength_nm = wavelength_nm(wavelength_draw);
        let theta_rad = cone_angle(beta, wavelength_nm)?;
        Some(Photon {
            wavelength_nm,
            theta_rad,
            phi_rad: azimuth.rem_euclid(std::f64::consts::TAU),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_yield_matches_the_textbook_formula() {
        // 2π α (1 − 1/1.84²) · 833 mm⁻¹ = 26.85 photons/mm, worked by hand.
        let yield_per_mm = photons_per_mm(1.0, 1.84).expect("above threshold");
        assert!((yield_per_mm - 26.9).abs() < 0.2, "{yield_per_mm}");
    }

    #[test]
    fn a_ten_millimetre_path_sees_a_few_hundred_photons() {
        let count = expected_photons(10.0, 1.0, 1.84).unwrap();
        assert!((250.0..300.0).contains(&count), "{count}");
    }

    #[test]
    fn below_threshold_there_is_no_yield_rather_than_zero() {
        assert!(photons_per_mm(0.5, 1.5).is_none());
        assert!(photons_per_mm(1.0 / 1.5, 1.5).is_none());
    }

    #[test]
    fn wavelengths_are_weighted_towards_the_blue() {
        let mut below = 0;
        let mut above = 0;
        for step in 0..1000 {
            let wavelength = wavelength_nm(step as f64 / 1000.0);
            assert!((BLUE_NM..=RED_NM).contains(&wavelength), "{wavelength}");
            if wavelength < 487.0 { below += 1 } else { above += 1 }
        }
        assert!(below > above, "blue-biased: {below} vs {above}");
        assert!(wavelength_nm(0.5) < (BLUE_NM + RED_NM) / 2.0);
    }

    #[test]
    fn the_index_falls_from_blue_to_red() {
        assert!(index_at(BLUE_NM).unwrap() > index_at(RED_NM).unwrap());
    }

    #[test]
    fn the_cone_is_wider_in_the_blue() {
        let blue = cone_angle(1.0, BLUE_NM).unwrap();
        let red = cone_angle(1.0, RED_NM).unwrap();
        assert!(blue > red, "{blue} vs {red}");
    }

    #[test]
    fn a_below_threshold_beta_has_no_cone() {
        assert!(cone_angle(0.4, BLUE_NM).is_none());
    }

    #[test]
    fn the_source_and_the_emission_law_are_one_law() {
        let medium = crate::cherenkov::Cherenkov::new(index_at(500.0).unwrap(), 0.0).unwrap();
        assert!((cone_angle(1.0, 500.0).unwrap() - medium.angle_at_unit_beta()).abs() < 1e-12);
    }
}
