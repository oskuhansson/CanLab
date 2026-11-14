# The muon-direction detector

*Status: the design study is built and its first answer is negative. The timing
observable cannot give a direction; the position observable can. The numbers
below are from `cargo run --release -p canlab-muon --bin direction_study`.*

---

## What the detector is

A **tent prism** of high-index plastic, not a slab. The outline is the six-corner
tent of `canlab_optics::outline` — a wide low peak on two straight walls — extruded
through a thickness, with four SiPMs on four of its faces.

The tent is not arbitrary: it is what you can cut and still keep a
total-internal-reflection cavity with a flat face for the sensor. With a CNC
available (`mission::Manufacturing`) the outline is any closed 2.5D shape within
the tool's reach, so the tent is a *choice* the study should sweep rather than a
manufacturing limit.

| Quantity | Value | Where |
|---|---|---|
| Can | 115 mm tall, Ø66 mm | `CANSAT-GRUND.md` §3 |
| Can cross-section | 3421 mm², 34.2 cm² | `π·33²` |
| Usable after 1 mm of allowance | 3217 mm², 32.2 cm² | `Mission::can` |
| Design block | 20 mm scale, 10 mm deep | `Candidate::design` |
| Its silhouette | 40 mm × 90 mm × 10 mm | `tent_extent_mm` |
| Its volume and mass | 30 cm³, 35.7 g of PMMA | `tent_mass_g` |
| Largest tent that fits at 10 mm deep | 25.33 mm of scale | `Can::largest_scale_mm` |

**The geometry sweep's best row does not fit.** `muon_study` varies the scale from
15 to 30 mm and reports that 30 mm gives the most light; a 30 mm tent is
`4.5 × 30 = 135 mm` tall in a 115 mm can. `Can::admits` refuses it by name, and
`Mission::audit` reports it as a violation.

---

## The timing answer: no

The premise is that a photon's arrival time at sensor `i` is

```text
t_i = t₀ + (r_i · d) / v
```

Three unknowns against four timestamps, so a least-squares fit recovers the
heading. That arithmetic is correct, and `canlab_muon::timing::reconstruct`
implements it and returns the exact heading from exact timestamps.

**The scale is measurable, and the placement wastes it.** Light moves 0.2 mm/ps in
PMMA, so the *whole detector's* spread of `(r_i · d)/v` is **224 ps** — light needs
269 ps to cross the 54.1 mm between the two furthest sensors. The readout's jitter
is **47.4 ps**, so the signal-to-noise is 4.7.

| | ps |
|---|---|
| Timing signal across the whole detector | **224** |
| SiPM SPTR (100 ps FWHM → σ) | 42.5 |
| MAX35101 timer accuracy | 20.0 |
| OPA858 noise over the edge | 6.0 |
| MAX40026 jitter | 2.0 |
| **Total, quadrature** | **47.4** |

The Cramér–Rao bound on the heading is `σ·v/L`, and for the four face centres it
comes out at **81°** at the worst heading and 48° at the best. The reason is the
placement: the two end sensors sit at the centres of the ±z faces, only **10 mm
apart**, so a heading along that axis has almost no lever arm.

| Placement | Worst | Best |
|---|---|---|
| four face centres (the current one) | 81° | 48° |
| four block corners | 56° | **16°** |
| a well-spread 54 mm tetrahedron | 17.5° | 17.5° |

So timing works and this placement does not. Moving the sensors to the corners is
the fix, and it is a board change rather than a part change.

**This section was wrong by a factor of a thousand.** The first version said the
signal was 0.22 ps and the bound 81 000°, because `C_MM_PER_PS` was written as
299.79 — millimetres per nanosecond. `tests/units.rs` now states every physical
fact independently and is what caught it.

---

## The position answer: yes

A sensor that reports **where** a photon crossed its face — a pixel, a fibre, a
strip — gives a direction from two crossings. The uncertainty is `√2·σ_h/L`:

| Pixel | Separation | Bound |
|---|---|---|
| 0.1 mm | 54.1 mm | 0.15° |
| 1.0 mm | 54.1 mm | **1.5°** |
| 2.0 mm | 54.1 mm | 3.0° |

No timing, no picoseconds, no TDC. The block is still needed — it is what makes
the light — but the observable is the hit position.

---

## The parts, and the details that decide whether any of it works

