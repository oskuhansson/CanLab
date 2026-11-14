//! Plastic scintillator: how much light a muon leaves, and why it is not
//! Cherenkov light.
//!
//! The block in `canlab-optics` is a **Cherenkov radiator**: a high-index plastic
//! that emits ~27 photons per millimetre. That number is what makes the detector
//! photon-starved — a 1 mm² sensor 25 mm away sees 0.07 photons per muon — and it
//! is why every design in this crate is fighting for a timestamp.
//!
//! A **plastic scintillator** is a different material with a different mechanism.
//! A muon crossing it excites the fluors, which emit ~10 000 photons per MeV, and
//! a minimum-ionising muon deposits ~2 MeV per centimetre. The same centimetre
//! that gives 270 Cherenkov photons gives **20 000** scintillation photons: a
//! factor of 74.
//!
//! # The consequence, which is not the one the study expected
//!
//! The photon budget stops being the problem. A 5 mm plane gives ~1 000
//! photoelectrons, so the counting is easy and the *direction* is the only
//! question left. But the same factor breaks the sensor: the MICROFC-10035 has
//! **504 microcells**, and 1 000 photoelectrons saturate it twice over. The 1 mm²
//! part is the right choice for a Cherenkov radiator, which produces a handful of
//! photons, and the wrong choice for a scintillator, which produces thousands.
//!
//! That is the kind of detail a component list cannot carry: the part is correct
//! for one light source and wrong for the other, and the list does not say which.
//!
//! # What is assumed and what is measured
//!
//! The light yield, the density and the emission wavelength are the
//! scintillator's datasheet values for a PVT plastic such as EJ-200. The
//! **collection efficiency is an assumption**: it depends on the wrapping, the
//! light guide and the coupling, and a real paddle can be anywhere from 10 % to
//! 40 %. It is a parameter here rather than a constant for exactly that reason.

/// A plastic scintillator, by the numbers that decide its light output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scintillator {
    /// Photons emitted per MeV of energy deposited.
    pub light_yield_per_mev: f64,
    /// Density, grams per cubic centimetre.
    pub density_g_per_cm3: f64,
    /// Energy a minimum-ionising particle loses, MeV per centimetre.
    pub dedx_mev_per_cm: f64,
    /// The fraction of emitted light that reaches the sensor.
    pub collection: f64,
    /// The sensor's photon-detection efficiency at the emission wavelength.
    pub pde: f64,
}

impl Scintillator {
    /// A PVT plastic scintillator with a diffuse reflector and a good coupling.
    ///
    /// The yield, density and energy loss are EJ-200's; the collection is the
    /// middle of the 10–40 % a well-made paddle achieves, and the PDE is the
    /// C-series value at 420 nm.
    pub fn pvt() -> Scintillator {
        Scintillator {
            light_yield_per_mev: 10_000.0,
            density_g_per_cm3: 1.023,
            dedx_mev_per_cm: 1.95 * 1.023,
            collection: 0.25,
            pde: 0.35,
        }
    }

    /// Photons emitted per millimetre of a muon's path.
    pub fn photons_per_mm(&self) -> f64 {
        self.light_yield_per_mev * self.dedx_mev_per_cm / 10.0
    }

    /// Photons emitted by a path of `path_mm`.
    pub fn photons(&self, path_mm: f64) -> f64 {
        // A negative path is refused rather than answered with a negative
        // count; `NaN` is refused by the same test because `NaN < 0.0` is false
        // and `NaN.is_finite()` is not.
        if path_mm < 0.0 || !path_mm.is_finite() {
            return 0.0;
        }
        self.photons_per_mm() * path_mm
    }

    /// Photoelectrons the sensor registers from a path of `path_mm`.
    ///
    /// Not clamped by the microcell count: saturation is a property of the
    /// *sensor*, and hiding it here would make a saturating design look fine.
    /// [`Scintillator::saturates`] is the question, asked separately.
    pub fn photoelectrons(&self, path_mm: f64) -> f64 {
        self.photons(path_mm) * self.collection * self.pde
    }

    /// Whether a path of `path_mm` fills more than `cells` microcells.
    ///
    /// A SiPM reports one photoelectron per microcell per event, so a signal
    /// larger than the microcell count is clipped and the pulse height stops
    /// carrying energy information. The threshold is the count itself: at
    /// exactly `cells` photoelectrons the sensor is full.
    pub fn saturates(&self, path_mm: f64, cells: usize) -> bool {
        self.photoelectrons(path_mm) > cells as f64
    }
}

