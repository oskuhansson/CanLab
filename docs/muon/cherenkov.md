---
tags: [cansat, muon, cherenkov, design, reference]
status: aktuell
updated: 2026-11-14
---

# The Cherenkov timing design

*One acrylic block, four SiPMs, four timestamps. A muon crossing the block emits
Cherenkov light along a cone; the light reaches the four sensors at times that
depend on the muon's direction; and a network recovers the direction from those
times. This document is the design's whole story: why it exists, what the physics
gives it, what the parts cost it, what the simulation measured, and what is still
open.*

*Every number here comes from the code in this repository. Where a number is a
model rather than a measurement, it says so.*

---

## 1. The idea in one paragraph

A muon is faster than light in acrylic, so it radiates Cherenkov light in a cone
at **47.8°** around its track. The light bounces inside the block by total
internal reflection, and the fraction that escapes at a face is what the sensor
there sees. The *distance* from the track to each sensor differs, so the *arrival
time* differs: the furthest sensor fires last. Four arrival times over-determine a straight line's direction,
and the differences between them are the signal. The block is one solid piece —
no gap, no coincidence, no second plane — which is what makes it small and cheap.

The design's problem is not the idea. It is **how few photons arrive**: a 30 mm
crossing makes about 630 photons, but most are trapped by total internal
reflection, the sensors cover a fraction of the faces, and their PDE is 35 % — so
an event yields **3.25 photoelectrons** and the timestamps are noisy. Everything
below is a consequence of that one number.

---

## 2. The physics

### 2.1 The condition

A charged particle radiates when `v > c/n`, i.e. `βn > 1`.

| Quantity | Value | Where |
|---|---|---|
| PMMA's index at 500 nm | 1.491 | `canlab_optics::cherenkov` |
| Threshold β | 0.671 | `1/1.491` |
| Threshold kinetic energy | **157 MeV** | a 4 GeV muon is far above it |

The threshold is a completeness check, not a constraint: the mean sea-level muon
is 4 GeV.

### 2.2 The cone

```text
cos θ_c = 1 / (β n)
```

| Material | n | θ_c |
|---|---|---|
| PMMA | 1.491 | **47.8°** |
| High-index plastic | 1.84 | 57.1° |

The cone is not one angle: `n` depends on wavelength, and a plastic with
`dn/dλ ≈ −0.016/µm` spreads the cone by about **0.3°** over 400–600 nm. Over 100 mm
of path that is ~5 ps of arrival spread. The simulation draws the wavelength per
photon but computes the cone from the **block's own index**, so the dispersion is
not modelled — and until that was fixed it computed the cone from the high-index
plastic's curve whatever the block was, which put every PMMA photon **9.3°** wide
of its cone.

### 2.3 The yield

```text
dN/dx dλ = 2π α z² / λ² · (1 − 1/(β²n²))
```

integrated over the band the sensor sees:

| Material | Photons/mm | Photons/cm |
|---|---|---|
| PMMA | **21.0** | 210 |
| High-index plastic | 26.9 | 269 |

The textbook 250–300 photons/cm is for the full visible range; 400–600 nm is the
band a silicon sensor sees, and the crate integrates that band.

### 2.4 The photon budget, and why it is the whole story

A 1 mm² sensor at 25 mm from the track subtends `1/25² = 1.6 × 10⁻³` sr, which is
`1.6e-3 / 4π = 1.27 × 10⁻⁴` of the emission. A 10 mm path at n = 1.84 makes 269
photons, so the sensor sees

```text
269 × 1.27e-4 = 0.034 photons per muon
```

and with a 35 % PDE, **0.012 photoelectrons**. One photon every eighty muons.

The block is a total-internal-reflection cavity, and that is what rescues the
design: the light is not lost to the room, it bounces until it reaches a face. The
transport measures the result rather than estimating it — see §6.

---

## 3. The block

### 3.1 The tent

The original shape is a **tent prism**: a wide low peak with two straight walls to
a flat base, extruded `thickness_mm` deep. In can units, where 1 is the can's
radius:

