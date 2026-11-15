//! The readout: what a real discriminator and timer do to the photons.
//!
//! The transport gives the arrival times of the photoelectrons. A real
//! instrument does not see those times. It sees a pulse, compares it against a
//! threshold, and records the first crossing. Four things stand between the
//! photon and the timestamp, and this module models all four:
//!
//! * **The threshold.** A leading-edge discriminator fires when the pulse
//!   crosses its threshold, so it needs `ceil(threshold_pe)` photoelectrons, not
//!   one. The "first photon" the simulation has been taking is the optimistic
//!   limit of a threshold at half a photoelectron.
//! * **Dark counts.** The sensor avalanches spontaneously at 30–96 kHz per mm².
//!   A dark count before the real photon is a *false* timestamp, and it is early.
//! * **Crosstalk.** About 7 % of avalanches trigger a neighbour, adding a photon
//!   ~100 ps later. It raises the count, and it can push a two-photoelectron
//!   threshold over the line.
//! * **Time walk.** The comparator's propagation delay depends on the pulse
//!   amplitude, so a brighter pulse fires earlier. The MAX40026's 25 ps
//!   dispersion is this effect.
//!
//! # What is not modelled
//!
//! The threshold is modelled as "the k-th photon", which assumes the photons
//! overlap within the pulse width. A real leading-edge discriminator's crossing
//! time depends on the pulse shape, so the k-th photon is an approximation that
//! is good when the photons arrive within ~0.6 ns and poor when they are spread
//! over the whole window. The transport's arrival times are spread over ~1 ns,
//! so the approximation is reasonable but not exact.
//!
//! The dark-count rate is not temperature-corrected. A SiPM's dark rate roughly
//! doubles every 8 °C, so a can descending through 10 °C sees its dark rate rise
//! by a factor of 2.4. That is a second-order effect on the timestamp and is left
//! to the domain randomisation.

use crate::tracker::Rng;

/// A leading-edge readout: threshold, dark counts, crosstalk, walk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readout {
    /// The discriminator threshold, in photoelectrons.
    pub threshold_pe: f64,
    /// Dark counts per second per sensor.
    pub dark_rate_hz: f64,
    /// The timer's window, picoseconds. A dark count outside it cannot fire.
    pub window_ps: f64,
    /// The comparator's time walk over the pulse amplitude, picoseconds.
    pub walk_ps: f64,
    /// The crosstalk probability per avalanche.
    pub crosstalk: f64,
    /// The crosstalk delay, picoseconds.
    pub crosstalk_ps: f64,
}

impl Readout {
    /// The design point: a half-photoelectron threshold, 30 kHz dark counts,
    /// 100 ns window, 25 ps walk, 7 % crosstalk.
    ///
    /// The threshold of 0.5 p.e. is the "first photon" the earlier simulation
    /// assumed. It is the *best* case; a real threshold is set above the noise,
    /// so 1–2 p.e. is the realistic range, and the domain randomisation spans it.
    pub fn nominal() -> Readout {
        Readout {
            threshold_pe: 0.5,
            dark_rate_hz: 30_000.0,
            window_ps: 100_000.0,
            walk_ps: 25.0,
            crosstalk: 0.07,
            crosstalk_ps: 100.0,
        }
    }

    /// The ideal readout: the first photon, no dark counts, no crosstalk, no walk.
    ///
    /// This is what the earlier simulation assumed. It is the upper bound on the
    /// readout's performance, and the gap between it and [`Readout::nominal`] is
    /// the price of a real instrument.
    pub fn ideal() -> Readout {
        Readout {
            threshold_pe: 0.5,
            dark_rate_hz: 0.0,
            window_ps: 100_000.0,
            walk_ps: 0.0,
            crosstalk: 0.0,
            crosstalk_ps: 100.0,
        }
    }

    /// A conservative readout: a 2 p.e. threshold, 96 kHz dark counts, a 200 ns
    /// window, 25 ps walk, 7 % crosstalk.
    ///
    /// This is the "if it goes wrong" case. A 2 p.e. threshold needs two photons
    /// to overlap, which halves the fraction of events that fire; 96 kHz is the
    /// datasheet's upper dark rate; and 200 ns is a long TDC window, so a dark
    /// count has twice the chance to fire first.
    pub fn conservative() -> Readout {
        Readout {
            threshold_pe: 2.0,
            dark_rate_hz: 96_000.0,
            window_ps: 200_000.0,
            walk_ps: 25.0,
            crosstalk: 0.07,
            crosstalk_ps: 100.0,
        }
    }

    /// The number of photoelectrons the threshold needs.
    pub fn photons_needed(&self) -> usize {
        self.threshold_pe.ceil().max(1.0) as usize
    }

    /// Add crosstalk photons to an arrival list, and sort it.
    ///
    /// Each primary photon triggers a neighbour with probability `crosstalk`,
    /// `crosstalk_ps` later. The secondary is a real avalanche, so it counts
    /// toward the threshold and can push a two-photon threshold over the line.
    pub fn with_crosstalk(&self, times: &[f64], rng: &mut Rng) -> Vec<f64> {
        let mut extended = times.to_vec();
        for time in times {
            if rng.uniform() < self.crosstalk {
                extended.push(time + self.crosstalk_ps);
            }
        }
        extended.sort_by(f64::total_cmp);
        extended
    }

