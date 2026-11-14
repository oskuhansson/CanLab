//! Domain randomisation: the parameters nobody has measured, and how they vary.
//!
//! A network trained on one value of a parameter learns that value. If the real
//! sheet scatters over 25 mm and the training data used 15 mm, the network has
//! learned a bias that no amount of training removes — it is a property of the
//! data, not of the fit. Randomising the parameter per event means the network
//! cannot learn it, so it learns the physics that survives the variation.
//!
//! # What is randomised, and why each one
//!
//! | Parameter | Range | Why it is unknown |
//! |---|---|---|
//! | `lambda_mm` | 4–40 mm | The light spreading length of a thin scintillator sheet depends on its thickness, its wrapping and its polish, and nobody has measured this sheet |
//! | `ballistic_fraction` | 0.15–0.85 | How much light reaches the pixel under the hit without scattering depends on the coupling and the reflector |
//! | `collection` | 0.10–0.40 | The fraction reaching the array; a real paddle spans this |
//! | `pde` | 0.25–0.45 | The datasheet's 31–41 % at two overvoltages, plus temperature |
//! | `crosstalk` | 0–0.12 | 7 % is the datasheet's typical at 35 µm cells; the spread is not |
//! | `dark_counts` | 0–3 pe | 30–96 kHz over a ~100 ns window, times the pixel count |
//! | `gain_spread` | 0–0.15 | No two pixels match; the spread is not specified |
//! | `noise_pe` | 0–1.5 pe | The readout's noise, which depends on the board |
//! | `gap_mm` | ±1 mm | The machined spacing has a tolerance |
//! | `pixel_pitch_mm` | ±0.3 mm | The array's own tolerance |
//! | `thickness_mm` | ±0.2 mm | The sheet's thickness has a tolerance |
//!
//! # What is *not* randomised, and why that is a decision
//!
//! The muon's angular distribution is not randomised: it is the measured
//! `cos²·¹θ` flux (`MuonFlux::measured`), and varying it would be varying the
//! physics rather than the uncertainty. The exponent's own scatter across
//! experiments is ±0.25, which is larger than the altitude trend, so a single
//! measured value is the honest choice. The
//! geometry's *nominal* dimensions are not randomised either — only their
//! tolerances — because a design that fits the can at 40 mm and not at 44 mm is a
//! design decision, not an uncertainty.
//!
//! # The two ways randomisation can go wrong
//!
//! **Too narrow**, and the network has learned the training values. **Too wide**,
//! and the network learns nothing because every event is a different detector;
//! the loss curve flattens and the model is useless in a way that looks like
//! underfitting. The ranges here are deliberately the *plausible* span for each
//! parameter and not a safety margin around a guess: a network trained across
//! them is trained on every sheet a team might build, which is the point.

use crate::tracker::{LightModel, Rng, Tracker};

/// The ranges every randomised parameter is drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Domain {
    /// The light spreading length, millimetres.
    pub lambda_mm: (f64, f64),
    /// The fraction of light that does not scatter.
    pub ballistic_fraction: (f64, f64),
    /// The fraction of emitted light reaching the array.
    pub collection: (f64, f64),
    /// The fraction crossing the bonded interface: 0.85–1.0 bonded, 0.13 with an
    /// air gap. The standard domain is the *bonded* build, because the adhesive
    /// is in the budget and the design assumes it.
    pub bond_transmission: (f64, f64),
    /// The photon-detection efficiency.
    pub pde: (f64, f64),
    /// The fraction of charge leaking to a neighbour.
    pub crosstalk: (f64, f64),
    /// Dark counts per pixel per event.
    pub dark_counts: (f64, f64),
    /// The pixels' relative gain spread.
    pub gain_spread: (f64, f64),
    /// Electronic noise, photoelectrons.
    pub noise_pe: (f64, f64),
    /// The gap's tolerance, millimetres either side.
    pub gap_tolerance_mm: f64,
    /// The pixel pitch's tolerance, millimetres either side.
    pub pitch_tolerance_mm: f64,
    /// The sheet's thickness tolerance, millimetres either side.
    pub thickness_tolerance_mm: f64,
}

