# The muon-direction detector

*A CanSat secondary mission: a muon that crosses a plastic block leaves light, and
that light carries the muon's direction. This document is the design study — what
the detector is, what the physics allows, which of six designs to build, and what
the simulation does and does not model.*

*Every number below is produced by `cargo run --release -p canlab-muon --bin
design_study` or `--bin direction_study`, and every constant is pinned by a test in
`crates/canlab-muon/tests/units.rs`.*

---

## 1. The headline

Three findings, in the order they matter.

**A direction from arrival times is measurable, and the block's sensor placement
wastes it.** Light crosses the 54.1 mm between the two furthest sensors in
**269 ps**, against a readout jitter of **47.4 ps** — a signal-to-noise of 5.7,
which is a measurement. The Cramér–Rao bound on the heading is **81°** at the worst
heading and 48° at the best, and the reason it is not the ~17° a well-spread
tetrahedron gives is that the two end sensors sit **10 mm apart**. Moving them to
the block's corners gives 56° at worst and **16° at best**.

**A direction from position is better and simpler.** Two planes separated by a gap
give two crossing points, and two points give a line. A 20 mm pixel over a 20 mm
gap is **23.4°**; over 40 mm it is 11.7°; over 60 mm it is 3.9°. No timing, no
picoseconds, no time-to-digital converter.

**A plastic scintillator gives 95 times the light of a Cherenkov radiator.** The
same centimetre that emits 210 Cherenkov photons emits 19 950 scintillation
photons. That removes the photon starvation the Cherenkov designs fight, and it
changes which sensor is correct: the 1 mm² part with 504 microcells saturates on a
scintillator and is right for a Cherenkov block.

**The recommendation is the short tracker**: 278 €, 23.4°, and 10 muons per flight.
It is the only design that passes the mission's audit.

---

## 2. The system

### 2.1 The can

The competition's can is a cylinder, 115 mm tall and Ø66 mm. Five millimetres of
allowance are taken off each dimension for the rolled rim, the lid and the
mounting frame, so a detector may use a **Ø56 mm circle** and **110 mm of height**.

| Quantity | Value | Where |
|---|---|---|
| Can | 115 mm tall, Ø66 mm | `CANSAT-GRUND.md` §3 |
| Cross-section | 3421 mm², 34.2 cm² | `π · 33²` |
| Usable after 5 mm of allowance | 2463 mm², **24.6 cm²** | `Can::cross_section_mm2` |
| Usable diameter | **56 mm** | `Can::inner_radius_mm` |
| Usable height | **110 mm** | `Can::inner_height_mm` |
| Mass budget | 300–350 g total | `CANSAT-GRUND.md` §3 |
| Detector's mass share | 15 % of 350 g = **52.5 g** | `Mission::audit` |
| Budget | ≤ 500 € total, **200–300 €** for the detector | `KOSTNADER.md` |

A detector that does not fit is refused by name, not silently shrunk:
`Can::admits` says which dimension is over and by how much. The geometry sweep's
best row is a 30 mm tent — 135 mm of block in a 110 mm can — and the first version
of the study reported it as the best geometry because it never asked.

### 2.2 The two light sources

A muon crossing a transparent plastic leaves light by one of two mechanisms, and
they differ by two orders of magnitude.

| | Cherenkov | Scintillation |
|---|---|---|
| Material | PMMA (n = 1.491) or a high-index plastic (n = 1.84) | PVT plastic scintillator |
| Mechanism | The muon outruns the phase velocity of light in the medium | The muon excites fluors, which emit |
| Yield | **21 photons/mm** at n = 1.49, 26.9 at n = 1.84 | **1 995 photons/mm** |
| Spectrum | `1/λ²`, 400–600 nm | 425 nm emission peak |
| Directionality | A **cone** at `cos θ = 1/(βn)` — 47.8° at n = 1.49 | Isotropic |
| Ratio | 1 | **95×** at PMMA's index, 74× at n = 1.84 |

The Cherenkov cone is what makes a Cherenkov block interesting — its angle encodes
the muon's direction — but the yield is so low that a 1 mm² sensor 25 mm away sees
**0.07 photons per muon**. The scintillator's yield is high enough that the
counting is easy and only the direction is in question.

### 2.3 The block

The Cherenkov block is a **tent prism**: the six-corner tent of
`canlab_optics::outline` — a wide low peak on two straight walls — extruded through
a thickness, with four SiPMs on four of its faces.

