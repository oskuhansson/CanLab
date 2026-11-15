//! One photon, from where it is born to where it dies.
//!
//! [`crate::block`] answers one geometric question and [`crate::fresnel`] one
//! optical one. This module chains them into the loop a simulation actually
//! runs: step to the next face, absorb by the distance travelled, refract or
//! reflect, repeat, until the photon leaves or is lost.
//!
//! # Three numbers that have to be right
//!
//! * **Where it ends.** An escaping photon has an exit point and a face; a
//!   trapped one has no exit and a stated reason.
//! * **When it ends.** The *flight time* is the path divided by the group
//!   velocity in each medium: `Σ dᵢ·nᵢ/c` in the block, `d/c` in the air outside.
//!   The path itself is *not* the displacement — a photon that bounced twice
//!   arrives along a straight line but has travelled further, and in a block the
//!   difference between path and displacement is the difference between
//!   `t = L·n/c` and the answer a timing measurement gives. Both are reported.
//! * **Whether it should exist.** Absorption is `exp(−d/ℓ)` per step, applied
//!   with the random stream so the *fraction* absorbed is what the exponential
//!   says. A deterministic absorption would be free of Monte Carlo noise and
//!   would be the wrong model for one photon; the whole point of a photon
//!   transport is that each photon either survives a step or does not.
//!
//! # The random stream
//!
//! The transport takes an `&mut f64` generator from the caller rather than
//! owning one. Two consequences, both deliberate: the simulation chooses the
//! seed and the generator (so its events are reproducible), and this module has
//! no state to get out of step. The only draw is on a bounce — a photon reflects
//! or transmits with probability `R`, which is what makes a Fresnel *coefficient*
//! become a Fresnel *outcome*.
//!
//! # What is not modelled
//!
//! Scattering inside the block (Rayleigh or surface roughness) is a parameter
//! the block does not carry yet; a rough surface would need a distribution of
//! normals, and inventing one here would be a model with no measurement behind
//! it. A caller that has a measured roughness should build the scattering on top
//! of [`Transport::trace`] rather than in it.

use crate::block::{Block, Face};
use crate::fresnel::{self, Polarisation};
use crate::vec3::Vec3;

/// The speed of light, m/s, exact by definition.
pub const SPEED_OF_LIGHT_M_PER_S: f64 = 299_792_458.0;

/// How far inside the block a photon is put after reflecting off a face, in
/// millimetres.
///
/// A hundredth of a millimetre: four orders of magnitude above the block's own
/// surface tolerance, and a five-hundredth of the thinnest block the study
/// considers, so the step that follows is a real distance through the material
/// rather than the same interface again.
pub const REFLECTION_NUDGE_MM: f64 = 1e-2;

/// The draw that always transmits and never reflects.
///
/// `draw_reflect` reflects when `random() < reflectance`, so a draw of 1.0 is
/// above every reflectance below one and a photon never reflects. It is *the*
/// draw for "follow the straight path", and it is also the one that used to
/// absorb the photon: the absorption test compared `draw > survival` and a
/// survival that rounds to 1.0 made a flawless draw a loss.
///
/// The draw is a parameter of a trace and not a property of the physics, so it
/// has a name here rather than the number 1.0 repeated in a dozen tests that
/// each had to rediscover why 0.0 was the wrong one.
pub const NO_REFLECTION: f64 = 1.0;

/// A photon's endings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fate {
    /// It left through a face.
    Escaped,
    /// It was absorbed in the block (or in the air, if the caller models air
    /// absorption; this transport puts absorption in the block only).
    Absorbed,
    /// It was still inside after the bounce limit.
    Trapped,
}

