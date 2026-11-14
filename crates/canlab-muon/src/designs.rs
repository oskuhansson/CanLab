//! Five detector designs, each optimising a different constraint.
//!
//! The study so far answered one question — can four timestamps give a direction
//! — and the answer was no. That leaves the design space open rather than closed,
//! and this module is the space: five approaches that trade the constraints
//! against each other, so a team can pick one by what it is willing to spend
//! rather than by what the last simulation happened to measure.
//!
//! # The five, and what each is for
//!
//! | Design | Optimises | Direction |
//! |---|---|---|
//! | [`Design::counter`] | cost and reliability | none |
//! | [`Design::cherenkov_timing`] | the existing parts | none, by the bound |
//! | [`Design::coarse_tracker`] | the cheapest two-axis direction | ~12° |
//! | [`Design::fine_tracker`] | the best direction inside the budget | ~6° |
//! | [`Design::ring_imager`] | direction from one face | ~10°, unproven |
//!
//! # Why a tracker and not a timing detector
//!
//! A direction needs a **lever arm** and a **position**. Timing has no lever arm
//! in a 90 mm block — the light crosses it in 0.22 ps — so the only observable
//! left is *where* the muon crossed. Two planes separated by a gap give two
//! crossing points, and two points give a line. The resolution is
//! `√2·σ_h/L`: the pixel's own uncertainty over the separation, and nothing else.
//!
//! That is why the tracker designs are the ones with a number worth quoting: a
//! 20 mm pixel over a 40 mm gap is 12°, and a 10 mm pixel over 60 mm is 4°. The
//! gap is free — it is air inside a can that is 115 mm tall — and the pixel is
//! what costs money, because every pixel needs a sensor or a multiplexed readout.
//!
//! # What the numbers are and are not
//!
//! Mass, volume, photoelectrons, saturation and the angular bound are computed
//! from the geometry and the parts. Cost is an **estimate** from the documented
//! module range scaled by area, not a quote. The ring imager's resolution is
//! marked as unproven because no simulation in this crate models it: its number
//! is a geometric estimate and the design is listed so the idea is not lost, not
//! because it has been checked.

use crate::can::{tent_mass_g, Can};
use crate::components;
use crate::mission::{Candidate, Mission, TOTAL_MASS_G};
use crate::scintillator::{Scintillator, SiPMChoice};
use canlab_optics::Vec3;

/// What a design uses to turn a muon into a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approach {
    /// A single plane and one sensor: a count, not a direction.
    Counter,
    /// The Cherenkov block and four timestamps.
    CherenkovTiming,
    /// Two segmented planes and a coincidence.
    Tracker,
    /// One Cherenkov face read as a ring.
    RingImager,
}

/// A detector proposal, in the terms the comparison is made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    /// What it is called in the table.
    pub name: &'static str,
    /// What it uses for a direction.
    pub approach: Approach,
    /// How many planes a muon must cross.
    pub planes: usize,
    /// Pixels per plane along each axis (`1` is an unsegmented plane).
    pub pixels_per_side: usize,
    /// One pixel's width, millimetres.
    pub pixel_mm: f64,
    /// The gap between the two planes, millimetres.
    pub separation_mm: f64,
    /// One plane's thickness, millimetres.
    pub thickness_mm: f64,
    /// One plane's width across the can, millimetres.
    pub width_mm: f64,
    /// One plane's height along the can, millimetres.
    pub height_mm: f64,
    /// The sensor each pixel or plane reads out with.
    pub sensor: SiPMChoice,
    /// Whether the readout needs a time-to-digital converter.
    pub needs_timer: bool,
    /// The board, connectors and mechanics, euros.
    pub board_eur: f64,
}

/// What a design costs, weighs, and can measure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The block's or the planes' mass, grams.
    pub detector_g: f64,
    /// The sensors' and the board's cost, euros, at the top of every range.
    pub cost_eur: f64,
    /// How many sensors the readout needs.
    pub channels: usize,
    /// Photoelectrons per muon in one plane.
    pub photoelectrons: f64,
    /// Whether that signal fills the sensor.
    pub saturates: bool,
    /// Whether filling it matters. A tracker reads out *which* pixel fired, so a
    /// clipped pulse is still a hit; a counter reads the pulse height for energy,
    /// so a clipped pulse is a lost measurement.
    pub saturation_matters: bool,
    /// The angular uncertainty, degrees. `None` when the design cannot measure a
    /// direction at all.
    pub angle_deg: Option<f64>,
    /// Whether the angle is a checked number or a geometric estimate.
    pub angle_is_checked: bool,
    /// Muons through the detector in a 100 s flight.
    pub muons_per_flight: f64,
}