```text
                 (0, 3)                  the apex
              /         \
     (-1, 1.5)           (1, 1.5)        the shoulders
        |                  |
     (-1,-1.5)           (1,-1.5)        the base
```

The tent's plane holds the can's axis, so the block is **40 mm across, 90 mm tall
and 10 mm deep** at the design point (`scale 20`). It is a tall thin slab: it
presents a large face to inclined muons and a small one to vertical muons.

| Scale | Depth | Mass | Fits? |
|---|---|---|---|
| 15 mm | 10 mm | 20.1 g | yes |
| **20 mm** | **10 mm** | **35.7 g** | **yes — the design point** |
| 24 mm | 20 mm | 103 g | yes |
| 25 mm | 10 mm | 55.8 g | no — 25 mm is past the can's radius |
| 30 mm | 10 mm | 80.3 g | no |

The largest tent that fits at 10 mm deep is **24.44 mm** of scale.

### 3.2 The hexagon

The can is round; the tent is not. A regular hexagonal prism inscribed in the
Ø56 mm can has a horizontal cross-section of `3√3/2 · 28² = 2 036 mm²`, which is
**five times** the tent's `40 × 10 = 400 mm²`, for *less* mass at the same
thickness. Six sides already capture 83 % of the circle's area, and the six flat
faces give the sensors six distinct faces to sit on.

`Tropical` carries its own corners, so the tent and the hexagon are two choices of
the same six-point polygon and the transport never asks which.

| Block | Mass | Horizontal area | Median | Within 30° |
|---|---|---|---|---|
| 20 mm tent, 10 mm | 36 g | 400 mm² | 27.2° | 56 % |
| 24 mm tent, 20 mm | 103 g | 960 mm² | 25.1° | 59 % |
| 28 mm hexagon, 20 mm | 48 g | 2 036 mm² | 26.7° | 57 % |
| 28 mm hexagon, 30 mm | 73 g | 2 036 mm² | 25.9° | 62 % |
| **28 mm hexagon, 40 mm** | **97 g** | **2 036 mm²** | **24.2°** | **62 %** |

The depth buys accuracy because the chord a vertical muon crosses *is* the
thickness: 30 mm makes 630 photons where 10 mm makes 210. The 40 mm block is the
best of the five at 24.2°, but it is 24 g heavier for 1.7°, and the run-to-run
noise of the training is about half a degree — the 30 mm is the one to build, and
the 40 mm is the one to build if the mass budget turns out to have room.

---

## 4. The sensors

Four **onsemi MICROFC-10035** SiPMs, one per face, on the faces
`[Top, Bottom, Side(0), Side(2)]`:

| Figure | Value | Source |
|---|---|---|
| Area | 1 mm² | datasheet |
| Microcells | 504 | datasheet |
| PDE | 31 % at Vbr+2.5 V, 41 % at Vbr+5 V at 420 nm | datasheet; the model uses one mid-band **35 %** |
| Dark count | 30 kHz typ, 96 kHz max | datasheet |
| Crosstalk | 7 % at 35 µm cells | datasheet |
| SPTR | **not in the datasheet** | the dominant uncertainty |

The 1 mm² part is the right choice here and the wrong one for a scintillator: a
Cherenkov event yields a handful of photons, so the sensor's area is not the
limit; a scintillator plane yields hundreds, and the 504 microcells would clip the
pulse. A tracker reads *which* pixel fired, so clipping is harmless; a counter
reads the pulse height, so it is not.

### The SPTR is the budget's open wound

The C-series sheet gives the fast output's rise time (0.3 ns) and pulse width
(0.6 ns FWHM) and **no single-photon time resolution at all**. Published
measurements of this class of part put SPTR at 70–120 ps FWHM, and a 1 mm² sensor
with 35 µm cells is at the slow end because SPTR grows with cell size. The model
uses 100 ps FWHM, converted to a standard deviation (`100 / 2.355 = 42.5 ps`).
Every other term in the budget is smaller than the uncertainty on this one, which
is why the budget cannot be closed without measuring the part.

