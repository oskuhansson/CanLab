//! The Cherenkov timing design, iterated and measured.
//!
//! ```text
//! cargo run --release -p canlab-muon --bin timing_iterations
//! ```
//!
//! Each row is a design, simulated event by event with the real photon transport.
//! The columns are the two numbers that decide whether it works:
//!
//! * **`full`** — the fraction of muons with all four timestamps. The fit needs
//!   four, so this is the fraction that can be reconstructed at all.
//! * **`median`** — the angular error of the reconstruction, in degrees, over the
//!   events that had one.
//!
//! And **`bound`**, the Cramér–Rao limit the earlier study quoted. The gap between
//! `median` and `bound` is the gap between the linear model the bound assumes and
//! the transport the detector actually runs: a photon is emitted along the track,
//! travels by a path that depends on where it was emitted, and the first photon to
//! arrive took the shortest path rather than the one from the track's midpoint.
//!
//! # What the sweep varies
//!
//! * **Scale** — the tent's half-width, 15 to 24 mm. Bigger is more light and a
//!   longer chord, and 24.44 mm is the largest that fits the 110 mm can.
//! * **Thickness** — the extrusion, 5 to 20 mm. Thicker is more light and a
//!   longer path for the photons to reach a face.
//! * **Index** — PMMA (1.49) against a high-index plastic (1.84). The higher
//!   index makes more Cherenkov light and traps more of it, and the sweep exists
//!   to show which wins.

use canlab_muon::timing_study::{run, TimingDesign};

/// How many muons per design. Two hundred is enough for a median and a fraction
/// and quick enough that the whole sweep is a few seconds.
const EVENTS: usize = 200;

fn main() {
    println!("CHERENKOV TIMING — simulated event by event, {EVENTS} muons per design");
    println!(
        "The block is a tent prism; the four SiPMs sit on the faces the transport detects on."
    );
    println!(
        "`full` is the fraction of muons with all four timestamps; `median` is the angular"
    );
    println!("error of the reconstruction over those events; `bound` is the Cramér–Rao limit.");
    println!();

    println!(
        "{:>6} {:>5} {:>5} {:>7} {:>9} {:>7} {:>9} {:>9} {:>7}",
        "scale", "deep", "n", "photons", "timed/4", "full", "median deg", "p68 deg", "bound"
    );
    for &scale in &[15.0, 18.0, 20.0, 22.0, 24.0] {
        for &thickness in &[5.0, 10.0, 20.0] {
            for &index in &[1.49, 1.84] {
                let design = TimingDesign {
                    name: "sweep",
                    scale_mm: scale,
                    thickness_mm: thickness,
                    index,
                    pde: 0.35,
                };
                let result = run(&design, EVENTS, 2026);
                let median = result
                    .median_error_deg
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "-".to_string());
                let p68 = result
                    .p68_error_deg
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "-".to_string());
                let bound = result
                    .bound_deg
                    .map(|value| format!("{value:.0}"))
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "{:>6.1} {:>5.1} {:>5.2} {:>7.0} {:>9.2} {:>7.2} {:>9} {:>9} {:>7}",
                    scale,
                    thickness,
                    index,
                    result.mean_photons,
                    result.mean_timed,
                    result.full_fraction,
                    median,
                    p68,
                    bound,
                );
            }
        }
    }

    // The design point on its own, with more events, so the headline number is
    // not a 200-event accident.
    println!();
    let design = TimingDesign::design();
    let result = run(&design, 400, 7);
    println!(
        "The design point (20 mm tent, 10 mm deep, PMMA), 400 muons: {:.0} photons, \
         {:.2} of 4 sensors timed, {:.0} % with all four.",
        result.mean_photons,
        result.mean_timed,
        result.full_fraction * 100.0,
    );
    println!(
        "  linear model, median error   {}   (the bound the earlier study quoted: {})",
        result
            .median_error_deg
            .map(|value| format!("{value:.1}°"))
            .unwrap_or_else(|| "none".to_string()),
        result
            .bound_deg
            .map(|value| format!("{value:.0}°"))
            .unwrap_or_else(|| "-".to_string()),
    );
    println!(
        "  Cherenkov model with the TRUE track point, median error {}",
        result
            .median_exact_deg
            .map(|value| format!("{value:.1}°"))
            .unwrap_or_else(|| "none".to_string()),
    );
    println!();
    println!(
        "The second line is a diagnostic, not a detector: a real detector does not know where\n\
         the track is. It separates two failures — a model that is wrong, and a model that is\n\
         right but cannot be identified from four timestamps. If it is small and the first is\n\
         large, the model is right and the observable is confounded by the track position."
    );
}