```text
                 (0, 3)                  the apex
              /         \
     (-1, 1.5)           (1, 1.5)        the shoulders
        |                  |
     (-1,-1.5)           (1,-1.5)        the base
```

The tent is not arbitrary: it is what you can cut and still keep a
total-internal-reflection cavity with a flat face for the sensor. With a CNC
available, the outline is any closed 2.5D shape within the tool's reach, so the
tent is a *choice* the study should sweep rather than a manufacturing limit.

| Design block | Value |
|---|---|
| Scale | 20 mm (the tent is 2·scale wide and 4.5·scale tall) |
| Silhouette | 40 mm × 90 mm |
| Thickness | 10 mm |
| Volume and mass | 30 cm³, 35.7 g of PMMA at 1.19 g/cm³ |
| Mounting | On edge: the tent's plane holds the can's axis |
| Largest tent that fits at 10 mm deep | 24.44 mm of scale |

The block is mounted **on edge** because its 40 × 90 mm silhouette cannot lie flat
in a 56 mm circle. On edge, the circle constrains the 40 × 10 mm cross-section
(20.6 mm of half-diagonal against 28 mm of radius) and the height constrains the
90 mm. The muon crosses the walls, so the face it enters is 90 × 10 mm and the
material it crosses is 40 mm.

---

## 3. The physics

### 3.1 The Cherenkov yield

A charged particle travelling faster than the phase velocity of light in a medium
of index `n` emits along a cone at

```text
cos θ_c = 1 / (β n),        threshold  β n = 1
```

with the spectrum

```text
dN/dx dλ = 2π α z² / λ² · (1 − 1/(β²n²))
```

Integrating over a band 400–600 nm gives

```text
dN/dx = 2π α (1 − 1/(β²n²)) (1/λ_blue − 1/λ_red)
```

with `λ` in the same length unit as `x`. At β ≈ 1 and n = 1.49 that is **21.0
photons/mm**, or 210 per centimetre; at n = 1.84 it is 26.9/mm. The textbook figure
of 250–300 photons/cm is for the full visible range, and 400–600 nm is the band
the sensors see.

The cone is not one angle: `n(λ)` falls from blue to red, so a plastic with
`dn/dλ ≈ −0.016/µm` spreads the cone by about 0.3° across the band. That is 5 ps of
arrival-time spread over 100 mm of path — the same order as the whole timing goal,
and therefore not a detail.

### 3.2 The scintillation yield

A plastic scintillator (PVT, such as EJ-200) emits about **10 000 photons per MeV**
of deposited energy. A minimum-ionising muon loses 1.95 MeV per (g/cm²), and the
density is 1.023 g/cm³, so it deposits **1.995 MeV/cm** — and the same centimetre
that gives 210 Cherenkov photons gives **19 950** scintillation photons.

That factor of 95 is the whole reason the tracker designs work. It also breaks the
component list's sensor: the MICROFC-10035 has **504 microcells**, and a 5 mm
scintillator plane gives 873 photoelectrons, so the sensor saturates. Saturation is
harmless where the measurement is *which pixel fired* — a clipped pulse is still a
hit — and fatal where it is the pulse height. That is why the counter uses the
6 mm part with 18 980 cells and the trackers use the 1 mm part.

### 3.3 Photon transport

A photon born inside the block travels in a straight line until it meets a face.
There it either escapes by refraction, reflects by total internal reflection, or is
absorbed. The rules:

- **Total internal reflection** above the critical angle `sin θ_c = 1/n`. At a
  PMMA/air face with n = 1.491 the critical angle is 42.2°, so only
  `(1 − cos 42.2°)/2 = 13 %` of the light arriving at that face escapes and **87 %
  is trapped**.
- **Fresnel reflection** below the critical angle, computed for both
  polarisations.
- **Absorption** by `exp(−d/L)` along the path, with `L` the absorption length.

The transport reports the path length and the *displacement* separately, because a
photon that bounced twice arrives along a straight line having travelled further —
and the difference is the difference between `t = L·n/c` and the answer a timing
measurement gives.

### 3.4 Detection

Three draws decide whether a photon becomes a photoelectron:

1. **Coverage.** A 6 × 6 mm sensor on a 236 mm² face sees 15 % of it. This is the
   loss that dominates everything else.
2. **PDE.** 31 % at Vbr + 2.5 V, 41 % at +5 V, peaking at 420 nm. The model uses
   one mid-band 35 %.
3. **Jitter.** A Gaussian with the sensor's single-photon time resolution.