/// A silicon photomultiplier, by the numbers that decide what it can read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiPMChoice {
    /// The part number.
    pub part: &'static str,
    /// The active area's side, millimetres.
    pub side_mm: f64,
    /// Microcells: the most photoelectrons one event can report.
    pub cells: usize,
    /// Photon-detection efficiency at the scintillator's emission wavelength.
    pub pde: f64,
    /// Cost, euros, as a range. An estimate from the documented module range,
    /// scaled by area; it is not a quote.
    pub eur: (f64, f64),
}

impl SiPMChoice {
    /// The 1 mm² part from the component list.
    pub fn microfc_10035() -> SiPMChoice {
        SiPMChoice { part: "MICROFC-10035", side_mm: 1.0, cells: 504, pde: 0.35, eur: (10.0, 25.0) }
    }

    /// The 3 mm² part: four times the cells, for a light source that needs them.
    pub fn microfc_30035() -> SiPMChoice {
        SiPMChoice { part: "MICROFC-30035", side_mm: 3.0, cells: 4_774, pde: 0.35, eur: (20.0, 45.0) }
    }

    /// The 6 mm² part: the largest the C-series offers.
    pub fn microfc_60035() -> SiPMChoice {
        SiPMChoice { part: "MICROFC-60035", side_mm: 6.0, cells: 18_980, pde: 0.35, eur: (40.0, 90.0) }
    }

    /// The area, mm².
    pub fn area_mm2(&self) -> f64 {
        self.side_mm * self.side_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scintillator_gives_seventy_times_the_cherenkov_light() {
        // The number the whole comparison turns on. A centimetre of PVT emits
        // 10 000 photons/MeV x 2 MeV = 20 000 photons; a centimetre of the
        // Cherenkov block emits 26.9 photons/mm x 10 = 269.
        let scintillator = Scintillator::pvt();
        assert!((scintillator.photons_per_mm() - 1_995.0).abs() < 1.0, "{}", scintillator.photons_per_mm());
        let cherenkov = canlab_optics::source::photons_per_mm(1.0, 1.84).expect("above threshold");
        let ratio = scintillator.photons_per_mm() / cherenkov;
        assert!((ratio - 74.0).abs() < 2.0, "{ratio}");
        // And at the design block's 10 mm path: 19 950 against 269.
        assert!((scintillator.photons(10.0) - 19_950.0).abs() < 10.0);
    }

    #[test]
    fn the_photoelectrons_are_the_photons_collection_and_pde() {
        // 5 mm: 9 975 photons, 25 % collected, 35 % detected = 873 p.e.
        let scintillator = Scintillator::pvt();
        let pe = scintillator.photoelectrons(5.0);
        assert!((pe - 873.0).abs() < 1.0, "{pe}");
        // Zero path is zero light, and a negative path is refused rather than
        // answered with a negative count.
        assert_eq!(scintillator.photoelectrons(0.0), 0.0);
        assert_eq!(scintillator.photons(-1.0), 0.0);
    }

    #[test]
    fn the_one_millimetre_sensor_saturates_on_a_scintillator_and_not_on_cherenkov() {
        // The component list's part is correct for one light source and wrong
        // for the other, and this is the assertion that says so.
        let scintillator = Scintillator::pvt();
        let small = SiPMChoice::microfc_10035();
        let big = SiPMChoice::microfc_30035();
        // A 5 mm scintillator plane: 873 p.e. against 504 cells.
        assert!(scintillator.saturates(5.0, small.cells), "{} p.e.", scintillator.photoelectrons(5.0));
        // The same plane on the 3 mm part: 873 against 4 774 cells, no.
        assert!(!scintillator.saturates(5.0, big.cells));
        // A Cherenkov block's 10 mm path gives 269 photons, which is 24 p.e.
        // after the same collection and PDE — nowhere near either sensor's limit.
        let cherenkov_pe = 269.0 * scintillator.collection * scintillator.pde;
        assert!(cherenkov_pe < small.cells as f64 / 10.0, "{cherenkov_pe}");
        assert!(!scintillator.saturates(0.1, small.cells));
    }

    #[test]
    fn the_sensor_choice_costs_what_its_area_says() {
        // The three parts are ordered by area and by price, and the price rises
        // faster than the area: a 6 mm part is 36 times the area and under four
        // times the top of the range.
        let parts = [SiPMChoice::microfc_10035(), SiPMChoice::microfc_30035(), SiPMChoice::microfc_60035()];
        for pair in parts.windows(2) {
            assert!(pair[1].area_mm2() > pair[0].area_mm2());
            assert!(pair[1].cells > pair[0].cells);
            assert!(pair[1].eur.1 > pair[0].eur.1);
        }
        assert!((parts[2].area_mm2() / parts[0].area_mm2() - 36.0).abs() < 1e-9);
        assert!(parts[2].eur.1 / parts[0].eur.1 < 4.0);
    }
}