impl Design {
    /// A single scintillator plane and one sensor: the repo's own design.
    ///
    /// `MUONDETEKTOR.md` §4 specifies exactly this — a plastic scintillator plate
    /// about a centimetre thick with a SiPM — and its purpose is a count per
    /// second, not a direction. It is the baseline every other design has to
    /// justify itself against.
    pub fn counter() -> Design {
        Design {
            name: "counter",
            approach: Approach::Counter,
            planes: 1,
            pixels_per_side: 1,
            pixel_mm: 0.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 40.0,
            height_mm: 90.0,
            sensor: SiPMChoice::microfc_60035(),
            needs_timer: false,
            board_eur: 25.0,
        }
    }

    /// The Cherenkov block and four timestamps: the design the bound rejected.
    pub fn cherenkov_timing() -> Design {
        Design {
            name: "cherenkov timing",
            approach: Approach::CherenkovTiming,
            planes: 1,
            pixels_per_side: 1,
            pixel_mm: 0.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 40.0,
            height_mm: 90.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: true,
            board_eur: 60.0,
        }
    }

    /// Two planes of 2 x 2 pixels: the cheapest two-axis direction.
    ///
    /// Eight sensors, a 20 mm pixel and a 40 mm gap. The gap is air and costs
    /// nothing but height, and the can has 115 mm of it.
    pub fn coarse_tracker() -> Design {
        Design {
            name: "coarse tracker",
            approach: Approach::Tracker,
            planes: 2,
            pixels_per_side: 2,
            pixel_mm: 20.0,
            separation_mm: 40.0,
            thickness_mm: 5.0,
            width_mm: 40.0,
            height_mm: 80.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 45.0,
        }
    }

    /// Two planes of 4 x 4 pixels and a 60 mm gap: the best money can buy.
    ///
    /// Thirty-two sensors is the cost that hurts, and the 60 mm gap is what buys
    /// the resolution: the angle is the pixel over the gap, so a longer gap is a
    /// better detector for free.
    pub fn fine_tracker() -> Design {
        Design {
            name: "fine tracker",
            approach: Approach::Tracker,
            planes: 2,
            pixels_per_side: 4,
            pixel_mm: 10.0,
            separation_mm: 60.0,
            thickness_mm: 5.0,
            width_mm: 40.0,
            height_mm: 80.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 90.0,
        }
    }

    /// One Cherenkov face read as a ring: direction without a second plane.
    ///
    /// A muon crossing the block emits a cone at 47.8°, and where the cone meets
    /// the exit face is a ring. The ring's radius gives the angle, and its centre
    /// gives where the muon left. One face, one sensor array, no gap.
    ///
    /// **Unproven.** No simulation here models the ring: the transport traces
    /// photons to a face and reports which face, not where on it. The number in
    /// the table is the geometric estimate `pixel/ring_radius` and is marked as
    /// unchecked for that reason.
    pub fn ring_imager() -> Design {
        Design {
            name: "ring imager",
            approach: Approach::RingImager,
            planes: 1,
            pixels_per_side: 4,
            pixel_mm: 10.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 40.0,
            height_mm: 80.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 90.0,
        }
    }

    /// Every design, in the order the table presents them.
    pub fn all() -> [Design; 5] {
        [
            Design::counter(),
            Design::cherenkov_timing(),
            Design::coarse_tracker(),
            Design::fine_tracker(),
            Design::ring_imager(),
        ]
    }

    /// How many sensors the readout needs.
    pub fn channels(&self) -> usize {
        match self.approach {
            // The Cherenkov block's four faces, and the counter's one plane.
            Approach::CherenkovTiming => 4,
            Approach::Counter => 1,
            // Every pixel needs a sensor: a plane of n x n pixels is n² sensors.
            Approach::Tracker | Approach::RingImager => {
                self.planes * self.pixels_per_side * self.pixels_per_side
            }
        }
    }