The jitter budget, from the four parts' datasheets:

| Part | ps | Random | Source |
|---|---|---|---|
| onsemi MICROFC-10035 | 42.5 | yes | **not in the datasheet**; 70–120 ps FWHM is the published range, 100 ps FWHM is the middle |
| ADI MAX35101 | 20.0 | yes | datasheet: 20 ps accuracy, 3.8 ps resolution |
| TI OPA858 | 6.0 | yes | 2.5 nV/√Hz over a 0.6 ns edge at a 10 mV pulse |
| ADI MAX40026 | 2.0 | yes | datasheet: 2 ps jitter, 150 ps edge, 100 mV overdrive |
| **Total** | **47.4** | | quadrature of the random terms |

The largest term is the one with no datasheet figure, which is why the budget
cannot be closed without measuring the part.

### 3.5 Light scattering, and the optical adhesive

A machined acrylic face has about a micrometre of roughness, and a photon trapped
by total internal reflection meets that roughness at every bounce. Fresnel
reflection assumes a mirror, so a machined block's light yield is an **upper**
bound until a scattering model exists.

Optical adhesive fixes that — it fills the scratches — and it fixes something
larger that the roughness discussion hides. At a PMMA/air face only 13 % of the
light escapes; an adhesive with an index above PMMA's makes the interface go from
lower to higher index, so there is **no critical angle at all** and every photon
that reaches the bonded face crosses it.

The escape-cone arithmetic says that is a factor of 7.3, and the transport says it
is **1.9**. The difference is that the trapped light was not lost — it bounced to
another face and escaped there. The measured table:

| Block | Adhesive | Detected | Gain |
|---|---|---|---|
| PMMA, n = 1.49 | none | 19.19 | — |
| PMMA, n = 1.49 | NOA 68 | **37.05** | **1.9×** |
| High-index, n = 1.84 | none | 7.50 | — |
| High-index, n = 1.84 | NOA 68 | **32.66** | **4.4×** |

The adhesive helps the high-index block *more* — a 4.4× gain against PMMA's 1.9× —
because a block that traps more has more to gain from opening one face. It closes
the 2.6× material gap to 1.1×, and PMMA still wins, and is cheaper and lighter.

The choice is **Norland NOA 68** (cured n = 1.54, formulated for acrylic adhesion,
€32.70 for a one-ounce bottle). All the candidates remove the critical angle and
all are index-matched well enough that the Fresnel loss is a part in 10 000; the
choice is adhesion to acrylic and availability in Europe.

The curing is the real risk: PMMA's transmission band starts at 380 nm and the
adhesive cures at 350–380 nm, so **you cannot cure through the block**. The
process is to cure from the bond line's edge with a 395–405 nm LED, or to bond the
sensor to a thin carrier first.

---

## 4. The two observables

### 4.1 Timing

The premise is that a photon's arrival time at sensor `i` is

```text
t_i = t₀ + (r_i · d) / v
```

Three unknowns against four timestamps, so a least-squares fit recovers the
heading. `canlab_muon::timing::reconstruct` implements it and returns the exact
heading from exact timestamps.

The scale is measurable: light moves 0.2 mm/ps in PMMA, so the whole detector's
spread of `(r_i · d)/v` is **224 ps** — light needs 269 ps to cross the 54.1 mm
between the two furthest sensors. Against 47.4 ps of jitter the signal-to-noise is
5.7.

The Cramér–Rao bound on the heading is `σ·v/L`, and it depends on where the sensors
are:

