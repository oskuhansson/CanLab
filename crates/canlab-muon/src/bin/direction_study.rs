//! Can four SiPMs time a muon, and what would instead?
//!
//! ```text
//! cargo run --release -p canlab-muon --bin direction_study
//! ```
//!
//! Two observables, one table each, and the comparison is the result:
//!
//! * **Timing.** The Cramér–Rao bound on the heading from four arrival times,
//!   for every geometry that fits in the can. The columns are the geometry, the
//!   light it collects, and the bound in degrees. A bound larger than 180° is a
//!   direction that is not measured, and the table's last column says by how
//!   many factors it misses.
//! * **Position.** The angular uncertainty of a line through two hit points, for
//!   a range of pixel sizes. This is the observable that carries a direction,
//!   and the table is what the next design should be built around.
//!
//! The photon counts come from `canlab_optics::detector::simulate` — the real
//! transport, not a formula — so the "does it even fire" column is the same
//! physics the event model uses. The bound is geometry and jitter only, which is
//! why it is printed next to the count rather than derived from it: the count
//! decides whether there *is* a timestamp, and the bound decides what a timestamp
//! would be worth if there were.

use canlab_muon::bound::{position_bound, timing_bound};
use canlab_muon::can::{tent_mass_g, Can};
use canlab_muon::components::{budget, total_jitter_ps, UNMODELLED};
use canlab_muon::mission::{Candidate, Mission, CAN_DIAMETER_MM, CAN_HEIGHT_MM, TOTAL_BUDGET_EUR};
use canlab_muon::timing::{speed_mm_per_ps, timing_signal_ps};
use canlab_optics::detector::simulate;
use canlab_optics::{
    Block, Optical, SiPM, Tropical, Vec3, SENSOR_FACES,
};

/// How many muons per geometry the photon count is averaged over.
const EVENTS: usize = 200;

/// The readout's total random jitter, picoseconds.
///
/// The quadrature of the four parts in [`canlab_muon::components`], not the
/// placeholder `SiPM::typical` carries: the sensor's 42.5 ps and the timer's
/// 20 ps are the two terms, and using one part's number for the whole chain is
/// how a budget stops being a budget. A part two times better would move every
/// row of the timing table by a factor of two and change no conclusion, which is
/// the point of printing the ratio.
fn jitter_ps() -> f64 {
    total_jitter_ps()
}

/// A deterministic uniform generator, so a rerun is the same table.
struct Uniform(u64);

impl Uniform {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// What one geometry gives: the light, the timestamps, and the bound.
struct Row {
    scale_mm: f64,
    thickness_mm: f64,
    index: f64,
    mass_g: f64,
    detected: f64,
    timed: f64,
    signal_ps: f64,
    bound_deg: f64,
}

fn measure(scale_mm: f64, thickness_mm: f64, index: f64, seed: u64) -> Option<Row> {
    let block = Block::new(Tropical::fits_in(scale_mm, thickness_mm).ok()?);
    let optical = Optical::block_in_air(index, 10_000.0);
    let sensors: [SiPM; 4] = SENSOR_FACES.map(SiPM::typical);
    let positions = block.sensor_positions();
    let speed = speed_mm_per_ps(index)?;

    // A muon across the block, through the walls: the orientation the sweep
    // found best, and the one a block mounted on edge sees.
    let entry = Vec3::new(-scale_mm * 1.5, 0.0, 0.0);
    let heading = Vec3::new(1.0, 0.0, 0.0);
    let length = scale_mm * 0.85;

    let mut uniform = Uniform(seed);
    let mut random = || uniform.next();
    let mut detected = 0usize;
    let mut timed = 0usize;
    let mut events = 0usize;
    for _ in 0..EVENTS {
        let Ok(event) = simulate(&block, &optical, &sensors, entry, heading, length, 1.0, &mut random)
        else {
            continue;
        };
        events += 1;
        detected += event.detected_count();
        timed += event.sensors_with_a_photon();
    }
    if events == 0 {
        return None;
    }

    // The bound is worst case over the heading, because a muon arrives from
    // anywhere: the direction along the block's long axis is the one with the
    // shortest lever arm, and averaging over headings would hide it.
    let mut worst = 0.0f64;
    for heading in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
    ] {
        let heading = heading.normalized()?;
        let bound = timing_bound(&positions, heading, jitter_ps(), speed)?;
        worst = worst.max(bound.sigma_deg());
    }
    // The signal is the best case, so the ratio is the most generous one.
    let mut signal = 0.0f64;
    for heading in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
    ] {
        let heading = heading.normalized()?;
        signal = signal.max(timing_signal_ps(&positions, heading, speed)?);
    }

    Some(Row {
        scale_mm,
        thickness_mm,
        index,
        mass_g: tent_mass_g(scale_mm, thickness_mm, 1.19)?,
        detected: detected as f64 / events as f64,
        timed: timed as f64 / events as f64,
        signal_ps: signal,
        bound_deg: worst,
    })
}

