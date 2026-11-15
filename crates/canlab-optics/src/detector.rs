//! The four SiPMs, and the simulation that turns a muon into timestamps.
//!
//! This is the piece the neural network will be trained on and the piece the
//! design question is about. It has two halves:
//!
//! * [`SiPM`] — one sensor: the face it reads, a photon-detection efficiency at
//!   the band's centre, the timing jitter of one avalanche, and a microcell
//!   count that bounds how many photons it can report in one event. A photon
//!   that reaches one of its faces at a time either produces a photoelectron or
//!   does not.
//! * [`simulate`] — a muon, its Cherenkov cone, and the transport of every
//!   photon to a sensor or to a loss. The result is one *event*: the arrival
//!   times at each sensor. That is exactly the input a network that recovers the
//!   muon's direction would see.
//!
//! # The timing, which is the whole point
//!
//! An arrival timestamp is not the physical arrival time. It is the arrival time
//! plus:
//!
//! * the **time of flight**, which the transport computes and which is the
//!   signal — it differs by tens of picoseconds between two sensors;
//! * the **avalanche jitter** of the sensor, a fixed standard deviation;
//! * the **statistics of how many photons arrived**, which does not shift one
//!   timestamp but decides whether there is one.
//!
//! [`simulate`] reports the timestamps and the true ones, so a test — or a
//! training run — can measure what a reconstruction could ever recover.
//!
//! # What is not modelled, and why it matters
//!
//! * **Gain and pulse height.** A real SiPM's waveform depends on its
//!   overvoltage, its cells and the amplifier. None of that changes a photon's
//!   *arrival* time, which is what this study is about.
//! * **Afterpulsing and crosstalk.** Both add late or extra timestamps at the
//!   per-mille level; they are the right thing to add once a real part number is
//!   chosen, and the wrong thing to invent now.
//! * **Dark counts and a per-wavelength PDE curve.** A real part has a
//!   dark-count rate and a PDE that varies across the band; both need a real
//!   part number, and the single mid-band `pde` is what the model has until one
//!   is chosen. Dark counts would add photons the model does not have, so this
//!   is the one omission that is not optimistic about the count.
//! * **Saturation.** A sensor with `N` microcells cannot report more than `N`
//!   photons in one event; the model clamps the event's count to the smallest
//!   microcell count among the four sensors, which is the saturation that
//!   matters for a timing study and is a single line.
//!
//! The omissions are not all in the same direction: gain and pulse height do not
//! change an arrival time, afterpulsing and crosstalk would add late or extra
//! timestamps, saturation caps the count, and dark counts would add photons the
//! model does not have. The documentation says so rather than letting a result
//! imply more than it shows.

use crate::block::{Face, SENSOR_FACES};
use crate::source::{self, Photon};
use crate::fresnel::Polarisation;
use crate::transport::{Fate, Optical, Transport};
use crate::vec3::Vec3;
use crate::Block;

/// The spectral range of the C-Series part, nanometres.
///
/// From the datasheet's Table 1, Note 4: "the range where PDE > 1 % at
/// Vbr + 5.0 V". It is 300-950 nm, which is why a Cherenkov band of 400-600 nm
/// sits inside it and a sensor can see the light the block makes.
pub const SENSOR_SPECTRAL_RANGE_NM: (f64, f64) = (300.0, 950.0);

/// The wavelength the part's PDE peaks at, nanometres (datasheet Table 1).
pub const SENSOR_PEAK_NM: f64 = 420.0;

/// The part's PDE at the peak, at Vbr + 2.5 V and at Vbr + 5.0 V.
///
/// Both are 41 % for the 1 mm and 6 mm 35 µm parts and 31 % for the 1 mm part at
/// the lower overvoltage. The spread is the part's own: the datasheet quotes a
/// range, so a single number cannot be more precise than the range it sits in.
pub const SENSOR_PEAK_PDE: (f64, f64) = (0.31, 0.41);

