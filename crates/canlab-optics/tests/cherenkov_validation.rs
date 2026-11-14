//! The Cherenkov law against closed forms and known material constants.
//!
//! Three kinds of check, each on both sides of its rule:
//!
//! * the *value* of the Cherenkov angle against the analytic `arccos(1/(βn))`,
//!   and its threshold behaviour just below `β = 1/n`;
//! * the *identity* between the angle the CAS parses and the angle the float
//!   evaluation returns, so the two transcriptions cannot drift;
//! * the *material*: the Sellmeier coefficients, the critical angle they imply,
//!   and the photon yield of the band the detectors can actually see.
//!
//! The reference values are the ones a physicist would write on paper: water at
//! `n = 1.33` gives 41.2° at unit β, fused silica at `n = 1.46` gives 46.8°, and
//! a block at `n = 1.84` gives 57.1°.

use canlab_core::Program;
use canlab_optics::cherenkov::{
    BLUE_NM, RED_NM, Cherenkov, default_plastic, mean_wavelength_nm, medium_index,
    photons_per_mm,
};

fn degrees(radians: f64) -> f64 {
    radians.to_degrees()
}

#[test]
fn water_gives_the_tabulated_cherenkov_angle() {
    // In water n = 1.33, cos θ = 1/1.33 = 0.7519 and θ = 41.22°. This is the
    // number in every textbook, and the test that would catch an inverted law.
    let water = Cherenkov::new(1.33, 0.005).expect("water");
    let theta = degrees(water.angle(1.0).expect("above threshold"));
    // arccos(1/1.33) = 41.2465°; the commonly quoted 41.2° is that rounded to
    // three significant figures, and the test holds the unrounded value.
    assert!((theta - 41.25).abs() < 0.01, "{theta}");
}

#[test]
fn fused_silica_gives_its_tabulated_angle() {
    let silica = Cherenkov::new(1.46, 0.01).expect("silica");
    let theta = degrees(silica.angle(1.0).expect("above threshold"));
    assert!((theta - 46.77).abs() < 0.02, "{theta}");
}

#[test]
fn the_high_index_block_gives_57_degrees() {
    let block = Cherenkov::new(1.84, 0.01).expect("block");
    let theta = degrees(block.angle(1.0).expect("above threshold"));
    // arccos(1/1.84) = 57.09°
    assert!((theta - 57.09).abs() < 0.02, "{theta}");
}

#[test]
fn exactly_at_threshold_the_angle_is_zero_and_just_below_there_is_none() {
    let medium = Cherenkov::new(1.5, 0.0).expect("medium");
    let threshold = medium.beta_threshold();
    assert!((threshold - 2.0 / 3.0).abs() < 1e-15);
    // At exactly β = 1/n the cosine is 1 and the cone collapses to a line.
    let at = medium.angle(threshold).expect("threshold emits");
    assert!(degrees(at).abs() < 1e-9);
    // A hair below, nothing is emitted: `None`, never a zero-length cone that a
    // counting simulation would tally as an event.
    assert!(medium.angle(threshold * (1.0 - 1e-9)).is_none());
    assert!(medium.cos_angle(threshold * (1.0 - 1e-9)).is_none());
}

#[test]
fn the_angle_grows_monotonically_with_beta() {
    let medium = Cherenkov::new(1.6, 0.0).expect("medium");
    let mut previous = -1.0;
    for step in 63..=100 {
        let beta = step as f64 / 100.0;
        if let Some(angle) = medium.angle(beta) {
            assert!(angle > previous, "angle fell at beta = {beta}");
            previous = angle;
        }
    }
    assert!(previous > 0.0, "nothing above threshold");
}

#[test]
fn the_symbolic_law_the_cas_parses_equals_the_float_law() {
    // The simulation uses `cos_angle` in its inner loop; the cas and the paper
    // use the parsed LaTeX `1/(n·β)`. `canlab-poly`'s `free_symbols` reads the
    // variable list out of the tree, which is what `Program::compile` needs, so
    // the test compiles the parsed expression the same way the CAS would and
    // evaluates it: a drift between the two forms of the law fails here rather
    // than becoming a wrong figure in a report.
    let expr = Cherenkov::latex_cosine().expect("the law parses");
    let variables = canlab_poly::free_symbols(&expr);
    assert_eq!(variables.len(), 2, "the law has two parameters: {variables:?}");
    let program = Program::compile(&expr, &variables).expect("compiles");
    for (n, beta) in [(1.33, 1.0), (1.84, 1.0), (1.5, 0.9), (1.6, 0.7), (2.0, 0.55)] {
        let medium = Cherenkov::new(n, 0.0).expect("medium");
        let float = medium.cos_angle(beta).expect("above threshold");
        let mut args = vec![0.0; variables.len()];
        for (slot, symbol) in args.iter_mut().zip(&variables) {
            *slot = if symbol.as_str() == "n" { n } else { beta };
        }
        let symbolic = program.eval(&args);
        assert!(
            (symbolic - float).abs() < 1e-14,
            "n={n} beta={beta}: CAS {symbolic} vs float {float}"
        );
    }
}

#[test]
fn the_angle_bound_contains_the_true_angle_and_is_not_symmetric_by_accident() {
    // A 1/sqrt(1 - 1/(βn)²)-like law is asymmetric in `n`; the bound must be a
    // real bound, i.e. it must contain both ends and not be a point.
    let medium = Cherenkov::new(1.84, 0.01).expect("medium");
    let bounds = medium.angle_bounds(1.0);
    assert!(!bounds.is_empty());
    let low_index = Cherenkov::new(1.83, 0.0).unwrap().angle(1.0).unwrap();
    let high_index = Cherenkov::new(1.85, 0.0).unwrap().angle(1.0).unwrap();
    assert!(bounds.contains(low_index), "{bounds}");
    assert!(bounds.contains(high_index), "{bounds}");
    assert!(bounds.hi() > bounds.lo(), "a point is not a bound");
    // A bigger sigma is a wider bound: the parameter has to matter.
    let wider = Cherenkov::new(1.84, 0.02).expect("medium").angle_bounds(1.0);
    assert!(wider.hi() - wider.lo() > bounds.hi() - bounds.lo());
}

