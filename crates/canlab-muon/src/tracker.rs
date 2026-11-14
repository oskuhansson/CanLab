//! A two-plane scintillator tracker, simulated event by event.
//!
//! This is the model the network is trained on, and it is the piece the earlier
//! study was missing: the tracker's angle was a formula with no simulated hits
//! behind it. Here a muon crosses two planes, each plane makes light, the light
//! **scatters** across the plane, and a pixel array reports photoelectrons.
//!
//! # What is modelled, and why each thing is modelled
//!
//! | Effect | Why it is in the model |
//! |---|---|
//! | Scintillation yield | 2 000 photons/mm, so a 5 mm crossing is ~1 MeV |
//! | Inclined path | A muon at 30° crosses 15 % more material and makes 15 % more light |
//! | **Light scattering** | The halo around the hit is what the network reads position from |
//! | Ballistic fraction | Some light reaches the pixel under the hit; the rest diffuses |
//! | Pixel crosstalk | Charge leaks to neighbours, blurring the pattern |
//! | Dark counts | The SiPM fires without light |
//! | Gain spread | No two pixels have the same gain |
//! | Electronic noise | The readout is not noiseless |
//! | Poisson statistics | The light is counted, not measured |
//!
//! # The scattering model, and what it is not
//!
//! The light from a point on a thin scintillator sheet spreads over the sheet.
//! The physically right kernel is a diffusion kernel — a modified Bessel function
//! that decays as `exp(−r/λ)/√r` — and this model uses the exponential part,
//! `exp(−r/λ)`, with `λ` the spreading length. The `1/√r` is a 15 % effect over
//! the range a 40 mm plane covers and is left out rather than approximated badly.
//!
//! The model is a **kernel, not a photon Monte Carlo**: it gives the expected
//! photoelectrons per pixel and draws the Poisson count, rather than tracing
//! individual photons through the sheet. That is the right trade here, because
//! what the network can learn from is the *distribution* — its centroid and its
//! width — and a kernel gives both exactly, where a Monte Carlo gives them with
//! noise. What a kernel cannot give is the correlation between two photons, and
//! nothing in this design reads that.
//!
//! # The two parameters that carry the uncertainty
//!
//! `lambda_mm` and `ballistic_fraction` are not known for a real sheet. They are
//! the parameters [`crate::domain`] randomises, because a network trained on one
//! value of them has learned that value rather than the physics.

use crate::scintillator::Scintillator;

/// A deterministic random stream: PCG32, so a seed reproduces a run exactly.
///
/// The generator the rest of the crate uses is a 64-bit LCG, which is fine for
/// sampling a direction and not fine for 30 000 events with Poisson draws: an LCG
/// with a power-of-two modulus has poor low-bit behaviour, and `next_u32 % n` is
/// exactly where that shows. PCG32 is two more lines and has no such defect.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    increment: u64,
}

impl Rng {
    /// A stream from a seed.
    pub fn new(seed: u64) -> Rng {
        let mut rng = Rng { state: 0, increment: (seed << 1) | 1 };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    /// The next 32 bits.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.increment);
        let shifted = ((old >> 18) ^ old) >> 27;
        let rotation = (old >> 59) as u32;
        (shifted as u32).rotate_right(rotation)
    }

    /// A uniform draw in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u32() as f64) / (u32::MAX as f64 + 1.0)
    }

    /// A uniform draw in `[low, high)`.
    pub fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.uniform()
    }

    /// A standard normal, by Box–Muller.
    pub fn normal(&mut self) -> f64 {
        let first = self.uniform().max(1e-300);
        let second = self.uniform();
        (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
    }

    /// A Poisson draw with mean `mean`.
    ///
    /// Knuth's product method below a mean of 50, and a rounded normal above it.
    /// The switch is stated because it is a change of distribution and not an
    /// optimisation: for a mean of 1 000 the two differ by a part in 10⁴ in the
    /// tails, which is far below the gain spread the model already has.
    pub fn poisson(&mut self, mean: f64) -> f64 {
        // A non-positive mean is refused rather than answered, and `NaN` is
        // refused by the same test because `NaN <= 0.0` is false and
        // `NaN.is_finite()` is not.
        if mean <= 0.0 || !mean.is_finite() {
            return 0.0;
        }
        if mean > 50.0 {
            return (mean + mean.sqrt() * self.normal()).round().max(0.0);
        }
        let limit = (-mean).exp();
        let mut product = 1.0;
        let mut count = 0.0;
        loop {
            product *= self.uniform();
            if product <= limit {
                return count;
            }
            count += 1.0;
            if count > 1_000.0 {
                return count;
            }
        }
    }
}