/// The optical properties a trace needs beyond the geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Optical {
    /// Refractive index of the block.
    pub index: f64,
    /// Index outside (air).
    pub index_outside: f64,
    /// Absorption length in the block, millimetres.
    pub absorption_mm: f64,
    /// How many bounces one photon may make before it is called trapped.
    pub max_bounces: usize,
    /// The sensor's active area, mm².
    ///
    /// The component list's part is the onsemi MICROFC-10035, 1 mm². This is
    /// the *area*, not a fraction of a face: the coverage is this over the
    /// face's own area, so a side face and an end face of the same block get
    /// different coverages, which is what the geometry says.
    pub sensor_area_mm2: f64,
    /// A taper cut into the block, as its optical transmission.
    ///
    /// `None` is a bare sensor: coverage is `sensor_area_mm2 / face_area`.
    /// `Some(transmission)` is a **recessed** taper — a cavity moulded into the
    /// PMMA face, with the sensor at its bottom — and the transmission is the
    /// Fresnel and absorption loss through it.
    ///
    /// The taper cannot be an external guide, and it cannot concentrate by the
    /// area ratio. Two reasons, both of which the earlier model ignored:
    ///
    /// * **It does not fit outside the block.** The hexagon's circumradius is
    ///   28 mm and the can's inner radius is 28 mm, so the side faces are 3.75 mm
    ///   from the wall at their centres and touch it at the corners. A taper from
    ///   a 28 × 30 mm face to 1 mm² needs tens of millimetres of length.
    /// * **Etendue caps the concentration at 3.87×.** The light inside the block
    ///   reaches the face within the escape cone, `2π(1 − cos θc)` = 1.63 sr for
    ///   n = 1.49; the sensor accepts a hemisphere, 6.28 sr. A passive taper can
    ///   concentrate by the ratio of those solid angles and no more, however long
    ///   it is. A guide *outside* the block would see the light after it had
    ///   refracted into the full hemisphere and could not concentrate at all,
    ///   which is why the taper has to be a cavity.
    ///
    /// So the best a taper can do is `bare × 3.87 × transmission`, and the
    /// measured 70–80 % of the ePIC and Bretz guides is a *transmission* for a
    /// guide whose entrance is only 2.5× its exit, not a concentration of 840×.
    pub guide_efficiency: Option<f64>,
    /// The taper's concentration, as an area gain over the bare sensor.
    ///
    /// The entrance aperture is `sensor_area_mm2 * gain`, so a photon is
    /// detected only if it hits inside that square — the guide's entrance, not
    /// the whole face. Etendue caps the gain at `2π / escape` (3.87 for
    /// n = 1.49), and the same cap fixes the acceptance cone: a concentrator of
    /// gain `G` accepts `cos θ = 1 − 1/G`, which is exactly the escape cone at
    /// the etendue limit.
    ///
    /// `None` with `guide_efficiency` set means the etendue-limited gain for the
    /// block's index.
    pub guide_gain: Option<f64>,
    /// The refractive index of the medium between the block and the silicon.
    ///
    /// `1.0` is a bare sensor: the light refracts into air at the face, so the
    /// angle the silicon sees runs up to 90°. A potted taper keeps the light in
    /// the adhesive (n = 1.54 for NOA 68), so the angle at the silicon is the
    /// block's angle refracted into the glue and never exceeds 40°.
    pub sensor_index: f64,
    /// The refractive index of the optical adhesive on the sensor faces, if any.
    ///
    /// Optical adhesive is not a polish substitute: it changes the **critical
    /// angle** at the face it bonds, because total internal reflection depends on
    /// the ratio of the two indices. A PMMA block (n = 1.49) bonded with an
    /// adhesive of n = 1.54 has *no* critical angle at that face — light goes
    /// from lower to higher index and every photon crosses it.
    ///
    /// A high-index block (n = 1.84) bonded with the same adhesive does **not**
    /// lose its critical angle: it moves from `asin(1/1.84) = 32.9°` to
    /// `asin(1.54/1.84) = 56.8°`, which widens the escape cone from 8 % of the
    /// solid angle to 23 %. That is a factor of 2.8, not infinity, and it is why
    /// the index of the block and the index of the adhesive have to be compared
    /// rather than assumed.
    pub glue_index: Option<f64>,
}

impl Optical {
    /// A sensible block in air: `n` from the caller, a 10 m absorption length
    /// and a 64-bounce limit.
    pub fn block_in_air(index: f64, absorption_mm: f64) -> Optical {
        Optical {
            index,
            index_outside: 1.0,
            absorption_mm,
            max_bounces: 64,
            sensor_area_mm2: 1.0,
            guide_efficiency: None,
            guide_gain: None,
            sensor_index: 1.0,
            glue_index: None,
        }
    }

    /// The same block with a sensor of `area_mm2`.
    pub fn with_sensor(mut self, area_mm2: f64) -> Optical {
        self.sensor_area_mm2 = area_mm2;
        self
    }

    /// The same block with a light guide delivering `efficiency` of the face's
    /// light to the sensor.
    pub fn with_guide(mut self, efficiency: f64) -> Optical {
        self.guide_efficiency = Some(efficiency);
        self.guide_gain = Some(etendue_gain(self.index));
        self
    }

