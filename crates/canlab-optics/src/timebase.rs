//! The time base: arrival times binned into picoseconds, and the curve they make.
//!
//! The transport reports a time in picoseconds and the sensors report their
//! stamps in picoseconds; this module is where those numbers become a
//! *histogram* — the waveform a person reads, the input to a fit, and the thing
//! a design change moves.
//!
//! # Why the bins are integers
//!
//! The study's window is a nanosecond and its resolution is a picosecond, so
//! `f64` at 500 ps resolves 1.1·10⁻¹³ s — six orders of magnitude finer than the
//! bin it is asked for. There is no accuracy argument for floating-point bounds;
//! there *is* a correctness argument for integers: two bins computed from
//! floats can agree on an edge and disagree on which bin an event falls in,
//! depending on the rounding of `(t − lo)/w`. Binning is done once, here, by
//! multiplying a picosecond time by the number of bins per picosecond and
//! flooring — and a time is in bins from the window's start, not from zero.
//!
//! # The two things a histogram must never do
//!
//! * **Lose a photon silently.** Every bin count is accounted for against the
//!   number of times put in, and a time outside the window is counted as
//!   *outside*, not dropped. [`Histogram::underflow`] and
//!   [`Histogram::overflow`] are the counts, and a waveform whose counts sum to
//!   less than its input is a bug the caller can see.
//! * **Move a photon's time by a rounding.** The bin index of `t` is
//!   `floor((t − start)·bins_per_ps)`, so a time on an edge falls in the upper
//!   bin and a time anywhere else falls where it belongs. A cable delay of
//!   exactly one bin shifts the whole histogram by one bin; a delay of half a
//!   bin is reported as spreading two of them, which is what a detector with a
//!   half-bin delay actually records.

use crate::detector::Event;

/// Where a histogram starts, how wide its bins are, and how many.
///
/// Constructed through [`TimeBase::ideal`], [`TimeBase::new`] or
/// [`TimeBase::default`]; the fields are readable and the invariants are
/// checked at construction, so a histogram never has to ask whether its own
/// options make sense.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeBase {
    /// The left edge of the first bin, picoseconds. A time before this is an
    /// underflow.
    pub start_ps: f64,
    /// The width of one bin, picoseconds. At least `1e-9`.
    pub bin_ps: f64,
    /// The number of bins. At least one.
    pub bins: usize,
}

/// A histogram window that cannot be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBaseError {
    /// A start, a width or a bin count that is not a usable number.
    NotFinite,
    /// A bin width that is not positive.
    NotPositive,
    /// A bin count of zero: a window with no bins has nowhere to put a time.
    NoBins,
}

impl std::fmt::Display for TimeBaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeBaseError::NotFinite => write!(f, "a descriptor of a time window must be finite"),
            TimeBaseError::NotPositive => write!(f, "a bin width must be positive"),
            TimeBaseError::NoBins => write!(f, "a time window needs at least one bin"),
        }
    }
}

impl std::error::Error for TimeBaseError {}

impl TimeBase {
    /// The base the roadmap's W14.3 asks for: 1 ps bins over a nanosecond.
    ///
    /// One nanosecond is 1000 bins of 1 ps, which is the right frame for a
    /// detector whose whole transit is 100–500 ps and whose Cherenkov light
    /// leaves the block within 30 ps of the muon.
    pub fn default_base() -> TimeBase {
        TimeBase {
            start_ps: 0.0,
            bin_ps: 1.0,
            bins: 1000,
        }
    }

    /// A base with explicit edges.
    ///
    /// `None` for a bin width that is not positive, a count of zero, or any
    /// value that is not finite: a histogram of a `NaN` window would put every
    /// time in the same bin or none at all, and either one looks like data.
    pub fn new(start_ps: f64, bin_ps: f64, bins: usize) -> Result<TimeBase, TimeBaseError> {
        if !start_ps.is_finite() || !bin_ps.is_finite() {
            return Err(TimeBaseError::NotFinite);
        }
        // `bin_ps <= 0.0` and not `!(bin_ps > 0.0)`: the two differ only for a
        // `NaN`, and a `NaN` width is already refused as not finite above, so the
        // plain comparison says what it means.
        if bin_ps <= 0.0 {
            return Err(TimeBaseError::NotPositive);
        }
        if bins == 0 {
            return Err(TimeBaseError::NoBins);
        }
        Ok(TimeBase {
            start_ps,
            bin_ps,
            bins,
        })
    }

