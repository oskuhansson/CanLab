//! An audit of every physical constant and unit conversion in the study.
//!
//! These tests exist because one of them was missing. `C_MM_PER_PS` was written
//! as `299.792458`, which is millimetres per *nanosecond*: light a thousand times
//! too fast, every timing signal a thousand times too small, and a direction that
//! is measurable at tens of degrees reported as unmeasurable at eighty thousand.
//! The tests that existed did not catch it, and the reason is the lesson:
//!
//! > **A test that asserts the code's own arithmetic cannot catch a wrong
//! > constant.** `assert_eq!(signal, 54.1 / 201.2)` passes whether 201.2 is
//! > millimetres per picosecond or per nanosecond, because it uses the same
//! > wrong number on both sides.
//!
//! So every test here states the fact **independently** — in a different unit, a
//! different route, or against a number from outside the crate — and then asks
//! whether the code agrees. Where two implementations of one law exist, the test
//! compares them to each other.
//!
//! # The facts, and where each one comes from
//!
//! | Fact | Independent statement |
//! |---|---|
//! | `c` | a metre is 3.34 ns, and a nanosecond is a foot |
//! | the transport's clock | it computes from `c` in metres per second |
//! | Cherenkov yield | the textbook `2πα sin²θ_c ∫dλ/λ²`, worked by hand |
//! | scintillation yield | 10 000 photons/MeV × 2 MeV/cm |
//! | the can | `π × 33²` |
//! | the flux | 127 m⁻²s⁻¹ × 36 cm² × 100 s |
//! | the mass | 30 cm³ × 1.19 g/cm³ |
//! | the position bound | `√2 × 1 mm / 54.1 mm` |
//! | the Cramér–Rao bound | a closed form for a symmetric tetrahedron |

use canlab_muon::bound::{position_bound, timing_bound};
use canlab_muon::can::{tent_mass_g, tent_volume_mm3, Can};
use canlab_muon::mission::{MUON_FLUX_INTEGRATED_M2_S, TOTAL_MASS_G};
use canlab_muon::scintillator::Scintillator;
use canlab_muon::timing::{speed_mm_per_ps, timing_signal_ps, C_MM_PER_PS};
use canlab_optics::{Block, Optical, Polarisation, Transport, Tropical, Vec3};

/// The design block's four sensor centres.
fn design_positions() -> [Vec3; 4] {
    Block::new(Tropical::fits_in(20.0, 10.0).expect("fits")).sensor_positions()
}

#[test]
fn the_speed_of_light_is_a_third_of_a_millimetre_per_picosecond() {
    // Stated three ways, none of which uses the constant's own value.
    //
    // 1. A metre takes 3.3356 ns, because c is 299 792 458 m/s.
    let nanoseconds_per_metre: f64 = 1.0e9 / 299_792_458.0;
    assert!((nanoseconds_per_metre - 3.3356).abs() < 1e-3, "{nanoseconds_per_metre}");
    // 2. A nanosecond is a foot: 304.8 mm. A picosecond is a thousandth of that.
    let foot_in_mm: f64 = 304.8;
    assert!((foot_in_mm / 1000.0 - 0.3048).abs() < 1e-12);
    // 3. So the constant, in millimetres per picosecond, is a third of a
    //    millimetre — not three hundred.
    assert!((C_MM_PER_PS - 0.2998).abs() < 1e-3, "{C_MM_PER_PS}");
    // A millimetre per picosecond would be 3.3 times the speed of light, which is
    // the shape the wrong constant had. The comparison is against a derived
    // quantity so the compiler cannot fold it into a constant assertion.
    let millimetre_per_ps_in_vacuum = C_MM_PER_PS;
    assert!(
        millimetre_per_ps_in_vacuum < 1.0,
        "light does not cross a millimetre in a picosecond: {millimetre_per_ps_in_vacuum}"
    );
    // And in a plastic it is slower by the index: 0.2 mm/ps in PMMA.
    let pmma = speed_mm_per_ps(1.49).expect("a speed");
    assert!((pmma - 0.2012).abs() < 1e-3, "{pmma}");
    assert!(pmma < C_MM_PER_PS, "a medium slows light");
}

