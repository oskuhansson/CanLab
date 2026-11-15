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

> **Status, 2026-11-15 — the sensor model changed after these measurements.**
> The light guide as modelled delivered 60 % of a face to a 1 mm² sensor, which no
> passive element can: the light reaches the face within the escape cone
> (1.63 sr at n = 1.49) and the sensor accepts a hemisphere (6.28 sr), so etendue
> caps a taper at **3.87×** the bare coverage. The taper also has to be a **cavity
> in the block**, because the hexagon's corners touch the can's wall and a side
> face has only 3.75 mm of clearance at its centre. The sensor's jitter now scales
> with its area as well: the C-series fast output rises in 0.3 / 0.6 / 1.0 ns for
> the 1 / 3 / 6 mm parts, and the measured SPTR of a 3 mm part is 179 ps FWHM
> against ~100 ps for the 1 mm, so the 6 mm part pays **143 ps sigma** (337 ps
> FWHM) for its light.
>
> The design point is therefore the **6 × 6 mm part behind the taper**, and the
> numbers below are re-measured with it. The hexagon at 30 mm is **29.9° median
> and 50.3 % within 30°** through the nominal readout — the design does not pass
> the mission's criterion, and the light budget, the aperture and the jitter, not
> the geometry, are why: 20, 30 and 40 mm deep measure 29.6°, 29.9° and 29.9°.
> [vmf.md](vmf.md) has the distribution head measured on the same model.

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
an event yields about **10.2 photoelectrons across the four sensors** and
**2.27 of them fire**. Everything below is a consequence of those numbers — and
of the aperture that sets them.

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
design: the light is not lost to the room, it bounces until it reaches a face. It
does not rescue a 1 mm² sensor — the transport measures 0.09 photons per muon for
one — which is why the design point is the 6 × 6 mm part (§4).

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
| 20 mm tent, 10 mm | 36 g | 400 mm² | 31.5° | 47 % |
| 24 mm tent, 20 mm | 103 g | 960 mm² | 32.5° | 45 % |
| 28 mm hexagon, 20 mm | 48 g | 2 036 mm² | 29.6° | 51 % |
| **28 mm hexagon, 30 mm** | **73 g** | **2 036 mm²** | **29.9°** | **50 %** |
| 28 mm hexagon, 40 mm | 97 g | 2 036 mm² | 29.9° | 50 % |

**The depth buys almost nothing.** With the 6 × 6 mm part behind the taper the
light budget is set by the sensor's area and the taper, not by the chord: 20, 30
and 40 mm deep measure 29.6°, 29.9° and 29.9°, a spread of 0.3° against a
sampling error of about 1.3° on the median. The depth buys photons, not
resolution.

**The shape buys rate more than resolution.** The 20 mm tent measures 31.5° and
47 % against the hexagon's 29.9° and 50 % — 1.6° apart, about one sampling error,
for half the mass. What the hexagon buys is **muon rate**: 2 036 mm² of horizontal
area against 400, five times the crossings. The resolution is set by the
sensor's aperture on the face it sits on, not by the face's size.

So the depth and the shape buy photons and muons, and the sensor's aperture and
jitter set the resolution. The 20 mm tent is 36 g against the 30 mm hexagon's
73 g for a resolution that is within the noise of it; the hexagon is worth its
mass only if the muon rate is the binding constraint.

---

## 4. The sensors

Four **onsemi MICROFC-60035** SiPMs, one per face, on the faces
`[Top, Bottom, Side(0), Side(2)]`, each behind a **recessed taper** moulded into
the block:

| Figure | Value | Source |
|---|---|---|
| Area | 6 × 6 mm | datasheet |
| Microcells | 18 980 | datasheet |
| PDE | 31 % at Vbr+2.5 V, 41 % at Vbr+5 V at 420 nm | datasheet; the model uses one flat **35 %** |
| Spectral range | 300–950 nm, peak 420 nm | datasheet |
| Dark count | 1 200 kHz typ, 3 400 kHz max | datasheet |
| Crosstalk | 7 % at 35 µm cells | datasheet |
| Fast-output rise time | 1.0 ns (0.3 ns for the 1 mm part) | datasheet |
| SPTR | **not in the datasheet**; the model takes 333 ps FWHM from the rise time | the dominant uncertainty |

The sensor's area is a trade, not a free choice. The measured light budget, per
muon, on the hexagon at 30 mm:

| Sensor | Photons detected | Sensors timed | Aperture | Coverage of a side face | Jitter, sigma |
|---|---|---|---|---|---|
| 1 mm² bare | 0.11 | 0.08 | 1.0 mm | 0.12 % | 47.4 ps |
| 1 mm² + taper | 0.26 | 0.22 | 2.0 mm | 0.35 % | 47.4 ps |
| 3 × 3 mm + taper | 2.91 | 1.26 | 5.9 mm | 3.1 % | 87.5 ps |
| 6 × 6 mm bare | 2.51 | 1.15 | 6.0 mm | 4.3 % | 143.1 ps |
| **6 × 6 mm + taper** | **10.16** | **2.27** | **11.8 mm** | **12.4 %** | **143.1 ps** |

