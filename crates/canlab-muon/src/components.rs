//! The four parts, their datasheet numbers, and the jitter budget they add up to.
//!
//! The simulation's sensor model carries a single `jitter_ps` placeholder of
//! 100 ps. That number is not a budget: it is one part's number standing in for
//! four, and the four are not the same size. This module is the budget, with each
//! figure tied to the document it came from, so a change to a part changes one
//! line and the study's answer moves with it.
//!
//! # The parts, and what each contributes
//!
//! | Part | Number | Contributes |
//! |---|---|---|
//! | onsemi MICROFC-10035 | SPTR **not in the datasheet** | the dominant term |
//! | TI OPA858 | 2.5 nV/√Hz, 5.5 GHz | a few ps, from noise over the edge's slope |
//! | ADI MAX40026 | jitter **2 ps**, overdrive dispersion **25 ps** | 2 ps random, 25 ps systematic |
//! | ADI MAX35101 | accuracy **20 ps**, resolution **3.8 ps** | 20 ps |
//!
//! # The three details that decide whether the budget means anything
//!
//! 1. **The SiPM's SPTR is not in its datasheet.** The C-series sheet gives the
//!    fast output's rise time (0.3 ns) and pulse width (0.6 ns FWHM) and no
//!    single-photon time resolution at all. Published measurements of
//!    state-of-the-art SiPMs put SPTR at 70–120 ps FWHM, and a 1 mm² sensor with
//!    35 µm cells is at the slow end of that because SPTR grows with cell size.
//!    Every other term in the budget is smaller than the uncertainty on this one,
//!    which is why the budget cannot be closed without measuring the part.
//! 2. **FWHM is not a standard deviation.** The placeholder 100 ps is used in the
//!    transport as a *standard deviation*, and if the 100 ps is a FWHM — which is
//!    how SPTR is quoted — the model is 2.355 times too pessimistic. The
//!    difference is not academic: it is the difference between a bound of 171 000°
//!    and one of 73 000°, and both are hopeless, so the error does not change the
//!    conclusion but it does mean the model's number is not the part's number.
//! 3. **The comparator's 25 ps is a *dispersion*, not a jitter.** It is the
//!    change in propagation delay with overdrive, so it is a *systematic* walk
//!    that depends on the pulse's amplitude. A SiPM pulse of one photoelectron
//!    and one of three differ by a factor of three in amplitude, so the walk
//!    differs by tens of picoseconds *in a way that correlates with the photon
//!    count*. Adding it in quadrature with the random terms would be wrong; it
//!    belongs in the model as an amplitude-dependent bias.
//!
//! # What the budget buys
//!
//! The timing signal across the whole detector is **224 ps** and this budget is
//! **47.4 ps**, so the signal-to-noise is 4.7: the electronics can resolve the
//! geometry, and the budget is what decides how well. Halving it would halve the
//! angular uncertainty, which is why the terms are worth knowing.
//!
//! The budget is not the limit on the design. The limit is **where the sensors
//! are**: the two end sensors sit 10 mm apart, so the worst heading has almost no
//! lever arm and the Cramér–Rao bound is 81° however good the clock is. A
//! placement on the block's corners gives 16° at its best heading with this same
//! budget.
//!
//! The first version of this section said the opposite — that no budget could
//! help — because the study's speed of light was a thousand times too large.

/// One component's contribution to the timing budget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contribution {
    /// The part, as the bill of materials names it.
    pub part: &'static str,
    /// The figure, picoseconds.
    pub ps: f64,
    /// Whether the figure is a random jitter (adds in quadrature) or a
    /// systematic walk (does not).
    pub random: bool,
    /// Where the number came from, or why there is not one.
    pub source: &'static str,
}

