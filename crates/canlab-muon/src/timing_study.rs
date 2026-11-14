//! The Cherenkov timing design, simulated event by event.
//!
//! The earlier study reported the timing design's angle as a **Cramér–Rao bound**
//! computed from the linear model `t_i = t₀ + (r_i · d)/v`. That bound is a limit
//! on an *idealised* estimator: it assumes the arrival time at a sensor is a
//! linear function of the sensor's position projected on the heading. The real
//! light does not do that — a photon is emitted somewhere along the muon's track,
//! travels to the sensor by a path that depends on where it was emitted, and the
//! *first* photon to arrive is the one that took the shortest path, not the one
//! from the track's midpoint.
//!
//! This module closes that gap. It runs the real transport, takes the first
//! photon at each sensor, reconstructs the heading with the same least-squares fit
//! the bound is about, and measures the error. The result is the number the design
//! actually delivers, and it is the one to quote.
//!
//! # What the simulation does
//!
//! For each event:
//!
//! 1. Draw a muon direction from the measured sea-level flux, `cos²·¹θ`.
//! 2. Draw a point inside the block, and put the track through it.
//! 3. Run [`canlab_optics::detector::simulate`] — the real photon transport, with
//!    total internal reflection, Fresnel loss, absorption, the sensors' coverage
//!    and their PDE.
//! 4. Take the **first** arrival at each sensor. A sensor with no photon has no
//!    timestamp, and an event with fewer than four has no fit.
//! 5. Reconstruct the heading and measure the angle to the truth.
//!
//! # The two numbers it reports
//!
//! * **`timed`** — how many of the four sensors fired. The fit needs four, so the
//!   fraction of events with four is the fraction that can be reconstructed at
//!   all. This is the number the photon budget decides.
//! * **`error_deg`** — the angular error of the reconstruction, for the events
//!   that have one. Its median is the design's real resolution.

use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, Tropical, Vec3, SENSOR_FACES};

use crate::bound::timing_bound;
use crate::components;
use crate::readout::Readout;
use crate::timing::{reconstruct, Observation};
use crate::tracker::Rng;

/// A Cherenkov timing design: the block and the readout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimingDesign {
    /// What the table calls it.
    pub name: &'static str,
    /// The tent's scale, millimetres.
    pub scale_mm: f64,
    /// The extrusion thickness, millimetres.
    pub thickness_mm: f64,
    /// The block's refractive index.
    pub index: f64,
    /// The sensor's photon-detection efficiency.
    pub pde: f64,
}

impl TimingDesign {
    /// The design point: a 20 mm tent, 10 mm deep, PMMA.
    pub fn design() -> TimingDesign {
        TimingDesign { name: "design", scale_mm: 20.0, thickness_mm: 10.0, index: 1.49, pde: 0.35 }
    }

    /// The block, or `None` for a size the outline refuses.
    pub fn block(&self) -> Option<Block> {
        Tropical::fits_in(self.scale_mm, self.thickness_mm).ok().map(Block::new)
    }

    /// The four sensors, with the readout's total jitter.
    ///
    /// `jitter_ps` is the **readout's total** — 47.4 ps from the four parts — not
    /// the SiPM's own 42.5, because the timestamp a timer records has the timer's
    /// jitter in it as well as the sensor's. Using the total here is what makes
    /// the measured error comparable with the bound, which also uses the total.
    ///
    /// The microcell count is the transport's default 40 000, not the part's 504,
    /// because `simulate` clamps the **born** photon count to it: a 40 mm crossing
    /// makes ~840 photons and only ~19 reach a sensor, so a 504-cell clamp would
    /// throw away light the sensor never sees. The saturation question is the
    /// design table's, not this module's.
    pub fn sensors(&self) -> [SiPM; 4] {
        let jitter = components::total_jitter_ps();
        let pde = self.pde;
        SENSOR_FACES.map(|face| SiPM { face, pde, jitter_ps: jitter, microcells: 40_000 })
    }