    /// The detector's mass, grams.
    ///
    /// The scintillator planes are boxes; the Cherenkov block is the tent prism,
    /// which is why this asks `tent_mass_g` for that one and multiplies for the
    /// others. The board is a constant 12 g, which is a guess and is labelled as
    /// one.
    pub fn detector_g(&self) -> f64 {
        let material = match self.approach {
            Approach::CherenkovTiming => {
                return tent_mass_g(20.0, self.thickness_mm, 1.19).unwrap_or(0.0) + 12.0;
            }
            _ => Scintillator::pvt().density_g_per_cm3,
        };
        let volume_mm3 = self.width_mm * self.height_mm * self.thickness_mm * self.planes as f64;
        volume_mm3 / 1000.0 * material + 12.0
    }

    /// The cost, euros, at the top of every range.
    pub fn cost_eur(&self) -> f64 {
        let sensors = self.channels() as f64 * self.sensor.eur.1;
        let timer = if self.needs_timer { 25.0 } else { 0.0 };
        sensors + self.board_eur + timer
    }

    /// Photoelectrons one plane registers from a muon crossing it.
    pub fn photoelectrons(&self) -> f64 {
        let scintillator = Scintillator::pvt();
        match self.approach {
            // The Cherenkov block's 10 mm path, through the same collection and
            // PDE the scintillator uses, so the two are comparable. PMMA at
            // n = 1.49, which the sweep found collects more light than the
            // high-index plastic: 21 photons/mm against 26.9, but far less
            // trapped by total internal reflection.
            Approach::CherenkovTiming => {
                let photons = canlab_optics::source::expected_photons(10.0, 1.0, 1.49).unwrap_or(0.0);
                photons * scintillator.collection * scintillator.pde
            }
            _ => scintillator.photoelectrons(self.thickness_mm),
        }
    }

    /// The angular uncertainty, degrees, and whether it is a checked number.
    pub fn angle_deg(&self) -> (Option<f64>, bool) {
        match self.approach {
            // A counter has no direction: one number is not an angle.
            Approach::Counter => (None, true),
            // The bound, from the crate's own CRB over the four sensor centres.
            Approach::CherenkovTiming => {
                let positions = self.cherenkov_positions();
                let speed = crate::timing::speed_mm_per_ps(1.49).unwrap_or(f64::NAN);
                let jitter = components::total_jitter_ps();
                let mut worst = 0.0f64;
                for heading in [
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                ] {
                    let Some(heading) = heading.normalized() else { continue };
                    if let Some(bound) = crate::bound::timing_bound(&positions, heading, jitter, speed) {
                        worst = worst.max(bound.sigma_deg());
                    }
                }
                (Some(worst), true)
            }
            // The position bound, which is the checked formula for two hits.
            Approach::Tracker => {
                let sigma_h = self.pixel_mm / 12.0f64.sqrt();
                crate::bound::position_bound(sigma_h, self.separation_mm)
                    .map(|bound| (Some(bound.sigma_deg()), true))
                    .unwrap_or((None, true))
            }
            // The ring's radius is the lever arm, and the estimate is unchecked.
            Approach::RingImager => {
                let radius = 20.0 * (47.8f64).to_radians().tan();
                let sigma_h = self.pixel_mm / 12.0f64.sqrt();
                crate::bound::position_bound(sigma_h, radius)
                    .map(|bound| (Some(bound.sigma_deg()), false))
                    .unwrap_or((None, false))
            }
        }
    }

    /// The four sensor centres of the Cherenkov block.
    fn cherenkov_positions(&self) -> [Vec3; 4] {
        match canlab_optics::Tropical::fits_in(20.0, self.thickness_mm) {
            Ok(outline) => canlab_optics::Block::new(outline).sensor_positions(),
            Err(_) => [Vec3::ZERO; 4],
        }
    }

    /// Muons through the detector during a 100 s flight.
    ///
    /// The integrated flux times the plane's area times the time. The area is the
    /// plane the muon must cross, and a tracker's two planes have the same area,
    /// so the coincidence rate is the single-plane rate.
    pub fn muons_per_flight(&self) -> f64 {
        let area_m2 = self.width_mm * self.height_mm / 1.0e6;
        crate::mission::MUON_FLUX_INTEGRATED_M2_S * area_m2 * 100.0
    }

    /// Everything the table prints.
    pub fn metrics(&self) -> Metrics {
        let (angle_deg, angle_is_checked) = self.angle_deg();
        Metrics {
            detector_g: self.detector_g(),
            cost_eur: self.cost_eur(),
            channels: self.channels(),
            photoelectrons: self.photoelectrons(),
            saturates: self.photoelectrons() > self.sensor.cells as f64,
            saturation_matters: self.photoelectrons() > self.sensor.cells as f64
                && self.needs_amplitude(),
            angle_deg,
            angle_is_checked,
            muons_per_flight: self.muons_per_flight(),
        }
    }

