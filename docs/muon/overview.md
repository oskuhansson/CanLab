---
tags: [cansat, muon, overview, design]
status: aktuell
updated: 2026-11-14
---

# The muon detector — current design

*A one-page overview of where the design stands: what it is, what it measures, how
well, and what is still uncertain. The full reasoning is in
[DESIGN.md](../../crates/canlab-muon/DESIGN.md); the physics is in
[physics.md](physics.md).*

---

## The design in one paragraph

A **PMMA hexagon**, 56 mm across and 30 mm deep, sits inside the can. A muon
crossing it emits Cherenkov light along a cone at 47.8°. Four **6 × 6 mm SiPMs**
on four of the block's faces, each behind a taper moulded into the acrylic,
record the photons that reach them, and a multi-hit time-to-digital converter
timestamps them. Twenty-four numbers per event — four first arrivals, four second
arrivals, four means, four spreads, four "did it fire" flags and four photon
counts — go into a small neural network, which returns the muon's direction.

**The measured resolution is 26.4° median (36.4° at the 68th percentile), and
56.8 % of events are within the mission's 30° — a fail, not a pass.**

---

## The signal chain

```mermaid
flowchart LR
    subgraph G_MUON["The muon"]
        MUON(("4 GeV muon, 40 mm through PMMA"))
    end
    subgraph G_LIGHT["The block"]
        CONE["Cherenkov cone, 47.8 deg, 21 ph/mm"]
        TIR["Total internal reflection"]
        FACE["13 percent escapes a face"]
    end
    subgraph G_SENSOR["The readout"]
        SIPM["Four SiPMs, 504 cells, 35 percent PDE"]
        TDC["First photon per sensor, 143 ps jitter"]
    end
    subgraph G_NET["The network"]
        FEAT["24 features: first, second, flags, counts, mean, spread"]
        MLP["64 x 64 network, 5890 parameters"]
        DIR{{"Direction, 26.4 deg median"}}
    end
    MUON -->|"emits"| CONE
    CONE -->|"trapped"| TIR
    TIR --> FACE
    FACE -->|"~2 percent reaches a sensor"| SIPM
    SIPM --> TDC
    TDC --> FEAT
    FEAT --> MLP
    MLP --> DIR
```

The number that decides everything is on the arrow from the block to the sensors:
**about 2 % of the light reaches a sensor**. Two losses do it — 87 % of the light
arriving at a PMMA/air face is trapped by total internal reflection, and the
sensors cover 15 % of the faces they sit on.

---

## The numbers

| Quantity | Value | Where |
|---|---|---|
| Block | 56 mm hexagon, 30 mm deep, PMMA, n = 1.49 | `Tropical::hexagon(28, 30)` |
| Block mass | 73 g | `hexagon_mass_g` |
| Cherenkov yield | 21 photons/mm, 630 per 30 mm crossing | `photons_per_mm(1.0, 1.49)` |
| Photons detected | 11.1 per muon across the four sensors | `sensor_study` |
| Sensors | 4 × MICROFC-60035, 18 980 microcells, 35 % PDE, behind a taper | `components` |
| Readout jitter | 143.1 ps total (141.4 SiPM, 20 timer, 6 amp, 2 comparator) | `SensorModel::jitter_ps` |
| Timestamps per event | 1.42 of 4 on average; 8.7 % fire all four | `sensor_study` |
| Timing signal | 209 ps across the detector | `timing_signal_ps` |
| **Direction resolution** | **26.4° median, 36.4° at 68 %, 56.8 % within 30°** | `train_realistic` |
| Cost | 308–513 € | `cherenkov.md` §8 |

---

## How the direction is recovered

The network is a **regressor**, not a classifier: it outputs two continuous numbers
`(dx, dy)`, the horizontal components of the direction, with the vertical one
recovered from the unit constraint. The loss is mean squared error, so it converges
to the conditional mean `E[direction | features]`.

The reported 26.4° is therefore the **median angular error of that point estimate**
over held-out events — the irreducible spread of the true direction around its own
conditional mean. Half the muons are reconstructed to better than 26.4°, and the
**realistic readout** (a threshold, dark counts, crosstalk and walk) keeps **57 %**
of events within the mission's 30°.

| Configuration | Median | 68th percentile | Within 30° |
|---|---|---|---|
| first and flags, one threshold (8 features) | 31.3° | 39.8° | 47.8 % |
| **first, second, flags, counts (16 features)** | **24.6°** | **32.2°** | **62.8 %** |
| everything (24 features) | 26.4° | 36.4° | 56.8 % |

The extra stops are the **second arrival** at each sensor — what a multi-hit TDC
records and a single-threshold readout throws away — plus the photon counts. The
second arrival and the counts together are worth **6.7°** and fifteen points of
within-30°. The mean and spread over every photon are not worth adding: they
*raise* the median by 1.8°, because they are dominated by the emission-time
spread, which is noise for the direction.

### Why the network beats the algebra

Two analytic reconstructions were tried first, and both are worse:

| Method | Median error | Why |
|---|---|---|
| Linear fit, `t = t₀ + (r·d)/v` | 119.5° | The model is wrong: the light does not leave from a point |
| Cherenkov model, **true** track point | 81.1° | The model is right, but four timestamps cannot determine the track position |
| **Network** | **26.4°** | Learns `p(direction \| features)`, marginalising over the track position |