/// The four parts of the readout chain, with the numbers the datasheets give.
///
/// The SiPM's entry is an *assumption*, and it says so in its `source`: the
/// C-series datasheet has no SPTR figure, so the 100 ps FWHM is the published
/// range for this class of part and not this part's specification.
pub fn budget() -> [Contribution; 4] {
    [
        Contribution {
            part: "onsemi MICROFC-10035 (SiPM)",
            // 100 ps FWHM, converted to a standard deviation.
            ps: 100.0 / 2.355,
            random: true,
            source: "not in the C-series datasheet; 70-120 ps FWHM is the published range for this class of part, and 100 ps FWHM is the middle of it",
        },
        Contribution {
            part: "TI OPA858 (TIA)",
            // 2.5 nV/√Hz over a 0.6 ns edge of 10 mV: noise 103 µV, slope
            // 16.7 mV/ns, so 6 ps. A 100 mV pulse is ten times faster.
            ps: 6.0,
            random: true,
            source: "2.5 nV/√Hz input voltage noise over the fast output's 0.6 ns FWHM edge, at a 10 mV pulse",
        },
        Contribution {
            part: "ADI MAX40026 (comparator)",
            ps: 2.0,
            random: true,
            source: "datasheet: jitter 2 ps typ, measured with a 150 ps edge and 100 mV overdrive",
        },
        Contribution {
            part: "ADI MAX35101 (timer)",
            ps: 20.0,
            random: true,
            source: "datasheet: time measurement accuracy 20 ps (differential), resolution 3.8 ps",
        },
    ]
}

/// The systematic walk the comparator adds, picoseconds.
///
/// The MAX40026's propagation delay changes by 25 ps as the overdrive goes from
/// 10 mV to 1 V. A single photoelectron and three of them differ by a factor of
/// three in amplitude, so a fixed threshold sees them tens of picoseconds apart
/// — and the difference tracks the photon count, which the transport knows. It
/// is therefore a term the model can *compute* rather than a term it has to
/// assume, once the pulse amplitude per photoelectron is known.
pub const COMPARATOR_WALK_PS: f64 = 25.0;

/// The total random jitter, picoseconds, by quadrature.
///
/// Only the random terms are summed here. The walk is not one of them: it is a
/// function of the pulse amplitude, and adding a systematic to a jitter is the
/// arithmetic that makes a budget look closed when it is not.
pub fn total_jitter_ps() -> f64 {
    budget()
        .iter()
        .filter(|contribution| contribution.random)
        .map(|contribution| contribution.ps * contribution.ps)
        .sum::<f64>()
        .sqrt()
}

/// The largest single random term and its share of the variance.
///
/// The share is what decides where the next measurement should go: a term with
/// 80 % of the variance is the only one worth improving.
pub fn dominant() -> Contribution {
    let mut best = budget()[0];
    for contribution in budget() {
        if contribution.ps > best.ps {
            best = contribution;
        }
    }
    best
}

/// The optical adhesive, with the datasheet numbers that decide whether it works.
///
/// The block's faces are machined, and a machined acrylic face has a micrometre
/// of roughness: light trapped by total internal reflection meets that roughness
/// at every bounce, and Fresnel reflection assumes a mirror. Optical adhesive
/// fixes that — it fills the scratches, so the interface becomes optically
/// smooth — and it fixes something larger that the roughness discussion hides.
///
/// # The larger effect: the escape cone
///
/// At a PMMA/air face with `n = 1.491`, the critical angle is 42.2°, so only
/// `(1 − cos 42.2°)/2 = 13 %` of the light arriving at that face escapes and
/// **87 % is trapped**. An adhesive with `n > 1.491` makes the interface go from
/// lower to higher index, so there is **no critical angle at all**: every photon
/// that reaches the bonded face crosses it.
///
/// That is a factor of 7.7 on the sensor's face, against the 10–20 % the
/// roughness is worth. The adhesive is not a polish substitute; it is the largest
/// single optical change available to the block.
///
/// # Why NOA 68 and not the others
///
/// | Product | n cured | Δn vs PMMA | Plastic adhesion | Price |
/// |---|---|---|---|---|
/// | Norland NOA 65 | 1.52 | +0.029 | "Fair" | $38 / 1 oz |
/// | **Norland NOA 68** | **1.54** | **+0.049** | **"improved adhesion to acrylic"** | **€32.70 / 1 oz** |
/// | Norland NOA 88 | 1.56 | +0.069 | not stated | — |
/// | 3M LOCA 2321 | 1.49 | 0.000 | glass-to-glass | bulk only |
///
/// All four are index-matched well enough that the Fresnel loss is a part in
/// 10 000 — `((1.54 − 1.491)/(1.54 + 1.491))² = 2.6 × 10⁻⁴` — and all four have
/// `n > 1.491`, so all four remove the critical angle. The choice is therefore
/// **adhesion to acrylic**, which is what NOA 68's datasheet says it was
/// formulated for, and availability in Europe in a one-ounce bottle.
///
/// 3M's LOCA 2321 is the only perfect index match (1.49 against PMMA's 1.491) and
/// it is a display-bonding product sold by the litre, so it is not a candidate for
/// one detector.
pub struct Adhesive {
    /// The product.
    pub part: &'static str,
    /// The cured refractive index at 589 nm.
    pub index: f64,
    /// The fraction of light crossing a bonded interface into the sensor.
    ///
    /// Not 1.0 because the bond is not perfect: the TDS quotes a volume shrinkage
    /// of a few percent and the process leaves voids and unwetted patches. The
    /// figure is the *assumption* in this row and it is randomised.
    pub transmission: f64,
    /// The cost of the smallest purchase, euros.
    pub eur: f64,
    /// Where the numbers came from.
    pub source: &'static str,
}

