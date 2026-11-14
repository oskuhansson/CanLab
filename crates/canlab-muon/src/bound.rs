//! The best a direction can be measured: the Cramér–Rao bound, and the
//! alternative observable that is not hopeless.
//!
//! A bound is the right thing to compute before training a network, because a
//! network cannot beat it: if the bound says the direction is undetermined, the
//! network's loss will be flat and no architecture will help. The README in
//! `can-sat/sim/muon/` says exactly this — `direction_resolution_deg` is "not the
//! network's result, it is the information-theoretic limit" — and this module is
//! that limit for two observables.
//!
//! # The timing bound
//!
//! With `t_i = t₀ + (r_i · d)/v` and independent Gaussian jitter `σ` on each
//! timestamp, the Fisher information for `(t₀, d)` is
//!
//! ```text
//!   J = (1/σ²) · AᵀA,     A_i = [ 1, (u_i · e₁)/v, (u_i · e₂)/v ]
//! ```
//!
//! where `u_i = r_i − r̄` (centring is a reparametrisation of `t₀` and leaves the
//! direction's block unchanged) and `e₁, e₂` are an orthonormal basis of the
//! plane perpendicular to the true heading — the two free angles of a unit
//! vector. The bound on the *total* angular variance is the sum of the two
//! tangent variances, `J⁻¹₁₁ + J⁻¹₂₂`, because a small rotation by `(a, b)` moves
//! the heading by `√(a² + b²)`.
//!
//! # Why the answer is not a resolution
//!
//! `σ_angle ≈ σ · v / L` for a lever arm `L`. Light in a plastic moves 0.2 mm per
//! picosecond, so `σ·v` is the distance light covers during one jitter: 100 ps is
//! **20 metres**. A block 54 mm across has a lever arm 370 times shorter than the
//! distance the jitter blurs, and the bound comes out at tens of thousands of
//! degrees. A number larger than the sphere is a refusal, and it is the honest
//! answer to "can four SiPMs time a muon".
//!
//! # The observable that works
//!
//! Timing is not the only thing a sensor can report. A *position-sensitive*
//! sensor — a pixel, a scintillating fibre, a strip — reports **where** the
//! photon crossed its face, and two crossings on two faces define a line. The
//! angular uncertainty of that line is `√2·σ_h/L` for two independent hits with
//! perpendicular uncertainty `σ_h` separated by `L`: a millimetre of pixel over
//! 54 mm is 1.5°, which is a resolution. [`position_bound`] is that formula, and
//! it is a *formula* rather than a Fisher bound — it assumes two hits, one per
//! face, with the separation along the line joining them. It is the right
//! scaling and the right order of magnitude, and it is labelled as such.

use canlab_optics::Vec3;

use crate::solve;
use crate::timing;

/// An angular uncertainty, in both units the study needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bound {
    /// The standard deviation of the total angular error, radians.
    pub sigma_rad: f64,
}

impl Bound {
    /// The same bound in degrees.
    pub fn sigma_deg(&self) -> f64 {
        self.sigma_rad.to_degrees()
    }

    /// Whether the bound is larger than the sphere itself, in which case the
    /// observable carries no direction information worth the name.
    ///
    /// The threshold is the standard deviation of a uniformly random direction
    /// on a sphere, `√2` radians: a bound at or above that is indistinguishable
    /// from "no information", and a number below it is a resolution.
    pub fn is_hopeless(&self) -> bool {
        self.sigma_rad >= std::f64::consts::SQRT_2
    }
}