/// The light response of one plane: what the sheet does with the light it makes.
///
/// Every field is a parameter [`crate::domain`] randomises, because none of them
/// is measured for a real sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightModel {
    /// The spreading length of the scattered light, millimetres.
    pub lambda_mm: f64,
    /// The fraction of light that reaches the pixel under the hit without
    /// scattering.
    pub ballistic_fraction: f64,
    /// The fraction of the emitted light that reaches the pixel array.
    ///
    /// This is the *bulk* transport — scattering and trapping inside the sheet —
    /// and not the interface. The interface is [`LightModel::bond_transmission`],
    /// because the two are different physics and a model that folded them
    /// together could not tell an unbonded sheet from a dim one.
    pub collection: f64,
    /// The fraction of light crossing the bonded interface into the sensor.
    ///
    /// With optical adhesive this is ~0.95; with an air gap it is the escape
    /// cone's 13 %, which is what makes the adhesive worth its 33 EUR. See
    /// [`crate::components::adhesive`].
    pub bond_transmission: f64,
    /// The sensor's photon-detection efficiency.
    pub pde: f64,
    /// The fraction of a pixel's charge that leaks to its four neighbours.
    pub crosstalk: f64,
    /// Dark counts per pixel per event.
    pub dark_counts: f64,
    /// The relative standard deviation of the pixels' gains.
    pub gain_spread: f64,
    /// Electronic noise, in photoelectrons.
    pub noise_pe: f64,
}

impl LightModel {
    /// A nominal sheet: a 15 mm spreading length, half the light ballistic, a
    /// quarter collected and a third detected.
    pub fn nominal() -> LightModel {
        LightModel {
            lambda_mm: 15.0,
            ballistic_fraction: 0.5,
            collection: 0.25,
            bond_transmission: 0.95,
            pde: 0.35,
            crosstalk: 0.03,
            dark_counts: 0.5,
            gain_spread: 0.05,
            noise_pe: 0.5,
        }
    }

    /// Photoelectrons the plane produces from a path of `path_mm`.
    pub fn photoelectrons(&self, path_mm: f64) -> f64 {
        Scintillator::pvt().photons(path_mm) * self.collection * self.bond_transmission * self.pde
    }
}

/// A two-plane tracker's geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracker {
    /// The plane's width, millimetres.
    pub width_mm: f64,
    /// The plane's height, millimetres.
    pub height_mm: f64,
    /// The distance between the planes, millimetres.
    pub gap_mm: f64,
    /// Pixels along each side: `n` gives an `n × n` array.
    pub pixels_per_side: usize,
    /// The plane's thickness, millimetres.
    pub thickness_mm: f64,
}

impl Tracker {
    /// The short tracker: 40 × 48 mm planes, a 20 mm gap, 2 × 2 pixels.
    pub fn short() -> Tracker {
        Tracker { width_mm: 40.0, height_mm: 48.0, gap_mm: 20.0, pixels_per_side: 2, thickness_mm: 5.0 }
    }

    /// The coarse tracker: the same planes with a 40 mm gap.
    pub fn coarse() -> Tracker {
        Tracker { gap_mm: 40.0, ..Tracker::short() }
    }

    /// The fine tracker: 4 × 4 pixels and a 60 mm gap.
    pub fn fine() -> Tracker {
        Tracker { gap_mm: 60.0, pixels_per_side: 4, ..Tracker::short() }
    }

    /// How many pixels one plane has.
    pub fn pixels(&self) -> usize {
        self.pixels_per_side * self.pixels_per_side
    }

    /// One pixel's width, millimetres.
    pub fn pixel_width_mm(&self) -> f64 {
        self.width_mm / self.pixels_per_side as f64
    }

    /// One pixel's height, millimetres.
    pub fn pixel_height_mm(&self) -> f64 {
        self.height_mm / self.pixels_per_side as f64
    }