| Placement | Worst | Best |
|---|---|---|
| four face centres (the block's own faces) | 81° | 48° |
| four block corners | 56° | **16°** |
| a well-spread 54 mm tetrahedron | 17.5° | 17.5° |

So timing works and this placement does not. Moving the sensors to the corners is
the fix, and it is a board change rather than a part change.

**A correction this study had to make.** The first version of these numbers was
wrong by a factor of a thousand: the speed of light was written as 299.79 mm/ps,
which is millimetres per *nanosecond*. Light was a thousand times too fast, every
timing signal a thousand times too small, and a direction measurable at tens of
degrees was reported as unmeasurable at eighty thousand. The tests did not catch it
because they asserted the code's own arithmetic; `tests/units.rs` now states every
physical fact independently.

### 4.2 Position

A sensor that reports **where** a photon crossed its face — a pixel, a fibre, a
strip — gives a direction from two crossings. The uncertainty is

```text
σ_θ = √2 · σ_h / L
```

with `σ_h` the perpendicular uncertainty of one hit and `L` the separation between
the two. For a pixel of width `p`, `σ_h = p/√12`.

| Pixel | Separation | Bound |
|---|---|---|
| 10 mm | 20 mm | 23.4° |
| 10 mm | 40 mm | 11.7° |
| 10 mm | 60 mm | 3.9° |
| 1 mm | 54.1 mm | 1.5° |

No timing, no picoseconds, no TDC. The block is still needed — it is what makes
the light — but the observable is the hit position.

**The gap buys the angle and costs the sample.** A muon at 30° drifts
`40 × tan 30° = 23 mm` sideways over a 40 mm gap, which is most of the plane's
40 mm width, so it misses the second plane. Averaged over the sea-level `cos²θ`
flux, a 40 mm gap keeps **27 %** of the crossings and a 20 mm gap keeps **51 %**.

---

## 5. The six designs

| Design | EUR | g | Sensors | Angle | Crossings | Directed | Verdict |
|---|---|---|---|---|---|---|---|
| counter | 148 | 37.2 | 1 | — | 31.3 | 31.3 | no direction |
| cherenkov timing | 218 | 47.7 | 4 | 81.2° bound | 11.4 | 11.4 | fails at 30° |
| **short tracker** | **278** | **28.0** | **8** | **(23.4°) formula** | **19.8** | **10.1** | **passes** |
| coarse tracker | 278 | 28.0 | 8 | (11.7°) formula | 19.8 | 5.3 | too few muons |
| fine tracker | 923 | 28.0 | 32 | (3.9°) formula | 19.8 | 3.1 | over budget |
| ring imager | 523 | 22.7 | 16 | (10.6°) estimate | 11.4 | 11.4 | over budget |

*Crossings* are muons through the first plane in a 100 s flight; *directed* are the
ones that cross both planes and therefore have a direction.

### 5.1 The counter — optimises cost and reliability

One scintillator plane, one sensor, no direction. The repo's own design, from
`MUONDETEKTOR.md` §4: a plastic scintillator plate with a SiPM, counting pulses per
second and comparing the rate at 1 km with the rate on the ground.

Its plate is a **Ø56 mm disc**, 10 mm thick, mounted across the can's axis:
24.6 cm² and 31.3 muons in a 100 s flight. It uses the **largest** sensor because a
counter's science is a rate *and* a crude energy, so it needs the pulse height.

### 5.2 Cherenkov timing — optimises the existing parts

The block, four sensors, a timer. It fails the 30° direction requirement at 81.2°,
but not because timing is impossible: the two end sensors sit at the centres of the
±z faces, only 10 mm apart, so a heading along that axis has almost no lever arm.
The same block with the sensors on its corners gives 16° at best.

The photon count is the other problem: the transport measures **19 photoelectrons**
per muon for the Cherenkov block, because 11 % of the light is trapped by total
internal reflection and much of the rest leaves through a face no sensor covers.

### 5.3 The short tracker — the recommendation

Two planes of 2 × 2 pixels, a **20 mm** gap, eight sensors. **278 €, 23.4°, 10.1
muons — the only design that passes the audit.**

The resolution is `√2·σ_h/L`: a 20 mm pixel has `σ_h = 20/√12 = 5.77 mm`, and over
a 20 mm gap that is 0.408 rad = 23.4°. The 20 mm gap is the sample-first choice:
it keeps 51 % of the crossings against the 40 mm gap's 27 %.

Two things make it work. **The gap is free** — it is air inside a can that is
110 mm tall — and **saturation does not matter**, because a tracker reads out
*which* pixel fired, so it can use the 1 mm² part at 25 € instead of the 3 mm² at
45 €.

### 5.4 The coarse tracker — the angle per event

The same planes with a 40 mm gap: **11.7° and 5.3 muons**. Better per event and
worse per flight, because five muons is not a distribution. The audit fails it on
the sample.

### 5.5 The fine tracker — the best money can buy

Two planes of 4 × 4 pixels and a 60 mm gap: **923 € and 3.9°**. Three times the
budget for three times the resolution, and only 3.1 muons behind it. It is in the
table because "what would 3.9° cost" has an answer.

### 5.6 The ring imager — unproven

A Cherenkov block with a 4 × 4 array on its exit face. A muon emits a cone at
47.8°, and where the cone meets the face is a ring whose radius gives the angle.

**It has never been simulated**, and it has only 19 photoelectrons per muon. A ring
needs tens of photons to have a shape; nineteen is a handful of dots. Its 10.6° is
a geometric estimate and is marked unchecked wherever it appears.

---

## 6. The parts

From `can-sat`'s `komponenter` list, with datasheet numbers in
`canlab_muon::components`:

| Part | Number | Consequence |
|---|---|---|
| onsemi MICROFC-10035 | PDE 31 % at Vbr+2.5 V, 41 % at +5 V; 30 kHz dark count; 7 % crosstalk; **SPTR not specified** | the dominant jitter term is an assumption |
| TI OPA858 | 5.5 GHz, 2.5 nV/√Hz | 6 ps; but its bandwidth collapses with the anode's 100 pF |
| ADI MAX40026 | 280 ps delay, **2 ps jitter**, 25 ps overdrive dispersion | negligible random, non-negligible walk |
| ADI MAX35101 | **20 ps accuracy**, 3.8 ps resolution, 2 channels | **wrong part for four SiPMs** |

**The MAX35101 is the wrong part for four independent SiPMs.** Its START is
generated internally by its own pulse launcher — there is no external START pin —
so it measures *launch-to-echo*, which is an ultrasonic time-of-flight, not "when
did this SiPM fire". Its two STOP inputs are *analog* (10–700 mVpp) into a
comparator specified for 1 MHz transducers. And its 20 ps is a **differential**
accuracy: the datasheet says absolute measurements "include circuit delays and
cannot be considered accurate".

**The fast output must be used, not the anode.** The MICROFC-10035's
anode–cathode capacitance is 100 pF and its fast-terminal capacitance is 1 pF. An
OPA858 TIA behind 100 pF at 10 kΩ has a 30 MHz bandwidth — a 33 ns rise time,
which would destroy any timing measurement before the comparator saw it.

### 6.1 The budget

| Item | Cost | Note |
|---|---|---|
| Short tracker, 8 × MICROFC-10035 | 200 € | 25 € each at the top of the range |
| Board, connectors, mechanics | 45 € | |
| Norland NOA 68 adhesive | 32.70 € | the smallest purchase; the material used is 0.05 ml |
| **Total** | **278 €** | inside the 200–300 € allocation |

---

## 7. The simulation

`canlab-muon`, separate from the CAS, which is used only to draw results.

| Module | Question |
|---|---|
| `can` | does the block fit a Ø56 × 110 mm can? |
| `mission` | the parameters and requirements of every simulation, and an audit |
| `components` | the four parts' datasheet numbers, the jitter budget, the adhesive |
| `scintillator` | the yield, the saturation, the sensor choice |
| `timing` | least-squares heading from timestamps; the timing signal |
| `bound` | Cramér–Rao bound on the heading; the position-based alternative |
| `designs` | the six approaches, their metrics, their audit |
| `tracker` | a two-plane event: light, scattering, crosstalk, noise, Poisson |
| `domain` | the randomised parameter ranges |
| `net` | a small MLP with Adam, written out because the workspace is offline |
| `solve` | a 3×3 solve with a unit-free singularity test |

The physics is `canlab-optics` and is not repeated: the tent, the Cherenkov source,
Fresnel, the photon transport and one event's four timestamps.

### 7.1 What is modelled

| Effect | Why it is in the model |
|---|---|
| Scintillation yield | 1 995 photons/mm, so a 5 mm crossing is ~1 MeV |
| Inclined path | A muon at 30° crosses 15 % more material and makes 15 % more light |
| **Light scattering** | The halo around the hit is what the network reads position from |
| Ballistic fraction | Some light reaches the pixel under the hit; the rest diffuses |
| Pixel crosstalk | Charge leaks to neighbours, blurring the pattern |
| Dark counts | The SiPM fires without light |
| Gain spread | No two pixels have the same gain |
| Electronic noise | The readout is not noiseless |
| Poisson statistics | The light is counted, not measured |
| Coincidence | A muon that misses the second plane is refused, not weighted |

### 7.2 What is not modelled

| Not modelled | Why it matters |
|---|---|
| A PDE curve | The model uses one mid-band 35 %; the part is 31–41 % and peaks at 420 nm |
| SPTR | The vendor does not specify it, and the model's 100 ps is a placeholder |
| Crosstalk (7 %) | One photon can produce two timestamps |
| Dark counts (30–96 kHz) | A 100 ns window is not always empty |
| Vbr drift (21.5 mV/°C) | A can descending through 10 °C changes its PDE and SPTR in flight |
| Surface scattering | A machined face has ~1 µm of roughness and Fresnel assumes a mirror |
| Time walk | The comparator's 25 ps of overdrive dispersion tracks the pulse amplitude |
| A full photon Monte Carlo | The light model is a kernel, not a transport: it gives the expected photoelectrons per pixel and draws the Poisson count |

The omissions do not all point the same way, which is why they are listed rather
than corrected for: gain and pulse height do not change an arrival time,
afterpulsing and crosstalk would add late or extra timestamps, saturation caps the
count, and dark counts would add photons the model does not have.

### 7.3 Domain randomisation

A network trained on one value of a parameter learns that value. If the real sheet
scatters over 25 mm and the training data used 15 mm, the network has learned a
bias that no amount of training removes. Randomising the parameter per event means
the network cannot learn it, so it learns the physics that survives the variation.

| Parameter | Range | Why it is unknown |
|---|---|---|
| `lambda_mm` | 4–40 mm | The light spreading length depends on thickness, wrapping and polish, and nobody has measured this sheet |
| `ballistic_fraction` | 0.15–0.85 | Depends on the coupling and the reflector |
| `collection` | 0.10–0.40 | The fraction reaching the array; a real paddle spans this |
| `bond_transmission` | 0.85–1.0 | The bonded interface; 0.13 with an air gap |
| `pde` | 0.25–0.45 | The datasheet's 31–41 % at two overvoltages, plus temperature |
| `crosstalk` | 0–0.12 | 7 % is the typical at 35 µm cells; the spread is not |
| `dark_counts` | 0–3 pe | 30–96 kHz over a ~100 ns window, times the pixel count |
| `gain_spread` | 0–0.15 | No two pixels match; the spread is not specified |
| `noise_pe` | 0–1.5 pe | The readout's noise, which depends on the board |
| `gap_mm` | ±1 mm | The machined spacing has a tolerance |
| `pitch_mm` | ±0.3 mm | The array's own tolerance |
| `thickness_mm` | ±0.2 mm | The sheet's thickness has a tolerance |

The muon's angular distribution is **not** randomised: it is the `cos²θ` flux, and
varying it would be varying the physics rather than the uncertainty.

Two ways randomisation can go wrong. **Too narrow**, and the network has learned
the training values. **Too wide**, and the network learns nothing because every
event is a different detector; the loss curve flattens in a way that looks like
underfitting. The ranges are the plausible span for each parameter, not a safety
margin around a guess.

### 7.4 The network

A small multilayer perceptron, written out because the workspace is offline: three
dense layers, ReLU in the hidden ones, a linear output, mean-squared error, and
Adam. The input is the two planes' pixel patterns, normalised, with their totals:
`2 × n² + 2` values. The output is the direction's two horizontal components, with
the vertical one reconstructed from the unit constraint.

The loss is the mean squared error on `(dx, dy)`. The natural loss for an angle is
`1 − cos θ`, and for small angles the two agree to second order — `1 − cos θ ≈
θ²/2`, and `|d − d̂|² ≈ θ²`. The squared error is used because its gradient is
linear and does not vanish at the origin; the *reported* metric is the angle.

No dropout, no batch normalisation, no learning-rate schedule. The domain
randomisation is the regulariser, and it is a stronger one than dropout because it
changes the *data* rather than the network.

---

## 8. Open questions

1. **Is the direction mission worth flying with a position-sensitive detector?**
   A pixelated or fibre readout is a different build, and the answer decides
   whether the detector is a direction measurement or a counter.
2. **What is the MICROFC-10035's SPTR?** It is the largest term in the jitter
   budget and the only one with no datasheet figure. It needs a measurement.
3. **Is the MAX35101 fixed, or can the timer change?** As specified it cannot
   timestamp four independent pulses. A general-purpose TDC with an external START
   would be the replacement, and the bound says timing cannot beat position anyway.
4. **What is the block's material?** PMMA collects more light than a high-index
   plastic in this geometry — 8.06 against 4.74 detected photons at the sweep's 17 mm path — because a higher
   index traps more of what it makes. More Cherenkov light is not better light.
5. **Can the adhesive be cured?** PMMA absorbs at the adhesive's curing wavelength.
   The edge-cure process needs a trial on scrap acrylic before it is committed to.
6. **Does the tracker's 23.4° survive a real simulation?** It is a formula, not a
   simulated hit: no code in the crate generates a tracker event yet. The light
   model and the network are written; the training run is the next step.
