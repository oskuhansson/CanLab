//! What the sensor's area does to the light budget.
//!
//! ```text
//! cargo run --release -p canlab-muon --bin sensor_study
//! ```
//!
//! The coverge is the sensor's area over the face's, or a light guide's measured
//! efficiency. The three options the design has:
//!
//! * the component list's 1 mm² part, bare;
//! * the same part behind a PMMA light guide;
//! * a 6 × 6 mm part, bare.

use canlab_muon::timing_study::{
    simulate_one_realistic_with_sensor, Shape, SensorModel, TimingDesign,
};
use canlab_muon::{MuonFlux, Readout, Rng};

fn design() -> TimingDesign {
    TimingDesign {
        name: "hexagon",
        scale_mm: 28.0,
        thickness_mm: 30.0,
        index: 1.49,
        pde: 0.35,
        shape: Shape::Hexagon,
    }
}

fn run(design: &TimingDesign, sensor: SensorModel, events: usize, seed: u64) -> (f64, [f64; 4], f64, f64) {
    let readout = Readout::nominal();
    let mut rng = Rng::new(seed);
    let mut counts = [0usize; 4];
    let mut photons = 0usize;
    let mut timed = 0usize;
    let mut seen = 0usize;
    for _ in 0..events {
        let zenith = MuonFlux::measured().sample_zenith(rng.uniform());
        let azimuth = rng.range(0.0, std::f64::consts::TAU);
        let Some(outcome) = simulate_one_realistic_with_sensor(
            design, sensor, &readout, zenith, azimuth, &mut rng,
        ) else {
            continue;
        };
        seen += 1;
        photons += outcome.photons;
        timed += outcome.timed;
        for index in 0..4 {
            counts[index] += outcome.counts[index];
        }
    }
    let mean = |value: usize| value as f64 / seen as f64;
    (
        mean(photons),
        [mean(counts[0]), mean(counts[1]), mean(counts[2]), mean(counts[3])],
        mean(timed),
        seen as f64,
    )
}

fn main() {
    println!("The light budget against the sensor's area\n");

    let design = design();
    let block = design.block().expect("a block");
    println!("design: {:.0} mm hexagon, {:.0} mm deep, n = {:.2}", design.scale_mm, design.thickness_mm, design.index);
    let mut faces = Vec::new();
    for face in [canlab_optics::Face::Top, canlab_optics::Face::Bottom, canlab_optics::Face::Side(0), canlab_optics::Face::Side(2)] {
        faces.push((face, block.face_area(face)));
    }
    for (face, area) in &faces {
        println!("  {:?}: {area:.0} mm²", face);
    }
    println!();

    let models = [
        ("1 mm² bare", SensorModel::microfc_10035()),
        ("1 mm² + taper", SensorModel::guided(0.75)),
        ("3 x 3 mm bare", SensorModel::three_mm()),
        ("3 x 3 mm + taper", SensorModel::three_mm_guided()),
        ("6 x 6 mm bare", SensorModel::six_mm()),
        ("6 x 6 mm + taper", SensorModel::six_mm_guided()),
    ];

    println!("{:22} {:>9} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9} {:>8}",
        "sensor", "photons", "s0", "s1", "s2", "s3", "timed", "coverage", "jitter");
    println!("{}", "-".repeat(94));
    for (name, sensor) in models {
        let (photons, counts, timed, _seen) = run(&design, sensor, 300, 11);
        let coverage = match sensor.guide_efficiency {
            Some(efficiency) => format!("{efficiency:.3} (guide)"),
            None => {
                let side = sensor.area_mm2 / faces[2].1;
                format!("{side:.4} side")
            }
        };
        println!("{name:22} {photons:9.1} {:8.2} {:8.2} {:8.2} {:8.2} {timed:9.2} {coverage:>9} {:6.1} ps",
            counts[0], counts[1], counts[2], counts[3], sensor.jitter_ps());
    }

    println!();
    println!("coverage is the fraction of a face's escaping light the sensor");
    println!("registers. A bare 1 mm² part on an 840 mm² side face sees 0.12 %;");
    println!("a recessed taper multiplies that by the etendue ratio, 3.87×, and");
    println!("no more — a passive element cannot concentrate past the light's own");
    println!("etendue, and a guide outside the block would see the light after it");
    println!("had refracted into the full hemisphere and could not concentrate at");
    println!("all. That is why the taper has to be a cavity in the PMMA.");
}
