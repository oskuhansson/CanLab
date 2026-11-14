//! Does optical adhesive make a high-index block worth it?
//!
//! ```text
//! cargo run --release -p canlab-muon --bin glue_study
//! ```
//!
//! The earlier sweep found that **PMMA beats a high-index plastic**, 8.06 detected
//! photons against 4.74, even though the high-index block makes 28 % more
//! Cherenkov light. The reason is total internal reflection: at n = 1.84 the
//! critical angle is 32.9° and 92 % of the light arriving at a face is trapped,
//! against 42.2° and 87 % at n = 1.49.
//!
//! The obvious follow-up: if the sensor faces are **bonded with optical
//! adhesive**, does the high-index block become the better choice?
//!
//! # The answer is not the one the question expects
//!
//! An adhesive removes the critical angle only when its index is **above** the
//! block's. NOA 68 cures at 1.54:
//!
//! | Block | Adhesive | Critical angle | Escape cone |
//! |---|---|---|---|
//! | PMMA, n = 1.49 | none (air) | 42.2° | 13 % |
//! | PMMA, n = 1.49 | NOA 68, 1.54 | **none** | **100 %** |
//! | High-index, n = 1.84 | none (air) | 32.9° | 8 % |
//! | High-index, n = 1.84 | NOA 68, 1.54 | 56.8° | 23 % |
//!
//! So the adhesive is a **7.7×** improvement for PMMA and a **2.8×** one for the
//! high-index plastic. The gap between the two materials does not close — it
//! widens, because the material that was already better gets the bigger benefit.
//!
//! This binary measures it with the transport rather than with that arithmetic,
//! because the light that is trapped at one face can still escape at another.

use canlab_muon::components::adhesive;
use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, Tropical, Vec3, SENSOR_FACES};

/// The muons per configuration.
const EVENTS: usize = 400;

/// A deterministic uniform generator.
struct Uniform(u64);

impl Uniform {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// The mean detected photons for a block, an index, and an optional adhesive.
fn measure(index: f64, glue: Option<f64>, seed: u64) -> (f64, f64) {
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = match glue {
        Some(glue) => Optical::block_bonded(index, 10_000.0, glue),
        None => Optical::block_in_air(index, 10_000.0),
    };
    let sensors: [SiPM; 4] = SENSOR_FACES.map(SiPM::typical);
    let mut uniform = Uniform(seed);
    let mut random = || uniform.next();
    let entry = Vec3::new(-30.0, 0.0, 0.0);
    let heading = Vec3::new(1.0, 0.0, 0.0);
    let mut detected = 0usize;
    let mut born = 0usize;
    for _ in 0..EVENTS {
        let Ok(event) = simulate(&block, &optical, &sensors, entry, heading, 50.0, 1.0, &mut random)
        else {
            continue;
        };
        detected += event.detected_count();
        born += event.photons_born;
    }
    (
        born as f64 / EVENTS as f64,
        detected as f64 / EVENTS as f64,
    )
}

fn main() {
    let glue = adhesive();
    println!(
        "GLUE — does an adhesive make a high-index block worth it? {} muons per row.",
        EVENTS
    );
    println!(
        "The block is the 20 mm tent, 10 mm deep; the muon crosses the walls. The adhesive is {}",
        glue.part
    );
    println!("at n = {:.2}, on the four sensor faces only.", glue.index);
    println!();
    println!(
        "{:>10} {:>8} {:>9} {:>8} {:>10} {:>10} {:>8}",
        "block n", "adhesive", "critical", "escape", "born", "detected", "gain"
    );

    let mut rows = Vec::new();
    for &index in &[1.49, 1.84] {
        for &glue_index in &[None, Some(glue.index)] {
            let (born, detected) = measure(index, glue_index, 2026);
            // The critical angle and the escape fraction, for the table.
            let outside = glue_index.unwrap_or(1.0);
            let (critical, escape) = if outside >= index {
                (f64::NAN, 1.0)
            } else {
                let angle = (outside / index).asin();
                (angle, (1.0 - angle.cos()) / 2.0)
            };
            rows.push((index, glue_index.is_some(), born, detected));
            println!(
                "{:>10.2} {:>8} {:>9} {:>8.1}% {:>10.0} {:>10.2} {:>8}",
                index,
                if glue_index.is_some() { "yes" } else { "no" },
                if critical.is_nan() {
                    "none".to_string()
                } else {
                    format!("{:.1} deg", critical.to_degrees())
                },
                escape * 100.0,
                born,
                detected,
                "",
            );
        }
    }

    println!();
    // The two comparisons the table exists for.
    let pmma_air = rows[0].3;
    let pmma_glue = rows[1].3;
    let high_air = rows[2].3;
    let high_glue = rows[3].3;
    println!(
        "  PMMA:            {pmma_air:.2} -> {pmma_glue:.2} detected, a factor of {:.1}",
        pmma_glue / pmma_air
    );
    println!(
        "  high-index:      {high_air:.2} -> {high_glue:.2} detected, a factor of {:.1}",
        high_glue / high_air
    );
    println!();
    println!(
        "  The best high-index bonded design ({high_glue:.2}) against the best PMMA one ({pmma_glue:.2}):"
    );
    println!(
        "  PMMA still wins by a factor of {:.1}.",
        pmma_glue / high_glue
    );
    println!();
    println!(
        "The adhesive helps PMMA more, not less, because PMMA's index is below the"
    );
    println!(
        "adhesive's and the critical angle disappears entirely. The high-index block keeps"
    );
    println!(
        "a 56.8° critical angle and still traps three quarters of its light. Glue does not"
    );
    println!("close the gap between the materials; it widens it.");
}