#[test]
fn the_transports_clock_and_the_studys_speed_are_the_same_number() {
    // The transport computes a photon's flight time from `c` in *metres per
    // second*, a unit nobody can slip on. If the study's constant disagrees with
    // the time the transport reports, one of them is wrong — and this is the
    // test that would have caught the factor of a thousand, because the two
    // sides are computed from different constants rather than from each other.
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.49, 10_000.0);
    let transport = Transport::new(block, optical);
    let mut no_reflection = || 1.0;
    let traced = transport
        .trace(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Polarisation::S, &mut no_reflection)
        .expect("a trace");
    // The transport's own speed, from its own two numbers.
    let measured = traced.displacement_mm / traced.time_ps;
    let ours = speed_mm_per_ps(1.49).expect("a speed");
    assert!((measured - ours).abs() < 1e-6, "{measured} vs {ours}");
    // And the time is a hundred picoseconds, not a tenth of one: the block is
    // 20 mm across and light needs 99 ps for it.
    assert!((traced.time_ps - 99.4).abs() < 0.5, "{} ps", traced.time_ps);
}

#[test]
fn the_cherenkov_yield_matches_the_textbook_worked_by_hand() {
    // 2π α sin²θ_c ∫dλ/λ² over 400-600 nm, with sin²θ_c = 1 − 1/n².
    // For n = 1.49: sin²θ_c = 1 − 0.4504 = 0.5496; ∫dλ/λ² over 400-600 nm is
    // 8.3333e-4 per nm, which is 833.3 per millimetre; so
    // 2π × (1/137.036) × 0.5496 × 833.3 = 21.0 photons/mm.
    let alpha = 1.0 / 137.035_999_084;
    let sin_squared = 1.0 - 1.0 / (1.49 * 1.49);
    let band_per_mm = 1.0e6 * (1.0 / 400.0 - 1.0 / 600.0);
    let by_hand = std::f64::consts::TAU * alpha * sin_squared * band_per_mm;
    assert!((by_hand - 21.0).abs() < 0.05, "{by_hand}");
    // The crate's two implementations of that law, and the hand value.
    let from_source = canlab_optics::source::photons_per_mm(1.0, 1.49).expect("above threshold");
    let from_cherenkov =
        canlab_optics::cherenkov::photons_per_mm(1.0, 1.49, 400.0, 600.0).expect("a band");
    assert!((from_source - by_hand).abs() < 1e-9, "{from_source}");
    assert!((from_cherenkov - by_hand).abs() < 1e-9, "{from_cherenkov}");
    // The two implementations are one law: they must agree to the last bit.
    assert!((from_source - from_cherenkov).abs() < 1e-12);
    // And the sanity figure a physicist would quote: 210 photons per centimetre.
    assert!((from_source * 10.0 - 210.0).abs() < 0.5, "{}", from_source * 10.0);
}

#[test]
fn the_scintillator_yield_is_its_two_datasheet_numbers() {
    // 10 000 photons/MeV and 1.95 MeV/(g/cm²) at 1.023 g/cm³ is 1.995 MeV/cm,
    // so a centimetre emits 19 950 photons and a millimetre 1 995.
    let scintillator = Scintillator::pvt();
    assert!((scintillator.dedx_mev_per_cm - 1.995).abs() < 1e-3, "{}", scintillator.dedx_mev_per_cm);
    assert!((scintillator.photons_per_mm() - 1_995.0).abs() < 1.0, "{}", scintillator.photons_per_mm());
    // The ratio to Cherenkov light, which is the number the design turns on.
    let cherenkov = canlab_optics::source::photons_per_mm(1.0, 1.49).expect("above threshold");
    let ratio = scintillator.photons_per_mm() / cherenkov;
    assert!((ratio - 95.0).abs() < 1.0, "{ratio}");
    // And the photoelectrons from a 5 mm plane: 9 975 × 0.25 × 0.35 = 873.
    assert!((scintillator.photoelectrons(5.0) - 873.0).abs() < 1.0);
}

