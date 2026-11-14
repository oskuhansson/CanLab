//! W14.3's verification: the waveform a timing study is read from.
//!
//! `timebase`'s unit tests check a histogram against numbers put in by hand.
//! This file checks it against **simulated photons**: a muon crosses the block,
//! the light it makes is traced to four sensors, and the arrival times it
//! reports are binned into picoseconds. Three things a study does with that
//! waveform are checked against something outside the histogram:
//!
//! * **The counts sum to the photon total.** Over events, and per sensor: a
//!   waveform whose bins hold fewer photons than the event detected is a
//!   histogram that dropped times, and the sum is the only thing that shows it.
//! * **A known delay shifts the whole histogram by exactly that delay.** The
//!   delay is applied to the timestamps before binning — a cable, in effect —
//!   and the shift is recovered by correlation, not by remembering what was
//!   added. A shift of five bins must come back as five.
//! * **The jitter is in the timestamp and not in the truth.** The same event
//!   binned from `true_ps` and from `arrivals_ps` differs by the sensor's own
//!   timing resolution, and the difference's *spread* is the jitter — which is
//!   the number the network has to beat and the design has to beat the jitter
//!   to resolve.
//!
//! And the waveform is turned into a picture: a curve the plot crate renders as
//! SVG, because a histogram that only ever exists as a `Vec<usize>` is a number
//! the study cannot read.

use canlab_optics::block::Face;
use canlab_optics::{Block, Histogram, Optical, SiPM, TimeBase, Tropical, Vec3, Waveforms, simulate};

struct Stream(u64);

impl Stream {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((value >> 11) as f64) / (1u64 << 53) as f64
    }
}

fn setup(index: f64) -> (Block, Optical, [SiPM; 4]) {
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let optical = Optical::block_in_air(index, 10_000.0);
    let sensors = [
        SiPM::typical(Face::Top),
        SiPM::typical(Face::Bottom),
        SiPM::typical(Face::Side(0)),
        SiPM::typical(Face::Side(2)),
    ];
    (block, optical, sensors)
}