/// The part's photon-detection efficiency at a wavelength, as a fraction.
///
/// A piecewise-linear curve through three independent anchors rather than a
/// fit: the peak of 41 % at 420 nm and the 1 % floor at 950 nm from the
/// datasheet, and the FWHM of 310-560 nm measured for the same blue-sensitive
/// SensL die by Otte et al. (arXiv:1606.05186), which puts the half-maximum at
/// 20.5 % at 560 nm. Outside the spectral range the part does not respond and
/// the answer is zero, not a small number.
///
/// The model does not use this per photon: [`SiPM::pde`] is one flat number and
/// [`band_weighted_pde`] is what that number approximates. This function is
/// here so the approximation can be checked against the curve it replaces
/// instead of being asserted from memory.
pub fn sensor_pde_at(wavelength_nm: f64) -> f64 {
    const ANCHORS: [(f64, f64); 13] = [
        (300.0, 0.010),
        (310.0, 0.205),
        (350.0, 0.280),
        (400.0, 0.373),
        (420.0, 0.410),
        (450.0, 0.370),
        (500.0, 0.300),
        (560.0, 0.205),
        (600.0, 0.150),
        (650.0, 0.100),
        (700.0, 0.060),
        (800.0, 0.025),
        (950.0, 0.010),
    ];
    if wavelength_nm <= ANCHORS[0].0 {
        return ANCHORS[0].1;
    }
    if wavelength_nm >= ANCHORS[ANCHORS.len() - 1].0 {
        return ANCHORS[ANCHORS.len() - 1].1;
    }
    for pair in ANCHORS.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if x0 <= wavelength_nm && wavelength_nm <= x1 {
            let t = (wavelength_nm - x0) / (x1 - x0);
            return y0 + t * (y1 - y0);
        }
    }
    0.0
}

/// The average of [`sensor_pde_at`] over a Cherenkov band, weighted by the
/// `1/λ²` spectrum that makes the photons.
///
/// This is the number a flat [`SiPM::pde`] stands in for. A flat 0.35 is 12.6 %
/// above it, but 0.35 also sits inside the part's own 0.31-0.41 overvoltage
/// spread, so the two choices are not the same size of error: the flat value is
/// a choice of operating point, not a spectral mistake.
pub fn band_weighted_pde(blue_nm: f64, red_nm: f64) -> Option<f64> {
    if !(0.0 < blue_nm && blue_nm < red_nm) {
        return None;
    }
    let steps = 2000;
    let step = (red_nm - blue_nm) / steps as f64;
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for i in 0..steps {
        let lambda = blue_nm + (i as f64 + 0.5) * step;
        let weight = 1.0 / (lambda * lambda);
        numerator += weight * sensor_pde_at(lambda);
        denominator += weight;
    }
    Some(numerator / denominator)
}

/// One silicon photomultiplier watching one face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiPM {
    /// The face it reads.
    pub face: Face,
    /// The photon-detection efficiency at the band's centre.
    pub pde: f64,
    /// Timing jitter of one avalanche, picoseconds (standard deviation).
    pub jitter_ps: f64,
    /// Microcells, for the saturation limit.
    pub microcells: usize,
}

impl SiPM {
    /// A sensor with the numbers this project has assumed.
    ///
    /// `pde` 0.35 is the mid-band value of a blue-sensitive SiPM through the
    /// block's face; `jitter_ps` 100 is a typical single-photon time resolution
    /// for this class of part, and is a *placeholder* until a datasheet is
    /// chosen — the simulation will show how much the answer depends on it.
    pub fn typical(face: Face) -> SiPM {
        SiPM {
            face,
            pde: 0.35,
            jitter_ps: 100.0,
            microcells: 40_000,
        }
    }
}

/// The four sensors of a block: one on each face in [`SENSOR_FACES`].
///
/// The face list is the block's, not this module's, so the sensors the transport
/// detects on are the sensors [`Block::sensor_positions`] draws. The first
/// version kept its own list here and the block kept another; the two disagreed
/// about two of the four faces, and the disagreement was invisible because no
/// test compared them.
pub fn four_sensors(block: &Block) -> [SiPM; 4] {
    let _ = block;
    SENSOR_FACES.map(SiPM::typical)
}

/// One muon's event, as the sensors saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// For each sensor: the arrival times of its photons, picoseconds, sorted.
    pub arrivals_ps: [Vec<f64>; 4],
    /// For each sensor: the true arrival times, without jitter.
    pub true_ps: [Vec<f64>; 4],
    /// Photons that escaped through a face and were not detected.
    pub lost: usize,
    /// Photons that are in the block and never leave it: trapped by total
    /// internal reflection after the bounce limit, so a sensor never sees them.
    /// Counting these separately is the difference between "the sensor missed
    /// it" and "the light never got out", and a design change fixes them in
    /// opposite ways.
    pub trapped: usize,
    /// Photons absorbed in the block.
    pub absorbed: usize,
    /// How many photons the event started with: the Poisson count the muon
    /// produced, clamped to the microcell count, before any transport loss.
    pub photons_born: usize,
    /// Photons that were born but could not be traced: their sample was empty,
    /// or the birth point was outside the block. Every photon is counted
    /// exactly once, so `lost + trapped + absorbed + missed + detected` equals
    /// `photons_born` and a missing photon is a bug in a test, not a rounding
    /// difference.
    pub missed: usize,
}