impl Domain {
    /// The ranges this study uses, and the ones its tables were produced with.
    pub fn standard() -> Domain {
        Domain {
            lambda_mm: (4.0, 40.0),
            ballistic_fraction: (0.15, 0.85),
            collection: (0.10, 0.40),
            bond_transmission: (0.85, 1.0),
            pde: (0.25, 0.45),
            crosstalk: (0.0, 0.12),
            dark_counts: (0.0, 3.0),
            gain_spread: (0.0, 0.15),
            noise_pe: (0.0, 1.5),
            gap_tolerance_mm: 1.0,
            pitch_tolerance_mm: 0.3,
            thickness_tolerance_mm: 0.2,
        }
    }

    /// A domain with no variation at all: every event uses the nominal model.
    ///
    /// This is the control. A network trained on it and tested on it measures what
    /// the architecture can do; the same network tested on [`Domain::standard`]
    /// measures what randomisation bought. Without the control the comparison has
    /// no baseline.
    pub fn fixed() -> Domain {
        let nominal = LightModel::nominal();
        Domain {
            lambda_mm: (nominal.lambda_mm, nominal.lambda_mm),
            ballistic_fraction: (nominal.ballistic_fraction, nominal.ballistic_fraction),
            collection: (nominal.collection, nominal.collection),
            bond_transmission: (nominal.bond_transmission, nominal.bond_transmission),
            pde: (nominal.pde, nominal.pde),
            crosstalk: (nominal.crosstalk, nominal.crosstalk),
            dark_counts: (nominal.dark_counts, nominal.dark_counts),
            gain_spread: (nominal.gain_spread, nominal.gain_spread),
            noise_pe: (nominal.noise_pe, nominal.noise_pe),
            gap_tolerance_mm: 0.0,
            pitch_tolerance_mm: 0.0,
            thickness_tolerance_mm: 0.0,
        }
    }

    /// A domain outside the training range, for measuring the gap.
    ///
    /// The light spreads twice as far as anything the network was trained on. A
    /// network that learned the physics degrades gracefully; one that learned the
    /// training values fails. This is the closest thing to a sim-to-real test that
    /// can be run without a real detector.
    pub fn shifted() -> Domain {
        Domain {
            lambda_mm: (55.0, 90.0),
            ballistic_fraction: (0.02, 0.12),
            ..Domain::standard()
        }
    }