/// The Cramér–Rao bound on the heading from four (or more) timestamps.
///
/// `positions` are the sensor centres, `heading` the true direction, `jitter_ps`
/// the per-timestamp standard deviation and `speed_mm_per_ps` the light's speed
/// in the block. `None` when the sensors are coplanar (the bound is infinite and
/// an infinity is not a number this type can carry), or when a parameter is not
/// usable.
pub fn timing_bound(
    positions: &[Vec3],
    heading: Vec3,
    jitter_ps: f64,
    speed_mm_per_ps: f64,
) -> Option<Bound> {
    if positions.len() < 4 || jitter_ps <= 0.0 || !jitter_ps.is_finite() || speed_mm_per_ps <= 0.0
        || !speed_mm_per_ps.is_finite()
    {
        return None;
    }
    let heading = heading.normalized()?;
    let (first, second) = timing::tangent_basis(heading)?;

    // Centre the positions: a shift of the origin is absorbed by `t₀` and does
    // not change the direction's block of the information matrix.
    let count = positions.len() as f64;
    let mut mean = Vec3::ZERO;
    for position in positions {
        mean = mean.add(*position);
    }
    mean = mean.scale(1.0 / count);

    let mut information = [[0.0f64; 3]; 3];
    for position in positions {
        let u = position.sub(mean);
        let row = [1.0, u.dot(first) / speed_mm_per_ps, u.dot(second) / speed_mm_per_ps];
        for i in 0..3 {
            for j in 0..3 {
                information[i][j] += row[i] * row[j];
            }
        }
    }
    // The jitter enters as a common factor: `J = AᵀA/σ²`, so the inverse is
    // `σ²·(AᵀA)⁻¹` and the variance is `σ²` times the geometric part.
    let geometric = solve::inverse(information)?;
    let variance = jitter_ps * jitter_ps * (geometric[1][1] + geometric[2][2]);
    if variance < 0.0 || !variance.is_finite() {
        return None;
    }
    Some(Bound { sigma_rad: variance.sqrt() })
}