impl Event {
    /// The photons detected at all four sensors.
    pub fn detected_count(&self) -> usize {
        self.arrivals_ps.iter().map(Vec::len).sum()
    }

    /// How many of the four sensors have at least one timestamp.
    pub fn sensors_with_a_photon(&self) -> usize {
        self.arrivals_ps.iter().filter(|times| !times.is_empty()).count()
    }
}

/// How far inside the block a photon is born, in millimetres.
///
/// See the comment in [`simulate`]: a birth point on the surface is not in the
/// material, and the transport says so.
const BIRTH_INSET_MM: f64 = 1e-3;

/// Why an event simulates to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventError {
    /// The block's optics refused the muon (a bad index or absorption length).
    BadOptics,
    /// No photon was born — the muon is below threshold or the path is empty.
    NoPhotons,
}

/// The speed of light, millimetres per picosecond.
///
/// A muon at `beta` travels at `beta · C_MM_PER_PS`, and that is what turns a
/// birth point into an emission time.
const C_MM_PER_PS: f64 = 0.299_792_458;

/// One photon's path, for the visualiser.
///
/// The event loop does not build these — a `Vec` per photon is an allocation
/// per photon — so they exist only when [`simulate_recording`] asked for them.
#[derive(Debug, Clone, PartialEq)]
pub struct PhotonRecord {
    /// The polyline from birth to the last face it met, millimetres.
    pub path: Vec<Vec3>,
    /// What happened to it.
    pub fate: Fate,
    /// The face it escaped through, if it did.
    pub face: Option<Face>,
    /// The flight time from the birth, picoseconds.
    pub time_ps: f64,
    /// When it was emitted, picoseconds after the muon reached the block.
    ///
    /// The arrival a timer records is this plus `time_ps`. The flight time alone
    /// is *not* the arrival: the photon was emitted when the muon passed its
    /// birth point, which is tens of picoseconds into the crossing.
    pub birth_ps: f64,
    /// The sensor that detected it, if one did.
    ///
    /// `None` for a photon that escaped through a face no sensor covers, one
    /// the sensor's coverage missed, and one below the PDE — the three ways an
    /// escaped photon is still not a timestamp.
    pub detected: Option<usize>,
}

/// Simulate one muon crossing the block and return what the four sensors see.
///
/// The muon enters the block at `entry`, travels along `heading` (a unit
/// vector), and leaves after `path_mm`. Photons are born uniformly along that
/// path, with the Cherenkov cone around the heading. Each is traced to its end
/// with [`Transport::trace`] and, if it escapes through a face a sensor watches,
/// either detected or lost.
///
/// `random` is a uniform generator in `[0, 1)`; the same one the transport uses,
/// so one stream drives everything and a seed reproduces an event exactly.
pub fn simulate(
    block: &Block,
    optical: &Optical,
    sensors: &[SiPM; 4],
    entry: Vec3,
    heading: Vec3,
    path_mm: f64,
    beta: f64,
    random: &mut dyn FnMut() -> f64,
) -> Result<Event, EventError> {
    simulate_inner(block, optical, sensors, entry, heading, path_mm, beta, random, None)
}

/// The same event as [`simulate`], plus every photon's path.
///
/// For the visualiser: a few hundred photons, each with its polyline, is what
/// the 3D view draws. The physics is the same function, so a picture and a
/// number cannot disagree.
pub fn simulate_recording(
    block: &Block,
    optical: &Optical,
    sensors: &[SiPM; 4],
    entry: Vec3,
    heading: Vec3,
    path_mm: f64,
    beta: f64,
    random: &mut dyn FnMut() -> f64,
) -> Result<(Event, Vec<PhotonRecord>), EventError> {
    let mut records = Vec::new();
    let event = simulate_inner(
        block,
        optical,
        sensors,
        entry,
        heading,
        path_mm,
        beta,
        random,
        Some(&mut records),
    )?;
    Ok((event, records))
}