    /// The Cramér–Rao bound on the heading, worst case over four headings.
    ///
    /// This is the number the earlier study quoted. It is reported next to the
    /// measured error so the gap between the model and the transport is visible.
    pub fn bound_deg(&self) -> Option<f64> {
        let block = self.block()?;
        let positions = block.sensor_positions();
        let speed = crate::timing::speed_mm_per_ps(self.index)?;
        let jitter = components::total_jitter_ps();
        let mut worst = 0.0f64;
        for heading in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
        ] {
            let Some(heading) = heading.normalized() else { continue };
            if let Some(bound) = timing_bound(&positions, heading, jitter, speed) {
                worst = worst.max(bound.sigma_deg());
            }
        }
        Some(worst)
    }
}

/// What one simulated event gave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimingOutcome {
    /// How many of the four sensors fired.
    pub timed: usize,
    /// The angular error of the **linear** reconstruction, degrees, if it could
    /// be made. This is the estimator the Cramér–Rao bound is about.
    pub error_deg: Option<f64>,
    /// The angular error of the **Cherenkov** reconstruction with the *true*
    /// track point, degrees.
    ///
    /// This is a diagnostic and not a design: a detector does not know where the
    /// track is. It separates two failures — a model that is wrong, and a model
    /// that is right but cannot be identified from four timestamps.
    pub error_exact_deg: Option<f64>,
    /// The Cherenkov photons the muon produced.
    pub photons: usize,
    /// The first arrival at each sensor, picoseconds; `None` for no photon.
    pub times: [Option<f64>; 4],
    /// The second arrival at each sensor, picoseconds.
    ///
    /// A multi-hit TDC records it, and it is information a single-threshold
    /// readout throws away: the gap between the first and second photon depends
    /// on how far the track is from the sensor, which is the latent variable the
    /// direction fit needs.
    pub second_times: [Option<f64>; 4],
    /// How many photons each sensor registered.
    pub counts: [usize; 4],
    /// The muon's true direction, as the two horizontal components.
    pub direction: [f64; 2],
}

/// Simulate one muon through the block and reconstruct its heading.
///
/// `zenith_rad` is the angle from the vertical and `azimuth_rad` the compass
/// direction. The track is put through a random point inside the block, so the
/// path length is the block's own chord rather than a constant.
pub fn simulate_one(
    design: &TimingDesign,
    zenith_rad: f64,
    azimuth_rad: f64,
    rng: &mut Rng,
) -> Option<TimingOutcome> {
    let block = design.block()?;
    let optical = Optical::block_in_air(design.index, 10_000.0);
    let sensors = design.sensors();

    let heading = Vec3::new(
        zenith_rad.sin() * azimuth_rad.cos(),
        zenith_rad.sin() * azimuth_rad.sin(),
        -zenith_rad.cos(),
    )
    .normalized()?;

    // A point inside the block, by rejection over its bounding box. The tent's
    // box is 2·scale wide and 4.5·scale tall, centred on the waist.
    let mut inside = None;
    for _ in 0..64 {
        let point = Vec3::new(
            rng.range(-design.scale_mm, design.scale_mm),
            rng.range(-1.5 * design.scale_mm, 3.0 * design.scale_mm),
            rng.range(-design.thickness_mm / 2.0, design.thickness_mm / 2.0),
        );
        if block.strictly_contains(point) {
            inside = Some(point);
            break;
        }
    }
    let inside = inside?;

    // The track is a line through that point: back off along the heading.
    let entry = inside.sub(heading.scale(100.0));
    let mut random = || rng.uniform();
    let event = simulate(&block, &optical, &sensors, entry, heading, 200.0, 1.0, &mut random).ok()?;

    // The first photon at each sensor is the timestamp a leading-edge
    // discriminator would record.
    let positions = block.sensor_positions();
    let mut observations = Vec::new();
    for (index, position) in positions.iter().enumerate() {
        if let Some(first) = event.arrivals_ps[index].first() {
            observations.push(Observation { position_mm: *position, time_ps: *first });
        }
    }
    let timed = observations.len();
    let error_deg = if timed >= 4 {
        reconstruct(&observations).ok().map(|fit| {
            fit.heading.cos_with(heading).clamp(-1.0, 1.0).acos().to_degrees()
        })
    } else {
        None
    };
    let error_exact_deg = if timed >= 4 {
        reconstruct_cherenkov(&observations, inside, design.index).map(|fitted| {
            fitted.cos_with(heading).clamp(-1.0, 1.0).acos().to_degrees()
        })
    } else {
        None
    };
    let mut times = [None; 4];
    let mut second_times = [None; 4];
    let mut counts = [0usize; 4];
    for index in 0..4 {
        times[index] = event.arrivals_ps[index].first().copied();
        second_times[index] = event.arrivals_ps[index].get(1).copied();
        counts[index] = event.arrivals_ps[index].len();
    }
    Some(TimingOutcome {
        timed,
        error_deg,
        error_exact_deg,
        photons: event.photons_born,
        times,
        second_times,
        counts,
        direction: [heading.x, heading.y],
    })
}

