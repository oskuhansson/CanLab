//! What the four-sensor detector can actually deliver, measured over a sweep.
//!
//! ```text
//! cargo run --release -p canlab-optics --bin muon_study
//! ```
//!
//! The question the design has to answer before anything is built: **can four
//! SiPMs time a muon?** A muon crosses the block along an axis, in two
//! orientations — up the can's `z` axis, so it crosses the thin faces the
//! sensors sit on, and across the block, so it crosses the walls. The first is
//! the one a CanSat sees if the block's large face is horizontal; the second if
//! the block is mounted on edge.
//!
//! Columns:
//!
//! * `photons` — Cherenkov photons the muon produced;
//! * `detected` — photons that reached a sensor, after Fresnel, absorption, the
//!   sensor's coverage of its face and its PDE;
//! * `empty%` — the fraction of the four sensors that saw nothing. This is the
//!   number that decides the mission: a sensor with no photon has no timestamp;
//! * `timed` — how many of the four sensors had at least one photon. One is no
//!   direction, two is a line, three is a point;
//! * `trapped%` — the fraction of photons lost to total internal reflection
//!   inside the block. A high block's light is trapped *inside* it, and only
//!   the small square a sensor occupies can see it: Fresnel out of the faces is
//!   not the path the mission depends on.
//! * `spread ps` — the spread of arrival times within one sensor. Above the
//!   sensor's 100 ps jitter, the sensor cannot resolve the geometry; below it,
//!   the geometry does not matter.
//!
//! # The numbers below are the second set
//!
//! The first run of this table reported `detected 0.00` and `timed 0.00` for a
//! vertical muon at every geometry, and `trapped 96 %`: a tidy story about
//! geometry that was almost entirely a set of bugs. The event was empty (every
//! photon's birth point was clipped to nothing), the photon could not leave
//! through a face it had reflected off, and the bottom face was a plane at the
//! wrong height, so a downward photon found no exit. The table now says what
//! the detector does: a vertical muon reaches a sensor sometimes, an across one
//! reaches one about half the time, and the loss is dominated by geometry (the
//! sensors cover 15 % of their faces) rather than by total internal reflection.
//!
//! Every one of those bugs was found by asking for a *number of photons*, not
//! by looking at the table and finding the story convincing.
//!
//! # `--waveform`: the picture a timing study is read from
//!
//! ```text
//! cargo run --release -p canlab-optics --bin muon_study -- --waveform
//! ```
//!
//! Bins one event's arrival times into picoseconds, per sensor and in total,
//! prints the waveform as a histogram of picosecond bins, and writes the curve
//! as an SVG next to the binary's working directory. The numbers under the
//! picture are the ones a fit is made from: the peak bin, the mean, and how
//! many photons were binned versus how many were outside the window — because a
//! waveform whose bars sum to less than its photons is a window that is wrong,
//! and it looks exactly like a window that is right.

use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, TimeBase, Vec3, Waveforms};

const EVENTS: usize = 300;

struct Uniform(u64);