    /// The centre of pixel `index`, in the plane's own frame, millimetres.
    pub fn pixel_centre(&self, index: usize) -> (f64, f64) {
        let side = self.pixels_per_side;
        let (row, column) = (index / side, index % side);
        let width = self.pixel_width_mm();
        let height = self.pixel_height_mm();
        (
            -self.width_mm / 2.0 + (column as f64 + 0.5) * width,
            -self.height_mm / 2.0 + (row as f64 + 0.5) * height,
        )
    }

    /// Which pixel contains a point, if any.
    pub fn pixel_at(&self, x: f64, y: f64) -> Option<usize> {
        if x.abs() > self.width_mm / 2.0 || y.abs() > self.height_mm / 2.0 {
            return None;
        }
        let side = self.pixels_per_side as f64;
        let column = (((x + self.width_mm / 2.0) / self.width_mm) * side).floor() as usize;
        let row = (((y + self.height_mm / 2.0) / self.height_mm) * side).floor() as usize;
        Some(row.min(self.pixels_per_side - 1) * self.pixels_per_side + column.min(self.pixels_per_side - 1))
    }

    /// The expected fraction of the light that pixel `index` receives from a hit
    /// at `(x, y)`.
    ///
    /// The ballistic part lands on the pixel containing the hit; the rest spreads
    /// as `exp(−r/λ)` from the hit, and the whole is normalised over the array.
    /// The normalisation is what makes the array's edges matter: a hit near a
    /// corner puts its halo partly outside the array, so the pattern's centroid is
    /// pulled inward, and a network that learned the centroid would be biased
    /// without the normalisation.
    pub fn light_fractions(&self, x: f64, y: f64, light: &LightModel) -> Vec<f64> {
        let containing = self.pixel_at(x, y);
        let mut weights = vec![0.0; self.pixels()];
        let mut total = 0.0;
        #[allow(clippy::needless_range_loop)]
        for index in 0..self.pixels() {
            let (px, py) = self.pixel_centre(index);
            let r = (px - x).hypot(py - y);
            let scattered = (-r / light.lambda_mm.max(1e-6)).exp();
            let ballistic = if Some(index) == containing { 1.0 } else { 0.0 };
            let weight = light.ballistic_fraction * ballistic
                + (1.0 - light.ballistic_fraction) * scattered;
            weights[index] = weight;
            total += weight;
        }
        if total > 0.0 {
            for weight in &mut weights {
                *weight /= total;
            }
        }
        weights
    }
}

/// One simulated event: what the two planes saw, and where the muon went.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Photoelectrons per pixel on the first plane.
    pub first: Vec<f64>,
    /// Photoelectrons per pixel on the second plane.
    pub second: Vec<f64>,
    /// The muon's direction: the horizontal components, with `dz < 0`.
    pub dx: f64,
    /// The other horizontal component.
    pub dy: f64,
    /// Where the muon crossed the first plane.
    pub hit: (f64, f64),
}

impl Event {
    /// The direction as an angle from the vertical, degrees.
    pub fn zenith_deg(&self) -> f64 {
        (self.dx.hypot(self.dy)).atan().to_degrees()
    }

    /// The total photoelectrons on both planes.
    pub fn total(&self) -> f64 {
        self.first.iter().sum::<f64>() + self.second.iter().sum::<f64>()
    }
}