    /// A light model drawn from the domain, and the geometry it implies.
    ///
    /// The geometry's tolerances are drawn here too, because a network sees the
    /// pixels' *positions* only through the pattern, and a pitch that is 0.3 mm
    /// off is a systematic shift in every pattern.
    pub fn sample(&self, tracker: &Tracker, rng: &mut Rng) -> (LightModel, Tracker) {
        let light = LightModel {
            lambda_mm: rng.range(self.lambda_mm.0, self.lambda_mm.1),
            ballistic_fraction: rng.range(self.ballistic_fraction.0, self.ballistic_fraction.1),
            collection: rng.range(self.collection.0, self.collection.1),
            bond_transmission: rng.range(self.bond_transmission.0, self.bond_transmission.1),
            pde: rng.range(self.pde.0, self.pde.1),
            crosstalk: rng.range(self.crosstalk.0, self.crosstalk.1),
            dark_counts: rng.range(self.dark_counts.0, self.dark_counts.1),
            gain_spread: rng.range(self.gain_spread.0, self.gain_spread.1),
            noise_pe: rng.range(self.noise_pe.0, self.noise_pe.1),
        };
        let geometry = Tracker {
            gap_mm: tracker.gap_mm + rng.range(-self.gap_tolerance_mm, self.gap_tolerance_mm),
            thickness_mm: tracker.thickness_mm
                + rng.range(-self.thickness_tolerance_mm, self.thickness_tolerance_mm),
            // The pitch tolerance moves the plane's usable width, which is the
            // same as moving the pixels.
            width_mm: tracker.width_mm
                + rng.range(-self.pitch_tolerance_mm, self.pitch_tolerance_mm)
                    * tracker.pixels_per_side as f64,
            ..*tracker
        };
        (light, geometry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_range_is_ordered_and_inside_the_plausible_span() {
        // A reversed range would make `range` return values outside the domain
        // and no test downstream would notice, because a uniform draw from a
        // reversed interval is still a uniform draw.
        let domain = Domain::standard();
        let ranges = [
            ("lambda", domain.lambda_mm),
            ("ballistic", domain.ballistic_fraction),
            ("collection", domain.collection),
            ("bond", domain.bond_transmission),
            ("pde", domain.pde),
            ("crosstalk", domain.crosstalk),
            ("dark", domain.dark_counts),
            ("gain", domain.gain_spread),
            ("noise", domain.noise_pe),
        ];
        for (name, (low, high)) in ranges {
            assert!(low <= high, "{name}: {low} > {high}");
            assert!(low.is_finite() && high.is_finite(), "{name}");
        }
        // The physics bounds: a fraction is in [0, 1] and a length is positive.
        assert!(domain.ballistic_fraction.0 >= 0.0 && domain.ballistic_fraction.1 <= 1.0);
        assert!(domain.collection.1 <= 1.0 && domain.pde.1 <= 1.0);
        assert!(domain.crosstalk.0 >= 0.0 && domain.crosstalk.1 < 1.0);
        assert!(domain.lambda_mm.0 > 0.0);
    }

    #[test]
    fn the_fixed_domain_is_the_nominal_model() {
        // The control: every draw is the same model, so a network trained on it
        // sees one detector and the comparison with the randomised domain
        // measures the randomisation and nothing else.
        let domain = Domain::fixed();
        let tracker = Tracker::short();
        let mut rng = Rng::new(1);
        let nominal = LightModel::nominal();
        for _ in 0..50 {
            let (light, geometry) = domain.sample(&tracker, &mut rng);
            assert_eq!(light, nominal);
            assert_eq!(geometry, tracker);
        }
    }

    #[test]
    fn a_sample_is_inside_the_domain_and_the_domain_is_actually_used() {
        // Every draw is inside its range, and the draws cover the range rather
        // than clustering: a sampler that returned the midpoint would pass the
        // first assertion and fail the second.
        let domain = Domain::standard();
        let tracker = Tracker::short();
        let mut rng = Rng::new(2026);
        let mut lambdas = Vec::new();
        for _ in 0..2_000 {
            let (light, geometry) = domain.sample(&tracker, &mut rng);
            assert!((domain.lambda_mm.0..=domain.lambda_mm.1).contains(&light.lambda_mm));
            assert!((domain.ballistic_fraction.0..=domain.ballistic_fraction.1)
                .contains(&light.ballistic_fraction));
            assert!((domain.collection.0..=domain.collection.1).contains(&light.collection));
            assert!((domain.pde.0..=domain.pde.1).contains(&light.pde));
            assert!((domain.gap_tolerance_mm).abs() >= 0.0);
            assert!(
                (geometry.gap_mm - tracker.gap_mm).abs() <= domain.gap_tolerance_mm + 1e-9,
                "{}",
                geometry.gap_mm
            );
            lambdas.push(light.lambda_mm);
        }
        let mean = lambdas.iter().sum::<f64>() / lambdas.len() as f64;
        let expected = (domain.lambda_mm.0 + domain.lambda_mm.1) / 2.0;
        assert!((mean - expected).abs() < 1.0, "{mean} vs {expected}");
        let low = lambdas.iter().filter(|value| **value < 10.0).count();
        let high = lambdas.iter().filter(|value| **value > 34.0).count();
        assert!(low > 100 && high > 100, "{low} low, {high} high");
    }

    #[test]
    fn the_shifted_domain_is_outside_the_training_range() {
        // The sim-to-real probe: if it overlapped the training domain it would
        // measure nothing.
        let standard = Domain::standard();
        let shifted = Domain::shifted();
        assert!(shifted.lambda_mm.0 > standard.lambda_mm.1, "{:?}", shifted.lambda_mm);
        assert!(shifted.ballistic_fraction.1 < standard.ballistic_fraction.0);
    }

    #[test]
    fn the_ranges_are_the_ones_the_tables_quote() {
        // Pinned, because a table in the documentation quotes them and a silent
        // edit to one would make the document wrong rather than the code.
        let domain = Domain::standard();
        assert_eq!(domain.lambda_mm, (4.0, 40.0));
        assert_eq!(domain.ballistic_fraction, (0.15, 0.85));
        assert_eq!(domain.collection, (0.10, 0.40));
        assert_eq!(domain.bond_transmission, (0.85, 1.0));
        assert_eq!(domain.pde, (0.25, 0.45));
        assert_eq!(domain.crosstalk, (0.0, 0.12));
        assert_eq!(domain.dark_counts, (0.0, 3.0));
        assert_eq!(domain.gain_spread, (0.0, 0.15));
        assert_eq!(domain.noise_pe, (0.0, 1.5));
    }
}