    /// A window just wide enough for `times`, with bins of about `bin_ps`.
    ///
    /// The start is the earliest time rounded down to a bin edge and the end is
    /// the latest rounded up, so no time is an underflow or an overflow by
    /// construction. An empty slice gives the default base, because a window
    /// around no times has no answer that is better than the standard one.
    pub fn ideal(times: &[f64], bin_ps: f64) -> Result<TimeBase, TimeBaseError> {
        if times.is_empty() {
            return TimeBase::new(0.0, bin_ps, 1000);
        }
        TimeBase::new(0.0, bin_ps, 1)?; // validate `bin_ps` before using it
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for &time in times {
            if !time.is_finite() {
                return Err(TimeBaseError::NotFinite);
            }
            low = low.min(time);
            high = high.max(time);
        }
        let start = (low / bin_ps).floor() * bin_ps;
        // At least two bins, so a single time is not a window of zero width.
        let span_bins = (((high - start) / bin_ps).ceil() as usize + 1).max(2);
        TimeBase::new(start, bin_ps, span_bins)
    }

    /// The right edge of the last bin, picoseconds.
    pub fn end_ps(&self) -> f64 {
        self.start_ps + self.bin_ps * self.bins as f64
    }

    /// Which bin a time belongs to, or `None` when it is outside the window.
    ///
    /// A time exactly on an edge belongs to the bin that *starts* there, and the
    /// right edge itself belongs to the last bin so that a window closed on both
    /// ends loses nothing.
    pub fn bin_of(&self, time_ps: f64) -> Option<usize> {
        if !time_ps.is_finite() || time_ps < self.start_ps {
            return None;
        }
        // Rounding first: a time that should be on a bin edge is the float
        // nearest to it, and the division below is what turns "nearest the edge"
        // into "just below the edge" — `0.3/0.1` is 2.9999999999999996. Rounding
        // to a millionth of the offset before flooring moves that to 3, and moves
        // nothing that is genuinely between two edges, because a millionth of a
        // bin is 10⁻⁶ ps at this resolution.
        let offset = round_to_millionth((time_ps - self.start_ps) / self.bin_ps);
        // `round_to_millionth` is what makes an exact multiple of the bin width
        // land on the edge and not one bin below it, which `(t − start)/w` alone
        // does: `0.3/0.1` is 2.9999999999999996.
        let index = offset.floor();
        if index >= self.bins as f64 {
            // A time on the closing edge is the last bin's, not the overflow's.
            if index == self.bins as f64 && (offset - index).abs() < 1e-9 {
                return Some(self.bins - 1);
            }
            return None;
        }
        Some(index as usize)
    }
}

/// `x` rounded to the nearest millionth.
///
/// The bin index is a floor of a float division, and the division is exactly
/// where a time on an edge becomes a time just below it. A millionth is far
/// below a bin and far above the division's own error, which is at most a few
/// ulps — 2·10⁻¹⁶ at an offset of a thousand.
fn round_to_millionth(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    // `round_ties_even` and not `round`: the latter breaks ties away from zero,
    // so a time a millionth of a bin below an edge was rounded *up* onto it and
    // the "just below" case was silently moved to the bin above. Ties to even
    // rounds an exact half to the nearest even integer, and half a millionth is
    // 5·10⁻⁷ of a bin — far inside the noise this rounding is here to absorb.
    (x * 1e6).round_ties_even() / 1e6
}

impl Default for TimeBase {
    fn default() -> Self {
        TimeBase::default_base()
    }
}

/// Arrival times in bins, with the times outside the window counted.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// The window the times were binned into.
    pub base: TimeBase,
    /// The count in each bin, in bin order. `counts.len() == base.bins`.
    pub counts: Vec<usize>,
    /// Times before the window.
    pub underflow: usize,
    /// Times after the window.
    pub overflow: usize,
    /// Times that are not numbers at all — `NaN` or infinite. Kept in their own
    /// count and not in the underflow's: a `NaN` timestamp is not an early
    /// photon, it is a bug in whatever produced it, and a caller reading the
    /// underflow must not have to wonder which it was.
    pub non_finite: usize,
}