/// Simulate one muon through the tracker.
///
/// `zenith_rad` is the angle from the vertical and `azimuth_rad` the compass
/// direction. The muon crosses the first plane at a uniform point and the second
/// at `gap · tan(zenith)` further along; if that lands outside the second plane
/// the event is refused, which is the coincidence acceptance the earlier study
/// computed from a formula and this one computes by rejection.
pub fn simulate(
    tracker: &Tracker,
    light: &LightModel,
    zenith_rad: f64,
    azimuth_rad: f64,
    rng: &mut Rng,
) -> Option<Event> {
    let dx = zenith_rad.sin() * azimuth_rad.cos();
    let dy = zenith_rad.sin() * azimuth_rad.sin();
    let dz = -zenith_rad.cos().max(1e-6);

    // The crossing of the first plane, uniform over it.
    let x1 = rng.range(-tracker.width_mm / 2.0, tracker.width_mm / 2.0);
    let y1 = rng.range(-tracker.height_mm / 2.0, tracker.height_mm / 2.0);
    // The second plane is `gap` further along the track.
    let travel = tracker.gap_mm / dz.abs();
    let x2 = x1 + dx * travel;
    let y2 = y1 + dy * travel;
    // A muon that misses the second plane has no direction: this is the
    // coincidence, and it is a rejection rather than a factor.
    if x2.abs() > tracker.width_mm / 2.0 || y2.abs() > tracker.height_mm / 2.0 {
        return None;
    }

    // The path through each plane is the thickness over the cosine, so an
    // inclined muon deposits more energy and makes more light.
    let path_mm = tracker.thickness_mm / zenith_rad.cos().max(1e-6);
    let total_pe = light.photoelectrons(path_mm);

    let first = read_plane(tracker, light, total_pe, x1, y1, rng);
    let second = read_plane(tracker, light, total_pe, x2, y2, rng);
    Some(Event { first, second, dx, dy, hit: (x1, y1) })
}