/// Simulate one muon through the block with a **realistic readout**.
///
/// The ideal [`simulate_one`] takes the first photon at each sensor, which is
/// what a threshold at half a photoelectron would record. This one runs the
/// same transport and then puts the readout in the way: the threshold needs
/// `ceil(threshold_pe)` photoelectrons, crosstalk adds delayed photons, a dark
/// count can fire before the real one, and the comparator's walk shifts the
/// timestamp. The result is what the instrument would actually record.
///
/// The reconstruction is the same linear fit. The difference between this and
/// the ideal case is the price of the readout, and it is the number to quote for
/// a real build.
pub fn simulate_one_realistic(
    design: &TimingDesign,
    readout: &Readout,
    zenith_rad: f64,
    azimuth_rad: f64,
    rng: &mut Rng,
) -> Option<TimingOutcome> {
    let block = design.block()?;
    let optical = Optical::block_in_air(design.index, 10_000.0);
    let sensors = design.sensors();

    let heading = Vec3::new(
        zenith_rad.sin() * azimuth_rad.cos(),
        zenith_rad.sin() * azimuth_rad.sin(),
        -zenith_rad.cos(),
    )
    .normalized()?;

    let mut inside = None;
    for _ in 0..64 {
        let point = Vec3::new(
            rng.range(-design.scale_mm, design.scale_mm),
            rng.range(-1.5 * design.scale_mm, 3.0 * design.scale_mm),
            rng.range(-design.thickness_mm / 2.0, design.thickness_mm / 2.0),
        );
        if block.strictly_contains(point) {
            inside = Some(point);
            break;
        }
    }
    let inside = inside?;

    let entry = inside.sub(heading.scale(100.0));
    let mut random = || rng.uniform();
    let event = simulate(&block, &optical, &sensors, entry, heading, 200.0, 1.0, &mut random).ok()?;

    // The readout, per sensor: crosstalk, then the threshold and the dark count.
    let positions = block.sensor_positions();
    let mut observations = Vec::new();
    let mut times = [None; 4];
    let mut second_times = [None; 4];
    let mut counts = [0usize; 4];
    for (index, position) in positions.iter().enumerate() {
        let extended = readout.with_crosstalk(&event.arrivals_ps[index], rng);
        counts[index] = extended.len();
        let needed = readout.photons_needed();
        if let Some(time) = readout.timestamp(&extended, rng) {
            times[index] = Some(time);
            // The next photon after the threshold is the multi-hit TDC's second
            // stop. It is `None` when there is no next photon.
            second_times[index] = extended.get(needed).copied();
            observations.push(Observation { position_mm: *position, time_ps: time });
        }
    }
    let timed = observations.len();
    let error_deg = if timed >= 4 {
        reconstruct(&observations).ok().map(|fit| {
            fit.heading.cos_with(heading).clamp(-1.0, 1.0).acos().to_degrees()
        })
    } else {
        None
    };
    Some(TimingOutcome {
        timed,
        error_deg,
        error_exact_deg: None,
        photons: event.photons_born,
        times,
        second_times,
        counts,
        direction: [heading.x, heading.y],
    })
}