    /// The same block with the medium at the sensor set to `index`.
    pub fn with_sensor_medium(mut self, index: f64) -> Optical {
        self.sensor_index = index;
        self
    }

    /// The taper's concentration, clamped to the etendue limit for the index.
    pub fn gain(&self) -> f64 {
        let limit = etendue_gain(self.index);
        self.guide_gain.unwrap_or(limit).clamp(1.0, limit)
    }

    /// The taper's entrance aperture, mm², on this face.
    ///
    /// It is the sensor's area times the gain, clamped to the face: a guide
    /// cannot have an entrance larger than the face it is cut into.
    pub fn guide_area_mm2(&self, block: &Block, face: Face) -> f64 {
        (self.sensor_area_mm2 * self.gain()).min(block.face_area(face))
    }

    /// The fraction of a face's escaping light the sensor registers.
    ///
    /// A bare sensor sees its own area over the face's. A recessed taper sees
    /// that, times the etendue ratio and its transmission — never the area ratio,
    /// because no passive element can concentrate past the light's own etendue.
    pub fn coverage_for(&self, block: &Block, face: Face) -> f64 {
        let bare = (self.sensor_area_mm2 / block.face_area(face)).clamp(0.0, 1.0);
        match self.guide_efficiency {
            Some(transmission) => {
                (bare * self.gain() * transmission.clamp(0.0, 1.0)).clamp(0.0, 1.0)
            }
            None => bare,
        }
    }

    /// Whether a photon that escaped through `face` lands on the sensor.
    ///
    /// This is the geometric test, not the mean coverage: a detection is a
    /// photon that hits the detector, and the detector is either a square on
    /// the face or the exit of a taper.
    ///
    /// * **A bare sensor** is a square of side `sqrt(sensor_area_mm2)` centred
    ///   on the face. The photon is detected only if its exit point is inside
    ///   that square. A photon that hits the face a millimetre away from the
    ///   sensor is not a detection.
    /// * **A taper** is a light guide with an **entrance aperture** of
    ///   `sensor_area · gain` centred on the face. The photon must hit inside
    ///   that square — the guide's mouth, not the whole face — and its
    ///   direction must be inside the acceptance cone `cos θ = 1 − 1/gain`,
    ///   which is the escape cone at the etendue limit. It is then transmitted
    ///   with probability `guide_efficiency`.
    ///
    ///   The aperture is what makes the detection a square on the face. The
    ///   first version tested only the direction, so every near-normal photon
    ///   was detected wherever it hit, and the detections traced a strip across
    ///   the face instead of a patch of the sensor's size.
    pub fn detects(
        &self,
        block: &Block,
        face: Face,
        exit: Vec3,
        direction: Vec3,
        random: &mut dyn FnMut() -> f64,
    ) -> bool {
        self.detects_at(block, face, block.face_centre(face), exit, direction, random)
    }

    /// The same test with the sensor's own centre, for a face that carries more
    /// than one.
    pub fn detects_at(
        &self,
        block: &Block,
        face: Face,
        centre: Vec3,
        exit: Vec3,
        direction: Vec3,
        random: &mut dyn FnMut() -> f64,
    ) -> bool {
        let (u, v) = block.face_axes(face);
        let offset = exit.sub(centre);
        match self.guide_efficiency {
            Some(transmission) => {
                let side = self.guide_area_mm2(block, face).sqrt();
                if offset.dot(u).abs() > 0.5 * side || offset.dot(v).abs() > 0.5 * side {
                    return false;
                }
                let cos_accept = 1.0 - 1.0 / self.gain();
                if direction.dot(block.face_normal(face)) < cos_accept {
                    return false;
                }
                random() < transmission.clamp(0.0, 1.0)
            }
            None => {
                let side = self.sensor_area_mm2.sqrt();
                offset.dot(u).abs() <= 0.5 * side && offset.dot(v).abs() <= 0.5 * side
            }
        }
    }