impl Uniform {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

struct Row {
    radius_mm: f64,
    thickness_mm: f64,
    index: f64,
    vertical: bool,
    photons: f64,
    detected: f64,
    empty: f64,
    timed: f64,
    trapped: f64,
    spread_ps: f64,
}

fn measure(radius_mm: f64, thickness_mm: f64, index: f64, vertical: bool, seed: u64) -> Option<Row> {
    let block = Block::new(canlab_optics::Tropical::fits_in(radius_mm, thickness_mm).ok()?);
    let optical = Optical::block_in_air(index, 10_000.0);
    let faces = [
        canlab_optics::block::Face::Top,
        canlab_optics::block::Face::Bottom,
        canlab_optics::block::Face::Side(0),
        canlab_optics::block::Face::Side(2),
    ];
    let sensors: [SiPM; 4] = faces.map(SiPM::typical);
    let mut uniform = Uniform(seed);
    let mut random = || uniform.next();
    let mut photons = 0usize;
    let mut detected = 0usize;
    let mut empty = 0usize;
    let mut timed = 0usize;
    let mut trapped = 0usize;
    let mut spread = 0.0;
    let mut spread_n = 0usize;
    for _ in 0..EVENTS {
        // A path along the block's axis. Vertically: through the thin faces,
        // which is what a block lying flat sees. Across: through the walls.
        let (entry, heading, length) = if vertical {
            (Vec3::new(0.0, 0.0, -thickness_mm * 2.0), Vec3::new(0.0, 0.0, 1.0), thickness_mm * 1.5)
        } else {
            (Vec3::new(-radius_mm * 1.5, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), radius_mm * 0.85)
        };
        let Ok(event) = simulate(&block, &optical, &sensors, entry, heading, length, 1.0, &mut random)
        else {
            continue;
        };
        photons += event.photons_born;
        trapped += event.trapped;
        detected += event.detected_count();
        let mut this_event = 0;
        for times in &event.true_ps {
            if times.is_empty() {
                empty += 1;
            } else {
                this_event += 1;
            }
            if times.len() >= 2 {
                let mean = times.iter().sum::<f64>() / times.len() as f64;
                let variance = times.iter().map(|t| (t - mean) * (t - mean)).sum::<f64>()
                    / (times.len() - 1) as f64;
                spread += variance.sqrt();
                spread_n += 1;
            }
        }
        timed += this_event;
    }
    Some(Row {
        radius_mm,
        thickness_mm,
        index,
        vertical,
        photons: photons as f64 / EVENTS as f64,
        detected: detected as f64 / EVENTS as f64,
        empty: empty as f64 / (EVENTS * 4) as f64,
        timed: timed as f64 / EVENTS as f64,
        trapped: trapped as f64 / photons.max(1) as f64,
        spread_ps: if spread_n > 0 { spread / spread_n as f64 } else { 0.0 },
    })
}

fn main() {
    if std::env::args().any(|argument| argument == "--waveform") {
        waveform();
        return;
    }
    println!(
        "{:>6} {:>5} {:>5} {:>9} {:>7} {:>9} {:>6} {:>6} {:>8} {:>10}",
        "R mm", "t mm", "n", "crossing", "photons", "detected", "empty%", "timed", "trapped%", "spread ps"
    );
    for &radius in &[15.0, 20.0, 25.0, 30.0] {
        for &thickness in &[5.0, 10.0] {
            for &index in &[1.49, 1.84] {
                for vertical in [true, false] {
                    let Some(row) = measure(radius, thickness, index, vertical, 2026) else {
                        continue;
                    };
                    println!(
                        "{:>6.1} {:>5.1} {:>5.2} {:>9} {:>7.1} {:>9.2} {:>6.1} {:>6.2} {:>8.1} {:>10.1}",
                        row.radius_mm,
                        row.thickness_mm,
                        row.index,
                        if row.vertical { "vertical" } else { "across" },
                        row.photons,
                        row.detected,
                        row.empty * 100.0,
                        row.timed,
                        row.trapped * 100.0,
                        row.spread_ps,
                    );
                }
            }
        }
    }
}


/// One event as a waveform, per sensor and in total, and as a picture.
fn waveform() {
    // The detector the study recommends: a 20 mm tent, 10 mm deep, mounted on
    // edge so that the muon crosses the walls. Binned at 10 ps, because the
    // geometry's own spread is of that order and a 1 ps bin would resolve the
    // sensor's jitter into a smear of single-photon spikes.
    let block = Block::new(canlab_optics::Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(1.49, 10_000.0);
    let faces = [
        canlab_optics::block::Face::Top,
        canlab_optics::block::Face::Bottom,
        canlab_optics::block::Face::Side(0),
        canlab_optics::block::Face::Side(2),
    ];
    let sensors: [SiPM; 4] = faces.map(SiPM::typical);
    let mut uniform = Uniform(2026);
    let mut random = || uniform.next();
    let event = simulate(
        &block,
        &optical,
        &sensors,
        Vec3::new(-20.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        40.0,
        1.0,
        &mut random,
    )
    .expect("a crossing");

    let base = TimeBase::new(0.0, 10.0, 200).expect("a 2 ns window in 10 ps bins");
    let waveforms = Waveforms::from_event(&event, base);
    println!(
        "{} photons detected in one event, binned at {} ps over {:.0} ns",
        waveforms.photons,
        base.bin_ps,
        (base.end_ps() - base.start_ps) / 1000.0
    );
    println!(
        "{:>8} {:>8} {:>10} {:>8} {:>8} {:>10}",
        "sensor", "photons", "in window", "under", "over", "peak bin"
    );
    for (index, histogram) in waveforms.per_sensor.iter().enumerate() {
        println!(
            "{index:>8} {:>8} {:>10} {:>8} {:>8} {:>10}",
            histogram.counted(),
            histogram.in_window(),
            histogram.underflow,
            histogram.overflow,
            histogram
                .peak()
                .map(|(bin, count)| format!("{bin}@{count}"))
                .unwrap_or_else(|| "-".to_string()),
        );
    }
    let all = &waveforms.all;
    println!(
        "{:>8} {:>8} {:>10} {:>8} {:>8} {:>10}",
        "all",
        waveforms.photons,
        all.in_window(),
        all.underflow,
        all.overflow,
        all.peak()
            .map(|(bin, count)| format!("{bin}@{count}"))
            .unwrap_or_else(|| "-".to_string()),
    );
    // The identity, in the same breath as the table it is about.
    assert_eq!(all.counted(), waveforms.photons, "the waveform lost a photon");
    if let Some(mean) = all.mean_ps() {
        println!("mean arrival {mean:.1} ps");
    }
    println!();
    // A text histogram, so the waveform is visible without opening the SVG.
    println!("arrivals, 10 ps per column (the window is 200 columns wide):");
    let peak = all.peak().map(|(_, count)| count).unwrap_or(1).max(1);
    for (index, &count) in all.counts.iter().enumerate() {
        if count == 0 && (index == 0 || all.counts[index - 1] == 0) {
            continue;
        }
        let bar = "#".repeat(count * 40 / peak);
        println!("{:>6} ps {:>3} {bar}", (index as f64 * base.bin_ps) as i64, count);
    }

    let curve = all.curve();
    let svg = canlab_plot::to_svg(
        &curve,
        &canlab_plot::SvgOptions {
            width: 900,
            height: 320,
            x_range: (base.start_ps, base.end_ps()),
            y_range: (0.0, peak as f64 * 1.1),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
            bands: Vec::new(),
        },
    );
    let path = std::env::current_dir()
        .map(|directory| directory.join("muon_waveform.svg"))
        .unwrap_or_else(|_| std::path::PathBuf::from("muon_waveform.svg"));
    match std::fs::write(&path, svg) {
        Ok(()) => println!("\nwrote {}", path.display()),
        Err(error) => println!("\ncould not write {}: {error}", path.display()),
    }
}
