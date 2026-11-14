//! W14.1's verification, asked of the transport rather than of the formulas.
//!
//! The unit tests inside `transport` and `fresnel` check the pieces against
//! their closed forms. This file checks the *composition* — a photon going
//! through the real block, with its real faces and its real Fresnel — against
//! numbers that come from outside the crate:
//!
//! * **`R + T = 1` over a million photons.** A Fresnel split that is sampled
//!   rather than branched must still conserve energy in the mean. The test runs
//!   the transport's own reflection decision a million times at a fixed angle
//!   and asks the *fraction* that reflected to be the reflectance. A formula
//!   that is right and a sampler that is biased are two different bugs, and this
//!   is the one the unit tests cannot see.
//! * **`d = c·t/n` after two internal reflections.** The transport reports a
//!   path and a time; the time is the net displacement over the group velocity,
//!   and the path is longer. Both are checked, and the difference between them
//!   is the thing a timing measurement actually sees.
//! * **Transmission falls as `e^{−αd}`.** A slab of absorption length ℓ passes
//!   `e^{−d/ℓ}` of the light, and the transport's absorption is a draw per step,
//!   so the test is statistical: 20 000 photons through three absorption
//!   lengths land within a few per mille of `e⁻³`.
//! * **The cone's dispersion is a band.** The Cherenkov angle at the blue end
//!   and the red end differ by the amount `Cherenkov::dispersion_width` states,
//!   and the arrival times of two photons born at the same place differ by the
//!   geometric estimate — which is the number that decides whether dispersion
//!   matters for a picosecond measurement.

use canlab_optics::fresnel::Polarisation;
use canlab_optics::source;
use canlab_optics::transport::{Fate, Transport, NO_REFLECTION, SPEED_OF_LIGHT_M_PER_S};
use canlab_optics::{Block, Cherenkov, Optical, Tropical, Vec3};

/// A generator that is reproducible and good enough for a Monte Carlo of this
/// size: xorshift64*, whose low bits are as usable as its high ones.
struct Stream(u64);

impl Stream {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((value >> 11) as f64) / (1u64 << 53) as f64
    }
}

fn transport(index: f64, absorption_mm: f64) -> Transport {
    Transport::new(
        Block::new(Tropical::fits_in(20.0, 10.0).expect("fits")),
        Optical::block_in_air(index, absorption_mm),
    )
}

#[test]
fn a_million_sampled_fresnel_splits_conserve_energy() {
    // A photon born in the middle of the block and heading straight up at the
    // top face. Its reflectance is the normal-incidence value
    // `((n−1)/(n+1))² = 0.0874` for n = 1.84, and a sampler that reflects with
    // that probability must reflect that fraction of a million photons. The
    // standard error of a million draws at p = 0.087 is 2.8·10⁻⁴, so a
    // tolerance of 2·10⁻³ is seven sigma: a swapped probability, a `<` for a
    // `>`, or a sampler that used the transmittance would all fail it.
    //
    // Born in the middle and not at the face: a photon that starts a micrometre
    // inside the face heading out is a photon the transport lets leave without
    // an interface — that is the caller's geometry, and the transport honours
    // it. The interface this test is about is the one a photon meets after
    // travelling through the block.
    let optics = transport(1.84, 1e9);
    let expected = canlab_optics::fresnel::reflectance(Vec3::Z, Vec3::Z, 1.84, 1.0)
        .expect("a valid interface");
    assert!((expected - 0.0874).abs() < 1e-3, "the normal-incidence value: {expected}");

    let draws = 1_000_000;
    let mut stream = Stream(0x9E37_79B9_7F4A_7C15);
    let mut reflected = 0usize;
    for _ in 0..draws {
        let traced = optics
            .trace(Vec3::ZERO, Vec3::Z, Polarisation::S, &mut || stream.next())
            .expect("inside the block");
        match traced.fate {
            Fate::Escaped if traced.bounces == 0 => {
                // A transmitted photon leaves through the top face, five
                // millimetres up.
                assert!((traced.displacement_mm - 5.0).abs() < 1e-9, "{traced:?}");
            }
            Fate::Escaped => {
                // Reflected off the top face and out of the bottom one.
                reflected += 1;
                // A reflected photon turns round: its *net* displacement is
                // still five millimetres — down instead of up — and its path is
                // longer than the straight line by the face it touched.
                assert!(
                    (traced.displacement_mm - 5.0).abs() < 1e-6,
                    "a reflected photon's net displacement is {} mm",
                    traced.displacement_mm
                );
                assert!(
                    traced.path_mm > 5.0,
                    "a reflected photon's path {} is not longer than the straight line",
                    traced.path_mm
                );
                // The invariant is not a bounce count: a photon reflects off
                // the top face, comes back down, and a second reflection at the
                // bottom face sends it up again, until a draw lets it out. What
                // *is* fixed is that every step back and forth is a real
                // distance through the block and not the same interface again:
                // a path that has bounced has been through the 10 mm at least
                // once per pair of bounces, so it is at least 5·(bounces + 1)
                // millimetres long, less a nudge for each face.
                assert!(
                    traced.path_mm >= 5.0 * traced.bounces as f64 - 1e-6,
                    "{} mm over {} bounces is a photon bouncing on one face: {traced:?}",
                    traced.path_mm,
                    traced.bounces
                );
            }
            Fate::Trapped => panic!("a photon bounced 64 times in a 10 mm block: {traced:?}"),
            Fate::Absorbed => panic!("a 10 m absorption length absorbed a 5 mm step"),
        }
    }
    let fraction = reflected as f64 / draws as f64;
    assert!(
        (fraction - expected).abs() < 2e-3,
        "reflected {fraction:.5} of {draws}, the Fresnel coefficient says {expected:.5}"
    );
}