fn simulate_inner(
    block: &Block,
    optical: &Optical,
    sensors: &[SiPM; 4],
    entry: Vec3,
    heading: Vec3,
    path_mm: f64,
    beta: f64,
    random: &mut dyn FnMut() -> f64,
    mut records: Option<&mut Vec<PhotonRecord>>,
) -> Result<Event, EventError> {
    let index = optical.index;
    let heading = heading.normalized().ok_or(EventError::BadOptics)?;
    // A frame around the heading: two unit vectors perpendicular to it.
    let reference = if heading.z.abs() < 0.9 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
    let side = heading.cross(reference).normalized().ok_or(EventError::BadOptics)?;
    let up = side.cross(heading).normalized().ok_or(EventError::BadOptics)?;

    // The photon's path is the *inside* of the block, not the caller's `path_mm`.
    // A caller that hands a path from outside the block to beyond it — as a
    // vertical muon's caller does — would otherwise have every birth point
    // rejected, and the event would report zero photons, zero trapped and zero
    // lost, which is a "nothing happened" that a reader cannot tell from a
    // physics result. The `let` binding is the reason: the clip is the path.
    let (inside, inside_length) = clip_to_block(block, entry, heading, path_mm);
    if inside_length <= 0.0 {
        return Err(EventError::NoPhotons);
    }
    // The count is the photons the muon makes *inside the block*, which is the
    // chord the clip measured — not the caller's path, which may run from
    // outside the block to beyond it. Counting the caller's path would credit a
    // 20 mm block with the light of a 200 mm one.
    let expected =
        source::expected_photons(inside_length, beta, index).ok_or(EventError::NoPhotons)?;
    if expected <= 0.0 {
        return Err(EventError::NoPhotons);
    }
    // Poisson around the mean, in log space: the product of uniforms underflows
    // to zero at a mean of about 709, so the product form would cap a long path
    // at that count instead of sampling its mean. The sum of logs does not
    // underflow.
    let mut log_product = 0.0;
    let mut count = 0usize;
    while log_product > -expected && count < 1_000_000 {
        log_product += random().max(1e-300).ln();
        count += 1;
    }
    // The loop stops after the draw that crossed the line; that draw is the
    // Poisson variate.
    let count = count.saturating_sub(1);
    // Saturation: a sensor with `N` microcells cannot report more than `N`
    // photons in one event. The four sensors share the light, so the event's
    // count is clamped to the smallest cell count among them. With the typical
    // 40 000 cells and a few hundred photons this changes nothing; with a
    // scarce sensor it is the limit the module documents. The clamp is before
    // the trace loop, not after it, so `photons_born` and the fates still
    // account for every photon exactly once.
    let microcells = sensors
        .iter()
        .map(|sensor| sensor.microcells)
        .min()
        .unwrap_or(usize::MAX);
    let count = count.min(microcells);

    let transport = Transport::new(*block, *optical);
    let mut event = Event {
        arrivals_ps: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        true_ps: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        lost: 0,
        trapped: 0,
        absorbed: 0,
        missed: 0,
        photons_born: count,
    };
    // A photon is born in the material, not on its surface. The clip's start is
    // the crossing, where the signed distance is zero and the transport — which
    // asks whether a photon is *strictly* inside — refuses the trace. One
    // micrometre in from the entry is inside the block: it is a thousand times
    // the block's own surface tolerance of a nanometre, not below it, and 10⁻⁴
    // of the block's thickness. It moves no photon's time by more than a
    // femtosecond and it is what makes the first photon of a crossing exist at
    // all.
    let birth_start = inside + heading.scale(BIRTH_INSET_MM);
    let birth_span = (inside_length - BIRTH_INSET_MM).max(0.0);
    for photon_index in 0..count {
        let along = if count > 1 {
            (photon_index as f64 + 0.5) / count as f64
        } else {
            0.5
        };
        let birth = birth_start + heading.scale(along * birth_span);
        let Some(photon) = Photon::sample(beta, random() * std::f64::consts::TAU, random(), index)
            .filter(|photon| photon.theta_rad > 0.0)
        else {
            event.missed += 1;
            continue;
        };
        let direction = heading
            .scale(photon.theta_rad.cos())
            .add(side.scale(photon.theta_rad.sin() * photon.phi_rad.cos()))
            .add(up.scale(photon.theta_rad.sin() * photon.phi_rad.sin()))
            .normalized();
        let Some(direction) = direction else {
            event.missed += 1;
            continue;
        };
        let mut path = Vec::new();
        let traced = if records.is_some() {
            transport.trace_recording(birth, direction, Polarisation::S, random, &mut path)
        } else {
            transport.trace(birth, direction, Polarisation::S, random)
        };
        let Some(traced) = traced else {
            event.missed += 1;
            continue;
        };
        let birth_ps = birth.sub(inside).dot(heading) / (beta * C_MM_PER_PS);
        if let Some(records) = records.as_deref_mut() {
            records.push(PhotonRecord {
                path: std::mem::take(&mut path),
                fate: traced.fate,
                face: traced.face,
                time_ps: traced.time_ps,
                birth_ps,
                detected: None,
            });
        }
        match traced.fate {
            Fate::Absorbed => {
                event.absorbed += 1;
                continue;
            }
            Fate::Trapped => {
                event.trapped += 1;
                continue;
            }
            Fate::Escaped => {}
        }
        let Some(face) = traced.face else {
            event.missed += 1;
            continue;
        };
        let Some(sensor_index) = sensors.iter().position(|sensor| sensor.face == face) else {
            event.lost += 1;
            continue;
        };
        let sensor = sensors[sensor_index];
        // The sensor does not cover the face. This is the loss that dominates
        // everything else, and leaving it out is how the model reported tens of
        // photons per sensor where the geometry allows a handful.
        if random() >= optical.coverage_for(block, face) {
            event.lost += 1;
            continue;
        }
        if random() >= sensor.pde {
            event.lost += 1;
            continue;
        }
        // Jitter: a normal draw with the sensor's own sigma, from two uniforms
        // by the Box-Muller transform — exact enough for a 100 ps sigma and
        // cheaper than an inverse CDF.
        let jitter = box_muller(random) * sensor.jitter_ps;
        event.true_ps[sensor_index].push(birth_ps + traced.time_ps);
        event.arrivals_ps[sensor_index].push(birth_ps + traced.time_ps + jitter);
        if let Some(records) = records.as_deref_mut() {
            if let Some(record) = records.last_mut() {
                record.detected = Some(sensor_index);
            }
        }
    }
    for times in &mut event.arrivals_ps {
        times.sort_by(f64::total_cmp);
    }
    for times in &mut event.true_ps {
        times.sort_by(f64::total_cmp);
    }
    Ok(event)
}

