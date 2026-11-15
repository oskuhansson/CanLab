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
use canlab_muon::timing_study::{
    training_sample_first_only_with_sensor, training_sample_realistic_with_sensor,
    training_sample_with_sensor, default_sensor, SensorModel, Shape, TimingDesign,
};
use canlab_muon::vmf::{expected_angle, VmfNetwork, VmfSample};
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

/// What the readout is allowed to know.
#[derive(Clone, Copy, PartialEq)]
enum Features {
    /// Everything: first and second arrivals, flags, counts, mean and spread.
    Full,
    /// First and second arrivals, flags and counts — no multi-hit summary.
    FirstSecond,
    /// Four first arrival times and four flags: one discriminator, one TDC stop
    /// per sensor. The instrument the component list describes.
    FirstOnly,
}

/// Generate a training set with the chosen readout.
fn generate(
    design: &TimingDesign,
    domain: &Domain,
    readout: &Readout,
    realistic: bool,
    count: usize,
    seed: u64,
    features: Features,
    sensor: SensorModel,
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
        let sample = match features {
            // The readout cannot count, so the sample is built from the first
            // arrival and the flags alone.
            Features::FirstOnly => {
                training_sample_first_only_with_sensor(&varied, sensor, readout, &mut rng)
            }
            _ if realistic => {
                training_sample_realistic_with_sensor(&varied, sensor, readout, &mut rng)
            }
            _ => training_sample_with_sensor(&varied, sensor, &mut rng),
        };
        if let Some(mut sample) = sample {
            if features == Features::FirstSecond {
                sample.features.truncate(16);
            }
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
    features: Features,
    sensor: SensorModel,
) -> Summary {
    let training = generate(design, domain, readout, realistic, TRAINING, 1, features, sensor);
    let testing = generate(design, domain, readout, realistic, TESTING, 2, features, sensor);
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

/// The 3D unit vector a pair of horizontal components names.
///
/// The training target is the two horizontal components of a unit heading, so
/// the vertical one is not a free parameter: it is the negative root of
/// `1 − x² − y²`, because a muon comes down. Reconstructing it here is what
/// lets the probabilistic network see the same target the regressor does.
fn to_unit3(target: [f64; 2]) -> [f64; 3] {
    let squared = target[0] * target[0] + target[1] * target[1];
    let z = (1.0 - squared).max(0.0).sqrt();
    let length = (squared + z * z).sqrt();
    if length < 1e-12 {
        [0.0, 0.0, -1.0]
    } else {
        [target[0] / length, target[1] / length, -z / length]
    }
}

/// `P(θ ≤ limit)` under a von Mises–Fisher distribution of concentration `κ`.
///
/// The density of the angle from the mean is
/// `p(θ) = κ/(2 sinh κ) · e^{κ cos θ} sin θ`, and this integrates it with
/// Simpson's rule. The `κ → 0` limit is the uniform sphere, `(1 − cos θ)/2`,
/// which is the value a network that has learned nothing should produce.
fn vmf_cdf(kappa: f64, limit_rad: f64) -> f64 {
    if kappa <= 1e-6 {
        return (1.0 - limit_rad.cos()) / 2.0;
    }
    let steps = 200;
    let h = limit_rad / steps as f64;
    let density = |theta: f64| {
        kappa / (2.0 * kappa.sinh()) * (kappa * theta.cos()).exp() * theta.sin()
    };
    let mut sum = density(0.0) + density(limit_rad);
    for step in 1..steps {
        let theta = step as f64 * h;
        sum += if step % 2 == 0 { 2.0 } else { 4.0 } * density(theta);
    }
    (sum * h / 3.0).clamp(0.0, 1.0)
}

/// Train the probabilistic network on the same samples and measure whether its
/// `κ` tells the truth.
///
/// The regressor reports one number per event and no uncertainty. The
/// von Mises–Fisher head reports a mean direction **and** a concentration, so
/// the question changes from "how wrong is it" to "does it know how wrong it
/// is". A calibrated network's own `κ` predicts the error it then makes; this
/// function is the measurement that decides it.
fn run_vmf(
    design: &TimingDesign,
    domain: &Domain,
    readout: &Readout,
    features: Features,
    sensor: SensorModel,
) {
    let training = generate(design, domain, readout, true, TRAINING, 1, features, sensor);
    let testing = generate(design, domain, readout, true, TESTING, 2, features, sensor);
    let to_vmf = |samples: &[Sample]| -> Vec<VmfSample> {
        samples
            .iter()
            .map(|sample| VmfSample {
                features: sample.features.clone(),
                target: to_unit3(sample.target),
            })
            .collect()
    };
    let training_vmf = to_vmf(&training);
    let testing_vmf = to_vmf(&testing);
    let inputs = training_vmf.first().map(|s| s.features.len()).unwrap_or(0);
    let mut rng = Rng::new(3);
    let mut network = VmfNetwork::new(inputs, HIDDEN, &mut rng);
    let curve = network.train(&training_vmf, EPOCHS, BATCH, LEARNING_RATE, &mut rng);

    let mut errors = Vec::with_capacity(testing_vmf.len());
    let mut predicted_30 = Vec::with_capacity(testing_vmf.len());
    let mut predicted_angle = Vec::with_capacity(testing_vmf.len());
    let mut kappas = Vec::with_capacity(testing_vmf.len());
    let mut nll = 0.0;
    for sample in &testing_vmf {
        let (direction, kappa) = network.predict(&sample.features);
        let cosine = (direction[0] * sample.target[0]
            + direction[1] * sample.target[1]
            + direction[2] * sample.target[2])
            .clamp(-1.0, 1.0);
        errors.push(cosine.acos().to_degrees());
        predicted_30.push(vmf_cdf(kappa, 30.0f64.to_radians()));
        predicted_angle.push(expected_angle(kappa).to_degrees());
        kappas.push(kappa);
        nll += -canlab_muon::vmf::log_normalizer(kappa) - kappa * cosine;
    }
    let summary = summarise(errors.clone());
    let mean_kappa = kappas.iter().sum::<f64>() / kappas.len().max(1) as f64;
    let mean_predicted: f64 =
        predicted_angle.iter().sum::<f64>() / predicted_angle.len().max(1) as f64;
    let mean_p30: f64 = predicted_30.iter().sum::<f64>() / predicted_30.len().max(1) as f64;

    println!("The network as a distribution (von Mises–Fisher)\n");
    println!(
        "{:<22} {:>9} {:>9} {:>10} {:>10} {:>10}",
        "configuration", "median", "p68", "within 30°", "within 60°", "final loss"
    );
    println!("{}", "-".repeat(75));
    println!(
        "{:<22} {:>8.1}° {:>8.1}° {:>9.1}% {:>9.1}% {:>9.3}",
        "vMF mean direction",
        summary.median,
        summary.p68,
        summary.within_30 * 100.0,
        summary.within_60 * 100.0,
        curve.last().copied().unwrap_or(f64::NAN)
    );
    println!();
    println!("mean κ {mean_kappa:.1}, mean predicted error {mean_predicted:.1}°, mean NLL {:.2}", nll / testing_vmf.len().max(1) as f64);
    println!();
    println!("Calibration: does the predicted error match the error it makes?");
    println!("{}", "-".repeat(75));
    println!("{:<22} {:>12} {:>12} {:>10}", "bin (by predicted)", "predicted", "observed", "ratio");
    println!("{}", "-".repeat(75));
    let mut order: Vec<usize> = (0..testing_vmf.len()).collect();
    order.sort_by(|a, b| predicted_angle[*a].total_cmp(&predicted_angle[*b]));
    let bins = 4;
    for bin in 0..bins {
        let start = bin * order.len() / bins;
        let end = (bin + 1) * order.len() / bins;
        let slice = &order[start..end];
        if slice.is_empty() {
            continue;
        }
        let predicted: f64 =
            slice.iter().map(|index| predicted_angle[*index]).sum::<f64>() / slice.len() as f64;
        let mut actual: Vec<f64> = slice.iter().map(|index| errors[*index]).collect();
        actual.sort_by(f64::total_cmp);
        let observed = actual[actual.len() / 2];
        println!(
            "{:<22} {:>11.1}° {:>11.1}° {:>10.2}",
            format!("{}-{} %", bin * 100 / bins, (bin + 1) * 100 / bins),
            predicted,
            observed,
            observed / predicted.max(1e-9)
        );
    }
    println!();
    println!(
        "P(θ ≤ 30°): predicted {:.1} % by the network's own κ, observed {:.1} %",
        mean_p30 * 100.0,
        summary.within_30 * 100.0
    );
    println!();
    println!("A ratio near 1 in every bin is a calibrated network: the κ it");
    println!("reports is the width of the error it actually makes. A ratio that");
    println!("rises across the bins means the network is over-confident on the");
    println!("events it thinks are easy and under-confident on the rest.");
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

    let args: Vec<String> = std::env::args().collect();
    let sensor = if let Some(index) = args.iter().position(|argument| argument == "--guide") {
        let efficiency = args.get(index + 1).and_then(|value| value.parse().ok()).unwrap_or(0.6);
        SensorModel::guided(efficiency)
    } else if args.iter().any(|argument| argument == "--six-mm") {
        SensorModel::six_mm_guided()
    } else if args.iter().any(|argument| argument == "--three-mm") {
        SensorModel::three_mm_guided()
    } else if args.iter().any(|argument| argument == "--one-mm") {
        SensorModel::guided(0.75)
    } else {
        default_sensor()
    };
    let features = if std::env::args().any(|argument| argument == "--first-only") {
        Features::FirstOnly
    } else if std::env::args().any(|argument| argument == "--first-second-only") {
        Features::FirstSecond
    } else {
        Features::Full
    };
    let design = design_from_args();
    let shape = match design.shape {
        Shape::Tent => "tent",
        Shape::Hexagon => "hexagon",
    };
    println!(
        "design: {:.0} mm {shape}, {:.0} mm deep, n = {:.2}\n",
        design.scale_mm, design.thickness_mm, design.index
    );
    let domain = Domain::fixed();
    let ideal = Readout::ideal();
    let nominal = Readout::nominal();
    let conservative = Readout::conservative();

    // The probabilistic head is a separate question from the regressor's table,
    // and it costs a second training, so it is a flag rather than another row.
    if args.iter().any(|argument| argument == "--vmf") {
        run_vmf(&design, &domain, &nominal, features, sensor);
        return;
    }

    println!(
        "{:<22} {:>9} {:>9} {:>10} {:>10} {:>10}",
        "configuration", "median", "p68", "within 30°", "within 60°", "final loss"
    );
    println!("{}", "-".repeat(75));

    run_configuration("ideal readout", &design, &domain, &ideal, false, features, sensor);
    run_configuration("nominal readout", &design, &domain, &nominal, true, features, sensor);
    run_configuration("conservative readout", &design, &domain, &conservative, true, features, sensor);

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