/// The angular uncertainty of a line through two hit points.
///
/// `hit_sigma_mm` is the perpendicular uncertainty of one hit and `separation_mm`
/// the distance between the two hits. Two independent hits give `√2·σ_h/L`; this
/// is the small-angle form, so it is a bound for `σ_h ≪ L` and an
/// order-of-magnitude answer beyond that. `None` for a separation of zero (two
/// hits at one point define no line) or a non-positive sigma.
pub fn position_bound(hit_sigma_mm: f64, separation_mm: f64) -> Option<Bound> {
    if hit_sigma_mm <= 0.0 || !hit_sigma_mm.is_finite() || separation_mm <= 0.0
        || !separation_mm.is_finite()
    {
        return None;
    }
    Some(Bound { sigma_rad: std::f64::consts::SQRT_2 * hit_sigma_mm / separation_mm })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic uniform generator, so every test is reproducible.
    struct Uniform(u64);

    impl Uniform {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / (1u64 << 53) as f64
        }

        /// A standard normal, by Box–Muller.
        fn normal(&mut self) -> f64 {
            let first = self.next().max(1e-300);
            let second = self.next();
            (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
        }
    }

    /// A tetrahedron with a 4 m edge, tested with a 1 ps jitter.
    ///
    /// The lever arm has to be this long for a *resolved* case: `σ_angle ≈ σ·v/L`
    /// and `σ·v` is the distance light covers during one jitter, so a 100 ps
    /// jitter blurs 20 m and a 200 mm tetrahedron is still hopeless. A test of
    /// the bound's arithmetic needs a case where the bound is small, and the
    /// CanSat's own lever arm is tested separately, by asserting the answer is
    /// hopeless.
    fn long_tetrahedron() -> [Vec3; 4] {
        [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4000.0, 0.0, 0.0),
            Vec3::new(0.0, 4000.0, 0.0),
            Vec3::new(0.0, 0.0, 4000.0),
        ]
    }

    #[test]
    fn the_bound_is_attained_by_the_least_squares_fit() {
        // The bound is a lower bound on any estimator's variance; the fit is an
        // estimator; so the fit's Monte Carlo error must sit at or just above it.
        // A bound that came out *below* the fit's error would be wrong, and one
        // far above it would be a bound nobody could use.
        let positions = long_tetrahedron();
        let heading = Vec3::new(1.0, 2.0, 3.0).normalized().expect("a direction");
        let speed = 201.2;
        let jitter = 1.0;
        let bound = timing_bound(&positions, heading, jitter, speed).expect("a bound");
        assert!(!bound.is_hopeless(), "the test needs a resolved case: {}", bound.sigma_deg());
        assert!(bound.sigma_rad < 0.2, "the linearisation needs a small angle: {}", bound.sigma_deg());

        let mut uniform = Uniform(2026);
        let trials = 4000;
        let mut squared = 0.0;
        for _ in 0..trials {
            let observations: Vec<timing::Observation> = positions
                .iter()
                .map(|position| timing::Observation {
                    position_mm: *position,
                    time_ps: position.dot(heading) / speed + uniform.normal() * jitter,
                })
                .collect();
            let fit = timing::reconstruct(&observations).expect("a fit");
            let cosine = fit.heading.cos_with(heading).clamp(-1.0, 1.0);
            let angle = cosine.acos();
            squared += angle * angle;
        }
        let measured = (squared / trials as f64).sqrt();
        // Within 15 %: the residual is the normalisation's bias, which is second
        // order in the angle and therefore small here.
        let ratio = measured / bound.sigma_rad;
        assert!((0.85..1.15).contains(&ratio), "fit {measured:.4} vs bound {:.4} (ratio {ratio:.3})", bound.sigma_rad);
    }

    #[test]
    fn the_bound_scales_the_way_the_arithmetic_says() {
        // σ ∝ jitter, σ ∝ 1/lever arm, σ ∝ v. Three scalings, three assertions:
        // a bound that only happened to be right at one point would pass the
        // test above and fail this one.
        let positions = long_tetrahedron();
        let heading = Vec3::new(1.0, 2.0, 3.0).normalized().expect("a direction");
        let base = timing_bound(&positions, heading, 1.0, 201.2).expect("a bound").sigma_rad;
        let double_jitter = timing_bound(&positions, heading, 2.0, 201.2).expect("a bound").sigma_rad;
        assert!((double_jitter / base - 2.0).abs() < 1e-9, "{}", double_jitter / base);
        let double_speed = timing_bound(&positions, heading, 1.0, 402.4).expect("a bound").sigma_rad;
        assert!((double_speed / base - 2.0).abs() < 1e-9, "{}", double_speed / base);
        // Doubling the lever arm halves the bound.
        let doubled: Vec<Vec3> = positions.iter().map(|p| p.scale(2.0)).collect();
        let longer = timing_bound(&doubled, heading, 1.0, 201.2).expect("a bound").sigma_rad;
        assert!((longer / base - 0.5).abs() < 1e-9, "{}", longer / base);
    }

    #[test]
    fn collinear_sensors_have_no_bound_because_they_have_no_information() {
        // Four sensors on one line: `r_i · d` depends only on the heading's
        // component along that line, so the two transverse components are absent
        // from every timestamp and the information matrix is singular. The bound
        // is infinite and `None` is the answer.
        let collinear = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
        ];
        assert!(timing_bound(&collinear, Vec3::new(0.0, 0.0, 1.0), 1.0, 201.2).is_none());
        // And the same four sensors in three dimensions do have one, so the
        // refusal is about the layout and not about the count.
        assert!(timing_bound(&long_tetrahedron(), Vec3::new(0.0, 0.0, 1.0), 1.0, 201.2).is_some());
    }

    #[test]
    fn coplanar_sensors_have_a_bound_and_a_mirror_ambiguity() {
        // Four coplanar sensors are *not* singular, and saying they are would be
        // the wrong lesson. The timestamps determine the heading's two in-plane
        // components, and the unit-length constraint then fixes the third up to
        // a sign: a muon going up through the plane and one going down give the
        // same four timestamps. The bound is therefore finite and the *ambiguity*
        // is what a fifth sensor off the plane would resolve.
        let coplanar = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(20.0, 20.0, 0.0),
        ];
        let bound = timing_bound(&coplanar, Vec3::new(0.0, 0.0, 1.0), 1.0, 201.2);
        assert!(bound.is_some(), "coplanar sensors carry information; they carry it twice");
        // The two headings that share a timestamp set: `+z` and `-z`.
        let up = timing_bound(&coplanar, Vec3::new(0.0, 0.0, 1.0), 1.0, 201.2).expect("a bound");
        let down = timing_bound(&coplanar, Vec3::new(0.0, 0.0, -1.0), 1.0, 201.2).expect("a bound");
        assert!((up.sigma_rad - down.sigma_rad).abs() < 1e-12, "the mirror is symmetric");
    }

    #[test]
    fn the_design_block_cannot_measure_a_direction_at_all() {
        // The number this crate exists to produce. The four sensor centres of a
        // 20 mm tent, 10 mm deep, are the ones `Block::sensor_positions` returns;
        // the jitter is the SiPM's; the speed is light in PMMA.
        let positions = [
            Vec3::new(0.0, 8.0, 5.0),
            Vec3::new(0.0, 8.0, -5.0),
            Vec3::new(-10.0, 45.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
        ];
        let speed = timing::speed_mm_per_ps(1.49).expect("a speed");
        let heading = Vec3::new(0.0, 0.0, 1.0);
        let bound = timing_bound(&positions, heading, 100.0, speed).expect("a bound");
        assert!(bound.is_hopeless(), "{:.0}° is not hopeless", bound.sigma_deg());
        // The signal the fit is trying to read, against the noise it reads it
        // with: a third of a picosecond against a hundred.
        let signal = timing::timing_signal_ps(&positions, heading, speed).expect("a signal");
        assert!(signal < 0.5, "the timing signal is {signal} ps");
        assert!(signal / 100.0 < 0.01, "the signal-to-jitter ratio is {}", signal / 100.0);
        // And the same geometry with a *perfect* clock would still be hopeless
        // at a picosecond, which is the point: the lever arm is the problem.
        let perfect = timing_bound(&positions, heading, 1.0, speed).expect("a bound");
        assert!(perfect.is_hopeless(), "{:.0}° at 1 ps", perfect.sigma_deg());
    }

    #[test]
    fn a_position_sensitive_face_is_a_resolution_where_timing_is_not() {
        // Two hits 54 mm apart with a millimetre of pixel: √2/54 = 1.5°.
        let bound = position_bound(1.0, 54.1).expect("a bound");
        assert!((bound.sigma_deg() - 1.496).abs() < 0.01, "{}", bound.sigma_deg());
        assert!(!bound.is_hopeless());
        // The scaling: a tenth of the pixel is a tenth of the angle, and ten
        // times the separation is a tenth of it.
        assert!((position_bound(0.1, 54.1).unwrap().sigma_rad / bound.sigma_rad - 0.1).abs() < 1e-12);
        assert!((position_bound(1.0, 541.0).unwrap().sigma_rad / bound.sigma_rad - 0.1).abs() < 1e-12);
        // Two hits at one point define no line.
        assert!(position_bound(1.0, 0.0).is_none());
        assert!(position_bound(0.0, 54.1).is_none());
        // And the formula is the small-angle limit: a Monte Carlo of two noisy
        // hits separated by 54 mm with 1 mm of noise must land on it.
        let mut uniform = Uniform(7);
        let mut squared = 0.0;
        let trials = 20_000;
        for _ in 0..trials {
            let first = uniform.normal() * 1.0;
            let second = uniform.normal() * 1.0;
            let angle = ((second - first) / 54.1).atan();
            squared += angle * angle;
        }
        let measured = (squared / trials as f64).sqrt();
        let ratio = measured / bound.sigma_rad;
        assert!((0.97..1.03).contains(&ratio), "measured {measured:.5} vs bound {:.5}", bound.sigma_rad);
    }
}
