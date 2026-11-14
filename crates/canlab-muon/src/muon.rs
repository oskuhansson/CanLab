//! Where the muons come from: the angular distribution, and how it changes with
//! altitude.
//!
//! The simulation has been drawing zenith angles from `cos²θ`, which is the
//! textbook approximation. The measurements say the exponent is **not exactly 2**
//! and that it **depends on the muon's momentum, the geomagnetic cutoff and the
//! altitude**. This module carries that, so the training data is drawn from a
//! distribution the literature supports rather than from a round number.
//!
//! # The law
//!
//! ```text
//! I(θ) = I₀ cosⁿ θ
//! ```
//!
//! with `I₀` the vertical flux and `n` between about 1.7 and 2.2 for the
//! integrated (all-momentum) flux. The exponent is a *momentum* effect: low-energy
//! muons decay in the extra atmosphere an inclined path crosses, so they are
//! suppressed at large θ (large `n`); high-energy muons do not decay and their
//! distribution flattens towards `1/cos θ` (`n → 1`).
//!
//! # The measurements
//!
//! | Where | Altitude | Momentum cut | n | I₀ (m⁻²s⁻¹sr⁻¹) |
//! |---|---|---|---|---|
//! | Mumbai, 19°N | sea level | > 255 MeV/c | 2.10 ± 0.05 | 66.7 ± 1.5 |
//! | Monastir, 36°N | 38 m | > 330 MeV/c | 1.82 ± 0.11 | 68.8 ± 1.9 |
//! | Monastir, 36°N | 38 m | > 910 MeV/c | 1.72 ± 0.10 | 56.7 ± 1.6 |
//! | Sakarya, 40°N (Geant4) | 230 m | > 1 GeV | 1.95 ± 0.08 | — |
//! | Riggi et al. | 3100 m | > 200 MeV/c | 1.83 ± 0.13 | 83 ± 8 |
//!
//! The spread is real: `n` falls as the momentum cut rises, and the vertical flux
//! depends on the geomagnetic cutoff and the solar cycle. A single `n = 2` is a
//! convenient fiction, and the domain randomisation treats it as one.
//!
//! # Altitude
//!
//! **The flux rises; the shape does not measurably change.**
//!
//! * **The flux rises.** `I(H) = I(0) · e^{H/L(p)}` with `L(p) = 4900 + 750p`
//!   metres and `p` in GeV/c. At 1 km that is **+13.5 %** for a 4 GeV muon and
//!   **+19.4 %** for a 1 GeV one. The integrated flux is dominated by the lower
//!   energies, so the measured rise at 1 km is nearer **+15–25 %**.
//!
//!   The often-quoted "+36 % at 600 m" is **not an altitude measurement**: it
//!   compares Mumbai (19°N, geomagnetic cutoff 16.4 GV) with Prince Albert
//!   (53°N, cutoff below 1 GV). The vertical sea-level flux itself runs from
//!   ~67 m⁻²s⁻¹sr⁻¹ at 19°N to ~91 at 53°N, so the latitude alone accounts for
//!   the difference.
//!
//! * **The shape does not change measurably.** The depth effect (less decay at
//!   altitude, which would *flatten* the distribution) and the spectral effect
//!   (a softer spectrum at altitude, and low-energy muons have a *steeper*
//!   distribution) nearly cancel. The measurements bear that out: sea level
//!   `n = 2.10 ± 0.05 ± 0.25` (Mumbai), 2.27 km `n = 1.90 ± 0.06 ± 0.10` and
//!   `2.12 ± 0.19 ± 0.11` (two analyses of Mexico City), 3.1 km `n = 1.83 ± 0.13`
//!   (Riggi). The sea-level scatter alone (1.745–2.345 across experiments) is
//!   larger than any altitude trend, so **there is no resolved trend** and this
//!   module does not model one.
//!
//! So the altitude matters as a **rate**, not as a shape. That is worth saying
//! because the mission's science is a rate comparison, and a network trained at
//! one altitude sees the same angular distribution at another.
//!
//! # The inverse
//!
//! The PDF is `∝ cosⁿθ sinθ`, so the CDF is `1 − cos^{n+1}θ` and the inverse is
//!
//! ```text
//! cos θ = (1 − u)^{1/(n+1)}
//! ```
//!
//! which is one `powf` and no rejection. For `n = 2` that is the `(1−u)^{1/3}` the
//! earlier code used.

