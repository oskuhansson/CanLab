//! The mission's parameters and requirements: one file, every simulation.
//!
//! Every number a detector study depends on lives here, with the document it came
//! from. A simulation that carries its own copy of "the can is 115 mm" is a
//! simulation that will one day disagree with the mission about the can, and the
//! disagreement will be found by a competition judge rather than by a test.
//!
//! # Where each figure comes from
//!
//! The can, the mass, the budget, the flight profile and the scoring come from
//! `can-sat/dokument/CANSAT-GRUND.md` and `KOSTNADER.md`. The parts and their
//! datasheet numbers are in [`crate::components`]. The muon flux is
//! `can-sat/dokument/MUONDETEKTOR.md` §2. The manufacturing envelope is the
//! team's own: a CNC machine is available, which changes the geometry space more
//! than any other line in this file.
//!
//! # Why the CNC matters to the physics, not just to the shape
//!
//! The block was a *tent prism* because a tent is what you can cut and still keep
//! a total-internal-reflection cavity. With a CNC, the outline is any closed
//! 2.5D shape within the tool's reach, so the tent is a choice rather than a
//! manufacturing limit — and the study should sweep outlines, not just scales.
//!
//! The same machine introduces a loss the transport does not model: **tool marks
//! scatter**. A machined acrylic face has a roughness of order a micrometre, and
//! a photon trapped by total internal reflection meets that face at every bounce.
//! Scattering at a rough face is not the same as Fresnel at a smooth one, and
//! [`Manufacturing::roughness_um`] is the parameter a scattering model would need.
//! Until that model exists, a machined block's light yield is an *upper* bound.
//!
//! # The requirements are checked, not described
//!
//! [`Mission::audit`] takes a candidate detector and returns every requirement it
//! breaks, with the measured number and the limit. A requirement that is only
//! written down is a requirement nobody enforces.

use crate::can::{tent_mass_g, Can, CanError};
use crate::components;

/// The competition's can: 115 mm tall, Ø66 mm.
///
/// The dimensions are the European drinks can the rules specify, so they are not
/// a design choice. The wall allowance is, and it is a parameter of [`Can`].
pub const CAN_HEIGHT_MM: f64 = 115.0;
/// The can's diameter, millimetres.
pub const CAN_DIAMETER_MM: f64 = 66.0;
/// The can's nominal volume, millilitres — the "~330 ml" the rules quote.
pub const CAN_VOLUME_ML: f64 = 330.0;

/// The total mass the can may have, grams, including the battery.
pub const TOTAL_MASS_G: (f64, f64) = (300.0, 350.0);
/// The whole project's budget, euros.
pub const TOTAL_BUDGET_EUR: f64 = 500.0;
/// The share of the budget the muon detector may spend, euros.
///
/// A range and not a number: the team's own allocation, not a rule. The lower end
/// is what a single-sensor counter costs, the upper end what a four-sensor
/// timing detector costs once the board and the connectors are counted.
pub const DETECTOR_BUDGET_EUR: (f64, f64) = (200.0, 300.0);

/// The acceleration the rules require the can to survive, g.
pub const G_REQUIRED: f64 = 20.0;
/// The acceleration the team designs for, g — the measured value in Vasa.
pub const G_DESIGN: f64 = 30.0;
/// The parachute mount's tensile requirement, newtons.
pub const PARACHUTE_MOUNT_N: f64 = 50.0;
/// The axial space above the can in the rocket, millimetres.
pub const LID_SPACE_MM: f64 = 45.0;
/// The hours of operation one charge must cover.
pub const POWER_HOURS: f64 = 2.0;

/// The apogee the flight reaches, metres.
pub const APOGEE_M: (f64, f64) = (750.0, 1100.0);
/// The descent speed the rules allow, metres per second.
pub const DESCENT_MPS: (f64, f64) = (5.0, 12.0);
/// The data-taking time that follows, seconds: apogee divided by descent.
pub const MEASUREMENT_S: (f64, f64) = (68.0, 137.0);
/// The temperature the can flies in, degrees Celsius — March in Vasa.
pub const OPERATING_C: (f64, f64) = (0.0, 10.0);