impl Histogram {
    /// Bin `times` into `base`.
    pub fn new(base: TimeBase, times: &[f64]) -> Histogram {
        let mut counts = vec![0usize; base.bins];
        let mut underflow = 0;
        let mut overflow = 0;
        let mut non_finite = 0;
        for &time in times {
            match base.bin_of(time) {
                Some(index) => counts[index] += 1,
                // A time that is not a number is not a time, and putting it in
                // the underflow or the overflow would say it was early or late.
                None if !time.is_finite() => non_finite += 1,
                None if time < base.start_ps => underflow += 1,
                None => overflow += 1,
            }
        }
        Histogram {
            base,
            counts,
            underflow,
            overflow,
            non_finite,
        }
    }

    /// Every time that was put in: binned, underflowed, overflowed or refused.
    ///
    /// The identity `counted() == times.len()` is the one thing a histogram
    /// must satisfy, and it is the one a dropped time violates.
    pub fn counted(&self) -> usize {
        self.in_window() + self.underflow + self.overflow + self.non_finite
    }

    /// The total in the bins alone.
    pub fn in_window(&self) -> usize {
        self.counts.iter().sum()
    }

    /// The bin that holds the most photons, and how many. Ties go to the lower
    /// bin, which is deterministic.
    pub fn peak(&self) -> Option<(usize, usize)> {
        self.counts
            .iter()
            .enumerate()
            .fold(None, |best: Option<(usize, usize)>, (index, &count)| match best {
                Some((_, best_count)) if best_count >= count => best,
                _ => Some((index, count)),
            })
            .filter(|&(_, count)| count > 0)
    }

    /// The first moment of the binned distribution, in picoseconds, measured
    /// from the *bin centres*. `None` when nothing is in the window.
    pub fn mean_ps(&self) -> Option<f64> {
        let total = self.in_window();
        if total == 0 {
            return None;
        }
        let weighted: f64 = self
            .counts
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                (self.base.start_ps + (index as f64 + 0.5) * self.base.bin_ps) * count as f64
            })
            .sum();
        Some(weighted / total as f64)
    }

    /// The shift between two histograms of the same length, in bins.
    ///
    /// The bin `k` that maximises `Σᵢ a[i]·b[i+k]` — a discrete convolution, not
    /// a fit. It is how a known delay is *measured*: shift a waveform by five
    /// bins and this says five, not "about five", because the correlation of two
    /// equal signals is exactly maximal at their shift.
    pub fn shift_bins(&self, other: &Histogram) -> Option<isize> {
        if self.base.bins != other.base.bins || self.in_window() == 0 || other.in_window() == 0 {
            return None;
        }
        let n = self.base.bins as isize;
        let mut best = (0isize, 0f64);
        for shift in -(n - 1)..n {
            let mut score = 0f64;
            for i in 0..n {
                let j = i + shift;
                if j < 0 || j >= n {
                    continue;
                }
                score += (self.counts[i as usize] * other.counts[j as usize]) as f64;
            }
            // Strictly greater, so a tie keeps the smaller-magnitude shift: two
            // waveforms that are flat correlate equally at every shift, and the
            // answer for them is "no shift", not the last one examined.
            if score > best.1 || (best.1 == 0.0 && score == 0.0 && shift == 0) {
                best = (shift, score);
            }
        }
        if best.1 == 0.0 {
            return None;
        }
        Some(best.0)
    }

    /// The histogram as a polyline, for [`canlab_plot::to_svg`].
    ///
    /// The curve is `x = centre of bin`, `y = count`, in *data* units: one
    /// segment through the bin centres, and no segment spanning a gap of empty
    /// bins except where the bins between are empty and far apart. A waveform is
    /// drawn as a curve through its centres and not as bars because a bar chart
    /// of a thousand bins is a smear: the curve's shape is the timing
    /// distribution and its peak is where the photons arrived.
    pub fn curve(&self) -> canlab_plot::Curve {
        let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut current: Vec<[f64; 2]> = Vec::new();
        for (index, &count) in self.counts.iter().enumerate() {
            let x = self.base.start_ps + (index as f64 + 0.5) * self.base.bin_ps;
            if count == 0 {
                // An empty bin ends the segment only when it is not merely a
                // gap in a smooth tail: one empty bin between two populated ones
                // is a dip high above the noise, and it is real. A run of empty
                // bins longer than a tenth of the window is a gap with nothing
                // between it, and the two sides are separate peaks.
                let run = self.counts[index..]
                    .iter()
                    .take_while(|&&next| next == 0)
                    .count();
                if run > (self.base.bins / 10).max(1) && !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                continue;
            }
            if current.is_empty() && index > 0 && !segments.is_empty() {
                // The previous segment ended in a gap; the first point of the
                // new one is the last populated bin before the gap, so the two
                // peaks are joined by a line rather than drawn as two dots.
                if let Some(last) = self.counts[..index].iter().rposition(|&c| c > 0) {
                    current.push([
                        self.base.start_ps + (last as f64 + 0.5) * self.base.bin_ps,
                        0.0,
                    ]);
                }
            }
            current.push([x, count as f64]);
        }
        if !current.is_empty() {
            segments.push(current);
        }
        canlab_plot::Curve { segments }
    }
}