#[test]
fn dispersion_at_the_blue_end_is_larger_than_at_the_red_end() {
    let block = Cherenkov::new(1.84, 0.0).expect("block");
    let width = block.dispersion_width(1.0, 1.88, 1.810);
    assert!(!width.is_empty());
    // The blue end has the higher index, so its cone is wider.
    let blue = Cherenkov::new(1.88, 0.0).unwrap().angle(1.0).unwrap();
    let red = Cherenkov::new(1.81, 0.0).unwrap().angle(1.0).unwrap();
    assert!((width.hi() - blue).abs() < 1e-12, "{width} {blue}");
    assert!((width.lo() - red).abs() < 1e-12, "{width} {red}");
    // And the spread over this band is of the order the docs claim: 0.5-2°.
    let spread = degrees(width.hi() - width.lo());
    assert!((0.3..3.0).contains(&spread), "spread {spread}°");
}

#[test]
fn the_sellmeier_index_falls_towards_the_red_and_is_near_the_design_value() {
    let plastic = default_plastic();
    let blue = medium_index(&plastic, BLUE_NM / 1000.0).expect("blue");
    let red = medium_index(&plastic, RED_NM / 1000.0).expect("red");
    assert!(blue > red, "normal dispersion: blue must have the higher index");
    assert!((1.75..1.95).contains(&blue), "blue index {blue}");
    assert!((1.75..1.95).contains(&red), "red index {red}");
}

#[test]
fn the_sellmeier_form_refuses_a_wavelength_at_a_pole() {
    let plastic = default_plastic();
    assert!(medium_index(&plastic, 0.0).is_none());
    assert!(medium_index(&plastic, -0.5).is_none());
    // A pole is a real, physical resonance; evaluating *at* one is not a number.
    assert!(medium_index(&plastic, plastic[0].1).is_none());
}

#[test]
fn the_mean_wavelength_shifts_blue_of_the_band_centre() {
    // 1/λ² weights the blue; the mean of 400-600 nm is 488 nm, not 500.
    let mean = mean_wavelength_nm(400.0, 600.0).expect("band");
    // 400·600·ln(1.5)/200 = 486.56 nm. The prose in the module said 488; the
    // formula is the authority and the prose was corrected to match.
    assert!((mean - 486.6).abs() < 0.2, "{mean}");
    // Both ends of the weighted distribution lie inside the band.
    assert!(mean > 400.0 && mean < 600.0);
    assert!(mean_wavelength_nm(600.0, 400.0).is_none(), "reversed band");
}

#[test]
fn the_photon_yield_matches_the_textbook_band_calculation() {
    // 2π α (1 - 1/(β²n²)) Δλ/λ² * 1e6 per millimetre, at β = 1 and n = 1.5:
    // 1 - 1/2.25 = 0.5556, the integral 1/281 nm⁻¹ for 300-700 nm * 1e6/mm
    // gives ~16 photons/mm. A muon's whole 100 mm path is ~1600 photons, which
    // is what a Cherenkov detector of this size delivers.
    // Worked out by hand: 2π·(1/137.036)·0.5556·(1/300 − 1/700)·1e6 = 48.5/mm.
    // An earlier version of this test expected 16.8 — the arithmetic had been
    // done without the 1e6 that turns nm⁻¹ into mm⁻¹, and the code was right.
    let yield_per_mm = photons_per_mm(1.0, 1.5, 300.0, 700.0).expect("yield");
    assert!((yield_per_mm - 48.5).abs() < 1.0, "{yield_per_mm}");
    // The 400-600 nm band the SiPMs see is smaller, by its own integral:
    // (1/400 − 1/600)·1e6 = 833 mm⁻¹ against 1905 for 300-700.
    let narrow = photons_per_mm(1.0, 1.5, 400.0, 600.0).expect("yield");
    assert!(narrow < yield_per_mm);
    assert!((narrow - 21.3).abs() < 1.0, "{narrow}");
}

#[test]
fn below_threshold_the_photon_yield_is_zero_not_a_negative_number() {
    let at = photons_per_mm(1.0 / 1.5, 1.5, 400.0, 600.0).expect("valid input");
    assert_eq!(at, 0.0);
    assert!(photons_per_mm(0.5, 1.5, 400.0, 600.0).map(|y| y == 0.0).unwrap_or(false));
    assert!(photons_per_mm(1.0, 1.5, 600.0, 400.0).is_none(), "reversed band");
    // The muon case at the design index: ~21 photons/mm in 400-600 nm, so a
    // 100 mm path yields ~2100 photons before any wall or sensor loss.
    let block = photons_per_mm(1.0, 1.84, 400.0, 600.0).expect("yield");
    // 2π·(1/137.036)·(1 − 1/1.84²)·833 = 26.9/mm. The expectation was written
    // as 30 before being computed; the code's number is the one that follows
    // from the formula.
    assert!((block - 26.9).abs() < 1.0, "{block}");
}

#[test]
fn a_medium_with_an_index_below_one_is_refused() {
    assert!(Cherenkov::new(0.99, 0.0).is_none());
    assert!(Cherenkov::new(1.5, -0.1).is_none());
    assert!(Cherenkov::new(f64::NAN, 0.0).is_none());
    assert!(Cherenkov::new(1.5, f64::INFINITY).is_none());
}
