//! What four timestamps can determine about a muon's direction.
//!
//! The detector's premise is that a photon's arrival time at sensor `i` is
//!
//! ```text
//!   t_i = t₀ + (r_i · d) / v
//! ```
//!
//! with `r_i` the sensor's position, `d` the muon's heading and `v` the light's
//! speed in the block. Three unknowns (`t₀` and two angles of `d`) against four
//! equations, so the direction is over-determined and a least-squares fit
//! returns it. That is the arithmetic the design was built on, and it is correct.
//!
//! # The number that decides the mission
//!
//! The fit is not the question. The question is the *scale*: how much does
//! `(r_i · d)/v` actually change when the heading rotates? Light travels
//! **0.3 mm per picosecond** in vacuum and 0.2 mm/ps in a plastic, so a sensor
//! 54 mm from another sees a heading change of one radian as an arrival-time
//! change of `54/201 = 0.27 ps`. A SiPM's single-photon time resolution is
//! **100 ps**. The signal is 370 times smaller than the noise, and the direction
//! is not merely poorly measured — it is not measured.
//!
//! [`Bound`] is that statement as a number, and [`Bound::sigma_deg`] for the
//! design block is of order ten thousand degrees. An angle whose uncertainty is
//! larger than the sphere is a refusal, not a resolution.
//!
//! # What is and is not modelled
//!
//! The model is linear in `r_i · d`, which is exact for a plane wave and the
//! leading term for a Cherenkov cone. It is *not* the full likelihood of the
//! transport: a photon is emitted along the track and arrives by a path that
//! depends on where on the track it was born, so the true mean arrival time is
//! not exactly `t₀ + (r_i·d)/v`. The full-likelihood bound would need the
//! transport's own derivative, and it is the next step; the linear bound here is
//! the one the design was argued from, so it is the one that has to be answered
//! first. If the linear bound is hopeless by three orders of magnitude, the
//! correction cannot rescue it — the correction is a fraction of the mean, and
//! the mean's own lever arm is what is too small.

use canlab_optics::Vec3;

use crate::solve;

/// The speed of light in vacuum, mm/ps.
pub const C_MM_PER_PS: f64 = 299.792_458;

/// The speed of light in a block of index `n`, mm/ps.
pub fn speed_mm_per_ps(index: f64) -> Option<f64> {
    (index >= 1.0 && index.is_finite()).then(|| C_MM_PER_PS / index)
}

/// One sensor's reading: where it is, and when it fired.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    /// The sensor's centre, millimetres, in the block's frame.
    pub position_mm: Vec3,
    /// The arrival time, picoseconds.
    pub time_ps: f64,
}

/// A direction recovered from timestamps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reconstruction {
    /// The muon's heading: a unit vector along its travel.
    pub heading: Vec3,
    /// The light's speed the fit implies, mm/ps. Free, because the direction is
    /// `s/|s|` and the speed is `1/|s|` — the fit recovers both, and the speed
    /// is a check on the block's index rather than an input.
    pub speed_mm_per_ps: f64,
    /// The time at the origin of the block's frame, picoseconds.
    pub t0_ps: f64,
    /// The root-mean-square of the fit's residuals, picoseconds.
    pub residual_ps: f64,
}

/// Why a fit produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitError {
    /// Fewer than four observations: three unknowns and a scale need four.
    TooFew {
        /// How many were given.
        have: usize,
        /// How many are needed.
        need: usize,
    },
    /// The sensor positions span fewer than three dimensions, so the free-`s`
    /// solve has no unique answer. Four coplanar sensors are the case that
    /// matters, and it is a property of the *layout*, not of the data.
    ///
    /// Coplanar is not the same as uninformative: the four timestamps do fix the
    /// heading's two in-plane components, and the unit-length constraint then
    /// fixes the third up to a **sign** — a muon going up through the plane and
    /// one going down give the same timestamps. An estimator that returned one
    /// of those two without saying so would be wrong half the time, so this one
    /// refuses and names the reason. A fifth sensor off the plane removes the
    /// ambiguity; this estimator does not pretend to.
    Degenerate,
    /// A time or a position that is not finite.
    NotFinite,
}

impl std::fmt::Display for FitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FitError::TooFew { have, need } => {
                write!(formatter, "{have} timestamps cannot determine a direction; {need} are needed")
            }
            FitError::Degenerate => write!(
                formatter,
                "the sensors are coplanar, so the heading is determined only up to a mirror reflection through their plane"
            ),
            FitError::NotFinite => write!(formatter, "a timestamp or a position is not finite"),
        }
    }
}

impl std::error::Error for FitError {}