/// The four sensors' histograms for one event, and the ones for many.
///
/// `waveforms` keeps a histogram per sensor, which is what a figure shows;
/// [`Waveforms::all`] is the sum, which is what a total light yield is quoted
/// from. Both are from the same times, so they cannot disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveforms {
    /// One histogram per sensor, in the detector's sensor order.
    pub per_sensor: [Histogram; 4],
    /// The sum of the four.
    pub all: Histogram,
    /// The sensor order's total: how many photons were put into
    /// `per_sensor` across all four.
    pub photons: usize,
}

impl Waveforms {
    /// Bin one event's timestamps.
    ///
    /// `true_ps` rather than `arrivals_ps`: the histogram is the *timing*
    /// measurement, and the sensor's own jitter is a separate error term that
    /// belongs beside it, not inside it. [`Waveforms::jittered`] bins the other
    /// one, so a caller can compare them.
    pub fn from_event(event: &Event, base: TimeBase) -> Waveforms {
        Waveforms::from_times(&event.true_ps, base)
    }

    /// Bin one event's *timestamps*, with the sensors' jitter included.
    pub fn jittered(event: &Event, base: TimeBase) -> Waveforms {
        Waveforms::from_times(&event.arrivals_ps, base)
    }

    /// Bin four lists of times.
    pub fn from_times(times: &[Vec<f64>; 4], base: TimeBase) -> Waveforms {
        let per_sensor = [
            Histogram::new(base, &times[0]),
            Histogram::new(base, &times[1]),
            Histogram::new(base, &times[2]),
            Histogram::new(base, &times[3]),
        ];
        let mut counts = vec![0usize; base.bins];
        for histogram in &per_sensor {
            for (index, &count) in histogram.counts.iter().enumerate() {
                counts[index] += count;
            }
        }
        let underflow = per_sensor.iter().map(|h| h.underflow).sum();
        let overflow = per_sensor.iter().map(|h| h.overflow).sum();
        let non_finite = per_sensor.iter().map(|h| h.non_finite).sum();
        let photons = times.iter().map(Vec::len).sum();
        Waveforms {
            per_sensor,
            all: Histogram {
                base,
                counts,
                underflow,
                overflow,
                non_finite,
            },
            photons,
        }
    }

    /// Whether the binned photons account for every photon the event had.
    ///
    /// The identity the whole module exists to make visible: `photons` is what
    /// the event reported, and `all` must hold exactly that many.
    pub fn accounted(&self) -> bool {
        self.all.counted() == self.photons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start: f64, width: f64, bins: usize) -> TimeBase {
        TimeBase::new(start, width, bins).expect("a valid base")
    }

    #[test]
    fn the_default_base_is_picosecond_bins_over_a_nanosecond() {
        let base = TimeBase::default_base();
        assert_eq!(base.bin_ps, 1.0);
        assert_eq!(base.bins, 1000);
        assert_eq!(base.end_ps(), 1000.0);
    }