/// The vertical muon flux at sea level, per square metre per second per
/// steradian.
///
/// `MUONDETEKTOR.md` §2 quotes ~70 m⁻²s⁻¹sr⁻¹, which integrates to ~127 m⁻²s⁻¹
/// over the sky, or about one muon per square centimetre per minute. The
/// integrated figure is the one a counter's rate is compared with.
pub const MUON_FLUX_VERTICAL_M2_S_SR: f64 = 70.0;
/// The integrated flux over the upper hemisphere, per square metre per second.
pub const MUON_FLUX_INTEGRATED_M2_S: f64 = 127.0;
/// How much higher the flux is at 1 km than at sea level, as a factor.
pub const MUON_FLUX_1KM_FACTOR: (f64, f64) = (1.25, 1.40);
/// The mean muon energy at sea level, GeV.
pub const MUON_MEAN_ENERGY_GEV: f64 = 4.0;

/// The manufacturing envelope: what the team can actually make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Manufacturing {
    /// Whether a CNC machine is available. It is, and that is the point of this
    /// field: a constraint that exists is a constraint the study should use.
    pub cnc: bool,
    /// The smallest tool radius the machine can run, millimetres.
    ///
    /// An inside corner cannot be sharper than the cutter, so this is the minimum
    /// radius of any concave corner in the outline. A 3 mm end mill gives 1.5 mm.
    pub min_tool_radius_mm: f64,
    /// The deepest cut the machine can make in one setup, millimetres.
    pub max_depth_mm: f64,
    /// The surface roughness a machined acrylic face has, micrometres Ra.
    ///
    /// A micrometre is what a sharp cutter leaves on acrylic. It is small against
    /// the block and large against a wavelength: 500 nm light meeting a 1 µm
    /// scratch sees a rough surface, not a mirror, and the transport's Fresnel
    /// reflection assumes a mirror.
    pub roughness_um: f64,
    /// Whether the faces can be flame- or vapour-polished after machining.
    ///
    /// Polishing is what turns a scattering surface back into a total-internal-
    /// reflection surface, and it is a step a team can do with a hot air gun and
    /// a solvent. If it is available, `roughness_um` is the *pre-polish* figure.
    pub polish_available: bool,
}

impl Manufacturing {
    /// The team's own shop.
    pub fn available() -> Manufacturing {
        Manufacturing {
            cnc: true,
            min_tool_radius_mm: 1.5,
            max_depth_mm: 60.0,
            roughness_um: 1.0,
            polish_available: true,
        }
    }
}

/// A detector proposal, in the terms the requirements are stated in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// The tent's scale, millimetres: the half-width of the outline.
    pub scale_mm: f64,
    /// The extrusion thickness, millimetres.
    pub thickness_mm: f64,
    /// The block's refractive index at the band's centre.
    pub index: f64,
    /// The block's density, grams per cubic centimetre.
    pub density_g_per_cm3: f64,
    /// How many sensors the readout has.
    pub sensors: usize,
    /// One sensor's cost, euros, as a range.
    pub sensor_eur: (f64, f64),
    /// The readout board, the timer and the connectors, euros.
    pub board_eur: f64,
    /// The block's material and machining, euros.
    pub block_eur: f64,
}

impl Candidate {
    /// The design point the study has been using: a 20 mm tent, 10 mm deep,
    /// PMMA, four sensors.
    pub fn design() -> Candidate {
        Candidate {
            scale_mm: 20.0,
            thickness_mm: 10.0,
            index: 1.49,
            density_g_per_cm3: 1.19,
            sensors: 4,
            sensor_eur: (10.0, 25.0),
            board_eur: 60.0,
            block_eur: 8.0,
        }
    }

    /// The block's mass, grams. `None` for a size the outline refuses.
    pub fn mass_g(&self) -> Option<f64> {
        tent_mass_g(self.scale_mm, self.thickness_mm, self.density_g_per_cm3)
    }

    /// The detector's cost, euros, at the top of the sensor's range.
    ///
    /// The top and not the middle: a budget that is checked at the average is a
    /// budget that is exceeded half the time.
    pub fn cost_eur(&self) -> f64 {
        self.sensors as f64 * self.sensor_eur.1 + self.board_eur + self.block_eur
    }
}