/// The result of a run of `events` muons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimingRun {
    /// How many events were simulated.
    pub events: usize,
    /// The mean number of sensors that fired.
    pub mean_timed: f64,
    /// The fraction of events with all four timestamps.
    pub full_fraction: f64,
    /// The median angular error over the events that could be reconstructed.
    pub median_error_deg: Option<f64>,
    /// The 68th percentile of the same.
    pub p68_error_deg: Option<f64>,
    /// The fraction of reconstructed events within 30° of the truth.
    ///
    /// The mission's criterion. A design that is usually 20° and occasionally
    /// 150° is not the same as one that is always 25°, and the median cannot
    /// tell them apart.
    pub within_30: f64,
    /// The median error of the Cherenkov model **with the true track point**.
    pub median_exact_deg: Option<f64>,
    /// The mean Cherenkov photon count.
    pub mean_photons: f64,
    /// The Cramér–Rao bound the earlier study quoted.
    pub bound_deg: Option<f64>,
}

/// Run `events` muons through a design and summarise.
pub fn run(design: &TimingDesign, events: usize, seed: u64) -> TimingRun {
    let mut rng = Rng::new(seed);
    let mut timed_total = 0usize;
    let mut full = 0usize;
    let mut photons = 0usize;
    let mut errors = Vec::new();
    let mut exact_errors = Vec::new();
    let mut simulated = 0usize;
    for _ in 0..events {
        // The measured sea-level flux, by inverting its CDF.
        let zenith = crate::muon::MuonFlux::measured().sample_zenith(rng.uniform());
        let azimuth = rng.range(0.0, std::f64::consts::TAU);
        let Some(outcome) = simulate_one(design, zenith, azimuth, &mut rng) else {
            continue;
        };
        simulated += 1;
        timed_total += outcome.timed;
        photons += outcome.photons;
        if outcome.timed >= 4 {
            full += 1;
        }
        if let Some(error) = outcome.error_deg {
            errors.push(error);
        }
        if let Some(error) = outcome.error_exact_deg {
            exact_errors.push(error);
        }
    }
    if simulated == 0 {
        return TimingRun {
            events: 0,
            mean_timed: 0.0,
            full_fraction: 0.0,
            median_error_deg: None,
            p68_error_deg: None,
            within_30: 0.0,
            median_exact_deg: None,
            mean_photons: 0.0,
            bound_deg: design.bound_deg(),
        };
    }
    errors.sort_by(f64::total_cmp);
    exact_errors.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| -> Option<f64> {
        if errors.is_empty() {
            return None;
        }
        let index = ((errors.len() as f64 - 1.0) * fraction).round() as usize;
        Some(errors[index.min(errors.len() - 1)])
    };
    let within_30 = if errors.is_empty() {
        0.0
    } else {
        errors.iter().filter(|error| **error <= 30.0).count() as f64 / errors.len() as f64
    };
    TimingRun {
        events: simulated,
        mean_timed: timed_total as f64 / simulated as f64,
        full_fraction: full as f64 / simulated as f64,
        median_error_deg: percentile(0.5),
        p68_error_deg: percentile(0.68),
        within_30,
        median_exact_deg: if exact_errors.is_empty() {
            None
        } else {
            Some(exact_errors[exact_errors.len() / 2])
        },
        mean_photons: photons as f64 / simulated as f64,
        bound_deg: design.bound_deg(),
    }
}