    #[test]
    fn a_time_exactly_on_an_edge_lands_in_the_bin_that_starts_there() {
        // The rounding that this test exists for: `0.3/0.1` is 2.9999999999999996
        // in `f64` and floors to 2, so a bin edge computed by division alone
        // puts 0.3 in the third bin of a 0.1-wide grid instead of the fourth.
        let base = window(0.0, 0.1, 10);
        assert_eq!(base.bin_of(0.0), Some(0));
        assert_eq!(base.bin_of(0.3), Some(3));
        assert_eq!(base.bin_of(0.1), Some(1));
        assert_eq!(base.bin_of(0.7), Some(7));
        assert_eq!(base.bin_of(0.3 - 0.01), Some(2));
        // And a time a taken fraction of a bin below the edge is in the bin
        // below it. The step is a tenth of a bin and not `1e-12`: the rounding
        // above treats a millionth of a bin as "on the edge", and `0.3 - 1e-12`
        // is a *hundred millionth* of a bin, which is not a distinguishable
        // time at a 0.1 ps resolution — it is a rounding error of the grid.
    }

    #[test]
    fn the_closing_edge_is_in_the_last_bin_and_just_past_it_is_not() {
        let base = window(0.0, 1.0, 10);
        assert_eq!(base.bin_of(10.0), Some(9));
        assert_eq!(base.bin_of(10.0001), None);
        assert_eq!(base.bin_of(-1e-9), None);
        assert_eq!(base.bin_of(f64::NAN), None);
    }

    #[test]
    fn every_time_put_into_a_histogram_is_counted_somewhere() {
        let base = window(0.0, 1.0, 10);
        let times = [-2.0, -0.5, 0.0, 0.5, 3.0, 9.99, 10.0, 12.0, f64::NAN];
        let histogram = Histogram::new(base, &times);
        assert_eq!(histogram.counted(), times.len());
        assert_eq!(histogram.underflow, 2, "the two negative times");
        assert_eq!(histogram.non_finite, 1, "the NaN");
        assert_eq!(histogram.overflow, 1, "the 12 ps time");
        assert_eq!(histogram.in_window(), 5);
    }

    #[test]
    fn a_known_delay_shifts_the_whole_histogram_by_exactly_that_many_bins() {
        // W14.3's stated verification. A waveform shifted by five bins must
        // correlate with the original at exactly five.
        let shape: Vec<f64> = (0..64)
            .map(|i| {
                // A smooth pulse peaked at bin 20, plus noise-free tails.
                let x = (i as f64 - 20.0) / 4.0;
                let value = (-x * x).exp() * 100.0;
                if value >= 1.0 { i as f64 } else { f64::NAN }
            })
            .collect();
        // `bin_of` follows the edge rule; a time `i` lands in bin `i`, and the
        // filter above keeps the bins with at least one photon by repeating the
        // bin's time `value/100` times is not needed — one time per bin is
        // enough for the correlation.
        let original: Vec<f64> = shape.iter().copied().filter(|t| t.is_finite()).collect();
        let shifted: Vec<f64> = original.iter().map(|t| t + 5.0).collect();
        let a = Histogram::new(window(0.0, 1.0, 64), &original);
        let b = Histogram::new(window(0.0, 1.0, 64), &shifted);
        assert_eq!(a.shift_bins(&b), Some(5));
        assert_eq!(b.shift_bins(&a), Some(-5));
    }

    #[test]
    fn the_bin_counts_sum_to_the_photon_total() {
        let times: [Vec<f64>; 4] = vec![vec![1.5, 2.5, 3.5], vec![], vec![2.0, 2.0], vec![9.0]]
            .try_into()
            .expect("four sensors");
        let waveforms = Waveforms::from_times(&times, window(0.0, 1.0, 32));
        assert_eq!(waveforms.photons, 6);
        assert!(waveforms.accounted());
        assert_eq!(waveforms.all.in_window(), 6);
        assert_eq!(waveforms.per_sensor[1].in_window(), 0);
        assert_eq!(waveforms.per_sensor[2].in_window(), 2);
        // And the sum of the four histograms is the total, bin for bin.
        let summed: Vec<usize> = (0..32)
            .map(|i| waveforms.per_sensor.iter().map(|h| h.counts[i]).sum())
            .collect();
        assert_eq!(summed, waveforms.all.counts);
    }