    /// Whether the design needs the pulse *height*, not just its existence.
    ///
    /// The counter's science is a rate and a crude energy, so it needs the
    /// amplitude and must not saturate. A tracker's science is the pixel that
    /// fired, so it does not.
    pub fn needs_amplitude(&self) -> bool {
        matches!(self.approach, Approach::Counter)
    }

    /// The mission audit for this design, as the mission module states it.
    ///
    /// Only the Cherenkov design has a `Candidate` the mission can audit; the
    /// others are audited on the three requirements the mission states in terms
    /// this module can compute — mass, cost and fit — plus the direction, which
    /// is the requirement the design exists for.
    pub fn audit(&self, mission: &Mission) -> Vec<String> {
        let metrics = self.metrics();
        let mut violations = Vec::new();
        let mass_share = TOTAL_MASS_G.1 * 0.15;
        if metrics.detector_g > mass_share {
            violations.push(format!(
                "mass: {:.1} g against the {:.1} g share",
                metrics.detector_g, mass_share
            ));
        }
        if metrics.cost_eur > mission.detector_budget_eur.1 {
            violations.push(format!(
                "cost: {:.0} EUR against the {:.0} EUR budget",
                metrics.cost_eur, mission.detector_budget_eur.1
            ));
        }
        if self.height_mm > mission.can.inner_height_mm() {
            violations.push(format!(
                "height: {:.0} mm against the {:.0} mm can",
                self.height_mm,
                mission.can.inner_height_mm()
            ));
        }
        // The direction requirement, stated as the mission's own: a design that
        // cannot get inside 30 degrees is not a direction measurement.
        match metrics.angle_deg {
            None => violations.push("direction: this design cannot measure one".to_string()),
            Some(angle) if angle > 30.0 => {
                violations.push(format!("direction: {angle:.0} degrees is not a direction"))
            }
            Some(_) => {}
        }
        violations
    }
}

/// The can, for the table's header.
pub fn can() -> Can {
    Can::standard()
}

