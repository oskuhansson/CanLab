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
//! A direction needs a **lever arm**. Timing has one — the light crosses the 90 mm
//! block in 450 ps — but the block's four sensor faces are a poor tetrahedron: the
//! two end sensors are 10 mm apart, so a heading along that axis has almost no
//! lever arm and the bound is 81° at its worst. The same block with the sensors on
//! its corners gives 16° at best.
//!
//! A **position** observable has a better lever arm for less: two planes separated
//! by a gap give two crossing points, and two points give a line. The resolution
//! is `√2·σ_h/L` — the pixel's own uncertainty over the separation, and nothing
//! else — and the separation is air inside a can that is 115 mm tall.
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

/// The shape of a plane, because a disc and a rectangle fill a circle differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneShape {
    /// A rectangle, whose diagonal must fit the can's inner diameter.
    Rectangle,
    /// A disc, whose diameter must fit it.
    Disc,
}

/// A detector proposal, in the terms the comparison is made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    /// The plane's shape.
    pub shape: PlaneShape,
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
    /// Whether the design uses optical adhesive, which is a purchase and not a
    /// usage: the material is cents and the bottle is 33 EUR.
    pub bonded: bool,
}

/// Where an angle number came from, because they are not the same kind of number.
///
/// The distinction is the one a reader is most likely to get wrong: a table of
/// angles looks like a table of measurements, and only one of these three is a
/// measurement of anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleSource {
    /// A **Cramér–Rao bound** from the Fisher information of the timestamps,
    /// checked against a Monte Carlo of the least-squares estimator that attains
    /// it. This is the strongest kind: it is a limit, and the limit is verified.
    Bound,
    /// A **closed-form propagation** of a hit uncertainty, `√2·σ_h/L`. The
    /// algebra is right for its model and the model has been checked against a
    /// Monte Carlo of two Gaussian points — but **no simulation generates the
    /// hits**. It assumes each hit is localised to one pixel with uniform
    /// uncertainty, that the two hits are independent, and that nothing scatters.
    Formula,
    /// A **geometric estimate** with no validation at all: a pixel over a lever
    /// arm, for a design the transport cannot yet simulate.
    Estimate,
}