/// The muon flux at a place and an altitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MuonFlux {
    /// The vertical flux, per square metre per second per steradian.
    pub vertical: f64,
    /// The `cosⁿ θ` exponent.
    pub exponent: f64,
    /// The altitude, metres above sea level.
    pub altitude_m: f64,
}

impl MuonFlux {
    /// The sea-level reference: `I₀ = 70`, `n = 2`, the textbook values.
    pub fn sea_level() -> MuonFlux {
        MuonFlux { vertical: 70.0, exponent: 2.0, altitude_m: 0.0 }
    }

    /// The measured sea-level flux: Mumbai, `I₀ = 66.7`, `n = 2.10`.
    ///
    /// The simulation draws from this rather than from the round `70, 2`, because
    /// it is what the best sea-level measurement gives. The difference is small —
    /// the CDF moves by a few per cent — but it is the measured value, and the
    /// point of carrying a distribution at all is not to round it back.
    pub fn measured() -> MuonFlux {
        MuonFlux { vertical: 66.7, exponent: 2.10, altitude_m: 0.0 }
    }

    /// The same flux at an altitude: the rate rises, the shape does not.
    ///
    /// The scale uses `e^{H/L(p)}` at the mean muon momentum, 4 GeV, which is the
    /// number the mission's own documents quote. The exponent is left alone: the
    /// measured values at sea level and at 2.3 km overlap within their errors, and
    /// the depth and spectral effects that would move it in opposite directions
    /// nearly cancel. The 3.1 km point is lower, but it is one measurement and the
    /// sea-level scatter is as large.
    pub fn at_altitude(altitude_m: f64) -> MuonFlux {
        let base = MuonFlux::sea_level();
        // L(4 GeV) = 4900 + 750·4 = 7900 m.
        let scale_height = 4900.0 + 750.0 * 4.0;
        let vertical = base.vertical * (altitude_m / scale_height).exp();
        MuonFlux { vertical, exponent: base.exponent, altitude_m }
    }

    /// The vertical atmospheric depth at an altitude, g/cm².
    ///
    /// `X = X₀ e^{−h/h₀}` with `X₀ = 1030` g/cm² and `h₀ = 8.4` km.
    pub fn depth_g_per_cm2(&self) -> f64 {
        atmospheric_depth(self.altitude_m)
    }

    /// The flux at a zenith angle, per square metre per second per steradian.
    pub fn at_zenith(&self, zenith_rad: f64) -> f64 {
        let cosine = zenith_rad.cos().max(0.0);
        self.vertical * cosine.powf(self.exponent)
    }

    /// The flux integrated over the upper hemisphere, per square metre per second.
    ///
    /// `∫ I₀ cosⁿθ dΩ = I₀ · 2π/(n+1)`.
    pub fn integrated(&self) -> f64 {
        self.vertical * std::f64::consts::TAU / (self.exponent + 1.0)
    }

    /// A zenith angle drawn from the flux, from a uniform `u` in `[0, 1)`.
    ///
    /// The inverse of the CDF `1 − cos^{n+1}θ`, so `cos θ = (1 − u)^{1/(n+1)}`.
    pub fn sample_zenith(&self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0 - 1e-12);
        (1.0 - u).powf(1.0 / (self.exponent + 1.0)).acos()
    }

    /// The fraction of the flux above a zenith angle.
    ///
    /// `cos^{n+1}θ`, which is what a detector's acceptance cut needs.
    pub fn fraction_above(&self, zenith_rad: f64) -> f64 {
        zenith_rad.cos().max(0.0).powf(self.exponent + 1.0)
    }
}