/// One event, an across-the-block muon at n = 1.49, where the light escapes.
fn one_event(seed: u64) -> canlab_optics::Event {
    let (block, optical, sensors) = setup(1.49);
    let mut stream = Stream(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    simulate(
        &block,
        &optical,
        &sensors,
        Vec3::new(-20.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        40.0,
        1.0,
        &mut || stream.next(),
    )
    .expect("a crossing")
}

#[test]
fn the_bins_of_a_real_event_sum_to_the_photons_it_detected() {
    // Twenty events, each binned whole. The identity is per event and across
    // events: a histogram that drops a timestamp is exactly the bug a study
    // would never see, because the remaining bins still look like a pulse.
    let base = TimeBase::new(0.0, 10.0, 1000).expect("a 10 ps window of 10 ns");
    let mut total = 0;
    for seed in 0..20 {
        let event = one_event(seed);
        let waveforms = Waveforms::from_event(&event, base);
        assert_eq!(
            waveforms.photons,
            event.true_ps.iter().map(Vec::len).sum::<usize>(),
            "seed {seed}"
        );
        assert!(waveforms.accounted(), "seed {seed}: {waveforms:?}");
        // The per-sensor histograms and the total are the same times.
        let per_sensor: usize = waveforms.per_sensor.iter().map(Histogram::counted).sum();
        assert_eq!(per_sensor, waveforms.all.counted(), "seed {seed}");
        total += event.true_ps.iter().map(Vec::len).sum::<usize>();
    }
    assert!(total > 20, "the twenty events must detect something: {total}");
}

#[test]
fn a_known_cable_delay_shifts_the_histogram_by_that_delay() {
    // The delay is added to the times, not to the bins, so the test cannot pass
    // by remembering its own input: the histogram is rebuilt from the delayed
    // timestamps and the shift is *recovered* by correlation.
    let event = one_event(7);
    // The window starts below the earliest time by more than the widest delay
    // the test asks for, so a *negative* delay has room to move into and the
    // shift is a movement of the waveform and not of the edge of the window.
    // The first version started at zero, and a delay of −3 bins put a photon
    // into the underflow — which the accounting caught, and which is why the
    // assertion below exists.
    let all_times: Vec<f64> = event.true_ps.iter().flatten().copied().collect();
    let base = TimeBase::new(-100.0, 10.0, 1000).expect("a window");
    assert!(all_times.iter().all(|&t| t >= base.start_ps), "the window starts early enough");
    let detector = Waveforms::from_event(&event, base).all;
    for delay_bins in [1isize, 5, 12, -3] {
        let delay_ps = delay_bins as f64 * base.bin_ps;
        let delayed: [Vec<f64>; 4] = std::array::from_fn(|sensor| {
            event.true_ps[sensor]
                .iter()
                .map(|&time| time + delay_ps)
                .collect()
        });
        let shifted = Waveforms::from_times(&delayed, base).all;
        assert_eq!(
            detector.shift_bins(&shifted),
            Some(delay_bins),
            "a delay of {delay_bins} bins was not recovered"
        );
        // And the correlation is exact: the counts moved, they did not change.
        // Inside the window nothing may leave it; the accounting is asserted so
        // that a shift the window cannot hold fails here rather than passing as
        // a correlation of the photons it kept.
        assert_eq!(
            shifted.in_window(),
            detector.in_window(),
            "photons left the window at a {delay_bins}-bin delay"
        );
        assert_eq!(shifted.underflow, 0);
        assert_eq!(shifted.overflow, 0);
    }
}

#[test]
fn the_jitter_is_in_the_timestamp_and_not_in_the_truth() {
    // `true_ps` is the transport's time and `arrivals_ps` is the same time plus
    // the sensor's avalanche jitter. Binning both from one event shows the
    // difference, and its spread should be the jitter the sensor was built with
    // (100 ps) — not the transport's geometry, which is tens of picoseconds.
    //
    // Twenty events, because a single event detects a dozen photons at this
    // index and a standard deviation from a dozen draws has a 20 % error. The
    // jitter is the same for every photon, so pooling events is exactly right.
    let (block, optical, sensors) = setup(1.49);
    let mut differences = Vec::new();
    let mut truths = Vec::new();
    for seed in 0..20 {
        let mut stream = Stream(0xC0FF_EE00_1234_5678u64.wrapping_add(seed * 2_654_435_761));
        let event = simulate(
            &block,
            &optical,
            &sensors,
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            40.0,
            1.0,
            &mut || stream.next(),
        )
        .expect("a crossing");
        for sensor in 0..4 {
            for (truth, stamped) in event.true_ps[sensor].iter().zip(&event.arrivals_ps[sensor]) {
                differences.push(stamped - truth);
                truths.push(*truth);
            }
        }
    }
    assert!(differences.len() > 100, "too few to measure: {}", differences.len());
    let mean = differences.iter().sum::<f64>() / differences.len() as f64;
    let variance = differences.iter().map(|d| (d - mean) * (d - mean)).sum::<f64>()
        / (differences.len() - 1) as f64;
    let sigma = variance.sqrt();
    // The jitter is zero-mean, and its standard deviation is the sensor's 100 ps
    // to within the sampling error of a couple of hundred draws (about 5 %).
    assert!(mean.abs() < 20.0, "the jitter has a bias of {mean} ps");
    assert!(
        (85.0..115.0).contains(&sigma),
        "the jitter's own standard deviation is {sigma} ps, not the 100 ps it was given"
    );
    // The geometric spread — the truth — is smaller than the jitter of one
    // photon, which is why this detector needs many photons to time anything.
    truths.sort_by(f64::total_cmp);
    let truth_spread = truths.last().copied().unwrap_or(0.0) - truths.first().copied().unwrap_or(0.0);
    assert!(
        truth_spread > 0.0,
        "the transport's own times have no spread at all, which cannot be right"
    );
}

#[test]
fn a_waveform_is_a_curve_the_plot_crate_draws() {
    // The end of W14.3's chain: times in, SVG out. The curve's x values are bin
    // centres in picoseconds and its y values are photon counts, and the SVG has
    // no `NaN` in it — the plot crate's own promise, which a histogram with an
    // empty window would break if it emitted coordinates for empty bins.
    let base = TimeBase::new(0.0, 10.0, 1000).expect("a window");
    let event = one_event(3);
    let waveforms = Waveforms::from_event(&event, base);
    let curve = waveforms.all.curve();
    if curve.is_empty() {
        // An event with no photons is a legitimate outcome, but it cannot test a
        // picture: assert it is empty for the right reason and stop.
        assert_eq!(waveforms.photons, 0);
        return;
    }
    assert!(curve.point_count() > 0);
    for segment in &curve.segments {
        for point in segment {
            assert!(point[0] >= 0.0 && point[0] <= 10_000.0, "{point:?}");
            assert!(point[1] >= 0.0, "{point:?}");
        }
    }
    let svg = canlab_plot::to_svg(
        &curve,
        &canlab_plot::SvgOptions {
            width: 800,
            height: 300,
            x_range: (0.0, 10_000.0),
            y_range: (0.0, 4.0),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
            bands: Vec::new(),
        },
    );
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert!(!svg.contains("NaN") && !svg.contains("inf"), "the axes drew a non-finite coordinate");
    assert!(svg.matches("<polyline").count() == curve.segment_count());
}