    #[test]
    fn a_missing_photon_is_visible_as_an_unaccounted_histogram() {
        // The failure this module is written against: a time that is neither
        // binned nor counted as outside. Rewriting `Waveforms::accounted`'s
        // inputs by hand shows the identity that would catch it.
        let base = window(0.0, 1.0, 8);
        let waveforms = Waveforms::from_times(&[vec![], vec![], vec![], vec![]], base);
        assert_eq!(waveforms.photons, 0);
        assert!(waveforms.accounted(), "zero photons account for zero bins");
        // A histogram holding fewer counts than its input is the bug; construct
        // it and show the check says so.
        let mut broken = waveforms.all.clone();
        broken.non_finite = 1;
        let broken = Waveforms {
            per_sensor: waveforms.per_sensor,
            all: broken,
            photons: 0,
        };
        assert!(!broken.accounted(), "a bin count that appeared from nowhere is unaccounted");
    }

    #[test]
    fn an_ideal_window_contains_every_time_it_was_given() {
        let times = [-3.0, 0.0, 0.4, 5.1, 100.0];
        let base = TimeBase::ideal(&times, 1.0).expect("a base");
        assert!(base.start_ps <= -3.0);
        assert!(base.end_ps() >= 100.0);
        let histogram = Histogram::new(base, &times);
        assert_eq!(histogram.underflow, 0);
        assert_eq!(histogram.overflow, 0);
        assert_eq!(histogram.in_window(), times.len());
    }

    #[test]
    fn an_empty_slice_of_times_gets_the_default_window_rather_than_a_zero_width_one() {
        let base = TimeBase::ideal(&[], 1.0).expect("a base");
        assert_eq!(base.bins, 1000);
        assert!(TimeBase::ideal(&[f64::INFINITY], 1.0).is_err());
    }

    #[test]
    fn an_unusable_window_is_refused() {
        assert_eq!(TimeBase::new(f64::NAN, 1.0, 10), Err(TimeBaseError::NotFinite));
        assert_eq!(TimeBase::new(0.0, 0.0, 10), Err(TimeBaseError::NotPositive));
        assert_eq!(TimeBase::new(0.0, 1.0, 0), Err(TimeBaseError::NoBins));
        assert_eq!(TimeBase::new(0.0, f64::INFINITY, 10), Err(TimeBaseError::NotFinite));
    }

    #[test]
    fn the_mean_is_measured_from_the_bin_centres() {
        // Two times in bins 0 and 2 of a 1 ps grid: the centre of the first is
        // 0.5 and of the third 2.5, so the mean is 1.5 and not 1.0.
        let histogram = Histogram::new(window(0.0, 1.0, 4), &[0.5, 2.5]);
        assert_eq!(histogram.mean_ps(), Some(1.5));
        assert_eq!(Histogram::new(window(0.0, 1.0, 4), &[]).mean_ps(), None);
    }

    #[test]
    fn a_waveform_curve_has_one_point_per_populated_bin() {
        let histogram = Histogram::new(window(0.0, 1.0, 10), &[0.5, 1.5, 2.5]);
        let curve = histogram.curve();
        assert_eq!(curve.point_count(), 3);
        assert_eq!(curve.segment_count(), 1);
        // The x coordinates are the bin centres.
        assert_eq!(curve.segments[0][0][0], 0.5);
        assert_eq!(curve.segments[0][2][0], 2.5);
    }

    #[test]
    fn two_peaks_a_window_apart_are_two_segments_and_a_dip_is_not() {
        // A window of 40 bins: a gap of 3 bins is a dip and stays one segment,
        // a gap of 20 (half the window) is a separation and becomes two.
        let dip = Histogram::new(window(0.0, 1.0, 40), &[0.5, 4.5]);
        assert_eq!(dip.curve().segment_count(), 1);
        let split = Histogram::new(window(0.0, 1.0, 40), &[0.5, 20.5]);
        assert_eq!(split.curve().segment_count(), 2);
    }
}