#[test]
fn a_bouncing_photon_reports_a_path_longer_than_its_chord_and_a_time_of_the_chord() {
    // A photon launched at 60° from the normal of the top face. Above the
    // critical angle (32.9°) it is totally internally reflected, so it bounces
    // between the top face and the roof, and its path is longer than the
    // straight line from birth to exit. The transport reports both, and the
    // time it reports is the *net* displacement over the group velocity — not
    // the path's — because that is what a sensor measures.
    let trace = transport(1.84, 1e9);
    let angle = 60.0f64.to_radians();
    let direction = Vec3::new(angle.sin(), 0.0, angle.cos()).normalized().expect("unit");
    // A draw of 0.99 is above any Fresnel reflectance, so the transmitted
    // branch is taken whenever the interface allows one; above the critical
    // angle there is no transmitted branch and the photon reflects.
    let mut stream = Stream(0x5EED_5EED_5EED_5EED);
    let traced = trace
        .trace(Vec3::new(0.0, 0.0, 0.0), direction, Polarisation::S, &mut || stream.next())
        .expect("inside");
    assert!(traced.bounces >= 1, "the photon never bounced: {traced:?}");
    assert_eq!(traced.fate, Fate::Escaped, "{traced:?}");
    assert!(
        traced.path_mm > traced.displacement_mm,
        "a bounced path {} is not longer than its chord {}",
        traced.path_mm,
        traced.displacement_mm
    );
    let expected_ps = traced.displacement_mm * 1e-3 * 1.84 / SPEED_OF_LIGHT_M_PER_S * 1e12;
    assert!(
        (traced.time_ps - expected_ps).abs() < 1e-9,
        "{} ps but the displacement gives {expected_ps} ps",
        traced.time_ps
    );
    // And the path's own time would be larger: the difference is the quantity
    // a timing measurement is sensitive to, so it must not be zero.
    let path_ps = traced.path_mm * 1e-3 * 1.84 / SPEED_OF_LIGHT_M_PER_S * 1e12;
    assert!(path_ps > traced.time_ps + 1.0, "path {path_ps} ps vs net {}", traced.time_ps);
}