/// Run `events` muons through a design with a realistic readout.
///
/// The same summary as [`run`], but every timestamp goes through the readout's
/// threshold, dark counts, crosstalk and walk. The difference between the two
/// runs is the readout's price, and it is the number a real build should be
/// designed around.
pub fn run_realistic(
    design: &TimingDesign,
    readout: &Readout,
    events: usize,
    seed: u64,
) -> TimingRun {
    let mut rng = Rng::new(seed);
    let mut timed_total = 0usize;
    let mut full = 0usize;
    let mut photons = 0usize;
    let mut errors = Vec::new();
    let mut simulated = 0usize;
    for _ in 0..events {
        let zenith = crate::muon::MuonFlux::measured().sample_zenith(rng.uniform());
        let azimuth = rng.range(0.0, std::f64::consts::TAU);
        let Some(outcome) = simulate_one_realistic(design, readout, zenith, azimuth, &mut rng) else {
            continue;
        };
        simulated += 1;
        timed_total += outcome.timed;
        photons += outcome.photons;
        if outcome.timed >= 4 {
            full += 1;
        }
        if let Some(error) = outcome.error_deg {
            errors.push(error);
        }
    }
    if simulated == 0 {
        return TimingRun {
            events: 0,
            mean_timed: 0.0,
            full_fraction: 0.0,
            median_error_deg: None,
            p68_error_deg: None,
            within_30: 0.0,
            median_exact_deg: None,
            mean_photons: 0.0,
            bound_deg: design.bound_deg(),
        };
    }
    errors.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| -> Option<f64> {
        if errors.is_empty() {
            return None;
        }
        let index = ((errors.len() as f64 - 1.0) * fraction).round() as usize;
        Some(errors[index.min(errors.len() - 1)])
    };
    let within_30 = if errors.is_empty() {
        0.0
    } else {
        errors.iter().filter(|error| **error <= 30.0).count() as f64 / errors.len() as f64
    };
    TimingRun {
        events: simulated,
        mean_timed: timed_total as f64 / simulated as f64,
        full_fraction: full as f64 / simulated as f64,
        median_error_deg: percentile(0.5),
        p68_error_deg: percentile(0.68),
        within_30,
        median_exact_deg: None,
        mean_photons: photons as f64 / simulated as f64,
        bound_deg: design.bound_deg(),
    }
}

/// The earliest time a photon from a track can reach a sensor.
///
/// The linear model `t = t₀ + (r · d)/v` assumes the light leaves from a point
/// and travels at `v = c/n`. The real earliest photon is emitted somewhere along
/// the track, **at the Cherenkov angle**, and the two travel times add:
///
/// ```text
/// t(s) = s/c + |r − p₀ − s·d| · n/c
/// ```
///
/// with `s` the distance along the track. Its minimum is where the photon leaves
/// at `cos θ = 1/n` — the Cherenkov cone — which is the root of
///
/// ```text
/// (n² − 1)s² + 2a(1 − n²)s + n²a² − |w|² = 0,   a = w·d,  w = r − p₀
/// ```
///
/// This is the model the transport actually obeys, and the difference from the
/// linear one is why the measured error exceeds the linear bound.
pub fn earliest_time_ps(sensor: Vec3, track_point: Vec3, heading: Vec3, index: f64) -> f64 {
    let w = sensor.sub(track_point);
    let a = w.dot(heading);
    let w2 = w.dot(w);
    let n = index;
    let qa = n * n - 1.0;
    let qb = 2.0 * a * (1.0 - n * n);
    let qc = n * n * a * a - w2;
    let discriminant = qb * qb - 4.0 * qa * qc;
    if discriminant < 0.0 || qa == 0.0 {
        return f64::NAN;
    }
    let root = discriminant.sqrt();
    let mut best = f64::INFINITY;
    for s in [(-qb + root) / (2.0 * qa), (-qb - root) / (2.0 * qa)] {
        let distance = w.sub(heading.scale(s)).length();
        let time = (s + distance * n) / crate::timing::C_MM_PER_PS;
        if time.is_finite() && time < best {
            best = time;
        }
    }
    best
}

