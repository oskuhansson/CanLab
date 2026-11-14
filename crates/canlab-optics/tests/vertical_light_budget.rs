//! Where a vertical muon's light actually goes — the diagnostic that corrected
//! a wrong explanation, and then a second one.
//!
//! The vertical-crossing test first asserted that the photons were "trapped".
//! They are not: they escape, and they escape somewhere the sensors are not.
//! This test recorded that split, in a table that read `born 391 detected 0 lost
//! 0 absorbed 0 trapped 0` — an event that accounted for none of its photons,
//! which is what a *refused* trace looks like and not what a physics result
//! looks like. The explanation was wrong because the event was empty.
//!
//! With the birth point fixed the split is the measured one: most of the light
//! is trapped by total internal reflection, and what escapes leaves through the
//! walls, where the sensors are not. The test now asserts the accounting and
//! the split, so a future event that goes quiet again fails here.

use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, Tropical, Vec3};

struct Uniform(u64);

impl Uniform {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

#[test]
fn a_vertical_muons_light_is_trapped_and_what_escapes_misses_the_sensors() {
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.84, 10_000.0);
    let sensors = [
        SiPM::typical(canlab_optics::block::Face::Top),
        SiPM::typical(canlab_optics::block::Face::Bottom),
        SiPM::typical(canlab_optics::block::Face::Side(0)),
        SiPM::typical(canlab_optics::block::Face::Side(2)),
    ];
    let mut uniform = Uniform(11);
    let mut random = || uniform.next();
    let event = simulate(
        &block,
        &optical,
        &sensors,
        Vec3::new(0.0, 0.0, -20.0),
        Vec3::new(0.0, 0.0, 1.0),
        15.0,
        1.0,
        &mut random,
    )
    .expect("a muon at beta 1");
    let detected: usize = event.arrivals_ps.iter().map(Vec::len).sum();
    let seen = detected + event.lost + event.absorbed + event.trapped;
    println!(
        "born {} detected {} lost {} absorbed {} trapped {} (accounted {})",
        event.photons_born, detected, event.lost, event.absorbed, event.trapped, seen
    );
    // Every photon is accounted for: a transport that loses photons silently is
    // a transport whose loss numbers mean nothing.
    assert_eq!(seen, event.photons_born, "photons went missing");
    // Most of the light is trapped by total internal reflection: the Cherenkov
    // cone at 57° is outside the 33° cone the faces accept, so a photon that
    // reaches a face is reflected back rather than transmitted.
    assert!(
        event.trapped > event.photons_born / 2,
        "trapped {} of {}",
        event.trapped,
        event.photons_born
    );
    // What does escape leaves through the walls, and the walls' sensors cover
    // only a fraction of them, so almost nothing is detected.
    assert!(event.lost > 0, "nothing escaped at all");
    assert!(
        detected * 20 < event.photons_born,
        "detected {detected} of {}",
        event.photons_born
    );
    // And the two thin-face sensors still see nothing.
    assert!(event.arrivals_ps[0].is_empty() && event.arrivals_ps[1].is_empty());
}