/// Recover the heading from timestamps, by least squares.
///
/// The model is `t_i = t₀ + r_i·s` with `s = d/v`, which is *linear* in `s`; the
/// unit-length constraint on `d` is applied afterwards, by normalising `s`. That
/// is the standard two-step estimator and it is what the bound in [`crate::bound`]
/// is the bound for.
///
/// Centring the positions and the times removes `t₀` from the system before it
/// is solved, which is the difference between a 4×4 solve and a 3×3 one and also
/// the difference between a well-conditioned matrix and one whose first row is
/// all ones next to entries of tens.
pub fn reconstruct(observations: &[Observation]) -> Result<Reconstruction, FitError> {
    if observations.len() < 4 {
        return Err(FitError::TooFew { have: observations.len(), need: 4 });
    }
    for observation in observations {
        if !observation.time_ps.is_finite() || !observation.position_mm.is_finite() {
            return Err(FitError::NotFinite);
        }
    }
    let count = observations.len() as f64;
    let mut mean_position = Vec3::ZERO;
    let mut mean_time = 0.0;
    for observation in observations {
        mean_position = mean_position.add(observation.position_mm);
        mean_time += observation.time_ps;
    }
    mean_position = mean_position.scale(1.0 / count);
    mean_time /= count;

    // `M = Σ u uᵀ`, `b = Σ u (t − t̄)` with `u = r − r̄`. Then `M s = b`.
    let mut m = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for observation in observations {
        let u = observation.position_mm.sub(mean_position);
        let ua = [u.x, u.y, u.z];
        let dt = observation.time_ps - mean_time;
        for row in 0..3 {
            for column in 0..3 {
                m[row][column] += ua[row] * ua[column];
            }
            b[row] += ua[row] * dt;
        }
    }
    let s = solve::solve(m, b).ok_or(FitError::Degenerate)?;
    let magnitude = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
    if magnitude <= 0.0 || !magnitude.is_finite() {
        // Every timestamp equal: the muon crossed in no time, or the times carry
        // no information at all. Either way there is no heading in them.
        return Err(FitError::Degenerate);
    }
    let heading = Vec3::new(s[0] / magnitude, s[1] / magnitude, s[2] / magnitude);
    let speed = 1.0 / magnitude;
    let t0 = mean_time - mean_position.dot(Vec3::new(s[0], s[1], s[2]));

    let mut squared = 0.0;
    for observation in observations {
        let predicted = t0 + observation.position_mm.dot(Vec3::new(s[0], s[1], s[2]));
        squared += (observation.time_ps - predicted) * (observation.time_ps - predicted);
    }
    Ok(Reconstruction {
        heading,
        speed_mm_per_ps: speed,
        t0_ps: t0,
        residual_ps: (squared / count).sqrt(),
    })
}

/// The spread of `(r_i · d)/v` across the sensors: the largest arrival-time
/// difference the geometry can produce, picoseconds.
///
/// This is the signal the fit is trying to read, and it is the number that has to
/// be compared with the jitter. For the design block it is a third of a
/// picosecond.
pub fn timing_signal_ps(positions: &[Vec3], heading: Vec3, speed_mm_per_ps: f64) -> Option<f64> {
    if speed_mm_per_ps <= 0.0 || !speed_mm_per_ps.is_finite() || positions.len() < 2 {
        return None;
    }
    let heading = heading.normalized()?;
    let projections: Vec<f64> = positions
        .iter()
        .map(|position| position.dot(heading) / speed_mm_per_ps)
        .collect();
    let low = projections.iter().copied().fold(f64::INFINITY, f64::min);
    let high = projections.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some(high - low)
}

