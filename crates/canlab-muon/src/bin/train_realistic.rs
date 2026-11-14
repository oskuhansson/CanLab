//! Train the network on what the instrument actually records.
//!
//! ```text
//! cargo run --release -p canlab-muon --bin train_realistic
//! ```
//!
//! The earlier training used the **ideal** timestamps: the first photon at each
//! sensor, no dark counts, no crosstalk, no walk. This binary trains on the
//! **realistic** timestamps — the threshold's k-th photon, a dark count that can
//! fire first, crosstalk, and the comparator's walk — and measures what survives.
//!
//! The criterion is the mission's: **30° is acceptable**. So the number that
//! matters is not the median but the fraction of events reconstructed within 30°,
//! because a design that is usually 20° and occasionally 150° is not the same as
//! one that is always 25°.

use canlab_muon::domain::Domain;
use canlab_muon::net::{Network, Sample};
use canlab_muon::timing_study::{training_sample, training_sample_realistic, Shape, TimingDesign};
use canlab_muon::{Readout, Rng};

/// How many events to train on.
const TRAINING: usize = 8_000;
/// How many to test on.
const TESTING: usize = 1_500;
/// Epochs over the training set.
const EPOCHS: usize = 60;
/// Mini-batch size.
const BATCH: usize = 64;
/// Adam's learning rate.
const LEARNING_RATE: f64 = 0.003;
/// Hidden units per layer.
const HIDDEN: usize = 64;

/// The angle between a prediction and the truth, degrees.
fn angle_deg(predicted: [f64; 2], truth: [f64; 2]) -> f64 {
    let to_unit = |value: [f64; 2]| -> Option<[f64; 3]> {
        let squared = value[0] * value[0] + value[1] * value[1];
        let z = (1.0 - squared).max(0.0).sqrt();
        let length = (squared + z * z).sqrt();
        (length > 1e-12).then(|| [value[0] / length, value[1] / length, -z / length])
    };
    let (Some(a), Some(b)) = (to_unit(predicted), to_unit(truth)) else {
        return f64::NAN;
    };
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

/// The summary of a set of errors.
struct Summary {
    median: f64,
    p68: f64,
    within_30: f64,
    within_60: f64,
}

/// Summarise a set of errors.
fn summarise(mut errors: Vec<f64>) -> Summary {
    errors.sort_by(f64::total_cmp);
    let n = errors.len();
    if n == 0 {
        return Summary { median: f64::NAN, p68: f64::NAN, within_30: 0.0, within_60: 0.0 };
    }
    let at = |fraction: f64| {
        let index = ((n as f64 - 1.0) * fraction).round() as usize;
        errors[index.min(n - 1)]
    };
    let within = |limit: f64| errors.iter().filter(|e| **e <= limit).count() as f64 / n as f64;
    Summary { median: at(0.5), p68: at(0.68), within_30: within(30.0), within_60: within(60.0) }
}

/// Generate a training set with the chosen readout.
fn generate(
    design: &TimingDesign,
    domain: &Domain,
    readout: &Readout,
    realistic: bool,
    count: usize,
    seed: u64,
) -> Vec<Sample> {
    let mut rng = Rng::new(seed);
    let mut samples = Vec::with_capacity(count);
    let mut attempts = 0usize;
    while samples.len() < count && attempts < count * 20 {
        attempts += 1;
        let (light, _geometry) = domain.sample(&canlab_muon::Tracker::short(), &mut rng);
        let varied = TimingDesign {
            name: design.name,
            scale_mm: design.scale_mm,
            thickness_mm: design.thickness_mm,
            index: design.index,
            pde: light.pde,
            shape: design.shape,
        };
        let sample = if realistic {
            training_sample_realistic(&varied, readout, &mut rng)
        } else {
            training_sample(&varied, &mut rng)
        };
        if let Some(sample) = sample {
            samples.push(sample);
        }
    }
    samples
}

/// Train and report one configuration.
fn run_configuration(
    name: &str,
    design: &TimingDesign,
    domain: &Domain,
    readout: &Readout,
    realistic: bool,
) -> Summary {
    let training = generate(design, domain, readout, realistic, TRAINING, 1);
    let testing = generate(design, domain, readout, realistic, TESTING, 2);
    let inputs = training.first().map(|s| s.features.len()).unwrap_or(0);
    let mut rng = Rng::new(3);
    let mut network = Network::new(inputs, HIDDEN, &mut rng);
    let curve = network.train(&training, EPOCHS, BATCH, LEARNING_RATE, &mut rng);
    let errors: Vec<f64> = testing
        .iter()
        .map(|sample| {
            let predicted = network.predict(&sample.features);
            angle_deg([predicted[0], predicted[1]], sample.target)
        })
        .filter(|error| error.is_finite())
        .collect();
    let summary = summarise(errors);
    println!(
        "{:<22} {:>8.1}° {:>8.1}° {:>9.1}% {:>9.1}% {:>9.1}%",
        name,
        summary.median,
        summary.p68,
        summary.within_30 * 100.0,
        summary.within_60 * 100.0,
        curve.last().copied().unwrap_or(f64::NAN)
    );
    summary
}

/// The design to train on, from `--scale`, `--thickness` and `--index`, defaulting
/// to the study's design point.
fn design_from_args() -> TimingDesign {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str, default: f64| -> f64 {
        args.iter()
            .position(|value| value == flag)
            .and_then(|index| args.get(index + 1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let base = TimingDesign::design();
    TimingDesign {
        name: "cli",
        scale_mm: get("--scale", base.scale_mm),
        thickness_mm: get("--thickness", base.thickness_mm),
        index: get("--index", base.index),
        pde: base.pde,
        shape: if args.iter().any(|value| value == "--hexagon") {
            Shape::Hexagon
        } else {
            base.shape
        },
    }
}

fn main() {
    println!("The network on what the instrument actually records\n");
    println!(
        "The mission's criterion is 30°: the fraction within 30° is the number that"
    );
    println!("matters, not the median. A design that is usually 20° and sometimes");
    println!("150° is not the same as one that is always 25°.\n");

    let design = design_from_args();
    println!(
        "design: {:.0} mm tent, {:.0} mm deep, n = {:.2}\n",
        design.scale_mm, design.thickness_mm, design.index
    );
    let domain = Domain::fixed();
    let ideal = Readout::ideal();
    let nominal = Readout::nominal();
    let conservative = Readout::conservative();

    println!(
        "{:<22} {:>9} {:>9} {:>10} {:>10} {:>10}",
        "configuration", "median", "p68", "within 30°", "within 60°", "final loss"
    );
    println!("{}", "-".repeat(75));

    run_configuration("ideal readout", &design, &domain, &ideal, false);
    run_configuration("nominal readout", &design, &domain, &nominal, true);
    run_configuration("conservative readout", &design, &domain, &conservative, true);

    println!("\nWhat this means");
    println!("{}", "-".repeat(75));
    println!("The ideal row is the control: the network on the first photon.");
    println!("The nominal row is what a real build delivers: a threshold, dark");
    println!("counts, crosstalk and walk. The conservative row is the readout");
    println!("with a 2 p.e. threshold and 96 kHz dark counts.");
    println!();
    println!("If the realistic rows keep most events within 30°, the design works");
    println!("in reality. If they do not, the readout is the problem and the");
    println!("threshold or the dark-count rate is what to change.");
}