/// The stretch of a muon's path that lies inside the block, and where it starts.
///
/// A muon's path is a straight line, and the block is convex, so the path is
/// either inside or not: the intersection is the stretch between the entry and
/// the exit. A caller that starts outside the block and leaves it is
/// normal, and the caller's `path_mm` may be longer than the block on either
/// side; the photons are born inside the block only, so the block is what the
/// path is clipped to.
///
/// When the path misses the block entirely the answer is the entry point and a
/// length of zero, and [`simulate`] refuses the event: a muon that never
/// crossed the block made no light in it, and photons born outside it would be
/// rejected one by one and reported as a physics result.
///
/// A path that lies *on* the surface is a crossing. A muon entering at
/// `z = −thickness` and leaving at `+thickness` runs along the two end faces,
/// and [`Block::contains`] counts the surface as inside for exactly that
/// reason.
fn clip_to_block(block: &Block, entry: Vec3, heading: Vec3, path_mm: f64) -> (Vec3, f64) {
    // A convex block's intersection with a line is one stretch, so the first
    // point inside and the last point inside are the whole answer. The previous
    // implementation stepped along the path and could never see a block whose
    // thickness was smaller than the step, and the test caught it.
    let mut start = entry;
    let mut length = path_mm;
    if !block.contains(entry) {
        // The entry is outside: the first sample inside the block is the first
        // point of the path, and the path is the whole of the block's
        // intersection.
        let Some(first) = first_sample_inside(block, entry, heading, path_mm) else {
            return (entry, 0.0);
        };
        start = first;
        length = path_mm - (first - entry).length();
    }
    if !block.contains(start + heading.scale(length)) {
        // The end is outside: the last point inside is found by walking the
        // path *backwards* from its end. The previous version walked forwards
        // from the start and called the first point inside the last one, which
        // is only right when the start is outside — for a path that begins
        // inside and leaves, the first point inside is the start itself, and
        // the stretch collapsed to zero. Every muon that enters the block and
        // exits it took that path, and no test had asked one to.
        let Some(last) = first_sample_inside(block, start + heading.scale(length), heading.scale(-1.0), length)
        else {
            return (start, 0.0);
        };
        length = (last - start).length();
    }
    (start, length)
}