---

## 5. The readout

### 5.1 The chain

```text
SiPM (fast output) → TIA (OPA858) → comparator (MAX40026) → timer
```

| Part | Role | Figure | Verdict |
|---|---|---|---|
| onsemi MICROFC-10035 | SiPM | PDE 31–41 %, 504 cells | right for Cherenkov |
| TI OPA858 | TIA | 5.5 GHz, 2.5 nV/√Hz | fine, but the anode's 100 pF collapses its bandwidth — use the **fast** output (1 pF) |
| ADI MAX40026 | comparator | 280 ps delay, **2 ps jitter**, 25 ps overdrive dispersion | good; the dispersion is a *systematic walk*, not a jitter |
| ADI MAX35101 | "analog pulse timer" | 20 ps accuracy, 3.8 ps resolution, **2 channels** | **the wrong part for four SiPMs** |

### 5.2 The MAX35101 is not a general-purpose timer

This is the finding that changed the design, and it is worth stating plainly
because the part is listed as an "analog pulse timer" and reads like a TDC:

- **Its START is internal.** It fires its own pulse launcher and measures
  launch-to-echo. There is no external START pin, so it cannot answer "when did
  this SiPM fire".
- **Its STOP inputs are analog** — 10 to 700 mVpp into a comparator built for
  1 MHz ultrasonic transducers, not for a 0.6 ns SiPM pulse.
- **Its 20 ps is a differential accuracy.** The datasheet says absolute
  measurements "include circuit delays and cannot be considered accurate".
- **It has two channels**, not four.

A general-purpose alternative would be a TDC with an external START and STOP, such
as TI's TDC7200 or ScioSense's TDC-GP22. Neither is in the component list.

### 5.3 The readout model

The simulation's realistic readout (`Readout::nominal`):

| Term | Value | What it does |
|---|---|---|
| Threshold | 0.5 p.e. | the *k*-th photon, not the first, when the threshold is higher |
| Dark counts | 30 kHz | a dark count can fire before the light |
| Window | 100 ns | the timer's acquisition window |
| Walk | 25 ps | the comparator's amplitude-dependent delay |
| Crosstalk | 7 % | one photon can produce a second, delayed timestamp |

The ideal readout (the first photon, no dark counts, no crosstalk, no walk) is the
control: the gap between it and the nominal row is what a real instrument costs.

---

## 6. The timing

### 6.1 The signal

Light crosses the block at `c/n = 0.201 mm/ps`. The four sensors span **54.1 mm**
between the two furthest, so the largest arrival-time difference a heading can
produce is

```text
54.1 mm / 0.201 mm/ps = 269 ps
```

and the *worst-case over headings* signal is **224 ps** (the `y` axis: the sensors
span 45 mm there). `canlab_muon::timing::timing_signal_ps` computes it from the
sensor positions and the heading.

### 6.2 The jitter

| Part | ps | Random? |
|---|---|---|
| MICROFC-10035 | 42.5 | yes (100 ps FWHM converted to σ) |
| OPA858 | 6.0 | yes |
| MAX40026 | 2.0 | yes |
| MAX35101 | 20.0 | yes |
| **Total** | **47.4** | quadrature |

The signal-to-noise is `224 / 47.4 = 4.7`: the electronics can resolve the
geometry, and the budget decides how well. Halving it would halve the angular
uncertainty.

### 6.3 The bound

The Cramér–Rao bound on the heading from four timestamps, at 47.4 ps, is **81.2°
worst case** (48.2° at the best heading). That is the number the earlier study
quoted, and it is a *bound*: no estimator can do better from those four times
alone.