/// One plane's readout from a hit at `(x, y)`.
fn read_plane(
    tracker: &Tracker,
    light: &LightModel,
    total_pe: f64,
    x: f64,
    y: f64,
    rng: &mut Rng,
) -> Vec<f64> {
    let fractions = tracker.light_fractions(x, y, light);
    let pixels = tracker.pixels();
    let mut charge = vec![0.0; pixels];
    #[allow(clippy::needless_range_loop)]
    for index in 0..pixels {
        let mean = total_pe * fractions[index];
        // The gain spread is a property of the pixel, redrawn per event because
        // the domain randomisation treats it as unknown.
        let gain = 1.0 + light.gain_spread * rng.normal();
        let dark = rng.poisson(light.dark_counts);
        charge[index] = rng.poisson(mean) * gain + dark;
    }
    // Crosstalk: a fraction of each pixel's charge leaks to its four neighbours.
    if light.crosstalk > 0.0 {
        let leaked: Vec<f64> = charge.iter().map(|value| value * light.crosstalk).collect();
        for index in 0..pixels {
            charge[index] -= leaked[index];
        }
        let side = tracker.pixels_per_side;
        #[allow(clippy::needless_range_loop)]
        for index in 0..pixels {
            let (row, column) = (index / side, index % side);
            let mut share = 0.0;
            let mut neighbours = 0;
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (r, c) = (row as i32 + dr, column as i32 + dc);
                if r >= 0 && c >= 0 && (r as usize) < side && (c as usize) < side {
                    neighbours += 1;
                }
            }
            if neighbours > 0 {
                share = leaked[index] / neighbours as f64;
            }
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (r, c) = (row as i32 + dr, column as i32 + dc);
                if r >= 0 && c >= 0 && (r as usize) < side && (c as usize) < side {
                    charge[r as usize * side + c as usize] += share;
                }
            }
        }
    }
    // Electronic noise, then the floor: a pixel cannot report negative charge.
    for value in &mut charge {
        *value += light.noise_pe * rng.normal();
        *value = value.max(0.0);
    }
    charge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_stream_is_reproducible_and_uniform() {
        // The same seed gives the same stream, and a different seed does not.
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let first: Vec<f64> = (0..8).map(|_| a.uniform()).collect();
        let second: Vec<f64> = (0..8).map(|_| b.uniform()).collect();
        let third: Vec<f64> = (0..8).map(|_| c.uniform()).collect();
        assert_eq!(first, second);
        assert_ne!(first, third);
        // The mean of a uniform draw is a half and its range is the unit
        // interval: a generator that returned `0.0` would pass the equality.
        let mut rng = Rng::new(1);
        let draws: Vec<f64> = (0..20_000).map(|_| rng.uniform()).collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        assert!((mean - 0.5).abs() < 0.01, "{mean}");
        assert!(draws.iter().all(|value| (0.0..1.0).contains(value)));
    }

    #[test]
    fn the_poisson_mean_and_variance_are_its_mean() {
        // Both branches of the sampler: below the switch and above it.
        for mean in [0.5, 5.0, 30.0, 200.0, 2_000.0] {
            let mut rng = Rng::new(mean as u64 + 1);
            let draws: Vec<f64> = (0..20_000).map(|_| rng.poisson(mean)).collect();
            let average = draws.iter().sum::<f64>() / draws.len() as f64;
            let variance = draws.iter().map(|v| (v - average) * (v - average)).sum::<f64>()
                / (draws.len() - 1) as f64;
            let tolerance = (mean.sqrt() * 4.0 / (20_000f64).sqrt()).max(0.05);
            assert!((average - mean).abs() < tolerance.max(mean * 0.02), "mean {mean}: {average}");
            // A Poisson's variance is its mean, and a sampler that returned the
            // mean would have zero variance and pass the first assertion.
            assert!((variance - mean).abs() < (mean * 0.1).max(0.3), "mean {mean}: var {variance}");
        }
        assert_eq!(Rng::new(1).poisson(0.0), 0.0);
        assert_eq!(Rng::new(1).poisson(-1.0), 0.0);
    }

    #[test]
    fn the_light_fractions_are_a_distribution_and_peak_at_the_hit() {
        // Every hit puts all of its light somewhere, and the pixel under the hit
        // gets the largest share — that is what makes the pattern a position.
        let tracker = Tracker::short();
        let light = LightModel::nominal();
        for (x, y) in [(0.0, 0.0), (5.0, 7.0), (-15.0, 20.0), (19.0, 23.0)] {
            let fractions = tracker.light_fractions(x, y, &light);
            assert_eq!(fractions.len(), tracker.pixels());
            let total: f64 = fractions.iter().sum();
            assert!((total - 1.0).abs() < 1e-12, "{total}");
            assert!(fractions.iter().all(|value| *value >= 0.0));
            let brightest = fractions
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).expect("no NaN"))
                .map(|(index, _)| index)
                .expect("a pixel");
            assert_eq!(Some(brightest), tracker.pixel_at(x, y), "at ({x}, {y})");
        }
    }

    #[test]
    fn scattering_spreads_the_light_and_the_ballistic_fraction_sharpens_it() {
        // The two parameters that carry the uncertainty, and the effect each has
        // on the pattern: a longer spreading length puts more light further out,
        // and a larger ballistic fraction puts more under the hit.
        let tracker = Tracker::fine();
        let base = LightModel::nominal();
        let sharp = LightModel { lambda_mm: 3.0, ..base };
        let soft = LightModel { lambda_mm: 40.0, ..base };
        let (x, y) = (0.0, 0.0);
        let containing = tracker.pixel_at(x, y).expect("inside");
        let sharp_peak = tracker.light_fractions(x, y, &sharp)[containing];
        let soft_peak = tracker.light_fractions(x, y, &soft)[containing];
        assert!(sharp_peak > soft_peak, "{sharp_peak} vs {soft_peak}");
        // And the ballistic fraction does the same thing by another route.
        let ballistic = LightModel { ballistic_fraction: 0.95, ..base };
        let diffuse = LightModel { ballistic_fraction: 0.05, ..base };
        assert!(
            tracker.light_fractions(x, y, &ballistic)[containing]
                > tracker.light_fractions(x, y, &diffuse)[containing]
        );
    }

    #[test]
    fn a_muon_that_misses_the_second_plane_is_refused() {
        // The coincidence, by rejection rather than by a factor: a muon at 60
        // degrees over a 20 mm gap drifts 35 mm, which is outside a 40 mm plane.
        let tracker = Tracker::short();
        let light = LightModel::nominal();
        let mut rng = Rng::new(3);
        let mut accepted = 0;
        let trials = 2_000;
        for _ in 0..trials {
            if simulate(&tracker, &light, 60.0f64.to_radians(), 0.0, &mut rng).is_some() {
                accepted += 1;
            }
        }
        // 20 x tan(60) = 34.6 mm of drift against a 20 mm half-width, so only
        // the crossings that start within 5.4 mm of the near edge survive: about
        // 13 % in x and all of y, since the azimuth is zero.
        let fraction = accepted as f64 / trials as f64;
        assert!(fraction < 0.25, "{accepted} of {trials} at 60 degrees");
        assert!(fraction > 0.05, "{accepted} of {trials}: some must survive");
        // A vertical muon always reaches the second plane.
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            assert!(simulate(&tracker, &light, 0.0, 0.0, &mut rng).is_some());
        }
    }

    #[test]
    fn an_inclined_muon_makes_more_light_than_a_vertical_one() {
        // The path is the thickness over the cosine, so a 45 degree muon crosses
        // 41 % more material. Without this the network could not tell a bright
        // vertical muon from a dim inclined one, and the total charge would be a
        // misleading feature.
        let tracker = Tracker::short();
        let light = LightModel { ballistic_fraction: 1.0, crosstalk: 0.0, dark_counts: 0.0, gain_spread: 0.0, noise_pe: 0.0, ..LightModel::nominal() };
        let mut rng = Rng::new(11);
        let mut vertical = 0.0;
        let mut vertical_count = 0.0;
        let mut inclined = 0.0;
        let mut inclined_count = 0.0;
        let trials = 500;
        for _ in 0..trials {
            if let Some(event) = simulate(&tracker, &light, 0.0, 0.0, &mut rng) {
                vertical += event.total();
                vertical_count += 1.0;
            }
            if let Some(event) = simulate(&tracker, &light, 45.0f64.to_radians(), 0.0, &mut rng) {
                inclined += event.total();
                inclined_count += 1.0;
            }
        }
        // The mean per accepted event, not the sum: an inclined muon is rejected
        // more often, and a sum would divide the path-length ratio by the
        // acceptance ratio and measure neither.
        assert!(vertical_count > 100.0 && inclined_count > 100.0);
        let ratio = (inclined / inclined_count) / (vertical / vertical_count);
        assert!((ratio - 1.41).abs() < 0.10, "{ratio}");
    }

    #[test]
    fn crosstalk_moves_charge_to_the_neighbours_and_conserves_it() {
        // A 4 x 4 array so there are interior neighbours to leak to. The total
        // charge is conserved, which is what makes crosstalk a blur and not a
        // loss.
        let tracker = Tracker::fine();
        let clean = LightModel { crosstalk: 0.0, dark_counts: 0.0, gain_spread: 0.0, noise_pe: 0.0, ballistic_fraction: 1.0, ..LightModel::nominal() };
        let leaky = LightModel { crosstalk: 0.4, ..clean };
        let mut rng = Rng::new(5);
        let a = simulate(&tracker, &clean, 0.0, 0.0, &mut rng).expect("an event");
        let mut rng = Rng::new(5);
        let b = simulate(&tracker, &leaky, 0.0, 0.0, &mut rng).expect("an event");
        let sum_a: f64 = a.first.iter().sum();
        let sum_b: f64 = b.first.iter().sum();
        assert!((sum_a - sum_b).abs() < 1e-9, "{sum_a} vs {sum_b}");
        // And the pattern is flatter: the peak pixel has less of the total.
        let peak_a = a.first.iter().cloned().fold(0.0f64, f64::max) / sum_a.max(1.0);
        let peak_b = b.first.iter().cloned().fold(0.0f64, f64::max) / sum_b.max(1.0);
        assert!(peak_b < peak_a, "{peak_b} vs {peak_a}");
    }

    #[test]
    fn the_acceptance_matches_the_formula_the_study_used() {
        // The rejection sampler and `coincidence_acceptance` are two routes to one
        // number, and this is the test that ties them: the formula was the only
        // thing the earlier study had, and it is now checked against events.
        use crate::designs::coincidence_acceptance;
        let tracker = Tracker::short();
        let light = LightModel::nominal();
        let mut rng = Rng::new(2026);
        let trials = 20_000;
        let mut accepted = 0;
        for _ in 0..trials {
            // The cos²θ flux, sampled by inverting its CDF.
            let u = rng.uniform();
            let cosine = (1.0 - u).cbrt();
            let zenith = cosine.acos();
            let azimuth = rng.range(0.0, std::f64::consts::TAU);
            if simulate(&tracker, &light, zenith, azimuth, &mut rng).is_some() {
                accepted += 1;
            }
        }
        let measured = accepted as f64 / trials as f64;
        let formula = coincidence_acceptance(tracker.width_mm, tracker.height_mm, tracker.gap_mm);
        assert!((measured - formula).abs() < 0.02, "measured {measured} vs formula {formula}");
    }
}