/// Reconstruct a heading by fitting the **Cherenkov** arrival model.
///
/// The linear fit solves one linear system; this one has no closed form, so it
/// searches a grid of headings and keeps the one whose modelled arrival times
/// best match the measured ones. `t₀` is removed analytically for each candidate
/// — it is the mean offset — so the search is over two angles only.
///
/// The grid is 60 × 120 (zenith × azimuth), which is 2° in zenith and 3° in
/// azimuth. A finer grid would cost more than the accuracy it buys, because the
/// error being measured is hundreds of times coarser.
pub fn reconstruct_cherenkov(
    observations: &[Observation],
    track_point: Vec3,
    index: f64,
) -> Option<Vec3> {
    if observations.len() < 4 {
        return None;
    }
    let mut best: Option<(f64, Vec3)> = None;
    let zeniths = 60;
    let azimuths = 120;
    for zi in 0..zeniths {
        let zenith = std::f64::consts::FRAC_PI_2 * (zi as f64 + 0.5) / zeniths as f64;
        for ai in 0..azimuths {
            let azimuth = std::f64::consts::TAU * (ai as f64 + 0.5) / azimuths as f64;
            let heading = Vec3::new(
                zenith.sin() * azimuth.cos(),
                zenith.sin() * azimuth.sin(),
                -zenith.cos(),
            );
            // The modelled times, and the offset that minimises the residual.
            let mut modelled = Vec::with_capacity(observations.len());
            let mut measured = Vec::with_capacity(observations.len());
            let mut valid = true;
            for observation in observations {
                let time = earliest_time_ps(observation.position_mm, track_point, heading, index);
                if !time.is_finite() {
                    valid = false;
                    break;
                }
                modelled.push(time);
                measured.push(observation.time_ps);
            }
            if !valid {
                continue;
            }
            let offset = measured
                .iter()
                .zip(&modelled)
                .map(|(m, c)| m - c)
                .sum::<f64>()
                / modelled.len() as f64;
            let residual: f64 = measured
                .iter()
                .zip(&modelled)
                .map(|(m, c)| {
                    let difference = m - c - offset;
                    difference * difference
                })
                .sum();
            if best.map(|(value, _)| residual < value).unwrap_or(true) {
                best = Some((residual, heading));
            }
        }
    }
    best.map(|(_, heading)| heading)
}

/// The number of features one event gives the network.
///
/// Four first arrival times, four **second** arrival times, four "did it fire"
/// flags, and four photon counts.
///
/// The second arrival is the multi-hit TDC's extra stop. Its gap from the first
/// depends on how far the track is from the sensor, which is exactly the latent
/// variable the direction fit is missing — so it is the most valuable feature
/// the readout can add without a second threshold.
pub const FEATURES: usize = 16;

/// Turn an event into a training sample.
///
/// The features are the four arrival times **relative to their own mean**, so the
/// network sees the differences that carry the direction and not the absolute
/// time, which carries only the muon's arrival. A sensor with no photon gets a
/// time of zero and a flag of zero, so the network can learn that "no photon" is
/// information rather than a missing value.
///
/// The counts are divided by ten and clipped, so a sensor with a hundred photons
/// is not a hundred times more influential than one with ten.
pub fn features(outcome: &TimingOutcome) -> [f64; FEATURES] {
    let present: Vec<f64> = outcome.times.iter().flatten().copied().collect();
    let mean = if present.is_empty() {
        0.0
    } else {
        present.iter().sum::<f64>() / present.len() as f64
    };
    let mut features = [0.0; FEATURES];
    for index in 0..4 {
        match outcome.times[index] {
            Some(time) => {
                // A thousand picoseconds is the natural scale: the spread across
                // the block is a few hundred, and the jitter is tens.
                features[index] = (time - mean) / 1000.0;
                features[8 + index] = 1.0;
            }
            None => {
                features[index] = 0.0;
                features[8 + index] = 0.0;
            }
        }
        // The second arrival, relative to the same mean, and its gap from the
        // first. A sensor with no second photon gets zero, which the flag
        // disambiguates from a real zero.
        match outcome.second_times[index] {
            Some(second) => {
                features[4 + index] = (second - mean) / 1000.0;
            }
            None => {
                features[4 + index] = 0.0;
            }
        }
        features[12 + index] = (outcome.counts[index] as f64 / 10.0).min(3.0) / 3.0;
    }
    features
}

/// The features without the photon counts: eight values.
///
/// The ablation. The counts depend on the path length and therefore on where the
/// track is, so a network that beats the timestamps-only version is using them to
/// infer the track position — which is the thing the analytic inversion cannot do.
pub fn features_without_counts(outcome: &TimingOutcome) -> [f64; 8] {
    let full = features(outcome);
    let mut trimmed = [0.0; 8];
    trimmed[..8].copy_from_slice(&full[..8]);
    trimmed
}