fn main() {
    // The mission's own numbers first, so every table below is read against the
    // envelope it has to fit rather than against an assumption.
    let mission = Mission::standard();
    println!(
        "MISSION — can {CAN_HEIGHT_MM:.0} x D{CAN_DIAMETER_MM:.0} mm, mass {:.0}-{:.0} g, budget {TOTAL_BUDGET_EUR:.0} EUR, \
         detector {:.0}-{:.0} EUR, CNC {}",
        mission.total_mass_g.0,
        mission.total_mass_g.1,
        mission.detector_budget_eur.0,
        mission.detector_budget_eur.1,
        if mission.manufacturing.cnc { "available" } else { "not available" },
    );
    println!("\nJITTER BUDGET — the four parts, picoseconds");
    println!("{:>34} {:>8} {:>9}  source", "part", "ps", "random");
    for contribution in budget() {
        println!(
            "{:>34} {:>8.1} {:>9}  {}",
            contribution.part,
            contribution.ps,
            if contribution.random { "yes" } else { "no" },
            contribution.source,
        );
    }
    println!("{:>34} {:>8.1}  (quadrature of the random terms)", "total", total_jitter_ps());
    println!("\nNot modelled, and each would move a number:");
    for entry in UNMODELLED {
        println!("  - {entry}");
    }
    println!();

    let can = Can::standard();
    println!(
        "The can: {:.0} mm tall, Ø{:.0} mm, {:.1} cm² of cross-section after {:.0} mm of allowance.",
        can.height_mm,
        can.radius_mm * 2.0,
        can.cross_section_mm2() / 100.0,
        can.wall_mm,
    );
    println!(
        "The largest tent that fits at 10 mm deep is {:.2} mm of scale.\n",
        can.largest_scale_mm(10.0).unwrap_or(f64::NAN),
    );

    println!(
        "TIMING — the Cramér–Rao bound on the heading from four arrival times, {:.1} ps jitter",
        jitter_ps(),
    );
    println!(
        "{:>6} {:>5} {:>5} {:>7} {:>9} {:>6} {:>9} {:>12} {:>8}",
        "scale", "deep", "n", "mass g", "detected", "timed", "signal ps", "bound deg", "turns"
    );
    for &scale in &[15.0, 20.0, 25.0, 26.0, 30.0] {
        for &thickness in &[5.0, 10.0] {
            for &index in &[1.49, 1.84] {
                let fits = can.admits(scale, thickness).is_ok();
                let Some(row) = measure(scale, thickness, index, 2026) else {
                    continue;
                };
                // The bound divided by a full turn: how many complete circles
                // the uncertainty spans. One turn is already no resolution, so
                // this column is the count of factors by which the geometry
                // misses being a detector.
                let turns = row.bound_deg / 360.0;
                println!(
                    "{:>6.1} {:>5.1} {:>5.2} {:>7.1} {:>9.2} {:>6.2} {:>9.3} {:>12.0} {:>8.0e}{}",
                    row.scale_mm,
                    row.thickness_mm,
                    row.index,
                    row.mass_g,
                    row.detected,
                    row.timed,
                    row.signal_ps,
                    row.bound_deg,
                    turns,
                    if fits { "" } else { "  DOES NOT FIT" },
                );
            }
        }
    }

    println!("\nPOSITION — the angular uncertainty of a line through two hit points");
    println!("{:>10} {:>12} {:>10}", "pixel mm", "separation mm", "bound deg");
    for &pixel in &[0.05, 0.1, 0.5, 1.0, 2.0] {
        for &separation in &[40.0, 54.1, 90.0] {
            let Some(bound) = position_bound(pixel, separation) else {
                continue;
            };
            println!("{:>10.2} {:>12.1} {:>10.2}", pixel, separation, bound.sigma_deg());
        }
    }

    // The comparison, in one sentence, from the two numbers the tables contain.
    // The heading sweep is the same one `measure` uses, so the bound quoted here
    // is the bound in the 20 mm / 10 mm / 1.49 row and not a different number
    // computed a different way.
    let positions = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits")).sensor_positions();
    let speed = speed_mm_per_ps(1.49).expect("a speed");
    let mut signal = 0.0f64;
    let mut timing = 0.0f64;
    for heading in [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
    ] {
        let heading = heading.normalized().expect("a direction");
        signal = signal.max(timing_signal_ps(&positions, heading, speed).expect("a signal"));
        timing = timing.max(timing_bound(&positions, heading, jitter_ps(), speed).expect("a bound").sigma_deg());
    }
    let position = position_bound(1.0, 54.1).expect("a bound");
    // The audit turns the numbers into a verdict: which requirements the design
    // point breaks, with the measured value and the limit.
    let violations = mission.audit(&Candidate::design());
    println!("\nAUDIT — the design point against the mission's requirements");
    if violations.is_empty() {
        println!("  passes every requirement");
    } else {
        for violation in &violations {
            println!("  FAILS {violation}");
        }
    }
    println!(
        "\nThe design block, one number each: the timing signal is at most {signal:.2} ps across \
         the whole detector against {:.1} ps of readout jitter, so the heading's bound is \
         {:.0} degrees; a millimetre of pixel over 54 mm gives {:.1} degrees.",
        total_jitter_ps(),
        timing,
        position.sigma_deg(),
    );
}