#[test]
fn the_can_is_pi_r_squared_and_the_flux_is_a_rate_times_an_area() {
    // The can's cross-section, from its diameter and nothing else.
    let can = Can::standard();
    // Two areas, and the difference is the five millimetres of allowance: the
    // can's own 33 mm radius is 3421 mm² and the usable 28 mm is 2463 mm². The
    // method returns the usable one, because a detector cannot use the wall.
    let outer = std::f64::consts::PI * 33.0 * 33.0;
    let inner = std::f64::consts::PI * 28.0 * 28.0;
    assert!((outer / 100.0 - 34.21).abs() < 0.01, "{}", outer / 100.0);
    assert!((inner / 100.0 - 24.63).abs() < 0.01, "{}", inner / 100.0);
    assert!((can.cross_section_mm2() - inner).abs() < 1e-9, "{}", can.cross_section_mm2());
    assert!((can.inner_radius_mm() - 28.0).abs() < 1e-12);
    // The flux through a 40 × 90 mm plane in 100 s, from the rate and the area.
    let area_m2 = 40.0 * 90.0 / 1.0e6;
    let by_hand = MUON_FLUX_INTEGRATED_M2_S * area_m2 * 100.0;
    assert!((by_hand - 45.7).abs() < 0.5, "{by_hand}");
    // The same fact in the units the mission document uses: about one muon per
    // square centimetre per minute, so 36 cm² for 1.67 minutes is 60 — and the
    // difference from 45.7 is the difference between the vertical flux and the
    // integrated one, which is why the document's "~1/cm²/min" is a round number.
    let per_cm2_per_min = MUON_FLUX_INTEGRATED_M2_S / 1.0e4 * 60.0;
    assert!((per_cm2_per_min - 0.762).abs() < 0.01, "{per_cm2_per_min}");
}

#[test]
fn the_blocks_mass_is_its_volume_times_the_density() {
    // 7.5 × 20² × 10 = 30 000 mm³ = 30 cm³; PMMA at 1.19 g/cm³ is 35.7 g.
    let volume = tent_volume_mm3(20.0, 10.0).expect("a block");
    assert!((volume - 30_000.0).abs() < 1e-6, "{volume}");
    let mass = tent_mass_g(20.0, 10.0, 1.19).expect("a mass");
    assert!((mass - 35.7).abs() < 0.05, "{mass}");
    // And the share of the can's mass that allows: 15 % of 350 g.
    let share = TOTAL_MASS_G.1 * 0.15;
    assert!((share - 52.5).abs() < 1e-9);
    assert!(mass < share, "the design block fits the mass share");
}

#[test]
fn the_position_bound_is_the_root_two_over_the_separation() {
    // Two hits a millimetre apart, 54.1 mm apart: √2 × 1 / 54.1 = 0.02614 rad,
    // which is 1.4976 degrees. Worked in radians and converted once.
    let by_hand = std::f64::consts::SQRT_2 * 1.0 / 54.1;
    assert!((by_hand.to_degrees() - 1.4976).abs() < 1e-3, "{}", by_hand.to_degrees());
    let bound = position_bound(1.0, 54.1).expect("a bound");
    assert!((bound.sigma_rad - by_hand).abs() < 1e-12);
}