The network is not doing better physics. It is doing something the analytic
inversion cannot: the track position is a **latent variable**, and the network
absorbs it into the conditional distribution. The ablation shows where the
information comes from — the photon counts depend on the path length, and the path
length depends on where the track is, so the counts and the second arrival are
worth **6.7°** together.
That is less than the earlier study reported, and the difference is a bug fix: the
photon count used to be taken from the caller's 200 mm path instead of the block's
own chord, which credited the block with six times the light it makes.

### The network as a distribution

The regressor returns a point and no uncertainty. A second head — the same
network with a **von Mises–Fisher** output — returns a mean direction *and* a
concentration `κ`, so every event carries its own predicted error. On the same
8 000 events (hexagon at 30 mm) it lands on **26.0°** and **59.9 % within 30°**,
within half a degree and three points of the regressor, so the distribution costs
nothing in the mean direction. Its `κ` ranks events correctly — the events it
calls hard are harder — and its 30° probability is close: **62.7 % predicted,
59.9 % observed**. The vMF tail is slightly light, and a single scale on `κ` would
close the last three points without touching the direction. [vmf.md](vmf.md) has the measurement and the calibration table.

---

## The block

A **hexagon**, not a slab, because the can is round: a regular hexagon inscribed
in the Ø56 mm inner circle has a horizontal cross-section of `3√3/2 · 28² =
2 036 mm²`, five times the 40 × 10 mm tent's, and its six flat faces are what the
sensors sit on. The corners touch the wall, so the block uses the full diameter
and the depth is the only free dimension: 30 mm is the design point, and 20 and
40 mm measure the same resolution within the noise.

The taper is a **cavity** moulded into the PMMA, not a guide outside it: a side
face has 3.75 mm of clearance at its centre and none at the corners, and a guide
outside would see the light after it had refracted into the full hemisphere, where
no passive element can concentrate at all.

---

## The parts

| Part | Role | Key number |
|---|---|---|
| onsemi MICROFC-60035 | SiPM, 6 × 6 mm | 18 980 microcells, PDE 31–41 %, **SPTR not specified** |
| TI OPA858 | transimpedance amplifier | 5.5 GHz, 2.5 nV/√Hz |
| ADI MAX40026 | comparator | 280 ps delay, 2 ps jitter, 25 ps overdrive dispersion |
| ScioSense TDC-GPX2 | four-channel multi-hit timer | 20 ps rms, 16-stage FIFO per channel, **no external START — free-running against REFCLK** |
| Norland NOA 68 | optical adhesive | cured n = 1.54, €32.70 |

**The MAX35101 is the wrong part for four independent SiPMs.** Its START is
generated internally by its own pulse launcher, so it measures launch-to-echo
rather than "when did this SiPM fire", and its 20 ps is a *differential* accuracy.
The TDC-GPX2 is the replacement: four channels, multi-hit, and a free-running
reference counter so no START is needed. It is also €55–60 and QFN64, which is the
cost and assembly problem.

**Optical adhesive nearly doubles the light.** At a PMMA/air face only 13 % of the
light escapes and an adhesive with an index above PMMA's removes the critical angle
entirely — but the trapped light was not lost, it bounced to another face, so the
transport measures a **1.9× gain** (19.19 → 37.05 detected photons), not the 7.7×
the escape-cone arithmetic suggests.

It also **makes a high-index block viable**: bonding takes it from 7.50 to 32.66
detected photons, a 4.4× gain that closes a 2.6× gap to 1.1×. PMMA still wins
slightly, and it is cheaper and lighter.

The curing is the real risk: PMMA absorbs at the adhesive's 365 nm curing
wavelength, so it has to be cured from the bond line's edge with a 395–405 nm LED.

---

## What is simulated, and what is not

The simulation is `canlab-muon`, separate from the CAS, which is used only to draw
results.

**Modelled:** the Cherenkov yield, total internal reflection,
Fresnel reflection, absorption, the sensors' coverage and PDE, Poisson statistics,
the readout jitter, the coincidence, and the network's training.

**Not modelled:** a PDE curve (one mid-band 35 % is used), the SiPM's SPTR (the
vendor does not specify it; 100 ps FWHM is a placeholder), crosstalk (7 %), dark
counts (30–96 kHz), the breakdown voltage's temperature drift (21.5 mV/°C), surface
scattering from the machined faces, and the comparator's time walk.

The omissions do not all point the same way, which is why they are listed rather
than corrected for.

---

## What is still uncertain

1. **The SiPM's SPTR is not in its datasheet.** It is the largest term in the
   jitter budget and the only one with no figure. It needs a measurement.
2. **The tracker's light model is a kernel, not a transport.** It gives the
   expected photoelectrons per pixel and draws the Poisson count; it does not trace
   individual photons.
3. **The network's 26.4° is a simulation result.** The sim-to-real gap is
   unmeasured, and the domain randomisation only covers the parameters that were
   varied.
4. **The timer part is wrong.** The MAX35101 cannot timestamp four independent
   pulses.
5. **The adhesive's cure is unproven on this geometry.** It needs a trial on scrap
   acrylic.

---

## Se även

- [DESIGN.md](../../crates/canlab-muon/DESIGN.md) — the full design document
- [physics.md](physics.md) — every equation and constant
- [components.md](components.md) — the parts, the adhesive, and multiplexing
- [simulation.md](simulation.md) — how the simulation is built
- [designs.md](designs.md) — the six designs compared
- [constraints.md](constraints.md) — the mission's requirements