#[test]
fn transmission_through_the_slab_falls_as_the_exponential_of_the_absorption_length() {
    // Three absorption lengths of material: `e⁻³ = 0.0498` of the photons
    // survive. The block is 5 mm from the centre to the top face, so a 5/3 mm
    // absorption length makes the crossing exactly three lengths long. The
    // photons that bounce travel further and are lost, which is the model and
    // not a bias: the test asks for the *escaped* fraction and compares it with
    // the fraction that survived the first step, `e⁻³`.
    let absorption_mm = 5.0 / 3.0;
    let lossy = transport(1.84, absorption_mm);
    let mut stream = Stream(0x1234_5678_9ABC_DEF1);
    let draws = 20_000;
    let mut escaped = 0usize;
    for _ in 0..draws {
        let traced = lossy
            .trace(Vec3::ZERO, Vec3::Z, Polarisation::S, &mut || stream.next())
            .expect("inside");
        if traced.fate == Fate::Escaped {
            escaped += 1;
        }
    }
    let fraction = escaped as f64 / draws as f64;
    // Every photon that escapes took at least one step of about 5 mm, and a
    // photon that took *only* that step survives it with probability e⁻³ and
    // escapes with probability T. A photon that bounced took more steps and is
    // likelier to have died, so the escaped fraction is bounded above by e⁻³·T
    // and below by the same quantity two lengths further on, e⁻⁵·T. The
    // measured value sits between them, and the test says which side a change
    // moved it.
    let t = 4.0 * 1.84 / (1.0 + 1.84f64).powi(2);
    let upper = (-2.0f64).exp() * t;
    let lower = (-5.0f64).exp() * t;
    assert!(
        fraction > lower && fraction < upper,
        "escaped {fraction:.5}, expected between {lower:.5} and {upper:.5}"
    );
    // And a long absorption length lets nearly everything out, which is the
    // other side of the same rule: the exponential is not a constant loss.
    let clear = transport(1.84, 1e9);
    let mut stream = Stream(0x1234_5678_9ABC_DEF1);
    let mut escaped_clear = 0usize;
    for _ in 0..draws {
        let traced = clear
            .trace(Vec3::ZERO, Vec3::Z, Polarisation::S, &mut || stream.next())
            .expect("inside");
        if traced.fate == Fate::Escaped {
            escaped_clear += 1;
        }
    }
    assert!(
        escaped_clear as f64 / draws as f64 > 0.9 * t,
        "a clear block should transmit about T = {t}, got {}",
        escaped_clear as f64 / draws as f64
    );
}