    /// The timestamp a leading-edge readout records for one sensor.
    ///
    /// Returns `None` when the sensor does not reach the threshold. The
    /// dark count is drawn over the whole window, so a dark count before the
    /// real photon is the timestamp — and it is early, which is the failure mode
    /// that matters.
    pub fn timestamp(&self, times: &[f64], rng: &mut Rng) -> Option<f64> {
        let needed = self.photons_needed();
        if times.len() < needed {
            return None;
        }
        let real = times[needed - 1];
        // The dark count: a Poisson process over the window. The first one is
        // exponential with the rate. If it is before the real photon, it fires.
        let rate_per_ps = self.dark_rate_hz * 1e-12;
        let dark = if rate_per_ps > 0.0 {
            -rng.uniform().max(1e-12).ln() / rate_per_ps
        } else {
            f64::INFINITY
        };
        let time = if dark < real && dark < self.window_ps {
            dark
        } else {
            real
        };
        // The walk: a brighter pulse crosses the threshold earlier. The bias is
        // zero at one photoelectron and `-walk_ps` at four, so it scales
        // linearly with the pulse height above one. The height is the number of
        // photons the pulse carries, not the threshold: the threshold decides
        // *which* photon is recorded, and scaling the walk with it made the walk
        // a constant offset — the same on every sensor, so it cancelled in the
        // direction fit and modelled nothing. The pulse height is approximated
        // by the number of photons the sensor saw, which is the amplitude the
        // pulse reaches.
        let amplitude = (times.len() as f64).clamp(1.0, 4.0);
        let walk = -self.walk_ps * (amplitude - 1.0) / 3.0;
        Some(time + walk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_needs_its_photons() {
        // A sensor with fewer photons than the threshold does not fire.
        let readout = Readout::conservative();
        let mut rng = Rng::new(1);
        assert_eq!(readout.photons_needed(), 2);
        assert_eq!(readout.timestamp(&[100.0], &mut rng), None);
        assert!(readout.timestamp(&[100.0, 200.0], &mut rng).is_some());
    }

    #[test]
    fn a_dark_count_fires_before_the_photon() {
        // At 96 kHz over 200 ns, a dark count lands inside the window about 2 %
        // of the time. With a threshold of one photon, some events must return a
        // time earlier than the photon's own.
        let readout = Readout::conservative();
        let mut rng = Rng::new(2);
        let mut early = 0;
        for _ in 0..2_000 {
            // Two photons: the conservative readout needs two, so a one-photon
            // list would return None and the test would measure nothing.
            if let Some(time) = readout.timestamp(&[50_000.0, 60_000.0], &mut rng) {
                if time < 60_000.0 {
                    early += 1;
                }
            }
        }
        assert!(early > 0, "no dark count fired in 2000 events");
    }

    #[test]
    fn crosstalk_raises_the_count() {
        // A crosstalk photon is an extra entry in the list, so the mean count
        // rises by the crosstalk probability.
        let readout = Readout::nominal();
        let mut rng = Rng::new(3);
        let mut total = 0;
        let events = 2_000;
        for _ in 0..events {
            total += readout.with_crosstalk(&[10.0, 20.0, 30.0], &mut rng).len();
        }
        let mean = total as f64 / events as f64;
        assert!((mean - 3.21).abs() < 0.1, "{mean}");
    }

    #[test]
    fn the_walk_grows_with_the_pulse_height_not_the_threshold() {
        // The walk is the comparator's amplitude-dependent delay: a pulse of four
        // photoelectrons crosses the threshold 25 ps earlier than a pulse of one.
        // The threshold decides *which* photon is recorded, not how much the
        // crossing walks. The old model scaled the walk with `needed`, which made
        // it the same constant on every sensor, so it cancelled in the
        // mean-relative features and modelled nothing at all.
        let base = Readout { dark_rate_hz: 0.0, ..Readout::nominal() };
        let unwalked = Readout { walk_ps: 0.0, ..base };
        let mut rng = Rng::new(4);
        // One photon: no walk.
        let one = base.timestamp(&[400.0], &mut rng).unwrap();
        let one_flat = unwalked.timestamp(&[400.0], &mut rng).unwrap();
        assert!((one - one_flat).abs() < 1e-9, "one photon walked: {one} vs {one_flat}");
        // Four photons: the full 25 ps, and the first photon is still the one
        // recorded because the threshold is half a photoelectron.
        let four = base.timestamp(&[100.0, 200.0, 300.0, 400.0], &mut rng).unwrap();
        let four_flat = unwalked.timestamp(&[100.0, 200.0, 300.0, 400.0], &mut rng).unwrap();
        assert!((four_flat - four - 25.0).abs() < 1e-9, "{four_flat} vs {four}");
        // The threshold moves which photon is recorded and leaves the walk alone:
        // the same four-photon pulse walks 25 ps whatever the threshold is.
        let high = Readout { threshold_pe: 4.0, ..base };
        let high_flat = Readout { threshold_pe: 4.0, ..unwalked };
        let high_time = high.timestamp(&[100.0, 200.0, 300.0, 400.0], &mut rng).unwrap();
        let high_flat_time = high_flat.timestamp(&[100.0, 200.0, 300.0, 400.0], &mut rng).unwrap();
        assert!((high_flat_time - high_time - 25.0).abs() < 1e-9, "{high_flat_time} vs {high_time}");
    }
}