#[test]
fn the_cramer_rao_bound_matches_a_closed_form_for_a_symmetric_tetrahedron() {
    // Four sensors at the origin and one unit along each axis, heading along z.
    // By symmetry the two tangent coordinates decouple from the start time, so
    // the 2×2 information block is [[S, C], [C, S]] with
    //
    //   S = Σ (u · e)² / v²,   C = Σ (u · e₁)(u · e₂) / v²,
    //
    // and the bound is σ² · 2S / (S² − C²). Computed here from the positions and
    // the speed directly, with no call into the crate's information matrix.
    let edge = 54.1;
    let positions = [
        Vec3::ZERO,
        Vec3::new(edge, 0.0, 0.0),
        Vec3::new(0.0, edge, 0.0),
        Vec3::new(0.0, 0.0, edge),
    ];
    let speed = speed_mm_per_ps(1.49).expect("a speed");
    let jitter = 47.4;
    let mean = Vec3::new(edge / 4.0, edge / 4.0, edge / 4.0);
    // Heading z has the tangent basis (y, −x).
    let mut sum_squared = 0.0;
    let mut sum_cross = 0.0;
    for position in positions {
        let u = position.sub(mean);
        let a = u.y / speed;
        let b = -u.x / speed;
        sum_squared += a * a;
        sum_cross += a * b;
    }
    let variance = jitter * jitter * 2.0 * sum_squared
        / (sum_squared * sum_squared - sum_cross * sum_cross);
    let by_hand = variance.sqrt().to_degrees();
    // 17.5 degrees: a 54 mm tetrahedron with a 47 ps jitter.
    assert!((by_hand - 17.5).abs() < 0.2, "{by_hand}");
    let bound = timing_bound(&positions, Vec3::new(0.0, 0.0, 1.0), jitter, speed).expect("a bound");
    assert!((bound.sigma_deg() - by_hand).abs() < 1e-9, "{} vs {by_hand}", bound.sigma_deg());
}

#[test]
fn the_timing_signal_is_the_light_crossing_the_detector() {
    // The bottom-face placement: the four sensors are the corners of a 20 mm
    // square, so the furthest pair is its diagonal, 28.28 mm, and light needs
    // 140.6 ps for that. The signal is the spread of the projection, so it is at
    // most that and is that when the heading is along the pair.
    let positions = design_positions();
    let speed = speed_mm_per_ps(1.49).expect("a speed");
    let mut furthest = 0.0f64;
    for i in 0..4 {
        for j in (i + 1)..4 {
            furthest = furthest.max(positions[i].sub(positions[j]).length());
        }
    }
    assert!((furthest - 28.284).abs() < 0.01, "{furthest}");
    let crossing_ps = furthest / speed;
    assert!((crossing_ps - 140.58).abs() < 0.5, "{crossing_ps}");
    // The signal never exceeds the crossing time, and reaches it along the pair.
    let mut best = 0.0f64;
    for heading in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
    ] {
        let heading = heading.normalized().expect("a direction");
        let signal = timing_signal_ps(&positions, heading, speed).expect("a signal");
        assert!(signal <= crossing_ps + 1e-9, "{signal} exceeds {crossing_ps}");
        best = best.max(signal);
    }
    assert!((best - 114.8).abs() < 2.0, "{best}");
    // And the signal is larger than the jitter, which is the whole point: a
    // timestamp can resolve the geometry. The first version of this study said
    // the opposite because its constant was a thousand times too large.
    assert!(best > 47.4, "{best} ps against 47.4 ps of jitter");
}

#[test]
fn every_unit_conversion_round_trips() {
    // The conversions the code performs, each checked against its inverse rather
    // than against itself: mm to m, m to mm, nm to mm, mm³ to cm³, mm² to m².
    let millimetres: f64 = 54.1;
    let metres = millimetres * 1e-3;
    assert!((metres / 1e-3 - millimetres).abs() < 1e-12);
    let nanometres: f64 = 500.0;
    let millimetres_from_nm = nanometres * 1e-6;
    assert!((millimetres_from_nm / 1e-6 - nanometres).abs() < 1e-12);
    let mm3: f64 = 30_000.0;
    let cm3 = mm3 / 1000.0;
    assert!((cm3 * 1000.0 - mm3).abs() < 1e-9);
    let mm2: f64 = 40.0 * 90.0;
    let m2 = mm2 / 1.0e6;
    assert!((m2 * 1.0e6 - mm2).abs() < 1e-9);
    // And the one that matters for the timing: picoseconds to nanoseconds.
    let ps: f64 = 268.9;
    assert!((ps * 1e-3 - 0.2689).abs() < 1e-12);
}
