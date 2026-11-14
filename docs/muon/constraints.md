---
tags: [cansat, muon, constraints, requirements]
status: aktuell
updated: 2026-11-14
---

# The mission's parameters and requirements

*One file's worth of numbers, from `can-sat/dokument/CANSAT-GRUND.md`,
`KOSTNADER.md` and `MUONDETEKTOR.md`. The code that enforces them is
`canlab_muon::mission`.*

---

## The can

| Constraint | Value | Source |
|---|---|---|
| Volume | 115 mm high, Ø66 mm | `CANSAT-GRUND.md` §3 |
| Cross-section | 34.2 cm², 24.6 cm² after 5 mm of allowance | computed |
| Mass | 300–350 g total, including the battery | `CANSAT-GRUND.md` §3 |
| Budget | ≤ 500 € | `CANSAT-GRUND.md` §3 |
| Detector's share | 200–300 € | the team's allocation |
| Optical adhesive | 32.70 € | Norland NOA 68, 1 oz, AMS Technologies |
| Acceleration | 20 G required, ~30 G measured | `CANSAT-GRUND.md` §3 |
| Parachute mount | ≥ 50 N | `CANSAT-GRUND.md` §3 |
| Power | 2 h on one charge | `CANSAT-GRUND.md` §3 |
| Lid space | 4.5 cm axial above the can | `CANSAT-GRUND.md` §3 |

The mass matters twice: the whole can is 300–350 g, and the block's share of that
is capped at 15 % — 52.5 g — because the battery, the structure and the radio all
need room. A 20 mm tent is 35.7 g and fits; a 25 mm tent is 55.8 g and does not.

---

## The flight

| Quantity | Value |
|---|---|
| Apogee | 750–1100 m |
| Descent speed | 5–12 m/s, recommended 8–11 |
| Data-taking time | 68–137 s |
| Temperature | 0–10 °C, March in Vasa |
| Competition | Vasa, 11–14 March 2027 |
| Pre-Launch Report | 20 February 2027, max 15 pages, English |

The measurement time is the flight profile's own arithmetic, not a separate
figure: 750 m at 11 m/s is 68 s and 1100 m at 8 m/s is 137 s. A design that needs
minutes of data has to say so.

---

## The muon flux

| Quantity | Value |
|---|---|
| Vertical flux at sea level | ~70 m⁻²s⁻¹sr⁻¹ |
| Integrated over the sky | ~127 m⁻²s⁻¹ |
| At 1 km | 1.25–1.40 times sea level |
| Mean energy at sea level | ~4 GeV |

The integrated flux is the one a counter's rate is compared with: 127 m⁻²s⁻¹ is
0.76 muons per cm² per minute, or about one per cm² per minute. A 36 cm² plane
therefore sees **~46 muons in a 100 s flight**, which is a real sample and a small
one.

---

## The shop

The team has a **CNC machine**, and that changes the geometry space more than any
other line in this file.

| Capability | Value | Consequence |
|---|---|---|
| CNC available | yes | The outline is any closed 2.5D shape, not just a tent |
| Minimum tool radius | 1.5 mm | An inside corner cannot be sharper than the cutter |
| Maximum depth | 60 mm | A deeper cut needs a second setup, which re-datums the block |
| Surface roughness | ~1 µm Ra | **A machined face scatters** |
| Polishing available | yes | Turns a scattering face back into a TIR face |

The roughness is the physics, not the machining. Fresnel reflection assumes a
mirror, and a photon trapped by total internal reflection meets the same face at
every bounce. A 1 µm scratch against 500 nm light is a rough surface, so a
machined block's light yield is an **upper** bound until a scattering model exists.

The tent was chosen because it is what you can cut and still keep a
total-internal-reflection cavity with a flat face for the sensor. With a CNC that
is a *choice*, and the study should sweep outlines rather than only scales.

---

## The requirements, as the audit checks them

`Mission::audit` takes a candidate and returns every requirement it breaks, with
the measured number and the limit. A requirement that is only written down is a
requirement nobody enforces.

| Requirement | Limit | Checked against |
|---|---|---|
| The block fits inside the can | 114 mm tall, 32 mm radius | `Can::admits` |
| The block is at most 15 % of the can's mass | 52.5 g | `tent_mass_g` |
| The detector costs at most its budget | 300 € | `Candidate::cost_eur` |
| The block is within the machine's depth | 60 mm | `Manufacturing::max_depth_mm` |
| The timing jitter is within 10× of the timing signal | ratio ≤ 10 | `timing_signal_ps` |

The design point **passes** the timing requirement: the signal is 224 ps against
47.4 ps of jitter, a ratio of 0.21. It failed at a ratio of 212 in the first
version of this study, because the speed of light was a thousand times too large —
which is why the requirement is now checked against a constant that
`tests/units.rs` pins independently.

The design comparison adds two more, because a design that cannot measure a
direction is not a direction detector:

| Requirement | Limit | Note |
|---|---|---|
| The design measures a direction | angle ≤ 30° | A design with no direction fails |
| The design costs at most the detector's budget | 300 € | The fine tracker fails at 923 € |

---

## What the audit says about each design

| Design | Result |
|---|---|
| counter | fails: no direction |
| cherenkov timing | fails: 81.2° (placement, not clock) |
| coarse tracker | **passes** |
| fine tracker | fails: 923 € against 300 € |
| ring imager | fails: 523 € against 300 € |

One design passes, and it is the recommendation.

---

## Se även

- [README.md](README.md) — the overview
- [designs.md](designs.md) — the five designs against these requirements
- [components.md](components.md) — the parts the budget buys
- [CANSAT-GRUND.md](../../../can-sat/dokument/CANSAT-GRUND.md) — the rules in full