/// One requirement a candidate breaks.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// The requirement's name, as the rules or the team state it.
    pub requirement: &'static str,
    /// The candidate's number.
    pub measured: f64,
    /// The limit it had to stay within.
    pub limit: f64,
    /// What the numbers mean, in a sentence.
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {:.2} against a limit of {:.2} ({})",
            self.requirement, self.measured, self.limit, self.detail
        )
    }
}

/// The mission's requirements, as a thing that can be asked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mission {
    /// The can's envelope.
    pub can: Can,
    /// The mass the whole can may have, grams.
    pub total_mass_g: (f64, f64),
    /// What the detector may spend, euros.
    pub detector_budget_eur: (f64, f64),
    /// What the shop can make.
    pub manufacturing: Manufacturing,
}

impl Mission {
    /// The mission as the documents state it.
    pub fn standard() -> Mission {
        Mission {
            can: Can::standard(),
            total_mass_g: TOTAL_MASS_G,
            detector_budget_eur: DETECTOR_BUDGET_EUR,
            manufacturing: Manufacturing::available(),
        }
    }

    /// Every requirement `candidate` breaks, in the order they are checked.
    ///
    /// An empty list is a pass. The function does not stop at the first failure:
    /// a candidate that is both too heavy and too expensive should be told both,
    /// because the two have different fixes.
    pub fn audit(&self, candidate: &Candidate) -> Vec<Violation> {
        let mut violations = Vec::new();

        // 1. Does it fit the can? The envelope already knows how to say no, and
        //    its refusal carries the dimension, so the audit reuses it rather
        //    than re-deriving the geometry.
        if let Err(error) = self.can.admits(candidate.scale_mm, candidate.thickness_mm) {
            let (measured, limit) = match error {
                CanError::TooTall { height_mm, limit_mm } => (height_mm, limit_mm),
                CanError::TooWide { half_diagonal_mm, limit_mm } => (half_diagonal_mm, limit_mm),
                CanError::BadSize => (f64::NAN, f64::NAN),
            };
            violations.push(Violation {
                requirement: "the block fits inside the can",
                measured,
                limit,
                detail: error.to_string(),
            });
        }

        // 2. Does the block leave room for the rest of the can? The mass budget
        //    is the whole can's, and the detector is one line of it.
        if let Some(mass) = candidate.mass_g() {
            let limit = self.total_mass_g.1 * 0.15;
            if mass > limit {
                violations.push(Violation {
                    requirement: "the block is at most 15 % of the can's mass",
                    measured: mass,
                    limit,
                    detail: format!(
                        "{mass:.1} g of block in a {:.0}-{:.0} g can leaves too little for the battery and the structure",
                        self.total_mass_g.0, self.total_mass_g.1
                    ),
                });
            }
        }

        // 3. Does it fit the detector's share of the budget?
        let cost = candidate.cost_eur();
        if cost > self.detector_budget_eur.1 {
            violations.push(Violation {
                requirement: "the detector costs at most its budget",
                measured: cost,
                limit: self.detector_budget_eur.1,
                detail: format!(
                    "{:.0} sensors at {:.0} EUR each plus {:.0} EUR of board and {:.0} EUR of block",
                    candidate.sensors as f64,
                    candidate.sensor_eur.1,
                    candidate.board_eur,
                    candidate.block_eur
                ),
            });
        }

        // 4. Can the shop make it? A machined inside corner is never sharper than
        //    the cutter, and the tent's apex is the sharpest corner it has.
        if self.manufacturing.cnc {
            let reach = self.manufacturing.max_depth_mm;
            if candidate.thickness_mm > reach {
                violations.push(Violation {
                    requirement: "the block is within the machine's depth",
                    measured: candidate.thickness_mm,
                    limit: reach,
                    detail: "a deeper cut needs a second setup, which re-datums the block".to_string(),
                });
            }
        }

        // 5. Is the readout's timing worth having? The budget is the components'
        //    quadrature, and the signal is the geometry's own spread. A detector
        //    whose jitter dwarfs its signal cannot measure a direction, and that
        //    is a *requirement* rather than a remark: the mission is a direction
        //    measurement.
        let jitter = components::total_jitter_ps();
        // The *most favourable* heading, not an arbitrary one: a failure stated
        // at the best case is a failure that no other heading can rescue. Taking
        // one heading would make the verdict depend on which one was picked.
        let positions = self.can_sensor_positions(candidate);
        let speed = crate::timing::speed_mm_per_ps(candidate.index).unwrap_or(f64::NAN);
        let mut signal = f64::NAN;
        for heading in [
            canlab_optics::Vec3::new(1.0, 0.0, 0.0),
            canlab_optics::Vec3::new(0.0, 1.0, 0.0),
            canlab_optics::Vec3::new(0.0, 0.0, 1.0),
            canlab_optics::Vec3::new(1.0, 1.0, 1.0),
        ] {
            let Some(heading) = heading.normalized() else { continue };
            if let Some(this) = crate::timing::timing_signal_ps(&positions, heading, speed) {
                signal = if signal.is_finite() { signal.max(this) } else { this };
            }
        }
        if signal.is_finite() && jitter / signal > 10.0 {
            violations.push(Violation {
                requirement: "the timing jitter is within 10x of the timing signal",
                measured: jitter / signal,
                limit: 10.0,
                detail: format!(
                    "the whole detector's timing signal is {signal:.2} ps and the readout's jitter is {jitter:.1} ps, so a timestamp carries no direction"
                ),
            });
        }

        violations
    }