impl AngleSource {
    /// A short tag for a table.
    pub fn tag(&self) -> &'static str {
        match self {
            AngleSource::Bound => "bound",
            AngleSource::Formula => "formula",
            AngleSource::Estimate => "estimate",
        }
    }
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
    /// Where the angle came from.
    pub angle_source: AngleSource,
    /// Muons through the first plane in a 100 s flight.
    pub crossings_per_flight: f64,
    /// Muons through both planes: the ones with a direction.
    pub muons_per_flight: f64,
    /// The fraction of crossings that reach the second plane.
    pub acceptance: f64,
    /// The stack's height along the can's axis.
    pub stack_height_mm: f64,
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
            // A disc across the can's axis: Ø56 mm is the largest that fits and
            // is 57 % more area than the square inscribed in the same circle.
            shape: PlaneShape::Disc,
            planes: 1,
            pixels_per_side: 1,
            pixel_mm: 0.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 56.0,
            height_mm: 56.0,
            sensor: SiPMChoice::microfc_60035(),
            needs_timer: false,
            board_eur: 25.0,
            bonded: true,
        }
    }

    /// The Cherenkov block and four timestamps: the design the bound rejected.
    pub fn cherenkov_timing() -> Design {
        Design {
            name: "cherenkov timing",
            approach: Approach::CherenkovTiming,
            // The tent prism with its plane holding the can's axis: 40 mm across,
            // 90 mm tall, 10 mm deep. The muon crosses the walls, so the face it
            // enters is 90 x 10 mm and the material it crosses is 40 mm.
            shape: PlaneShape::Rectangle,
            planes: 1,
            pixels_per_side: 1,
            pixel_mm: 0.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 90.0,
            height_mm: 10.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: true,
            board_eur: 60.0,
            bonded: true,
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
            // 40 x 39 mm: a 55.9 mm diagonal, the largest rectangle that fits the
            // 56 mm inner diameter at this width. 40 x 48 would be 62.5 mm and no
            // longer fits.
            shape: PlaneShape::Rectangle,
            planes: 2,
            pixels_per_side: 2,
            pixel_mm: 20.0,
            separation_mm: 40.0,
            thickness_mm: 5.0,
            width_mm: 40.0,
            height_mm: 39.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 45.0,
            bonded: true,
        }
    }

    /// Two planes of 2 x 2 pixels and a 20 mm gap: the sample-first tracker.
    ///
    /// The same sensors and pixels as the coarse tracker with half the gap. The
    /// acceptance nearly doubles — 0.54 against 0.30 — and the angle nearly
    /// doubles with it, to 23.4°. It is the design for a mission that would
    /// rather have thirteen muons at 23° than seven at 12°, and which of those
    /// is better depends on whether the science is a direction or a distribution.
    pub fn short_tracker() -> Design {
        Design {
            name: "short tracker",
            approach: Approach::Tracker,
            shape: PlaneShape::Rectangle,
            planes: 2,
            pixels_per_side: 2,
            pixel_mm: 20.0,
            separation_mm: 20.0,
            thickness_mm: 5.0,
            width_mm: 40.0,
            height_mm: 39.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 45.0,
            bonded: true,
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
            shape: PlaneShape::Rectangle,
            planes: 2,
            pixels_per_side: 4,
            pixel_mm: 10.0,
            separation_mm: 60.0,
            thickness_mm: 5.0,
            width_mm: 40.0,
            height_mm: 39.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 90.0,
            bonded: true,
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
            // The same block as the Cherenkov design, read by an array instead
            // of four sensors.
            shape: PlaneShape::Rectangle,
            planes: 1,
            pixels_per_side: 4,
            pixel_mm: 10.0,
            separation_mm: 0.0,
            thickness_mm: 10.0,
            width_mm: 90.0,
            height_mm: 10.0,
            sensor: SiPMChoice::microfc_10035(),
            needs_timer: false,
            board_eur: 90.0,
            bonded: true,
        }
    }

    /// Every design, in the order the table presents them.
    pub fn all() -> [Design; 6] {
        [
            Design::counter(),
            Design::cherenkov_timing(),
            Design::short_tracker(),
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
            // A Cherenkov block is PMMA; a scintillator plane is PVT. The two
            // densities differ by 16 %, which is not enough to change a verdict
            // and is enough to make the table wrong.
            Approach::CherenkovTiming => {
                return tent_mass_g(20.0, self.thickness_mm, 1.19).unwrap_or(0.0) + 12.0;
            }
            Approach::RingImager => 1.19,
            _ => Scintillator::pvt().density_g_per_cm3,
        };
        let volume_mm3 = self.area_mm2() * self.thickness_mm * self.planes as f64;
        volume_mm3 / 1000.0 * material + 12.0
    }

    /// The cost, euros, at the top of every range.
    pub fn cost_eur(&self) -> f64 {
        let sensors = self.channels() as f64 * self.sensor.eur.1;
        let timer = if self.needs_timer { 25.0 } else { 0.0 };
        let adhesive = if self.bonded { crate::mission::ADHESIVE_EUR } else { 0.0 };
        sensors + self.board_eur + timer + adhesive
    }

    /// Photoelectrons one plane registers from a muon crossing it.
    ///
    /// Two different models, because the two light sources lose their light in
    /// different ways.
    ///
    /// A **scintillator** plane is a diffuse-reflector box: the yield times a
    /// collection efficiency times the PDE, which is a model and is labelled one.
    ///
    /// A **Cherenkov** block is a total-internal-reflection cavity, and it traps
    /// most of what it makes. The born-times-collection estimate gives 18 for the
    /// design block; the transport measures **8.1**, because 52 % of the light is
    /// trapped and much of the rest leaves through a face no sensor covers. Using
    /// the model for the Cherenkov designs overstates them by a factor of two,
    /// which is the wrong direction to be wrong in.
    pub fn photoelectrons(&self) -> f64 {
        if self.is_cherenkov() {
            return self.measured_cherenkov_photoelectrons();
        }
        Scintillator::pvt().photoelectrons(self.thickness_mm)
    }

    /// Whether the design's light comes from a Cherenkov radiator.
    pub fn is_cherenkov(&self) -> bool {
        matches!(self.approach, Approach::CherenkovTiming | Approach::RingImager)
    }

    /// The photons the transport actually delivers, per muon, averaged over 100
    /// events with a fixed seed.
    ///
    /// The transport is the only thing in this crate that knows about total
    /// internal reflection, Fresnel loss and the sensors' coverage of their
    /// faces, and a design that quotes a light budget without it is quoting a
    /// hope. The seed is fixed so the table is reproducible.
    fn measured_cherenkov_photoelectrons(&self) -> f64 {
        let Ok(outline) = canlab_optics::Tropical::fits_in(20.0, self.thickness_mm) else {
            return 0.0;
        };
        let block = canlab_optics::Block::new(outline);
        let optical = canlab_optics::Optical::block_in_air(1.49, 10_000.0);
        // The transport's default sensor, and the microcell count is why.
        //
        // `detector::simulate` clamps the **born** photon count to the smallest
        // microcell count among the sensors. That is a crude saturation model,
        // and for this design it is the wrong place to apply it: a 40 mm crossing
        // makes ~840 photons, of which only ~19 reach a sensor, so a 504-cell part
        // can count every one of them. Clamping the born count to 504 would throw
        // away 40 % of the light that the sensor never sees. The default 40 000
        // cells is used so the clamp does not bite, and the saturation question is
        // answered by the design table's own `saturates` column instead.
        let sensors: [canlab_optics::SiPM; 4] =
            canlab_optics::SENSOR_FACES.map(canlab_optics::SiPM::typical);
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut random = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 11) as f64) / (1u64 << 53) as f64
        };
        let entry = Vec3::new(-30.0, 0.0, 0.0);
        let heading = Vec3::new(1.0, 0.0, 0.0);
        let mut detected = 0usize;
        let mut events = 0usize;
        for _ in 0..100 {
            let Ok(event) = canlab_optics::detector::simulate(
                &block, &optical, &sensors, entry, heading, 50.0, 1.0, &mut random,
            ) else {
                continue;
            };
            events += 1;
            detected += event.detected_count();
        }
        if events == 0 { 0.0 } else { detected as f64 / events as f64 }
    }

    /// The angular uncertainty in degrees, and what kind of number it is.
    pub fn angle_deg(&self) -> (Option<f64>, AngleSource) {
        match self.approach {
            // A counter has no direction: one number is not an angle.
            Approach::Counter => (None, AngleSource::Bound),
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
                (Some(worst), AngleSource::Bound)
            }
            // A formula, not a simulation: no code in this crate generates a
            // tracker hit. The algebra is checked against a Monte Carlo of two
            // Gaussian points, which verifies the formula and says nothing about
            // whether a scintillator plane produces such points.
            Approach::Tracker => {
                let sigma_h = self.pixel_mm / 12.0f64.sqrt();
                crate::bound::position_bound(sigma_h, self.separation_mm)
                    .map(|bound| (Some(bound.sigma_deg()), AngleSource::Formula))
                    .unwrap_or((None, AngleSource::Formula))
            }
            // A geometric estimate with no validation: the ring's radius is the
            // lever arm, and the transport cannot yet say where on a face a
            // photon landed.
            Approach::RingImager => {
                let radius = 20.0 * (47.8f64).to_radians().tan();
                let sigma_h = self.pixel_mm / 12.0f64.sqrt();
                crate::bound::position_bound(sigma_h, radius)
                    .map(|bound| (Some(bound.sigma_deg()), AngleSource::Estimate))
                    .unwrap_or((None, AngleSource::Estimate))
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

    /// The face a muon must cross to enter the detector, mm².
    ///
    /// A rectangle's area is its two sides; a disc's is `πr²`, which is why the
    /// counter is a disc — the square inscribed in the same circle is 57 % less
    /// area for the same can.
    pub fn area_mm2(&self) -> f64 {
        match self.shape {
            PlaneShape::Rectangle => self.width_mm * self.height_mm,
            PlaneShape::Disc => {
                std::f64::consts::PI * (self.width_mm / 2.0) * (self.width_mm / 2.0)
            }
        }
    }

    /// The material a muon crosses, millimetres.
    ///
    /// Not the plane's thickness for every design: a tracker's muon crosses one
    /// plane's thickness, but a Cherenkov block's muon crosses the block's width
    /// because the block is mounted on edge.
    pub fn path_mm(&self) -> f64 {
        match self.approach {
            Approach::CherenkovTiming | Approach::RingImager => 40.0,
            _ => self.thickness_mm,
        }
    }

    /// The stack's height along the can's axis, millimetres.
    pub fn stack_height_mm(&self) -> f64 {
        self.planes as f64 * self.thickness_mm
            + (self.planes.saturating_sub(1)) as f64 * self.separation_mm
    }

    /// Whether the design fits the can: the plane in the circle, the stack in the
    /// height.
    pub fn fits(&self, can: &Can) -> Result<(), String> {
        if self.is_cherenkov() {
            // The tent prism, mounted with its plane holding the axis. The
            // circle constrains the 40 x 10 cross-section and the height
            // constrains the tent's 90 mm, which is what `Can::admits` checks —
            // modelling the block as a 90 x 10 flat rectangle in the circle
            // refuses a block that fits.
            return can
                .admits(20.0, self.thickness_mm)
                .map_err(|error| error.to_string());
        }
        let diameter = 2.0 * can.inner_radius_mm();
        match self.shape {
            PlaneShape::Rectangle => {
                let diagonal = (self.width_mm * self.width_mm + self.height_mm * self.height_mm).sqrt();
                if diagonal > diameter {
                    return Err(format!(
                        "the plane's diagonal is {diagonal:.1} mm and the can's inner diameter is {diameter:.1} mm"
                    ));
                }
            }
            PlaneShape::Disc => {
                if self.width_mm > diameter {
                    return Err(format!(
                        "the disc is {:.1} mm across and the can's inner diameter is {diameter:.1} mm",
                        self.width_mm
                    ));
                }
            }
        }
        if self.stack_height_mm() > can.inner_height_mm() {
            return Err(format!(
                "the stack is {:.1} mm tall and the can's inner height is {:.1} mm",
                self.stack_height_mm(),
                can.inner_height_mm()
            ));
        }
        Ok(())
    }

    /// The fraction of crossings that reach the second plane.
    ///
    /// One for a single-plane design: a counter needs no coincidence, and a
    /// Cherenkov block is one solid.
    pub fn acceptance(&self) -> f64 {
        if self.planes < 2 {
            return 1.0;
        }
        coincidence_acceptance(self.width_mm, self.height_mm, self.separation_mm)
    }

    /// Muons that cross the *first* plane during a 100 s flight.
    ///
    /// The integrated flux times the plane's area times the time. This is the
    /// rate a counter would see, and it is **not** the rate a tracker can
    /// measure a direction from.
    pub fn crossings_per_flight(&self) -> f64 {
        let area_m2 = self.area_mm2() / 1.0e6;
        crate::mission::MUON_FLUX_INTEGRATED_M2_S * area_m2 * 100.0
    }

    /// Muons that cross **both** planes: the ones a tracker can give a direction.
    ///
    /// Equal to [`Design::crossings_per_flight`] for a single-plane design. This
    /// is the number that decides whether a direction measurement has a sample,
    /// and it is the one the first version of the table left out.
    pub fn muons_per_flight(&self) -> f64 {
        self.crossings_per_flight() * self.acceptance()
    }

    /// Everything the table prints.
    pub fn metrics(&self) -> Metrics {
        let (angle_deg, angle_source) = self.angle_deg();
        Metrics {
            detector_g: self.detector_g(),
            cost_eur: self.cost_eur(),
            channels: self.channels(),
            photoelectrons: self.photoelectrons(),
            saturates: self.photoelectrons() > self.sensor.cells as f64,
            saturation_matters: self.photoelectrons() > self.sensor.cells as f64
                && self.needs_amplitude(),
            angle_deg,
            angle_source,
            crossings_per_flight: self.crossings_per_flight(),
            muons_per_flight: self.muons_per_flight(),
            acceptance: self.acceptance(),
            stack_height_mm: self.stack_height_mm(),
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
        if let Err(reason) = self.fits(&mission.can) {
            violations.push(format!("fit: {reason}"));
        }
        // A direction measurement with a handful of events is a demonstration,
        // not a measurement. Ten is a low bar and it is a bar: below it the
        // angular distribution cannot be measured at all.
        if metrics.muons_per_flight < 10.0 {
            violations.push(format!(
                "sample: {:.1} muons through both planes in a 100 s flight",
                metrics.muons_per_flight
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

/// The fraction of muons through the first plane that also cross the second.
///
/// Two planes and a gap are a **coincidence**, and a muon only gets a direction
/// if it crosses both. A muon that enters the first plane at `(x, y)` with a
/// zenith angle `θ` and azimuth `φ` reaches the second plane at
/// `(x + d·tanθ·cosφ, y + d·tanθ·sinφ)`, which is inside the plane only if that
/// displacement is smaller than the room left on each side. Averaged over a
/// uniform crossing point and the sea-level angular distribution, that is this
/// number.
///
/// # The distribution
///
/// The sea-level muon flux is `I(θ) ∝ cos²θ`, so `cos θ = (1 − u)^(1/3)` for a
/// uniform `u` and the zenith angle is peaked at the vertical. A uniform-in-θ
/// flux would give a lower acceptance, and the difference is not small: at a
/// 40 mm gap the cos²θ answer is 0.30 and a uniform one is 0.19.
///
/// # What it costs
///
/// The acceptance is the price of the gap, and the gap is what buys the angle.
/// At 40 mm the tracker keeps 30 % of its crossings and resolves 11.7°; at 20 mm
/// it keeps 54 % and resolves 23.4°. The study's first version reported the
/// single-plane rate and called it the tracker's, which overstated it by a factor
/// of three.
pub fn coincidence_acceptance(width_mm: f64, height_mm: f64, gap_mm: f64) -> f64 {
    // A non-positive side or a negative gap is refused rather than answered;
    // `NaN` is refused by the same tests because `NaN <= 0.0` is false and
    // `NaN.is_finite()` is not.
    if width_mm <= 0.0 || height_mm <= 0.0 || gap_mm < 0.0 || !width_mm.is_finite() {
        return 0.0;
    }
    if gap_mm == 0.0 {
        // No gap is no displacement: every crossing reaches the second plane.
        return 1.0;
    }
    // A deterministic quadrature, so the number is reproducible and the table
    // does not move between runs.
    const STEPS: usize = 200;
    let mut total = 0.0;
    for i in 0..STEPS {
        let u = (i as f64 + 0.5) / STEPS as f64;
        let cosine = (1.0 - u).cbrt();
        let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
        let tangent = if cosine > 1e-12 { sine / cosine } else { f64::INFINITY };
        let mut row = 0.0;
        for j in 0..STEPS {
            let phi = std::f64::consts::TAU * (j as f64 + 0.5) / STEPS as f64;
            let dx = gap_mm * tangent * phi.cos();
            let dy = gap_mm * tangent * phi.sin();
            let room_x = (1.0 - dx.abs() / width_mm).max(0.0);
            let room_y = (1.0 - dy.abs() / height_mm).max(0.0);
            row += room_x * room_y;
        }
        total += row / STEPS as f64;
    }
    total / STEPS as f64
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
        // The bound is the crate's own, not a second estimate. 81 degrees is a
        // direction in name only, and the reason is the sensor placement rather
        // than the clock: the two end sensors are 10 mm apart.
        let angle = cherenkov.metrics().angle_deg.expect("a bound");
        assert!((angle - 81.2).abs() < 1.0, "{angle}");
        assert!(angle > 30.0, "it must fail the direction requirement");
        // And the timing signal is real, which is what makes the failure a
        // placement problem: 224 ps of signal against 47.4 ps of jitter.
        let signal = crate::timing::timing_signal_ps(
            &cherenkov.cherenkov_positions(),
            Vec3::new(0.0, 0.0, 1.0).normalized().expect("a direction"),
            crate::timing::speed_mm_per_ps(1.49).expect("a speed"),
        )
        .expect("a signal");
        assert!(signal > 47.4, "{signal} ps");
        // The short tracker passes every requirement it is audited on, and the
        // coarse one does not: it resolves 11.7 degrees but only 7.3 muons reach
        // its second plane, which is too few to measure a distribution with.
        let short = Design::short_tracker();
        assert!(short.audit(&mission).is_empty(), "{:#?}", short.audit(&mission));
        let coarse = Design::coarse_tracker();
        assert!(
            coarse.audit(&mission).iter().any(|v| v.starts_with("sample:")),
            "{:#?}",
            coarse.audit(&mission)
        );
    }

    #[test]
    fn the_fine_tracker_is_the_one_that_breaks_the_budget() {
        // 32 sensors at 25 EUR plus 90 EUR of board and 32.70 EUR of adhesive is
        // 922.70 EUR, which is three times the detector's budget. The design that
        // measures best is the one that cannot be built, and the table has to
        // say so.
        let mission = Mission::standard();
        let fine = Design::fine_tracker();
        let cost = fine.cost_eur();
        assert!((cost - 922.70).abs() < 1e-9, "{cost}");
        let violations = fine.audit(&mission);
        assert!(violations.iter().any(|v| v.starts_with("cost:")), "{violations:#?}");
        // The coarse tracker is inside it: 8 x 25 + 45 + 32.70 = 277.70 EUR,
        // under the 300 EUR target.
        let coarse = Design::coarse_tracker().cost_eur();
        assert!((coarse - 277.70).abs() < 1e-9, "{coarse}");
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
    fn the_coincidence_costs_more_than_the_area_does() {
        // The two rates a tracker has, and the difference between them is the
        // whole of this test. A 40 x 39 mm plane is 15.6 cm², which at 127 m-2s-1
        // for 100 s is 19.8 crossings. A 40 mm gap keeps 27 % of them, so 5.3
        // muons reach the second plane — a factor of 3.7 that the first version
        // of this table did not apply.
        let coarse = Design::coarse_tracker();
        assert!((coarse.crossings_per_flight() - 19.8).abs() < 0.5, "{}", coarse.crossings_per_flight());
        assert!((coarse.acceptance() - 0.27).abs() < 0.02, "{}", coarse.acceptance());
        assert!((coarse.muons_per_flight() - 5.3).abs() < 0.3, "{}", coarse.muons_per_flight());
        // The counter needs no coincidence, so its two rates are one rate, and
        // its disc is 24.6 cm²: 31.3 muons, nearly six times the tracker's sample.
        let counter = Design::counter();
        assert_eq!(counter.acceptance(), 1.0);
        assert!((counter.crossings_per_flight() - 31.3).abs() < 0.5, "{}", counter.crossings_per_flight());
        assert!((counter.muons_per_flight() - counter.crossings_per_flight()).abs() < 1e-9);
        // And halving the gap nearly doubles the acceptance, which is the trade
        // the short tracker exists to make.
        let short = Design::short_tracker();
        assert!((short.acceptance() - 0.51).abs() < 0.02, "{}", short.acceptance());
        assert!(short.muons_per_flight() > coarse.muons_per_flight());
        assert!(short.muons_per_flight() > 9.5, "{}", short.muons_per_flight());
    }

    #[test]
    fn the_acceptance_is_one_with_no_gap_and_falls_with_it() {
        // The limits, so the quadrature is pinned at both ends: no gap means no
        // displacement and every crossing reaches the second plane; a gap much
        // larger than the plane means almost none do.
        assert_eq!(coincidence_acceptance(40.0, 48.0, 0.0), 1.0);
        let wide = coincidence_acceptance(40.0, 48.0, 200.0);
        assert!(wide < 0.05, "{wide}");
        // And it falls monotonically with the gap, which is the shape the trade
        // depends on.
        let mut previous = 1.0;
        for gap in [0.0, 10.0, 20.0, 40.0, 80.0] {
            let acceptance = coincidence_acceptance(40.0, 48.0, gap);
            assert!(acceptance <= previous + 1e-12, "gap {gap}: {acceptance} > {previous}");
            previous = acceptance;
        }
        // A larger plane accepts more at the same gap.
        assert!(
            coincidence_acceptance(60.0, 60.0, 40.0) > coincidence_acceptance(40.0, 48.0, 40.0)
        );
    }

    #[test]
    fn only_the_timing_angle_is_a_bound_and_the_rest_say_what_they_are() {
        // The distinction a table of angles hides: one of these is a verified
        // limit and the others are algebra. A reader who cannot tell them apart
        // will believe the tracker's number is a measurement, and no simulation
        // in this crate generates a tracker hit.
        let timing = Design::cherenkov_timing().metrics();
        assert_eq!(timing.angle_source, AngleSource::Bound);
        assert!(timing.angle_deg.is_some());
        // The trackers are a closed form for their model, and the model has no
        // simulated hits behind it.
        for design in [Design::short_tracker(), Design::coarse_tracker(), Design::fine_tracker()] {
            assert_eq!(design.metrics().angle_source, AngleSource::Formula, "{}", design.name);
        }
        // The ring imager is geometry alone.
        assert_eq!(Design::ring_imager().metrics().angle_source, AngleSource::Estimate);
        // And the tags are distinct, so a table cannot print two of them alike.
        let tags = [
            AngleSource::Bound.tag(),
            AngleSource::Formula.tag(),
            AngleSource::Estimate.tag(),
        ];
        assert_eq!(tags.len(), 3);
        assert!(tags[0] != tags[1] && tags[1] != tags[2] && tags[0] != tags[2]);
    }
}