/// The vertical atmospheric depth at an altitude, g/cm².
fn atmospheric_depth(altitude_m: f64) -> f64 {
    const SEA_LEVEL_DEPTH: f64 = 1030.0;
    const SCALE_HEIGHT_M: f64 = 8400.0;
    SEA_LEVEL_DEPTH * (-altitude_m / SCALE_HEIGHT_M).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inverse_cdf_reproduces_the_distribution() {
        // A histogram of the drawn angles must follow `cosⁿθ sinθ`. The test
        // compares the fraction above 30° with the closed form, which is the one
        // number a wrong inverse would miss.
        let flux = MuonFlux::sea_level();
        let mut above = 0;
        let trials = 20_000;
        for step in 0..trials {
            let u = (step as f64 + 0.5) / trials as f64;
            if flux.sample_zenith(u) > 30.0f64.to_radians() {
                above += 1;
            }
        }
        let measured = above as f64 / trials as f64;
        let expected = flux.fraction_above(30.0f64.to_radians());
        assert!((measured - expected).abs() < 0.01, "{measured} vs {expected}");
        // And the median angle is the one where the CDF is a half.
        let median = flux.sample_zenith(0.5);
        assert!((median - 37.5f64.to_radians()).abs() < 1.0, "{}", median.to_degrees());
    }

    #[test]
    fn the_exponent_moves_the_distribution_the_way_the_literature_says() {
        // A larger exponent is a steeper distribution: less flux at large angles.
        let shallow = MuonFlux { exponent: 1.8, ..MuonFlux::sea_level() };
        let steep = MuonFlux { exponent: 2.2, ..MuonFlux::sea_level() };
        let angle = 45.0f64.to_radians();
        assert!(shallow.at_zenith(angle) > steep.at_zenith(angle));
        // And the measured values, at the angles they were measured at.
        let mumbai = MuonFlux { vertical: 66.7, exponent: 2.10, altitude_m: 0.0 };
        assert!((mumbai.at_zenith(0.0) - 66.7).abs() < 1e-9);
        let cosine = 30.0f64.to_radians().cos();
        assert!((mumbai.at_zenith(30.0f64.to_radians()) - 66.7 * cosine.powf(2.10)).abs() < 1e-9);
    }

    #[test]
    fn the_altitude_raises_the_flux_and_leaves_the_shape_alone() {
        // The two altitude effects: the rate moves, the shape does not. The flux
        // rises by e^{H/7900} — 13.5 % at 1 km — and the exponent is unchanged,
        // because the sea-level and 2.3 km measurements overlap within their
        // errors and the depth and spectral effects cancel.
        let ground = MuonFlux::sea_level();
        let kilometre = MuonFlux::at_altitude(1000.0);
        assert!(kilometre.vertical > ground.vertical);
        let rise = kilometre.vertical / ground.vertical;
        assert!((rise - 1.135).abs() < 0.01, "{rise}");
        assert_eq!(kilometre.exponent, ground.exponent);
        // The rate keeps rising with altitude.
        let three_km = MuonFlux::at_altitude(3100.0);
        assert!(three_km.vertical > kilometre.vertical);
        assert_eq!(three_km.exponent, ground.exponent);
        // The measured sea-level value is what the simulation draws from, and it
        // is steeper than the textbook 2.
        let measured = MuonFlux::measured();
        assert!((measured.exponent - 2.10).abs() < 1e-12);
        assert!(measured.exponent > ground.exponent);
    }

    #[test]
    fn the_depth_is_the_one_the_atmosphere_has() {
        // 1030 g/cm² at sea level and 8.4 km scale height.
        assert!((atmospheric_depth(0.0) - 1030.0).abs() < 1e-9);
        assert!((atmospheric_depth(8400.0) - 1030.0 / std::f64::consts::E).abs() < 0.5);
        assert!(atmospheric_depth(1000.0) < atmospheric_depth(0.0));
        assert!((MuonFlux::at_altitude(1000.0).depth_g_per_cm2() - 914.0).abs() < 1.0);
    }

    #[test]
    fn the_integrated_flux_is_the_closed_form() {
        // I₀ · 2π/(n+1): 70 · 2π/3 = 146.6 for n = 2.
        let flux = MuonFlux::sea_level();
        assert!((flux.integrated() - 146.6).abs() < 0.5, "{}", flux.integrated());
        // The mission's 127 m⁻²s⁻¹ corresponds to n = 2.46 in this convention,
        // which is steeper than any measurement — the mission's figure is a
        // different convention and the two are not the same number.
        let implied = std::f64::consts::TAU * 70.0 / 127.0 - 1.0;
        assert!((implied - 2.46).abs() < 0.02, "{implied}");
    }

    #[test]
    fn the_fraction_above_an_angle_is_the_cdf() {
        let flux = MuonFlux::sea_level();
        assert!((flux.fraction_above(0.0) - 1.0).abs() < 1e-12);
        assert!((flux.fraction_above(std::f64::consts::FRAC_PI_2)).abs() < 1e-12);
        // Half the flux is above the median angle.
        let median = flux.sample_zenith(0.5);
        assert!((flux.fraction_above(median) - 0.5).abs() < 1e-9);
    }
}