/// One training sample from the design.
pub fn training_sample(
    design: &TimingDesign,
    rng: &mut Rng,
) -> Option<crate::net::Sample> {
    let zenith = crate::muon::MuonFlux::measured().sample_zenith(rng.uniform());
    let azimuth = rng.range(0.0, std::f64::consts::TAU);
    let outcome = simulate_one(design, zenith, azimuth, rng)?;
    Some(crate::net::Sample {
        features: features(&outcome).to_vec(),
        target: outcome.direction,
    })
}

/// One training sample from the design, through the realistic readout.
///
/// The same sample as [`training_sample`], but the timestamps are what the
/// instrument would record: the threshold's k-th photon, a dark count that may
/// fire first, and the comparator's walk. A network trained on these is trained
/// on what a real build delivers, not on the ideal first photon.
pub fn training_sample_realistic(
    design: &TimingDesign,
    readout: &Readout,
    rng: &mut Rng,
) -> Option<crate::net::Sample> {
    let zenith = crate::muon::MuonFlux::measured().sample_zenith(rng.uniform());
    let azimuth = rng.range(0.0, std::f64::consts::TAU);
    let outcome = simulate_one_realistic(design, readout, zenith, azimuth, rng)?;
    Some(crate::net::Sample {
        features: features(&outcome).to_vec(),
        target: outcome.direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_has_at_most_four_timestamps_and_a_finite_error() {
        // The two invariants the summary depends on: a sensor either fires once
        // or not at all, and an error is an angle between two directions.
        let design = TimingDesign::design();
        let mut rng = Rng::new(1);
        let mut seen = 0;
        for _ in 0..40 {
            let Some(outcome) = simulate_one(&design, 0.3, 0.7, &mut rng) else {
                continue;
            };
            seen += 1;
            assert!(outcome.timed <= 4, "{} timestamps", outcome.timed);
            if let Some(error) = outcome.error_exact_deg {
                assert!((0.0..=180.0).contains(&error), "{error}");
            }
            if let Some(error) = outcome.error_deg {
                assert!(outcome.timed >= 4, "a fit needs four timestamps");
                assert!((0.0..=180.0).contains(&error), "{error}");
            }
            assert!(outcome.photons > 0, "a muon at beta 1 makes photons");
        }
        assert!(seen > 20, "only {seen} of 40 events crossed the block");
    }

    #[test]
    fn the_run_reports_a_fraction_and_a_median() {
        // A run of a hundred events must produce a fraction in [0, 1] and a
        // median error that is one of the measured errors.
        let run = run(&TimingDesign::design(), 100, 7);
        assert!(run.events > 50, "{} events", run.events);
        assert!((0.0..=1.0).contains(&run.full_fraction), "{}", run.full_fraction);
        assert!((0.0..=4.0).contains(&run.mean_timed), "{}", run.mean_timed);
        if let Some(median) = run.median_error_deg {
            assert!((0.0..=180.0).contains(&median), "{median}");
        }
        assert!(run.mean_photons > 0.0);
    }

    #[test]
    fn the_bound_is_the_one_the_earlier_study_quoted() {
        // The design point's bound is 81.2 degrees. Pinning it here means the
        // measured error and the bound are compared against the same number the
        // table quotes.
        let bound = TimingDesign::design().bound_deg().expect("a bound");
        assert!((bound - 81.2).abs() < 1.0, "{bound}");
    }

    #[test]
    fn a_bigger_block_makes_more_photons() {
        // The photon count is the block's own chord, so a bigger block makes
        // more light. A run of a small block and a large one must differ in the
        // direction the geometry says.
        let small = TimingDesign { name: "small", scale_mm: 15.0, thickness_mm: 5.0, index: 1.49, pde: 0.35 };
        let large = TimingDesign { name: "large", scale_mm: 24.0, thickness_mm: 20.0, index: 1.49, pde: 0.35 };
        let small_run = run(&small, 60, 11);
        let large_run = run(&large, 60, 11);
        assert!(
            large_run.mean_photons > small_run.mean_photons,
            "{} vs {}",
            large_run.mean_photons,
            small_run.mean_photons
        );
    }
}