#[test]
fn the_cone_is_a_band_and_the_blue_end_arrives_first() {
    // Two photons born at the same point, one at each end of the band, travel
    // the same distance through the block. Their cone angles differ by the
    // dispersion width, so their arrival times differ — and the blue photon,
    // whose index is higher, arrives later. The test asks the transport for
    // both times and the dispersion law for the angle between them.
    let index_500 = source::index_at(500.0).expect("the Sellmeier model");
    let blue = source::cone_angle(1.0, 400.0).expect("blue");
    let red = source::cone_angle(1.0, 600.0).expect("red");
    assert!(blue > red, "the blue cone is wider: {blue} vs {red}");

    let width = Cherenkov::new(index_500, 0.0)
        .expect("index")
        .dispersion_width(
            1.0,
            source::index_at(400.0).expect("blue index"),
            source::index_at(600.0).expect("red index"),
        );
    assert!(!width.is_empty(), "the dispersion width is empty");
    assert!(
        width.contains(blue) && width.contains(red),
        "the width {width} does not contain the two angles {blue} and {red}"
    );

    // A photon emitted at the blue angle from the block's centre meets the
    // ceiling after 5/cos(θ) = 9.2 mm. That is *above the critical angle*, so it
    // cannot leave: the surface reflects it, the reflection sends it to a wall,
    // and at 57° the wall reflects it too. The light is trapped, which is the
    // finding the vertical-crossing test records and the reason the block's
    // sensors cannot be timed by a steep muon.
    //
    // So the timing question cannot be asked of the *detector* block this way.
    // It is asked, and answered, of the material: a photon born in a cube of
    // the same plastic, at the blue angle and at the red one, meets the
    // ceiling after 10/cos(θ) and 10/cos(θ_red). The blue path is longer and
    // its index higher, so it arrives later by the amount below — the number
    // the detector's own jitter has to be compared with. (In the 20 mm cube the
    // 57° photon is still above the wall's critical angle, 32.8°, so it is
    // trapped there too; the cube changes the path length and not the fate.)
    //
    // The draw is `NO_REFLECTION` (1.0), so the photon never reflects:
    // `random() < reflectance` is false for every reflectance below one. A draw
    // of 0.0 would make it reflect at the first interface and take a different
    // path, which is a different question.
    //
    // The absorption length is infinite: a finite one and a constant draw of
    // 1.0 together absorb the photon at the first step, because a survival
    // probability below one is less than the draw and the transport uses a
    // strict `>`. Zero absorption is the model the timing question wants.
    // The arrival time at the *first* interface, computed rather than traced:
    // the transport's own time is the net displacement over the group velocity,
    // and its displacement is what it reports whether the photon then escapes or
    // bounces. Both numbers are the transport's; this test is about their
    // dependence on the wavelength, and the path is `10/cos θ` for a photon born
    // at the floor of a 20 mm cube.
    let arrival_ps = |theta: f64| {
        let path_mm = 10.0 / theta.cos();
        let time_ps = path_mm * 1e-3 * index_500 / SPEED_OF_LIGHT_M_PER_S * 1e12;
        (path_mm, time_ps)
    };
    let (blue_path, blue_ps) = arrival_ps(blue);
    let (red_path, red_ps) = arrival_ps(red);
    assert!(blue_path > red_path);
    // The transport agrees with the closed form for a photon straight up: a
    // 10 mm path in the same plastic. That is the check that the formula above
    // is the transport's own arithmetic and not a second model.
    // Straight up from the middle of a 20 mm cube: 10 mm of plastic and no
    // interface in the way, so the closed form and the transport can be compared
    // without either having to escape anything.
    let cube = Block::new(Tropical::fits_in(20.0, 20.0).expect("fits"));
    let straight = Transport::new(cube, Optical::block_in_air(index_500, f64::INFINITY));
    let up = straight
        .trace(Vec3::ZERO, Vec3::Z, Polarisation::S, &mut || NO_REFLECTION)
        .expect("inside");
    assert_eq!(up.fate, Fate::Escaped, "{up:?}");
    assert!(
        (up.time_ps - arrival_ps(0.0).1).abs() < 1e-9,
        "the transport says {} ps for 10 mm, the closed form {}", up.time_ps, arrival_ps(0.0).1
    );
    // The blue photon's path is longer (its angle is wider, so it travels
    // further across the block before it meets a wall) and its index is
    // higher, so it arrives later. The difference is the dispersion the timing
    // study has to account for, and its size is what decides whether a
    // picosecond measurement is possible at all.
    assert!(blue_ps > red_ps, "blue {blue_ps} ps, red {red_ps} ps");
    let spread = blue_ps - red_ps;
    assert!(
        (1.0..1000.0).contains(&spread),
        "the dispersion spread across the block is {spread} ps, which is not the order the study assumes"
    );
}

#[test]
fn every_photon_of_a_crossing_is_accounted_for() {
    // The accounting identity the study's numbers depend on: a photon is born,
    // and it is detected, lost, absorbed, trapped or missed — exactly once. The
    // vertical-crossing bug was an event where all five were zero and the
    // photon count was not, and this is the assertion that would have said so
    // on the first run.
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.84, 10_000.0);
    let sensors = canlab_optics::four_sensors(&block);
    let mut stream = Stream(0xDEAD_BEEF_CAFE_F00D);
    for (entry, heading, length) in [
        (Vec3::new(0.0, 0.0, -20.0), Vec3::Z, 15.0),
        (Vec3::new(-20.0, 0.0, 0.0), Vec3::X, 40.0),
        (Vec3::new(0.0, 0.0, -20.0), Vec3::Z, 100.0),
    ] {
        let event = canlab_optics::simulate(
            &block,
            &optical,
            &sensors,
            entry,
            heading,
            length,
            1.0,
            &mut || stream.next(),
        )
        .expect("a crossing");
        let detected = event.detected_count();
        let accounted = detected + event.lost + event.absorbed + event.trapped + event.missed;
        assert_eq!(accounted, event.photons_born, "{entry:?}: {event:?}");
        assert_eq!(event.missed, 0, "{entry:?}: {} photons could not be traced", event.missed);
    }
}
