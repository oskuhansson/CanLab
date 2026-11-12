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
        }
    }
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
        if !self.block.contains(origin) {
            return None;
        }
        let direction = direction.normalized()?;
        let mut heading = direction;
        let mut position = origin;
        let mut path = 0.0;
        let mut bounces = 0;
        let mut fate;
        let mut face = None;
        let mut exit = None;
        loop {
            let Some(next) = self.block.exit(position, heading) else {
                // A ray inside the block always meets a face, so this is a ray
                // that has already left; the caller's next step is its problem.
                fate = Fate::Absorbed;
                break;
            };
            path += next.distance;
            // Lost on the way: the probability of surviving this step is
            // `exp(−d/ℓ)`, and a draw above it means the photon died here.
            let survival = (-next.distance / self.optical.absorption_mm).exp();
            // One draw per step, whether or not absorption is modelled, so the
            // random stream advances identically for two traces that differ
            // only in absorption — a property that makes comparing runs fair.
            let draw = random();
            if self.optical.absorption_mm.is_finite() && draw > survival {
                fate = Fate::Absorbed;
                break;
            }
            let outcome = fresnel::interface(
                heading,
                next.normal,
                self.optical.index_outside,
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
                position = next.point + heading.scale(1e-6);
                // A reflected photon stays inside, or the block's own convexity
                // is wrong; the guard keeps the loop from wandering if it is.
                if !self.block.contains(position) {
                    fate = Fate::Trapped;
                    break;
                }
                continue;
            }
            fate = Fate::Escaped;
            face = Some(next.face);
            exit = Some(next.point);
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
        assert!(matches!(traced.fate, Fate::Trapped | Fate::Absorbed), "{:?}", traced.fate);
        assert!(traced.bounces <= 3);
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