From `can-sat`'s `komponenter` list, with datasheet numbers in
`canlab_muon::components`:

| Part | Number | Consequence |
|---|---|---|
| onsemi MICROFC-10035 | PDE 31 % at Vbr+2.5 V, 41 % at +5 V; 30 kHz dark count; 7 % crosstalk; **SPTR not specified** | the dominant jitter term is an assumption |
| TI OPA858 | 5.5 GHz, 2.5 nV/√Hz | 6 ps; but its bandwidth collapses with the anode's 100 pF |
| ADI MAX40026 | 280 ps delay, **2 ps jitter**, 25 ps overdrive dispersion | negligible random, non-negligible walk |
| ADI MAX35101 | **20 ps accuracy**, 3.8 ps resolution, 2 channels | see below |

**The MAX35101 is the wrong part for four independent SiPMs.** Its START is
generated internally by its own pulse launcher — there is no external START pin —
so it measures *launch-to-echo*, which is an ultrasonic time-of-flight, not
"when did this SiPM fire". Its two STOP inputs are *analog* (10–700 mVpp) into a
comparator specified for 1 MHz transducers. And its 20 ps is a **differential**
accuracy: the datasheet says absolute measurements "include circuit delays and
cannot be considered accurate".

**The fast output must be used, not the anode.** The MICROFC-10035's
anode–cathode capacitance is 100 pF and its fast-terminal capacitance is 1 pF. An
OPA858 TIA behind 100 pF at 10 kΩ has a 30 MHz bandwidth — a 33 ns rise time,
which would destroy any timing measurement before the comparator saw it. The fast
output (1 pF, 0.3 ns rise, 0.6 ns FWHM) is the timing path; the anode is the
energy path.

**Five things the transport does not model**, each of which moves a number:

1. PDE is a curve, and the model uses one mid-band 35 %.
2. SPTR is not specified by the vendor; the model's 100 ps is a placeholder.
3. Crosstalk is 7 %, so one photon can produce two timestamps.
4. Dark count is 30–96 kHz, so a 100 ns window is not always empty.
5. Vbr drifts 21.5 mV/°C and gain −0.8 %/°C, so a can descending through 10 °C
   changes its PDE and its SPTR *during the flight*.

The fifth is the one a mission has to answer: a bias-tracking loop, or a
temperature model, or an acceptance that the count drifts.

**And one the CNC adds**: a machined acrylic face has ~1 µm of roughness, and a
photon trapped by total internal reflection meets that face at every bounce.
Fresnel reflection assumes a mirror. Until a scattering model exists, a machined
block's light yield is an **upper** bound — which is why
`Manufacturing::polish_available` is a parameter and not a footnote.

---

## What the crate does

`canlab-muon`, separate from the CAS, which is used only to draw results.

| Module | Question |
|---|---|
| `can` | does the block fit a Ø66 × 115 mm can? |
| `mission` | the parameters and requirements of every simulation, and an audit |
| `components` | the four parts' datasheet numbers and the jitter budget |
| `timing` | least-squares heading from timestamps; the timing signal |
| `bound` | Cramér–Rao bound on the heading; the position-based alternative |
| `solve` | a 3×3 solve with a unit-free singularity test |

The physics is `canlab-optics` and is not repeated: the tent, the Cherenkov
source, Fresnel, the photon transport and one event's four timestamps.

**No network is trained.** A network is a way to approach a bound; the bound for
timing is 81 000°, so a network trained on four timestamps would learn to output
the mean direction and its loss curve would be the loss curve of that mean. The
network belongs on the observable whose bound is small.

---

## Open questions

1. **Is the direction mission worth flying with a position-sensitive detector?**
   A pixelated or fibre readout is a different build, and the answer decides
   whether the detector is a direction measurement or a counter.
2. **What is the MICROFC-10035's SPTR?** It is the largest term in the budget and
   the only one with no datasheet figure. It needs a measurement, not a citation.
3. **Is the MAX35101 fixed, or can the timer change?** As specified it cannot
   timestamp four independent pulses.
4. **What is the block's material?** PMMA (n = 1.49) collects more light than a
   high-index plastic (n = 1.84) in this geometry — 9.7 against 5.9 detected
   photons — because a higher index traps more of what it makes. The sweep
   contradicts the intuition that more Cherenkov light is better.
