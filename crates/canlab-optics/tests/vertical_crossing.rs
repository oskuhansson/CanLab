//! Why a muon straight down the block's thin axis reaches no sensor, and what
//! that means for the design.
//!
//! The sweep in `muon_study` reported that a vertical muon gets *zero* detected
//! photons at every geometry. That is a strong claim, so it gets its own test
//! rather than living only in a table — and the first version of the test
//! failed, because the event was empty rather than quiet: the muon's path was
//! clipped to nothing and every photon was refused. The claim survived the
//! repair, but for the reason below rather than the one first written down.
//!
//! A muon going up the can's `z` axis leaves its Cherenkov light at 57° to that
//! axis. To reach the `z` faces — where two of the four sensors sit — a photon
//! would have to travel almost along the axis, inside a cone of half-angle 32.9°
//! about it (the critical angle). 57° is outside that cone, so the photon is
//! totally internally reflected and bounces until it is lost. The face a muon
//! crosses is the one face its light cannot reach.
//!
//! This is a real result about the detector and not a bug in the model: it says
//! that a block whose large faces carry the sensors will not be timed by a
//! vertical muon at all, and that the sensors and the crossing direction have to
//! be chosen together.

use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, Tropical, Vec3};

struct Uniform(u64);

impl Uniform {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

fn sensors() -> [SiPM; 4] {
    [
        SiPM::typical(canlab_optics::block::Face::Top),
        SiPM::typical(canlab_optics::block::Face::Bottom),
        SiPM::typical(canlab_optics::block::Face::Side(0)),
        SiPM::typical(canlab_optics::block::Face::Side(2)),
    ]
}

#[test]
fn a_vertical_muon_in_a_high_index_block_reaches_no_thin_face_sensor() {
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.84, 10_000.0);
    let mut uniform = Uniform(11);
    let mut random = || uniform.next();
    let event = simulate(
        &block,
        &optical,
        &sensors(),
        Vec3::new(0.0, 0.0, -20.0),
        Vec3::new(0.0, 0.0, 1.0),
        30.0,
        1.0,
        &mut random,
    )
    .expect("a muon at beta 1");
    assert!(event.photons_born > 100, "the muon did make light: {}", event.photons_born);
    // The two thin-face sensors are indices 0 and 1 (top and bottom).
    assert!(
        event.arrivals_ps[0].is_empty() && event.arrivals_ps[1].is_empty(),
        "a thin-face sensor saw the vertical muon: {:?}",
        event.arrivals_ps.iter().map(Vec::len).collect::<Vec<_>>()
    );
    // And most of the light is trapped rather than escaping.
    assert!(
        event.trapped as f64 > 0.5 * event.photons_born as f64,
        "trapped {} of {}",
        event.trapped,
        event.photons_born
    );
}

#[test]
fn the_critical_angle_is_smaller_than_the_cherenkov_angle_which_is_the_reason() {
    // For n = 1.84 in air, sin(theta_crit) = 1/n, so theta_crit = 32.9 degrees,
    // and the Cherenkov angle is arccos(1/1.84) = 57.1 degrees: the light
    // leaves the muon's path at an angle the faces cannot accept.
    use canlab_optics::cherenkov::Cherenkov;
    use canlab_optics::fresnel::critical_angle;
    let medium = Cherenkov::new(1.84, 0.0).expect("medium");
    let critical = critical_angle(1.84, 1.0).expect("a critical angle");
    assert!(
        critical < medium.angle_at_unit_beta(),
        "critical {} vs Cherenkov {}",
        critical.to_degrees(),
        medium.angle_at_unit_beta().to_degrees()
    );
    assert!((critical.to_degrees() - 32.94).abs() < 0.02, "{}", critical.to_degrees());
    assert!((medium.angle_at_unit_beta().to_degrees() - 57.09).abs() < 0.02);
}

#[test]
fn a_muon_across_a_low_index_block_does_reach_sensors() {
    // The contrast that makes the point: at n = 1.49 the light escapes more
    // easily and a horizontal muon reaches a wall sensor.
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.49, 10_000.0);
    let mut total = 0;
    for seed in 0..20 {
        let mut uniform = Uniform(seed);
        let mut random = || uniform.next();
        let event = simulate(
            &block,
            &optical,
            &sensors(),
            Vec3::new(-30.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            17.0,
            1.0,
            &mut random,
        )
        .expect("a muon at beta 1");
        total += event.detected_count();
    }
    assert!(total > 0, "no photon detected in twenty events at n = 1.49");
}