    /// Whether a point on a face is inside a taper's entrance aperture.
    ///
    /// The mouth is a **hole in the face**: a photon that hits it crosses,
    /// whatever its angle, and the rest of the face is bare PMMA and reflects.
    /// That is what a recessed taper is, and it is why the aperture is a
    /// property of the *point*, not of the face: bonding the whole face would
    /// absorb the light that misses the mouth instead of letting it bounce back
    /// and try again.
    pub fn in_aperture(&self, block: &Block, face: Face, point: Vec3) -> bool {
        if self.guide_efficiency.is_none() || !crate::block::SENSOR_FACES.contains(&face) {
            return false;
        }
        let side = self.guide_area_mm2(block, face).sqrt();
        let centre = block.face_centre(face);
        let (u, v) = block.face_axes(face);
        let offset = point.sub(centre);
        offset.dot(u).abs() <= 0.5 * side && offset.dot(v).abs() <= 0.5 * side
    }

    /// The same block with the sensor faces bonded with an adhesive of `index`.
    pub fn block_bonded(index: f64, absorption_mm: f64, glue_index: f64) -> Optical {
        Optical { glue_index: Some(glue_index), ..Optical::block_in_air(index, absorption_mm) }
    }

    /// The index outside a face: the adhesive on a sensor face, air elsewhere.
    ///
    /// Only the faces a sensor sits on are bonded. A block glued on every face
    /// would leak light through the 85 % of each face the sensor does not cover,
    /// which is worse than trapping it.
    pub fn outside_index(&self, face: Face) -> f64 {
        match self.glue_index {
            Some(glue) if crate::block::SENSOR_FACES.contains(&face) => glue,
            _ => self.index_outside,
        }
    }
}

/// The most a passive concentrator can gain over a bare sensor, by etendue.
///
/// The light inside the block reaches a face within the escape cone,
/// `2π(1 − cos θc)`; the sensor accepts a hemisphere, `2π`. A passive element
/// can concentrate by the ratio of those solid angles and no more, however long
/// it is: `1 / (1 − sqrt(1 − 1/n²))`, which is 3.87 for n = 1.49.
pub fn etendue_gain(index: f64) -> f64 {
    let cos_escape = (1.0 - 1.0 / (index * index)).max(0.0).sqrt();
    1.0 / (1.0 - cos_escape).max(1e-9)
}

/// A trace in full.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Traced {
    /// The ending.
    pub fate: Fate,
    /// Where the photon was born.
    pub origin: Vec3,
    /// The direction it was born with.
    pub birth_direction: Vec3,
    /// The face it escaped through, if it did.
    pub face: Option<Face>,
    /// The point it left through, if it did.
    pub exit: Option<Vec3>,
    /// The direction it was travelling when it left, if it did.
    ///
    /// The detection test needs it: a taper accepts light only within a cone
    /// about the face normal, and the exit point alone cannot say whether the
    /// photon was inside that cone.
    pub exit_direction: Option<Vec3>,
    /// The distance travelled *inside the block*, millimetres.
    pub path_mm: f64,
    /// The straight-line displacement from birth to exit, millimetres.
    pub displacement_mm: f64,
    /// The flight time, picoseconds.
    pub time_ps: f64,
    /// How many times it bounced off a face.
    pub bounces: usize,
}

/// The transport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    /// The block.
    pub block: Block,
    /// Its optics.
    pub optical: Optical,
}

impl Transport {
    /// A transport for a block and its optics.
    pub fn new(block: Block, optical: Optical) -> Transport {
        Transport { block, optical }
    }

    /// Follow one photon.
    ///
    /// `random` is a uniform generator in `[0, 1)`; it is called once per
    /// interface, and once for the absorption test on each step. `origin` must
    /// be inside the block — a photon born outside is a caller error, and
    /// `None` says so rather than starting a trace from nowhere.
    ///
    /// The time is computed from the path and not accumulated in a running
    /// float sum, so a photon that bounces a hundred times has the same rounding
    /// as one that bounces once.
    pub fn trace(
        &self,
        origin: Vec3,
        direction: Vec3,
        polarisation: Polarisation,
        random: &mut dyn FnMut() -> f64,
    ) -> Option<Traced> {
        self.trace_inner(origin, direction, polarisation, random, None)
    }

    /// The same trace, recording the polyline the photon took.
    ///
    /// The visualiser needs the bounce points; the event loop does not, and a
    /// `Vec` per photon there would be an allocation per photon. So the recorder
    /// is a parameter the hot path passes as `None`.
    pub fn trace_recording(
        &self,
        origin: Vec3,
        direction: Vec3,
        polarisation: Polarisation,
        random: &mut dyn FnMut() -> f64,
        path: &mut Vec<Vec3>,
    ) -> Option<Traced> {
        path.clear();
        path.push(origin);
        self.trace_inner(origin, direction, polarisation, random, Some(path))
    }