    /// The four sensor centres a candidate's block would have.
    ///
    /// Built from the same outline the transport uses, so the audit's geometry is
    /// the simulation's geometry.
    fn can_sensor_positions(&self, candidate: &Candidate) -> [canlab_optics::Vec3; 4] {
        match canlab_optics::Tropical::fits_in(candidate.scale_mm, candidate.thickness_mm) {
            Ok(outline) => canlab_optics::Block::new(outline).sensor_positions(),
            Err(_) => [canlab_optics::Vec3::ZERO; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_design_point_passes_the_missions_own_audit() {
        // The design block fits, is light enough, is affordable, and its timing
        // signal is 224 ps against 47.4 ps of jitter — a ratio of 0.21, well
        // inside the 10x the requirement allows.
        //
        // This test used to assert that the design point failed the timing
        // requirement, and it did: the study's speed of light was a thousand
        // times too large, so the signal was 0.22 ps and the ratio was 212. The
        // assertion was true and the number behind it was wrong, which is why
        // the audit cannot be the only check — `tests/units.rs` states the
        // physical facts independently and is what caught it.
        let mission = Mission::standard();
        let violations = mission.audit(&Candidate::design());
        assert!(violations.is_empty(), "{violations:#?}");
        // The direction requirement is the designs module's, and the Cherenkov
        // design fails it at 81 degrees. That failure is a placement problem.
        let signal = crate::timing::timing_signal_ps(
            &mission.can_sensor_positions(&Candidate::design()),
            canlab_optics::Vec3::new(0.0, 0.0, 1.0).normalized().expect("a direction"),
            crate::timing::speed_mm_per_ps(1.49).expect("a speed"),
        )
        .expect("a signal");
        assert!(signal > 47.4, "the signal is {signal} ps against 47.4 ps of jitter");
    }

    #[test]
    fn a_block_that_does_not_fit_is_told_which_dimension() {
        // The sweep's best geometry: a 30 mm tent is 135 mm tall.
        let mission = Mission::standard();
        let mut candidate = Candidate::design();
        candidate.scale_mm = 30.0;
        let violations = mission.audit(&candidate);
        let fit = violations
            .iter()
            .find(|v| v.requirement.contains("fits inside the can"))
            .expect("a fit violation");
        assert!((fit.measured - 135.0).abs() < 1e-9, "{fit}");
        assert!((fit.limit - 114.0).abs() < 1e-9, "{fit}");
    }

    #[test]
    fn every_broken_requirement_is_reported_and_not_just_the_first() {
        // A candidate that is too big, too heavy and too expensive must be told
        // all three: the fixes are different and a caller that stopped at the
        // first would send the team to the wrong one.
        let mission = Mission::standard();
        let candidate = Candidate {
            scale_mm: 30.0,
            thickness_mm: 10.0,
            index: 1.84,
            density_g_per_cm3: 1.30,
            sensors: 8,
            sensor_eur: (10.0, 40.0),
            board_eur: 120.0,
            block_eur: 20.0,
        };
        let violations = mission.audit(&candidate);
        let names: Vec<&str> = violations.iter().map(|v| v.requirement).collect();
        assert!(names.iter().any(|n| n.contains("fits inside")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("mass")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("budget")), "{names:?}");
        // 8 sensors at 40 EUR plus 140 EUR of board and block is 460 EUR.
        assert!((candidate.cost_eur() - 460.0).abs() < 1e-9);
    }

    #[test]
    fn the_mass_share_is_the_whole_cans_budget_and_not_the_detectors() {
        // 15 % of 350 g is 52.5 g. A 25 mm tent at 10 mm deep is 46.9 cm³ of
        // PMMA, which is 55.8 g: over the share, and the audit says so.
        let mission = Mission::standard();
        let mut candidate = Candidate::design();
        candidate.scale_mm = 25.0;
        let mass = candidate.mass_g().expect("a mass");
        assert!((mass - 55.8).abs() < 0.1, "{mass}");
        let violations = mission.audit(&candidate);
        let heavy = violations
            .iter()
            .find(|v| v.requirement.contains("mass"))
            .expect("a mass violation");
        assert!((heavy.limit - 52.5).abs() < 1e-9, "{heavy}");
    }

    #[test]
    fn the_requirements_are_the_documents_numbers() {
        // The constants are the file's contract with `CANSAT-GRUND.md` and
        // `KOSTNADER.md`. A test that pins them is what stops a later edit from
        // quietly moving the can.
        assert_eq!(CAN_HEIGHT_MM, 115.0);
        assert_eq!(CAN_DIAMETER_MM, 66.0);
        assert_eq!(TOTAL_MASS_G, (300.0, 350.0));
        assert_eq!(TOTAL_BUDGET_EUR, 500.0);
        assert_eq!(G_REQUIRED, 20.0);
        assert_eq!(G_DESIGN, 30.0);
        assert_eq!(PARACHUTE_MOUNT_N, 50.0);
        assert_eq!(POWER_HOURS, 2.0);
        assert_eq!(DESCENT_MPS, (5.0, 12.0));
        // The measurement time is the flight profile's own arithmetic, not a
        // separate figure: 750 m at 11 m/s is 68 s and 1100 m at 8 m/s is 137 s.
        assert!((APOGEE_M.0 / 11.0 - MEASUREMENT_S.0).abs() < 1.0, "{}", APOGEE_M.0 / 11.0);
        assert!((APOGEE_M.1 / 8.0 - MEASUREMENT_S.1).abs() < 1.0, "{}", APOGEE_M.1 / 8.0);
        // The flux integrates to the quoted figure over the sky.
        const _: () = assert!(MUON_FLUX_INTEGRATED_M2_S > MUON_FLUX_VERTICAL_M2_S_SR);
    }

    #[test]
    fn the_cnc_is_available_and_its_limits_are_stated() {
        // The shop's envelope is a parameter, and the one that changes the
        // geometry space: with a CNC the outline is not a tent by necessity.
        let shop = Manufacturing::available();
        assert!(shop.cnc);
        assert!(shop.min_tool_radius_mm > 0.0);
        assert!(shop.max_depth_mm >= 10.0, "the design block is 10 mm deep");
        // Roughness is in micrometres and is not zero: a machined face scatters,
        // and the transport's Fresnel reflection assumes it does not.
        assert!(shop.roughness_um > 0.0);
        assert!(shop.polish_available);
        // A block deeper than the machine can cut is refused.
        let mission = Mission::standard();
        let mut deep = Candidate::design();
        deep.thickness_mm = 70.0;
        let violations = mission.audit(&deep);
        assert!(
            violations.iter().any(|v| v.requirement.contains("machine's depth")),
            "{violations:#?}"
        );
    }
}