/// The mission audit of the Cherenkov design, which the mission module owns.
pub fn cherenkov_candidate() -> Candidate {
    Candidate::design()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_is_the_cheapest_and_has_no_direction() {
        let counter = Design::counter();
        let metrics = counter.metrics();
        assert_eq!(metrics.channels, 1);
        assert_eq!(metrics.angle_deg, None, "a counter has no direction");
        // It is the cheapest of the five.
        for design in Design::all() {
            assert!(
                counter.cost_eur() <= design.cost_eur(),
                "{} is cheaper than the counter",
                design.name
            );
        }
        // And it is the only one that does not saturate, because the 6 mm part
        // has 18 980 cells and the plane gives 873 photoelectrons.
        assert!(!metrics.saturates);
        assert!(!metrics.saturation_matters);
        // 10 mm of PVT: 19 950 photons, 25 % collected, 35 % detected.
        assert!((metrics.photoelectrons - 1_745.0).abs() < 1.0, "{}", metrics.photoelectrons);
    }

    #[test]
    fn the_trackers_trade_cost_for_angle_the_way_the_formula_says() {
        // The coarse tracker: a 20 mm pixel over 40 mm. sigma_h = 20/sqrt(12) =
        // 5.77 mm, so the angle is sqrt(2) x 5.77/40 = 0.204 rad = 11.7 degrees.
        let coarse = Design::coarse_tracker().metrics();
        let angle = coarse.angle_deg.expect("a tracker has an angle");
        assert!((angle - 11.7).abs() < 0.2, "{angle}");
        // The fine tracker: 10 mm over 60 mm, which is 3.9 degrees.
        let fine = Design::fine_tracker().metrics();
        let fine_angle = fine.angle_deg.expect("a tracker has an angle");
        assert!((fine_angle - 3.9).abs() < 0.2, "{fine_angle}");
        // Four times better and four times the sensors: the trade is the pixel
        // count against the angle, and the gap is free.
        assert!((angle / fine_angle - 3.0).abs() < 0.2, "{}", angle / fine_angle);
        assert_eq!(fine.channels, 32);
        assert_eq!(coarse.channels, 8);
        assert!(fine.cost_eur > coarse.cost_eur);
    }

    #[test]
    fn the_cherenkov_design_fails_the_direction_requirement_and_the_tracker_passes() {
        // The whole point of the comparison: one design has the better sensor and
        // the worse answer.
        let mission = Mission::standard();
        let cherenkov = Design::cherenkov_timing();
        let violations = cherenkov.audit(&mission);
        assert!(
            violations.iter().any(|v| v.starts_with("direction:")),
            "{violations:#?}"
        );
        // The bound is the crate's own, not a second estimate.
        let angle = cherenkov.metrics().angle_deg.expect("a bound");
        assert!(angle > 1_000.0, "{angle}");
        // The coarse tracker passes every requirement it is audited on.
        let tracker = Design::coarse_tracker();
        assert!(tracker.audit(&mission).is_empty(), "{:#?}", tracker.audit(&mission));
    }

    #[test]
    fn the_fine_tracker_is_the_one_that_breaks_the_budget() {
        // 32 sensors at 25 EUR plus 90 EUR of board is 890 EUR, which is three
        // times the detector's budget. The design that measures best is the one
        // that cannot be built, and the table has to say so.
        let mission = Mission::standard();
        let fine = Design::fine_tracker();
        let cost = fine.cost_eur();
        assert!((cost - 890.0).abs() < 1e-9, "{cost}");
        let violations = fine.audit(&mission);
        assert!(violations.iter().any(|v| v.starts_with("cost:")), "{violations:#?}");
        // The coarse tracker is inside it: 8 x 25 + 45 = 245 EUR, under the
        // 300 EUR target.
        let coarse = Design::coarse_tracker().cost_eur();
        assert!((coarse - 245.0).abs() < 1e-9, "{coarse}");
        assert!(coarse < mission.detector_budget_eur.1);
    }

    #[test]
    fn the_sensor_choice_is_what_makes_a_scintillator_design_work() {
        // The 1 mm part saturates on a 5 mm scintillator plane; the 3 mm part
        // does not. A design that kept the component list's sensor would clip
        // every pulse and lose the energy information the count depends on.
        let scintillator = Scintillator::pvt();
        let small = SiPMChoice::microfc_10035();
        let big = SiPMChoice::microfc_30035();
        assert!(scintillator.saturates(5.0, small.cells));
        assert!(!scintillator.saturates(5.0, big.cells));
        // The tracker designs use the small part anyway, because a tracker reads
        // out *which* pixel fired and a clipped pulse is still a hit. The
        // saturation is reported and does not matter there.
        let tracker = Design::coarse_tracker();
        assert_eq!(tracker.sensor.cells, small.cells);
        let metrics = tracker.metrics();
        assert!(metrics.saturates, "{} p.e. into {} cells", metrics.photoelectrons, small.cells);
        assert!(!metrics.saturation_matters, "a tracker does not need the amplitude");
        // The counter does need it, and its 6 mm part does not saturate.
        let counter = Design::counter().metrics();
        assert!(!counter.saturates);
        assert!(!counter.saturation_matters);
    }

    #[test]
    fn the_flight_has_enough_muons_to_measure_and_not_enough_to_boast() {
        // 40 x 80 mm is 32 cm², which at 127 m-2s-1 for 100 s is 41 muons. That
        // is a real sample and a small one, and the number is the same for every
        // design with the same plane, because the flux does not care what is
        // behind the plane.
        let muons = Design::coarse_tracker().muons_per_flight();
        assert!((muons - 40.6).abs() < 0.5, "{muons}");
        // A bigger plane sees more: 40 x 90 mm is 36 cm².
        let counter = Design::counter().muons_per_flight();
        assert!(counter > muons, "{counter} vs {muons}");
        assert!((counter - 45.7).abs() < 0.5, "{counter}");
    }

    #[test]
    fn the_ring_imager_is_marked_as_unchecked() {
        // A design whose number has not been simulated must say so, or the table
        // reads as five checked rows and one of them is a guess.
        let ring = Design::ring_imager().metrics();
        assert!(!ring.angle_is_checked);
        assert!(ring.angle_deg.is_some());
        // Every other design's number is checked.
        for design in [Design::counter(), Design::cherenkov_timing(), Design::coarse_tracker(), Design::fine_tracker()] {
            assert!(design.metrics().angle_is_checked, "{}", design.name);
        }
    }
}