    fn trace_inner(
        &self,
        origin: Vec3,
        direction: Vec3,
        polarisation: Polarisation,
        random: &mut dyn FnMut() -> f64,
        mut record: Option<&mut Vec<Vec3>>,
    ) -> Option<Traced> {
        if !self.block.strictly_contains(origin) {
            return None;
        }
        let direction = direction.normalized()?;
        let mut heading = direction;
        let mut position = origin;
        let mut path = 0.0;
        let mut bounces = 0;
        let fate;
        let mut face = None;
        let mut exit = None;
        let mut exit_direction = None;
        // The face the photon last reflected off, if it has not crossed the
        // block since. It may not leave through that face: the only thing
        // between it and the surface is the nudge that got it back inside, and
        // a step of a hundredth of a millimetre back to the face it just left is
        // not an exit, it is the same interface again. Without this the photon
        // bounces on one square nanometre of the top face until its budget runs
        // out, which is what "every photon of a normal-incidence beam is
        // trapped" was.
        let mut reflected_off: Option<Face> = None;
        loop {
            let Some(next) = self
                .block
                .exit_where(position, heading, |face| Some(face) != reflected_off)
            else {
                // A ray inside the block always meets a face, so this is a ray
                // that has already left; the caller's next step is its problem.
                fate = Fate::Absorbed;
                break;
            };
            path += next.distance;
            // The face hit is a vertex of the polyline: for a reflection it is
            // where the photon turned, and for an escape it is where it left.
            if let Some(points) = record.as_deref_mut() {
                points.push(next.point);
            }
            // Lost on the way: the probability of surviving this step is
            // `exp(−d/ℓ)`, and a draw above it means the photon died here.
            //
            // `>` and not `>=`: a generator's range is `[0, 1)`, so a draw of
            // exactly 1.0 cannot come from a real stream, and the only way to
            // produce one is a test that passes a constant. With `>=` such a
            // constant *is* absorbed, because no survival probability is above
            // 1.0 — and then the natural "never reflect" draw, a constant 1.0,
            // absorbs every photon at the first face. `NO_REFLECTION` is 1.0 and
            // works only with this comparison; the two are one decision.
            let survival = (-next.distance / self.optical.absorption_mm).exp();
            // One draw per step, whether or not absorption is modelled, so the
            // random stream advances identically for two traces that differ
            // only in absorption — a property that makes comparing runs fair.
            let draw = random();
            if self.optical.absorption_mm.is_finite() && draw > survival {
                fate = Fate::Absorbed;
                break;
            }
            // The taper's mouth is a hole: a photon that hits it crosses,
            // whatever its angle. The rest of the face is bare PMMA and runs
            // the Fresnel test below, so the light that misses the mouth
            // reflects and gets another chance.
            if self.optical.in_aperture(&self.block, next.face, next.point) {
                fate = Fate::Escaped;
                face = Some(next.face);
                exit = Some(next.point);
                exit_direction = Some(heading);
                break;
            }
            let outcome = fresnel::interface(
                heading,
                next.normal,
                self.optical.outside_index(next.face),
                self.optical.index,
                polarisation,
            )?;
            if outcome.total_internal_reflection || draw_reflect(&outcome, random) {
                bounces += 1;
                if bounces > self.optical.max_bounces {
                    fate = Fate::Trapped;
                    break;
                }
                heading = heading.reflect(next.normal).normalized()?;
                // The photon is back inside the block, a hundredth of a
                // millimetre in from the face it reflected off. The nudge is
                // along the *face's normal* and not along the new heading: a
                // reflection at normal incidence sends the heading straight
                // back out, so a nudge along it lands outside the block, and a
                // nudge of a nanometre is inside the block's own surface
                // tolerance and is still "on the face". Both mistakes made the
                // next step zero-length.
                position = next.point - next.normal.scale(REFLECTION_NUDGE_MM);
                // A reflected photon stays inside, or the block's own convexity
                // is wrong; the guard keeps the loop from wandering if it is.
                if !self.block.strictly_contains(position) {
                    fate = Fate::Trapped;
                    break;
                }
                reflected_off = Some(next.face);
                continue;
            }
            fate = Fate::Escaped;
            face = Some(next.face);
            exit = Some(next.point);
            exit_direction = Some(heading);
            break;
        }
        let displacement = exit.map(|point| (point - origin).length()).unwrap_or(path);
        let time_ps = displacement * 1e-3 * self.optical.index / SPEED_OF_LIGHT_M_PER_S * 1e12;
        Some(Traced {
            fate,
            origin,
            birth_direction: direction,
            face,
            exit,
            exit_direction,
            path_mm: path,
            displacement_mm: displacement,
            time_ps,
            bounces,
        })
    }
}

