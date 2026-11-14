//! The Cherenkov timing design with a realistic readout.
//!
//! ```text
//! cargo run --release -p canlab-muon --bin timing_realistic
//! ```
//!
//! The earlier study measured the design with an ideal readout: the first photon
//! at each sensor, no dark counts, no crosstalk, no walk. That is what a
//! threshold at half a photoelectron would record, and it is the best case. This
//! binary puts the real readout in the way — a threshold that needs more than one
//! photon, dark counts that fire first, crosstalk that changes the count, and a
//! comparator walk that shifts the timestamp — and measures what is left.
//!
//! The columns are the same as the design table's, so the two can be read
//! together. The "readout" column names the case: ideal, nominal, conservative.

use canlab_muon::timing_study::{run, run_realistic, Shape, TimingDesign};
use canlab_muon::Readout;

fn main() {
    println!("The Cherenkov timing design, with the readout in the way\n");
    println!(
        "The ideal case is the first photon at each sensor. The nominal case is a"
    );
    println!("half-photoelectron threshold, 30 kHz dark counts and a 100 ns window.");
    println!("The conservative case is a 2 p.e. threshold and 96 kHz over 200 ns.\n");

    let design = TimingDesign::design();
    let ideal = Readout::ideal();
    let nominal = Readout::nominal();
    let conservative = Readout::conservative();

    println!(
        "{:<14} {:>6} {:>7} {:>9} {:>9} {:>9} {:>9}",
        "readout", "timed", "full", "median", "p68", "within 30", "bound"
    );
    println!("{}", "-".repeat(70));

    let runs = [
        ("ideal", run(&design, 2_000, 1)),
        ("nominal", run_realistic(&design, &nominal, 2_000, 1)),
        ("conservative", run_realistic(&design, &conservative, 2_000, 1)),
    ];
    for (name, result) in &runs {
        println!(
            "{:<14} {:>6.2} {:>7.2} {:>9} {:>9} {:>9} {:>9}",
            name,
            result.mean_timed,
            result.full_fraction,
            format_angle(result.median_error_deg),
            format_angle(result.p68_error_deg),
            format_fraction(result.within_30),
            format_angle(result.bound_deg),
        );
    }

    println!("\nThe readout's price");
    println!("{}", "-".repeat(60));
    let ideal_median = runs[0].1.median_error_deg;
    for (name, result) in &runs[1..] {
        match (ideal_median, result.median_error_deg) {
            (Some(ideal), Some(real)) => {
                println!(
                    "  {name}: {:.1}° against {:.1}° ideal — a factor of {:.2}",
                    real,
                    ideal,
                    real / ideal
                );
            }
            _ => println!("  {name}: no reconstruction"),
        }
    }

    println!("\nThe design, iterated against the readout");
    println!("{}", "-".repeat(60));
    println!(
        "{:<22} {:>6} {:>7} {:>9} {:>9}",
        "design", "timed", "full", "median", "p68"
    );
    let candidates = [
        ("20 mm tent, 10 mm", TimingDesign::design()),
        (
            "24 mm tent, 20 mm",
            TimingDesign { name: "large", scale_mm: 24.0, thickness_mm: 20.0, index: 1.49, pde: 0.35, shape: Shape::Tent },
        ),
        (
            "16 mm tent, 10 mm",
            TimingDesign { name: "small", scale_mm: 16.0, thickness_mm: 10.0, index: 1.49, pde: 0.35, shape: Shape::Tent },
        ),
        (
            "20 mm tent, 20 mm",
            TimingDesign { name: "deep", scale_mm: 20.0, thickness_mm: 20.0, index: 1.49, pde: 0.35, shape: Shape::Tent },
        ),
        (
            "28 mm hexagon, 10 mm",
            TimingDesign { name: "hex", scale_mm: 28.0, thickness_mm: 10.0, index: 1.49, pde: 0.35, shape: Shape::Hexagon },
        ),
        (
            "28 mm hexagon, 20 mm",
            TimingDesign { name: "hex-deep", scale_mm: 28.0, thickness_mm: 20.0, index: 1.49, pde: 0.35, shape: Shape::Hexagon },
        ),
    ];
    for (name, candidate) in &candidates {
        let result = run_realistic(candidate, &nominal, 2_000, 1);
        println!(
            "{:<22} {:>6.2} {:>7.2} {:>9} {:>9}",
            name,
            result.mean_timed,
            result.full_fraction,
            format_angle(result.median_error_deg),
            format_angle(result.p68_error_deg),
        );
    }

    println!("\nWhat this means");
    println!("{}", "-".repeat(60));
    println!("The readout's price is the gap between the ideal and nominal rows.");
    println!("The design's accuracy is the median, and the fraction that fires all");
    println!("four sensors is the efficiency. A design that fires rarely but");
    println!("accurately and one that fires often but coarsely are different");
    println!("instruments, and the table shows both.");
}

fn format_angle(angle: Option<f64>) -> String {
    angle.map(|value| format!("{value:.1}°")).unwrap_or_else(|| "-".to_string())
}

fn format_fraction(fraction: f64) -> String {
    format!("{:.1}%", fraction * 100.0)
}