The aperture is the square on the face a photon must hit: the sensor's own side
for a bare part, `side × sqrt(gain)` for a tapered one. The taper's mouth is
11.8 mm for the 6 mm part, so it catches the light near the face centre that the
bare 6 mm square misses — 10.2 photons and 2.3 sensors against 2.5 and 1.2. It
does not make the sensor cover the face: a photon outside the 11.8 mm square is
lost however normal its direction.

The resolution follows the light:

| Sensor | Median | Within 30° | Four sensors, € |
|---|---|---|---|
| 1 mm² + taper | 35.9° | 38 % | 40–100 € |
| 3 × 3 mm + taper | 32.4° | 45 % | 80–180 € |
| **6 × 6 mm + taper** | **29.9°** | **50 %** | **160–360 €** |

So the sensor is a straight trade of money for resolution, and no point on it
passes the 30° criterion: the best is 50 % within 30°, and it is the most
expensive. The 1 mm² part cannot see the light even with the best possible taper:
etendue caps a taper at 3.87× the bare coverage, so 0.12 % becomes 0.35 %, one
photon per four muons. The 6 mm part sees 4.3 % bare and 12.4 % tapered — 2.21
sensors timed per muon and about 10.4 photons across the four. It pays for that
light with
jitter: the C-series fast output rises in 1.0 ns against the 1 mm part's 0.3 ns,
and the jitter follows the rise time, so the total readout jitter is **143 ps
sigma** against 47.4 ps — 337 ps against 112 ps FWHM, the units a datasheet
quotes SPTR in.

### The taper has to be a cavity, and its mouth is finite

A guide outside the block cannot work. The hexagon's circumradius is 28 mm and
the can's inner radius is 28 mm, so its corners touch the wall: a side face has
3.75 mm of clearance at its centre and none at the corners. A guide outside would
also see the light after it had refracted into the full hemisphere, where no
passive element can concentrate at all. The taper is therefore a cavity moulded
into the PMMA with the sensor at its bottom, and etendue caps what it can
deliver: the light reaches the face within the escape cone (1.63 sr at
n = 1.49), the sensor accepts a hemisphere (6.28 sr), and the ratio is 3.87×.

That cap fixes the **entrance aperture**. The guide's mouth is a square of
`sensor area × gain` — **11.8 mm on a side** for the 6 × 6 mm part at n = 1.49 —
centred on the face, and a photon outside it is not detected however normal its
direction. The earlier model tested only the direction, so every near-normal
photon was detected wherever it hit, and the detections traced a **strip across
the face** instead of a patch of the sensor's size. The aperture is also what
makes the taper buildable: 11.8 → 6 mm over a few millimetres is a 2:1 linear
taper, where a face-covering 45 → 6 mm cone would be 7.5:1 and very long.

### The SPTR is the budget's open wound

The C-series sheet gives the fast output's rise time and pulse width and **no
single-photon time resolution at all**. The model takes the jitter to scale with
the rise time — the measured SPTR of a 3 × 3 mm part is 179 ps FWHM against
100–120 ps for the 1 mm, the same factor of two as the rise times — so the 6 mm
part is 333 ps FWHM, or 141 ps sigma. The 143 ps the rest of this document
quotes is a sigma; the FWHM equivalent is 337 ps. That is the largest term in the budget and
the only one with no datasheet figure.

---

## 5. The readout

### 5.1 The chain

```text
SiPM (fast output) → TIA (OPA858) → comparator (MAX40026) → timer
```

| Part | Role | Figure | Verdict |
|---|---|---|---|
| onsemi MICROFC-60035 | SiPM | PDE 31–41 %, 18 980 cells | the light the 1 mm² part cannot see |
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

The figures are standard deviations; SPTR is quoted FWHM and converted.

| Part | 1 mm² budget, ps sigma | 6 mm design point, ps sigma |
|---|---|---|
| SiPM SPTR | 42.5 | 141.4 |
| OPA858 | 6.0 | 6.0 |
| MAX40026 | 2.0 | 2.0 |
| timer | 20.0 | 20.0 |
| **Total** | **47.4** | **143.1** |

The 1 mm² column is the component list's budget and the one the bound below is
quoted at. The design point's 6 mm part pays 143 ps sigma for its light: the
signal-to-noise falls from `224 / 47.4 = 4.7` to `224 / 143 = 1.6`, which is why
the measured resolution is 28° and not the 20° the 1 mm² column suggests.

### 6.3 The bound

The Cramér–Rao bound on the heading from four timestamps, at 47.4 ps, is **81.2°
worst case** (48.2° at the best heading). That is the number the earlier study
quoted, and it is a *bound* on the linear model `t = t₀ + (r·d)/v` — the model the
linear fit uses, which is wrong because the light does not leave from a point. At
the design point's 143 ps the same formula gives ~245°, past the point where it
says anything at all. The measured 26° is not a violation of it: it comes from a
different model, and the ablation shows where — the four timestamps and the flags
alone give 33.5°, and the second arrival and the counts add the rest.

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
30 mm is 29.9° with the mean and spread and 30.0° without: a tenth of a degree,
which is the noise. The mean and spread are dominated by the emission-time spread
and the path-length spread, and they buy nothing. The 16-feature set is the one
to ship because it is the smaller instrument for the same number.