The bound is not the limit on the design. The limit is **where the sensors are**:
the two end sensors sit 10 mm apart (the block's thickness), so a heading along
that axis has almost no lever arm.

| Sensor layout | Worst | Best | Arm | Signal |
|---|---|---|---|---|
| four face centres (the design) | 81.2° | 48.2° | 54.1 mm | 224 ps |
| **four corners** | 56.3° | **16.4°** | 72.1 mm | 351 ps |
| two ends, two walls | 103.1° | 70.9° | 40.0 mm | 199 ps |

Moving the sensors to the corners buys a factor of three at the best heading with
the same parts. The design still uses the face centres because the corner
placement is a machining and assembly change that has not been simulated
end-to-end.

---

## 7. The reconstruction

### 7.1 Three ways to get a direction, and what each gives

| Method | Median error | Why |
|---|---|---|
| Linear fit, `t = t₀ + (r·d)/v` | 119.5° | the model is wrong: the light does not leave from a point |
| Cherenkov model, **true** track point | 81.1° | the model is right, but four timestamps cannot determine the track position |
| **Network (24 features)** | **27.0°** | learns `p(direction \| features)`, marginalising over the track position |

The network is not doing better physics. It is doing something the analytic
inversion cannot: the track position is a **latent variable**, and the network
absorbs it into the conditional distribution. The photon counts depend on the path
length, and the path length depends on where the track is, so the counts are worth
**2.9°** on their own.

The 24 features are: four first arrival times, four **second** arrival times, four
**mean** arrival times, four **spreads**, four "did it fire" flags, and four photon
counts. The second, mean and spread are what a multi-hit TDC records and a
single-threshold readout throws away.

**The multi-hit summary does not pay.** The ablation is direct: the hexagon at
30 mm is 25.9° with the mean and spread and 25.5° without, and the tent at 20 mm is
27.2° with and 27.5° without — a half-degree in opposite directions, which is the
run-to-run noise. The first photon and the count already carry most of what the
extra stops know, and the mean is dominated by the emission-time spread, which is
noise for the direction. The features are kept because they cost nothing and the
panel shows the data they summarise, but the design should not be sold on them.

**But the count is not what the readout can supply.** The component list is a
comparator and one TDC stop per sensor: a discriminator fires once per pulse and
the TDC records one timestamp. That is eight features — four first arrival times
and four flags — and retraining on exactly that (`train_realistic --first-only`)
gives, for the hexagon at 30 mm through the nominal readout:

| Features | Median | 68th | Within 30° | Within 60° |
|---|---|---|---|---|
| 24 — everything | 25.9° | 33.3° | 62 % | 95 % |
| 16 — first, second, flags, counts | 25.5° | 34.0° | 60 % | 95 % |
| **8 — first and flags, one threshold** | **33.6°** | **42.4°** | **43 %** | **91 %** |

The **second arrival** is worth **8°**. It costs more than every other feature
combined, and it takes the design from "borderline" to "fails the criterion". The
count and the mean/spread are noise by comparison, because the second arrival is
the feature that carries the track position — its gap from the first is the extra
path length, which is the distance to the track.

So the readout is the design. A single-threshold TDC gives 33.6° and 43 %; a
multi-hit TDC gives 25.5° and 60 %. The TDC-GPX2 has a 16-stage FIFO per channel
and unlimited multi-hit, so it is the part this design needs, not an optional
extra — and that is also the €55–60 and QFN64 problem.

### 7.2 The measured results

Through the realistic readout, on the corrected photon count:

| Block | Median | 68th | Within 30° | Within 60° |
|---|---|---|---|---|
| 20 mm tent, 10 mm | 27.2° | 36.2° | 56 % | 94 % |
| 24 mm tent, 20 mm | 25.1° | 34.9° | 59 % | 95 % |
| 28 mm hexagon, 30 mm | 25.9° | 33.3° | 62 % | 95 % |

The mission's criterion is 30°. The hexagon reaches it for three events in five,
which is **borderline rather than a pass** — the mission wants most events inside
30°, not three fifths.

These numbers are lower than the study first reported (the hexagon was 21.6° and
66 %), and the difference is a fix, not a regression: the timestamps used to be
the photon's **flight time** alone, without the time the muon took to reach the
point that emitted it. A timer records the arrival, which is emission plus
flight, and the emission time varies by up to 100 ps along the track — the same
order as the jitter. The absolute arrival is the honest number.

### 7.3 The photon count the transport measures

The design block fires **2.53 of its four sensors** on average over the measured
sky (`timing_realistic`), and **2.02** for a muon along its long axis
(`direction_study`). A horizontal muon through the 40 mm width yields **3.25
photoelectrons** across the four sensors. Those are the numbers that decide the
timing, and they are measured, not modelled: both commands run the transport event
by event.

An earlier version of the study credited the block with six times that light,
because the photon count was taken from the caller's 200 mm path instead of the
block's own chord, and the Poisson sampler underflowed at a mean of 709. Both are
fixed; the corrected count is what the table above reflects.

---

## 8. The cost

| Item | EUR |
|---|---|
| 4 × MICROFC-10035 at the top of the 10–25 € range | 100 |
| Board, connectors, mechanics | 60 |
| Timer (TDC7200 or equivalent) | 25 |
| Optical adhesive, Norland NOA 68, one 1 oz bottle | 32.70 |
| **Total** | **217.70** |

The detector's budget is 200–300 €, so the design fits with ~80 € of headroom. The
adhesive is a purchase and not a usage: the material is cents, the bottle is 33 €,
and one bottle bonds many blocks.

The block itself is the cheap part: PMMA is a few euros, and 73 g of it is a
hexagon 56 mm across.

---

## 9. The simulation

The design is simulated event by event in `canlab-muon`, and the picture in the
GUI is the same call:

1. Draw a muon direction from the measured sky (`I(θ) = I₀ cos²·¹ θ`, `I₀ = 66.7`).
2. Put the track through a random point inside the block.
3. Emit Cherenkov photons along the track, with the wavelength drawn from the
   `1/λ²` spectrum and the cone angle from `n(λ)`.
4. Trace each photon with total internal reflection, Fresnel loss, absorption,
   the sensors' coverage and their PDE.
5. Take the first (and second) arrival at each sensor; a sensor with no photon has
   no timestamp.
6. Reconstruct with the linear fit and with the network; measure the angle to the
   truth.

| What | Where |
|---|---|
| The block, the source, the transport | `canlab-optics` |
| The event loop, the features, the network | `canlab-muon::timing_study` |
| The readout model | `canlab-muon::readout` |
| The 3D view | `canlab-gui::detector` |

Commands:

```text
cargo run --release -p canlab-muon --bin timing_realistic   # the readout's price
cargo run --release -p canlab-muon --bin train_realistic    # the network, realistic
cargo run --release -p canlab-muon --bin direction_study    # the bound and the budget
cargo run --release -p canlab-muon --bin timing_iterations  # the design sweep
```

`train_realistic` takes `--scale`, `--thickness`, `--index` and `--hexagon`, which
is how the table in §3.2 was produced.

---

## 10. What is open

1. **The SiPM's SPTR is not in its datasheet.** It is the largest term in the
   jitter budget and the only one with no figure. It needs a measurement.
2. **The corner sensor placement is simulated only as a bound.** It gives 16.4° at
   the best heading against the face centres' 48.2°, and it has not been run
   end-to-end.
3. **The timer part is wrong.** The MAX35101 cannot timestamp four independent
   pulses; a TDC with an external START is needed, and none is in the component
   list yet.
4. **The adhesive's cure is unproven on this geometry.** NOA 68 cures at 365 nm,
   and PMMA absorbs there; it has to be cured from the bond line's edge with a
   395–405 nm LED, which needs a trial on scrap acrylic.
5. **The network's 25.5° is a simulation result.** The sim-to-real gap is
   unmeasured, and the domain randomisation only covers the parameters that were
   varied.
6. **The 30° criterion is met for three events in five**, not for most. Whether
   that is enough is a mission decision, not a simulation one.

---

## 11. The one-line summary

A 73 g hexagon of acrylic, four 1 mm² SiPMs and four timestamps recover a muon's
direction to **25.5° median, 60 % within 30°**, for **218 €** — which is the best
this approach gives, and it is borderline against the mission's criterion.