/// The first point of a path inside the block, by sampling it, then the first
/// crossing of the path's surface, by bisection in the path's parameter.
///
/// The path is a line, so a point's test is a point's test: the first sample
/// inside is the first point, and the block's convexity means the first sample
/// is the first crossing. The sample is the path's parameter, and the bisection
/// is 40 halvings, which is the arithmetic's resolution, not a search's.
fn first_sample_inside(block: &Block, origin: Vec3, heading: Vec3, length: f64) -> Option<Vec3> {
    let mut sample = 0.0;
    let mut step = 0usize;
    let mut found = false;
    while step < 64 {
        step += 1;
        sample = length * step as f64 / 64.0;
        if block.contains(origin + heading.scale(sample)) {
            found = true;
            break;
        }
    }
    if !found {
        return None;
    }
    // The first sample is inside and the previous one is outside: the first
    // crossing is between them, and the sample's parameter is the first.
    let mut inside = sample;
    let mut outside = sample - length / 64.0;
    for _ in 0..40 {
        let middle = 0.5 * (inside + outside);
        if block.contains(origin + heading.scale(middle)) {
            inside = middle;
        } else {
            outside = middle;
        }
    }
    Some(origin + heading.scale(inside))
}

/// A standard normal from two uniform draws, by the Box-Muller transform.
fn box_muller(random: &mut dyn FnMut() -> f64) -> f64 {
    let first = random().max(1e-300);
    let second = random();
    (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::Tropical;

    struct Uniform(u64);

    impl Uniform {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / (1u64 << 53) as f64
        }
    }

    fn setup() -> (Block, Optical, [SiPM; 4]) {
        let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
        let optical = Optical::block_in_air(1.84, 10_000.0);
        let sensors = four_sensors(&block);
        (block, optical, sensors)
    }

    /// The design point: the same block with a light guide on every sensor face.
    ///
    /// The bare 1 mm² part covers 0.12 % of a side face, so one event of a few
    /// hundred photons usually detects nothing at all — the physics is right and
    /// the test's premise ("some photons reach a sensor") is not. A guide covering
    /// the face is what makes the premise true, and it is what the design uses.
    fn setup_guided() -> (Block, Optical, [SiPM; 4]) {
        let (block, optical, sensors) = setup();
        (block, optical.with_guide(0.6), sensors)
    }

    #[test]
    fn the_four_sensors_watch_four_different_faces() {
        let (block, _, sensors) = setup();
        let faces: Vec<Face> = sensors.iter().map(|sensor| sensor.face).collect();
        assert_eq!(faces.iter().filter(|face| **face == Face::Top).count(), 1);
        assert_eq!(faces.iter().filter(|face| **face == Face::Bottom).count(), 1);
        assert_eq!(faces.iter().filter(|face| matches!(face, Face::Side(_))).count(), 2);
        assert_eq!(block.sensor_positions().len(), 4);
        // The detector's faces and the block's drawn positions are one list.
        // They were two, and they named different faces for two of the four.
        assert_eq!(faces, SENSOR_FACES.to_vec());
        for (sensor, position) in sensors.iter().zip(block.sensor_positions()) {
            assert_eq!(position, block.face_centre(sensor.face));
        }
    }

    #[test]
    fn a_muon_makes_photons_and_some_of_them_reach_a_sensor() {
        // A sensor that can actually see light. A bare 1 mm² part sees 0.12 % of
        // a side face, so a single 268-photon event lands ~0.4 photons on it and
        // this seed gets none — the test failed for the right reason, twice. Even
        // the best possible taper only reaches ~0.35 %, so the test uses the 6 × 6
        // mm part, which is the size the light budget actually needs.
        let (block, optical, sensors) = setup();
        let optical = optical.with_sensor(36.0);
        let mut uniform = Uniform(42);
        let mut random = || uniform.next();
        let event = simulate(
            &block,
            &optical,
            &sensors,
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            10.0,
            1.0,
            &mut random,
        )
        .expect("a muon at beta 1");
        assert!(event.photons_born > 100, "{} photons", event.photons_born);
        let detected: usize = event.arrivals_ps.iter().map(Vec::len).sum();
        // Some photons reach a sensor: a block with total internal reflection
        // loses most of them, but "most" is not "all".
        assert!(detected > 0, "{event:?}");
        assert!(detected <= event.photons_born);
    }

    #[test]
    fn a_bare_sensor_sees_its_area_over_the_face() {
        // The coverage is the sensor's area over the face's, per face. A constant
        // fraction of "a face" is not the same quantity: the end and side faces of
        // one block differ by 2.4×, and the old 0.15 was a 236 mm² face's value
        // carried over to an 840 mm² one.
        let (block, optical, _) = setup();
        assert_eq!(optical.guide_efficiency, None);
        assert_eq!(optical.sensor_area_mm2, 1.0);
        for face in [Face::Top, Face::Bottom, Face::Side(0), Face::Side(2)] {
            let expected = optical.sensor_area_mm2 / block.face_area(face);
            let actual = optical.coverage_for(&block, face);
            assert!((actual - expected).abs() < 1e-12, "{face:?}: {actual} != {expected}");
        }
        // The faces really do differ, so a constant could not have been right.
        let end = optical.coverage_for(&block, Face::Top);
        let side = optical.coverage_for(&block, Face::Side(0));
        assert!(end < side, "the end face is larger, so its coverage is smaller");
        // And the numbers are the geometry's, not a constant's: a 1 mm² sensor on
        // this block's 361 mm² side face sees 0.28 %, not 15 %.
        assert!((end - 1.0 / block.face_area(Face::Top)).abs() < 1e-12);
        assert!((side - 1.0 / block.face_area(Face::Side(0))).abs() < 1e-12);
        assert!(side > 0.002 && side < 0.004, "a 1 mm² sensor on a side face: {side}");
        assert!(side < 0.15, "the old constant was 0.15, which is 50× too generous here");
    }

    #[test]
    fn a_taper_is_capped_by_etendue_not_by_area() {
        // The bug this prevents: a "light guide" that delivers 60 % of a face's
        // light to a 1 mm² sensor. No passive element can. The light reaches the
        // face within the escape cone — 1.63 sr for n = 1.49 — and the sensor
        // accepts a hemisphere, 6.28 sr, so the most a taper can concentrate is
        // 3.87×, times its own transmission. A model that beats that is a model
        // of a concentrator that does not exist.
        let (block, optical, _) = setup();
        let taper = optical.with_guide(0.75);
        // The limit is the ratio of the sensor's hemisphere to the escape cone,
        // and it depends on the block's index.
        let cos_escape = (1.0 - 1.0 / (optical.index * optical.index)).max(0.0).sqrt();
        let escape = 2.0 * std::f64::consts::PI * (1.0 - cos_escape);
        let limit = 2.0 * std::f64::consts::PI / escape;
        for face in [Face::Top, Face::Bottom, Face::Side(0), Face::Side(2)] {
            let bare = optical.coverage_for(&block, face);
            let guided = taper.coverage_for(&block, face);
            let gain = guided / bare;
            assert!(
                (gain - limit * 0.75).abs() < 0.05,
                "{face:?}: the taper gained {gain}×, the etendue limit is {limit}×"
            );
            assert!(
                guided < 0.05,
                "{face:?}: a 1 mm² sensor cannot see {guided} of its face — a few \
                 percent is the etendue limit, and 60 % was the model's mistake"
            );
        }
        // The gain is the same on every face, because it is the solid-angle
        // ratio and not the geometry.
        let top = taper.coverage_for(&block, Face::Top) / optical.coverage_for(&block, Face::Top);
        let side = taper.coverage_for(&block, Face::Side(0)) / optical.coverage_for(&block, Face::Side(0));
        assert!((top - side).abs() < 1e-12);
        // A transmission outside [0, 1] is clamped, not propagated.
        assert!(optical.with_guide(1.7).coverage_for(&block, Face::Top) < 0.01);
    }

    #[test]
    fn a_taper_cannot_make_a_one_millimetre_sensor_viable() {
        // The light budget, measured over many events so the comparison is the
        // physics and not one seed's luck. The bare 1 mm² part detects about one
        // photon per eleven muons; the best possible taper multiplies that by
        // 3.87 × 0.75 = 2.9, which is still one photon per four muons. The design
        // needs a bigger sensor, and the test says so with the number.
        let (block, optical, sensors) = setup();
        let taper = optical.with_guide(0.75);
        let total = |optical: &Optical| {
            let mut uniform = Uniform(99);
            let mut random = || uniform.next();
            let mut detected = 0usize;
            for _ in 0..200 {
                let event = simulate(
                    &block,
                    optical,
                    &sensors,
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    10.0,
                    1.0,
                    &mut random,
                )
                .expect("a muon at beta 1");
                detected += event.arrivals_ps.iter().map(Vec::len).sum::<usize>();
            }
            detected as f64 / 200.0
        };
        let bare = total(&optical);
        let guided = total(&taper);
        assert!(bare < 1.0, "the bare 1 mm² part: {bare} photons per muon");
        // The taper is a gain of 2.9, not of 500: it must stay under one photon
        // per muon, which is the number that says the design needs a bigger part.
        assert!(guided < 1.0, "the taper: {guided} photons per muon");
        assert!(guided > bare, "the taper is {guided} vs {bare}");
    }

    #[test]
    fn a_photon_count_above_the_microcell_count_is_reported_as_the_microcell_count() {
        // Saturation. The same muon that makes hundreds of photons with the
        // typical 40 000 cells makes exactly `microcells` when the cells are
        // scarce, and every photon it reports still has a fate.
        let (block, optical, mut sensors) = setup();
        for sensor in &mut sensors {
            sensor.microcells = 3;
        }
        let expected = source::expected_photons(10.0, 1.0, optical.index).expect("a mean");
        assert!(expected > 3.0, "the muon makes {expected} photons on average");
        let mut uniform = Uniform(42);
        let mut random = || uniform.next();
        let event = simulate(
            &block,
            &optical,
            &sensors,
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            10.0,
            1.0,
            &mut random,
        )
        .expect("a muon at beta 1");
        assert_eq!(
            event.photons_born, 3,
            "the count was not clamped to the microcells"
        );
        let accounted = event.arrivals_ps.iter().map(Vec::len).sum::<usize>()
            + event.lost
            + event.trapped
            + event.absorbed
            + event.missed;
        assert_eq!(accounted, event.photons_born, "a clamped photon was lost");
    }

    #[test]
    fn the_same_seed_gives_the_same_event() {
        // Reproducibility is a property the training data depends on: a rerun
        // with the same seed must produce byte-identical timestamps.
        let (block, optical, sensors) = setup();
        let run = || {
            let mut uniform = Uniform(7);
            let mut random = || uniform.next();
            simulate(
                &block,
                &optical,
                &sensors,
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                1.0,
                &mut random,
            )
            .unwrap()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_different_seed_gives_a_different_event() {
        let (block, optical, sensors) = setup();
        let run = |seed| {
            let mut uniform = Uniform(seed);
            let mut random = || uniform.next();
            simulate(&block, &optical, &sensors, Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 10.0, 1.0, &mut random).unwrap()
        };
        assert_ne!(run(1), run(2));
    }

    #[test]
    fn jitter_moves_a_timestamp_but_not_the_true_time() {
        let (block, optical, sensors) = setup();
        let mut uniform = Uniform(3);
        let mut random = || uniform.next();
        let event = simulate(&block, &optical, &sensors, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 10.0, 1.0, &mut random).unwrap();
        let mut moved = false;
        for (arrivals, truth) in event.arrivals_ps.iter().zip(&event.true_ps) {
            for (arrival, true_time) in arrivals.iter().zip(truth) {
                if (arrival - true_time).abs() > 1.0 {
                    moved = true;
                }
            }
        }
        if event.true_ps.iter().map(Vec::len).sum::<usize>() > 0 {
            assert!(moved, "the jitter never moved a timestamp");
        }
    }

    #[test]
    fn a_muon_that_crosses_the_block_entirely_makes_light() {
        // The regression that cost the study its vertical column. A muon from
        // outside the block to outside it on the far side has its whole path
        // inside, and the first photon is born where the path crosses the
        // surface — a point whose signed distance is exactly zero, which the
        // transport refuses because it asks for *strictly* inside. Every photon
        // of such an event was rejected, the study reported zero light from a
        // vertical muon at every geometry, and the number was read as physics.
        // The test asks for the count, so the difference between "no photons"
        // and "photons that were thrown away" is visible.
        let (block, optical, sensors) = setup();
        for (entry, heading, length) in [
            (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 1.0), 30.0),
            (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 40.0),
        ] {
            let mut uniform = Uniform(11);
            let mut random = || uniform.next();
            let event = simulate(&block, &optical, &sensors, entry, heading, length, 1.0, &mut random)
                .expect("a muon through the block");
            assert!(event.photons_born > 100, "{entry:?}: {} photons", event.photons_born);
            assert_eq!(event.missed, 0, "{entry:?}: {} of {} photons could not be traced", event.missed, event.photons_born);
        }
    }

    #[test]
    fn a_muon_that_misses_the_block_makes_no_event() {
        // The other side of the same rule: a path that never enters the block
        // is refused, rather than reporting photons born in the air.
        let (block, optical, sensors) = setup();
        let mut uniform = Uniform(5);
        let mut random = || uniform.next();
        let result = simulate(
            &block,
            &optical,
            &sensors,
            Vec3::new(0.0, 0.0, 100.0),
            Vec3::new(0.0, 0.0, 1.0),
            10.0,
            1.0,
            &mut random,
        );
        assert_eq!(result, Err(EventError::NoPhotons));
    }

    #[test]
    fn below_threshold_there_is_no_event() {
        let (block, optical, sensors) = setup();
        let mut uniform = Uniform(1);
        let mut random = || uniform.next();
        let result = simulate(&block, &optical, &sensors, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 10.0, 0.5, &mut random);
        assert_eq!(result, Err(EventError::NoPhotons));
    }
}