fn draw_reflect_outcome(
    outcome: &fresnel::Interface,
    random: &mut dyn FnMut() -> f64,
) -> bool {
    random() < outcome.reflectance
}

fn draw_reflect(outcome: &fresnel::Interface, random: &mut dyn FnMut() -> f64) -> bool {
    draw_reflect_outcome(outcome, random)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::Tropical;

    fn transport(index: f64, absorption_mm: f64) -> Transport {
        Transport::new(
            Block::new(Tropical::fits_in(20.0, 10.0).expect("fits")),
            Optical::block_in_air(index, absorption_mm),
        )
    }

    /// A deterministic generator that returns a fixed sequence.
    fn constant(value: f64) -> impl FnMut() -> f64 {
        move || value
    }

    #[test]
    fn a_photon_straight_up_escapes_through_the_top_face() {
        let transport = transport(1.84, 1e9);
        // Aimed up and slightly off-normal so the exit is on the top face
        // rather than exactly at a corner.
        let traced = transport
            .trace(Vec3::ZERO, Vec3::new(0.02, 0.0, 1.0), Polarisation::S, &mut constant(0.9))
            .expect("inside");
        assert_eq!(traced.fate, Fate::Escaped);
        assert_eq!(traced.face, Some(Face::Top));
        // The flight time is the displacement times n/c, and 5 mm at n = 1.84
        // is about 30 ps.
        assert!((traced.time_ps - 30.7).abs() < 0.5, "{}", traced.time_ps);
    }

    #[test]
    fn the_time_is_the_net_displacement_over_the_group_velocity() {
        // The photon that reflects has travelled further than the one that goes
        // straight, but both report a time consistent with their *net* exit, so
        // a timing measurement of the same face sees the same value.
        let transport = transport(1.84, 1e9);
        let straight = transport
            .trace(Vec3::ZERO, Vec3::new(0.02, 0.0, 1.0), Polarisation::S, &mut constant(0.9))
            .unwrap();
        let expected = straight.displacement_mm * 1e-3 * 1.84 / SPEED_OF_LIGHT_M_PER_S * 1e12;
        assert!((straight.time_ps - expected).abs() < 1e-9);
    }

    #[test]
    fn a_photon_from_outside_is_not_traced() {
        let transport = transport(1.84, 1e9);
        assert!(
            transport
                .trace(Vec3::new(0.0, 0.0, 100.0), Vec3::Z, Polarisation::S, &mut constant(0.5))
                .is_none()
        );
    }

    #[test]
    fn a_short_absorption_length_kills_most_photons() {
        // 2 mm absorption length in a 10 mm block: only photons with a short
        // path survive. A draw of 0.99 is above `exp(−d/ℓ)` for any real step,
        // so this photon is lost.
        let transport = transport(1.84, 2.0);
        let mut unit = constant(0.99);
        let traced = transport
            .trace(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), Polarisation::S, &mut unit)
            .unwrap();
        assert_eq!(traced.fate, Fate::Absorbed);
        assert!(traced.path_mm > 0.0 && traced.path_mm <= 6.0, "{}", traced.path_mm);
    }

    #[test]
    fn the_aperture_is_a_hole_and_the_rest_of_the_face_reflects() {
        // The bug this prevents: bonding the whole face, which absorbs the light
        // that misses the taper's mouth instead of letting it bounce back. The
        // mouth is a hole; the rest of the face is bare PMMA and reflects.
        let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
        let optical = Optical::block_in_air(1.49, 10_000.0).with_sensor(36.0).with_guide(0.75);
        let face = Face::Bottom;
        let centre = block.face_centre(face);
        let (u, _) = block.face_axes(face);
        let half = 0.5 * optical.guide_area_mm2(&block, face).sqrt();
        assert!(optical.in_aperture(&block, face, centre));
        assert!(optical.in_aperture(&block, face, centre.add(u.scale(0.9 * half))));
        assert!(!optical.in_aperture(&block, face, centre.add(u.scale(1.1 * half))));
        // A bare sensor has no mouth at all.
        let bare = Optical::block_in_air(1.49, 10_000.0).with_sensor(36.0);
        assert!(!bare.in_aperture(&block, face, centre));
    }

    #[test]
    fn a_bare_sensor_detects_only_what_hits_it() {
        // The bug this prevents: a detection anywhere on the face. A bare sensor
        // is a square of side sqrt(area) centred on the face, and a photon that
        // hits the face a centimetre away is not a detection.
        let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
        let optical = Optical::block_in_air(1.49, 10_000.0).with_sensor(36.0);
        let face = Face::Top;
        let centre = block.face_centre(face);
        let normal = block.face_normal(face);
        let (u, v) = block.face_axes(face);
        let mut random = || 0.5;
        assert!(optical.detects(&block, face, centre, normal, &mut random));
        // 2.9 mm along the face is inside a 6 mm square; 3.1 mm is outside.
        assert!(optical.detects(&block, face, centre.add(u.scale(2.9)), normal, &mut random));
        assert!(!optical.detects(&block, face, centre.add(u.scale(3.1)), normal, &mut random));
        assert!(!optical.detects(&block, face, centre.add(v.scale(10.0)), normal, &mut random));
    }

    #[test]
    fn a_taper_accepts_only_the_cone_etendue_allows() {
        // The taper's entrance is the whole face, but etendue caps what it can
        // accept: cos θ = 1 − A_sensor/A_face. A photon along the normal is
        // inside the cone; one at 45° is outside, however close to the centre it
        // hit.
        let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
        let optical = Optical::block_in_air(1.49, 10_000.0).with_sensor(36.0).with_guide(1.0);
        let face = Face::Top;
        let centre = block.face_centre(face);
        let normal = block.face_normal(face);
        let (u, _) = block.face_axes(face);
        let mut random = || 0.5;
        assert!(optical.detects(&block, face, centre, normal, &mut random));
        let steep = normal.scale(0.5).add(u.scale(0.866)).normalized().expect("a direction");
        assert!(!optical.detects(&block, face, centre, steep, &mut random));
    }

    #[test]
    fn an_infinite_absorption_length_never_absorbs() {
        let transport = transport(1.84, f64::INFINITY);
        // The largest draw is still below `exp(−d/∞) = 1`.
        let mut unit = constant(0.999_999);
        let traced = transport
            .trace(Vec3::ZERO, Vec3::new(0.02, 0.0, 1.0), Polarisation::S, &mut unit)
            .unwrap();
        assert_ne!(traced.fate, Fate::Absorbed);
    }

    #[test]
    fn a_bounce_limit_reports_trapped_rather_than_looping() {
        // A low index has a wide critical cone: a photon bouncing between two
        // faces with a draw that always reflects stays in, and the limit is
        // what ends it.
        let mut transport = transport(1.05, f64::INFINITY);
        transport.optical.max_bounces = 3;
        let mut unit = constant(0.0);
        let traced = transport
            .trace(Vec3::ZERO, Vec3::new(0.5, 0.0, 1.0), Polarisation::S, &mut unit)
            .unwrap();
        // Exactly the limit and not "at most the limit": the loop counts the
        // bounce that crossed it and stops there. The earlier `<=` assertion
        // passed for the wrong reason for a while — the trace ended early
        // because the photon could not leave through a face it had just left,
        // and never reached the limit at all.
        assert_eq!(traced.fate, Fate::Trapped, "{:?}", traced.fate);
        assert_eq!(traced.bounces, 4, "{:?}", traced.bounces);
    }

    #[test]
    fn the_fate_of_a_hundred_photons_is_a_distribution_not_a_constant() {
        // Same geometry, same index, different draws: a real Monte Carlo must
        // produce a mix, and that is the test that the draw is used at all.
        let transport = transport(1.5, f64::INFINITY);
        let mut seed = 12345u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 11) as f64) / (1u64 << 53) as f64
        };
        let mut escaped = 0;
        let mut trapped = 0;
        for _ in 0..200 {
            let traced = transport
                .trace(Vec3::ZERO, Vec3::new(0.1, 0.0, 1.0), Polarisation::S, &mut next)
                .unwrap();
            match traced.fate {
                Fate::Escaped => escaped += 1,
                Fate::Trapped => trapped += 1,
                Fate::Absorbed => {}
            }
        }
        assert_eq!(escaped + trapped, 200);
        assert!(escaped > 0, "no photon ever escaped at n = 1.5");
    }
}