/// A unit basis of the plane perpendicular to `heading`.
///
/// The reference axis is chosen away from the heading so the cross product is
/// well conditioned: a heading within 25° of `z` uses `x`, everything else uses
/// `z`. The returned pair is orthonormal and both are perpendicular to `heading`.
pub fn tangent_basis(heading: Vec3) -> Option<(Vec3, Vec3)> {
    let heading = heading.normalized()?;
    let reference = if heading.z.abs() < 0.9 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let first = heading.cross(reference).normalized()?;
    let second = heading.cross(first).normalized()?;
    Some((first, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tetrahedron of sensors, so the four span three dimensions.
    pub(crate) fn tetrahedron() -> [Vec3; 4] {
        [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(0.0, 0.0, 20.0),
        ]
    }

    fn observations(positions: &[Vec3], heading: Vec3, speed: f64, t0: f64) -> Vec<Observation> {
        positions
            .iter()
            .map(|position| Observation {
                position_mm: *position,
                time_ps: t0 + position.dot(heading) / speed,
            })
            .collect()
    }

    #[test]
    fn exact_timestamps_give_the_heading_back() {
        let positions = tetrahedron();
        let heading = Vec3::new(1.0, 2.0, 3.0).normalized().expect("a direction");
        let speed = 201.2;
        let fit = reconstruct(&observations(&positions, heading, speed, 17.0)).expect("a fit");
        assert!(fit.heading.cos_with(heading) > 1.0 - 1e-12, "{:?}", fit.heading);
        assert!((fit.speed_mm_per_ps - speed).abs() < 1e-9, "{}", fit.speed_mm_per_ps);
        assert!((fit.t0_ps - 17.0).abs() < 1e-9, "{}", fit.t0_ps);
        assert!(fit.residual_ps < 1e-9, "{}", fit.residual_ps);
    }

    #[test]
    fn the_heading_does_not_depend_on_the_speed_it_was_made_with() {
        // The fit solves for `s = d/v` and normalises, so the speed is a
        // by-product: the same geometry at two speeds gives one heading and two
        // speeds. This is the property that makes the fit independent of the
        // block's index, and it is worth a test because an estimator that
        // assumed `v` would look identical on a well-posed input.
        let positions = tetrahedron();
        let heading = Vec3::new(-1.0, 0.5, 2.0).normalized().expect("a direction");
        let fast = reconstruct(&observations(&positions, heading, 201.2, 0.0)).expect("a fit");
        let slow = reconstruct(&observations(&positions, heading, 162.9, 0.0)).expect("a fit");
        assert!(fast.heading.cos_with(slow.heading) > 1.0 - 1e-12);
        assert!((fast.speed_mm_per_ps - 201.2).abs() < 1e-9);
        assert!((slow.speed_mm_per_ps - 162.9).abs() < 1e-9);
    }

    #[test]
    fn three_timestamps_are_refused_and_the_refusal_counts_them() {
        let positions = tetrahedron();
        let heading = Vec3::new(1.0, 0.0, 0.0);
        let mut few = observations(&positions, heading, 201.2, 0.0);
        few.pop();
        match reconstruct(&few) {
            Err(FitError::TooFew { have, need }) => {
                assert_eq!(have, 3);
                assert_eq!(need, 4);
            }
            other => panic!("three timestamps were not refused: {other:?}"),
        }
    }

    #[test]
    fn coplanar_sensors_are_refused_because_the_layout_is_what_is_wrong() {
        // Four sensors in the plane z = 0. The heading's z component cancels out
        // of every `r_i · d`, so no data can recover it — and the refusal says
        // so rather than returning a heading with a made-up z.
        let coplanar = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(20.0, 20.0, 0.0),
        ];
        let mut observations = observations(&coplanar, Vec3::new(1.0, 1.0, 1.0).normalized().unwrap(), 201.2, 0.0);
        // Perturb one time so the data is not exactly consistent: a singular
        // layout must be refused for its geometry, not for its arithmetic.
        observations[3].time_ps += 0.5;
        assert_eq!(reconstruct(&observations), Err(FitError::Degenerate));
    }

    #[test]
    fn non_finite_input_is_refused() {
        let positions = tetrahedron();
        let mut observations = observations(&positions, Vec3::new(1.0, 0.0, 0.0), 201.2, 0.0);
        observations[1].time_ps = f64::NAN;
        assert_eq!(reconstruct(&observations), Err(FitError::NotFinite));
    }

    #[test]
    fn the_timing_signal_is_the_light_crossing_the_sensors() {
        // Two sensors 54.1 mm apart along the heading: 54.1/201.2 = 0.269 ps.
        let positions = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(54.1, 0.0, 0.0)];
        let signal = timing_signal_ps(&positions, Vec3::new(1.0, 0.0, 0.0), 201.2).expect("a signal");
        assert!((signal - 54.1 / 201.2).abs() < 1e-9, "{signal}");
        // Across the heading the projection does not change, so there is no
        // signal at all: the sensors are side by side as far as the muon knows.
        let across = timing_signal_ps(&positions, Vec3::new(0.0, 1.0, 0.0), 201.2).expect("a signal");
        assert!(across.abs() < 1e-12, "{across}");
        assert!(timing_signal_ps(&positions, Vec3::new(1.0, 0.0, 0.0), 0.0).is_none());
        assert!(timing_signal_ps(&positions[..1], Vec3::new(1.0, 0.0, 0.0), 201.2).is_none());
    }

    #[test]
    fn the_tangent_basis_is_orthonormal_and_perpendicular_to_the_heading() {
        for heading in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(-0.3, 0.9, 0.05),
        ] {
            let heading = heading.normalized().expect("a direction");
            let (first, second) = tangent_basis(heading).expect("a basis");
            assert!(first.length() - 1.0 < 1e-12);
            assert!(second.length() - 1.0 < 1e-12);
            assert!(first.dot(heading).abs() < 1e-12);
            assert!(second.dot(heading).abs() < 1e-12);
            assert!(first.dot(second).abs() < 1e-12);
        }
    }
}