**But the count is not what the readout can supply.** The component list is a
comparator and one TDC stop per sensor: a discriminator fires once per pulse and
the TDC records one timestamp. That is eight features — four first arrival times
and four flags — and retraining on exactly that (`train_realistic --first-only`)
gives, for the hexagon at 30 mm through the nominal readout:

| Features | Median | 68th | Within 30° | Within 60° |
|---|---|---|---|---|
| 24 — everything | 29.9° | 39.4° | 50.3 % | 92.5 % |
| **16 — first, second, flags, counts** | **30.0°** | **38.8°** | **50.1 %** | **92.5 %** |
| 8 — first and flags, one threshold | 33.5° | 42.8° | 42.2 % | 89.9 % |

The multi-hit record is worth **3.5°** in total and eight points of within-30°:
33.5° and 42.2 % on one threshold, 30.0° and 50.1 % with the second arrival and
the counts. The gap is the **second arrival**: its gap from the first is the
extra path length, and the extra path length is the distance to the track — the
latent variable the direction fit is missing. The mean and spread are not worth
adding on top.

So the readout is the design. A single-threshold TDC gives 33.5° and 42.2 %; a
multi-hit TDC gives 30.0° and 50.1 %. The TDC-GPX2 has a 16-stage FIFO per channel
and unlimited multi-hit, so it is the part this design needs, not an optional
extra — and that is also the €55–60 and QFN64 problem.

### 7.2 The measured results

Through the realistic readout, on the corrected photon count:

| Block | Median | 68th | Within 30° | Within 60° |
|---|---|---|---|---|
| 20 mm tent, 10 mm | 31.5° | 40.9° | 47 % | 91 % |
| 24 mm tent, 20 mm | 32.5° | 40.9° | 45 % | 92 % |
| 28 mm hexagon, 20 mm | 29.6° | 37.8° | 51 % | 94 % |
| 28 mm hexagon, 30 mm | 29.9° | 39.4° | 50 % | 93 % |
| 28 mm hexagon, 40 mm | 29.9° | 38.5° | 50 % | 93 % |

The mission's criterion is 30°. The best block reaches it for 59 % of the events,
which is **a fail rather than a pass** — the mission wants most events inside
30°, and the spread between the five blocks is smaller than the gap to the
criterion.

These numbers are lower than the study first reported (the hexagon was 21.6° and
66 %), and the difference is a fix, not a regression: the timestamps used to be
the photon's **flight time** alone, without the time the muon took to reach the
point that emitted it. A timer records the arrival, which is emission plus
flight, and the emission time varies by up to 100 ps along the track — the same
order as the jitter. The absolute arrival is the honest number.

### 7.3 The photon count the transport measures

The design block fires **2.27 of its four sensors** on average over the measured
sky, with **10.2 photons** across the four (`sensor_study`). Those are the numbers
that decide the timing, and they are measured, not modelled: the command runs the
transport event by event.

An earlier version of the study credited the block with six times that light,
because the photon count was taken from the caller's 200 mm path instead of the
block's own chord, and the Poisson sampler underflowed at a mean of 709. Both are
fixed; the corrected count is what the table above reflects.

---

## 8. The cost

| Item | EUR |
|---|---|
| 4 × MICROFC-60035 at 40–90 € | 160–360 |
| Board, connectors, mechanics | 60 |
| Timer, TDC-GPX2 (four channels, multi-hit) | 55–60 |
| Optical adhesive, Norland NOA 68, one 1 oz bottle | 32.70 |
| **Total** | **308–513** |

The detector's budget is 200–300 €, so the design is **at or over it**: 308 € at
the bottom of the sensor's price range and 513 € at the top. The sensor decides
it — four 6 mm parts are 160–360 € where four 1 mm parts were 40–100 €. The
3 × 3 mm part (20–45 €) brings the total to 228–328 € and costs resolution; the
1 mm² part fits comfortably and cannot see the light.

The timer line is the TDC-GPX2, not the TDC7200 the earlier table quoted: the
TDC7200 needs an external START and has two stops, and the design needs four
multi-hit channels (§5). That is the €55–60 and QFN64 problem.

The adhesive is a purchase and not a usage: the material is cents, the bottle is
33 €, and one bottle bonds many blocks.

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
5. **The network's 29.9° is a simulation result.** The sim-to-real gap is
   unmeasured, and the domain randomisation only covers the parameters that were
   varied.
6. **The 30° criterion is met for a little over half the events**, not for most,
   and the sensor that gets closest to it is at the top of the budget. Whether
   that is enough is a mission decision, not a simulation one.

---

## 11. The one-line summary

A 73 g hexagon of acrylic, four 6 × 6 mm SiPMs behind recessed tapers and four
timestamps recover a muon's direction to **29.9° median, 50 % within 30°**, for
**308–513 €** — which is the best this approach gives, it **fails** the mission's
30° criterion, and it is at or over the detector's budget.