/// The adhesive this study recommends, with its datasheet figures.
pub fn adhesive() -> Adhesive {
    Adhesive {
        part: "Norland NOA 68",
        index: 1.54,
        transmission: 0.95,
        eur: 32.70,
        source: "NOA 68 TDS: cured index 1.54, viscosity 5000 cps, 4.5 J/cm2 at 350-380 nm, formulated for acrylic adhesion; price from AMS Technologies, 1 oz bottle",
    }
}

/// The fraction of light that escapes a PMMA/air face, for comparison.
///
/// `(1 − cos θc)/2` with `sin θc = 1/1.491`, which is 13 %. The bonded case is
/// [`Adhesive::transmission`], and the ratio is what the adhesive buys.
pub fn escape_cone_fraction() -> f64 {
    let critical = (1.0 / 1.491f64).asin();
    (1.0 - critical.cos()) / 2.0
}

/// The datasheet facts the simulation's sensor model does not carry yet.
///
/// Each is a number the transport could use and currently does not, listed here
/// rather than in a comment so a test can check that the list is not a wish: the
/// model's `pde` is a single mid-band value and the part's PDE is a curve, its
/// `jitter_ps` is one number and the budget is four, and its microcell count
/// ignores the crosstalk that turns one photon into two timestamps.
pub const UNMODELLED: [&str; 5] = [
    "PDE is 31 % at Vbr+2.5 V and 41 % at Vbr+5 V at 420 nm, and the model uses one mid-band 35 %",
    "SPTR is not specified by the vendor, and the model's 100 ps is a placeholder",
    "crosstalk is 7 % at 35 um cells, so one photon can produce two timestamps",
    "dark count is 30 kHz typ and 96 kHz max, so a 100 ns window is not always empty",
    "Vbr drifts 21.5 mV/degC and gain -0.8 %/degC, so a descending can changes PDE and SPTR in flight",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_the_quadrature_of_its_random_terms() {
        // 42.5 ps from the SiPM, 6 from the amplifier, 2 from the comparator and
        // 20 from the timer: sqrt(1809 + 36 + 4 + 400) = 47.4 ps.
        let total = total_jitter_ps();
        assert!((total - 47.4).abs() < 0.1, "{total}");
        // And it is less than the sum and more than the largest, which is what
        // quadrature means. A budget that added linearly would be 70.5 ps.
        let linear: f64 = budget().iter().map(|c| c.ps).sum();
        assert!(total < linear, "{total} vs {linear}");
        assert!(total > dominant().ps);
    }

    #[test]
    fn the_sipm_dominates_and_the_comparator_does_not() {
        // The claim the budget exists to support: the sensor is the term, and
        // the comparator is 0.2 % of the variance. A design that spent its effort
        // on the comparator would be optimising the wrong part.
        let sipm = budget()[0];
        let comparator = budget()[2];
        let variance: f64 = budget().iter().map(|c| c.ps * c.ps).sum();
        let share = comparator.ps * comparator.ps / variance;
        assert!(share < 0.005, "the comparator is {:.3} % of the variance", share * 100.0);
        assert_eq!(dominant().part, sipm.part);
        // The SiPM is more than half the variance on its own.
        assert!(sipm.ps * sipm.ps / variance > 0.5);
    }

    #[test]
    fn the_sipm_figure_is_an_assumption_and_says_so() {
        // The one entry that is not a datasheet number must be labelled as an
        // assumption, because a budget whose weakest term is presented as
        // measured is a budget that lies about its own uncertainty.
        let sipm = budget()[0];
        assert!(sipm.source.contains("not in"), "{}", sipm.source);
        // And the conversion from FWHM is the 2.355, not a copy of the number.
        assert!((sipm.ps - 100.0 / 2.355).abs() < 1e-12);
        // Two of the four entries cite a datasheet figure; the SiPM's entry says
        // "not in the datasheet", so a substring test would count it too.
        assert_eq!(budget().iter().filter(|c| c.source.starts_with("datasheet")).count(), 2);
    }

    #[test]
    fn the_walk_is_not_added_to_the_jitter() {
        // A systematic that depends on the amplitude is not a random term. The
        // test is that `total_jitter_ps` does not contain it: adding 25 ps in
        // quadrature would give 53.6 ps, and the budget says 47.4.
        let with_walk = (total_jitter_ps().powi(2) + COMPARATOR_WALK_PS * COMPARATOR_WALK_PS).sqrt();
        assert!((with_walk - 53.6).abs() < 0.1, "{with_walk}");
        assert!((total_jitter_ps() - 47.4).abs() < 0.1);
        assert!(!budget().iter().any(|c| c.ps == COMPARATOR_WALK_PS));
    }

    #[test]
    fn the_adhesive_is_index_matched_and_removes_the_critical_angle() {
        // The two facts that decide whether the adhesive works, both from the
        // datasheet: its cured index is above PMMA's, so there is no critical
        // angle, and it is close enough that the Fresnel loss is negligible.
        let glue = adhesive();
        let pmma = 1.491;
        assert!(glue.index > pmma, "{} is not above {pmma}", glue.index);
        let reflectance = ((glue.index - pmma) / (glue.index + pmma)).powi(2);
        assert!(reflectance < 1e-3, "Fresnel reflectance {reflectance}");
        // And the air case it replaces: 13 % escapes, 87 % is trapped.
        let cone = escape_cone_fraction();
        assert!((cone - 0.13).abs() < 0.01, "{cone}");
        // The ratio is what the adhesive buys on the bonded face.
        let gain = glue.transmission / cone;
        assert!((gain - 7.3).abs() < 0.6, "{gain}");
        // The cost is the smallest purchase, not the material used: a 40 x 48 mm
        // bond at 25 um is 0.048 ml of a 30 ml bottle.
        assert!((glue.eur - 32.70).abs() < 1e-9);
        let bond_ml = 40.0 * 48.0 * 0.025 / 1000.0;
        assert!(bond_ml < 0.1, "{bond_ml} ml");
        assert!(glue.source.contains("TDS"), "{}", glue.source);
    }

    #[test]
    fn the_unmodelled_list_names_the_numbers_it_claims() {
        // Every entry has to be a fact with a figure in it, not a sentiment.
        assert_eq!(UNMODELLED.len(), 5);
        for entry in UNMODELLED {
            assert!(
                entry.chars().any(|c| c.is_ascii_digit()),
                "an entry with no number is not a fact: {entry}"
            );
        }
        // The two that decide the photon count and the timing are both there.
        assert!(UNMODELLED.iter().any(|e| e.contains("PDE")));
        assert!(UNMODELLED.iter().any(|e| e.contains("SPTR")));
    }
}
