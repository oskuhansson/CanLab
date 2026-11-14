---
tags: [cansat, muon, simulation, reference]
status: aktuell
updated: 2026-11-14
---

# The simulation, in detail

*What `canlab-optics` and `canlab-muon` compute, in the order they compute it,
with the number each step produces and the ones it does not.*

---

## The two crates

The physics is one crate and the design questions are another. The split is not
tidiness: the physics has a validation suite against closed forms and a fixture
that a second implementation is checked against, and the design questions change
every time a part changes. Mixing them would make every design iteration a change
to the validated code.

```mermaid
flowchart TB
    subgraph PHYS["canlab-optics: the physics"]
        OUTLINE["outline: tent polygon, distance"]
        BLOCK["block: exit, face, sensors"]
        CHER["cherenkov: the cone and dispersion"]
        FRES["fresnel: reflectance, both modes"]
        VEC["vec3: reflection, refraction"]
        TRANSPORT["transport: one photon, birth to death"]
        SOURCE["source: how many, what colour"]
        DETECTOR["detector: one muon, four timestamps"]
        TIMEBASE["timebase: 10 ps bins, histogram"]
    end
    subgraph STUDY["canlab-muon: the design questions"]
        CAN["can: does it fit 66 x 115 mm"]
        MISSION["mission: parameters and an audit"]
        COMPONENTS["components: four parts, jitter"]
        SCINT["scintillator: yield and saturation"]
        TIMING["timing: least squares heading"]
        BOUND["bound: Cramer-Rao, position"]
        DESIGNS["designs: five approaches compared"]
        SOLVE["solve: 3x3, unit-free singularity"]
    end
    subgraph OUT["Binaries"]
        MUONSTUDY(["muon_study: sweep and waveform"])
        DIRSTUDY(["direction_study: timing vs position"])
        DESIGNSTUDY(["design_study: five designs, one table"])
    end
    OUTLINE --> BLOCK
    VEC --> BLOCK
    BLOCK --> TRANSPORT
    FRES --> TRANSPORT
    CHER --> SOURCE
    SOURCE --> DETECTOR
    TRANSPORT --> DETECTOR
    DETECTOR --> TIMEBASE
    DETECTOR --> MUONSTUDY
    TIMEBASE --> MUONSTUDY
    BLOCK --> CAN
    CAN --> MISSION
    COMPONENTS --> MISSION
    CAN --> DESIGNS
    SCINT --> DESIGNS
    BOUND --> DESIGNS
    COMPONENTS --> DESIGNS
    MISSION --> DESIGNS
    TIMING --> BOUND
    SOLVE --- TIMING
    SOLVE --- BOUND
    TIMING --> DIRSTUDY
    BOUND --> DIRSTUDY
    DESIGNS --> DESIGNSTUDY
```

---

## One event, step by step

`detector::simulate` is the whole of one muon. It takes an entry point, a heading,
a path length and a random stream, and returns four sorted lists of arrival times.

```mermaid
sequenceDiagram
    autonumber
    participant MUON as Muon
    participant SIM as detector::simulate
    participant SRC as source
    participant TR as transport
    participant SIPM as SiPM
    participant EV as Event
    MUON->>SIM: entry, heading, path
    SIM->>SIM: clip the path to the block
    SIM->>SRC: expected photons for the path
    SRC->>SIM: mean count, Poisson draw
    Note over SIM: one loop per photon, in path order
    SIM->>SRC: sample wavelength and azimuth
    SRC->>SIM: theta from the cone at that wavelength
    SIM->>TR: birth point, direction, polarisation
    TR->>TR: step to the next face
    TR->>TR: absorb by exp(-d/L)
    TR->>TR: Fresnel: reflect or escape
    TR->>SIM: fate, face, time of flight
    SIM->>SIPM: coverage draw, then PDE draw
    SIPM->>EV: true time, plus jitter
    Note over EV: every photon has exactly one fate
```

### 1. The path is clipped to the block

A caller hands a path that usually starts outside the block and ends outside it.
Only the part inside the material makes light, so the path is clipped to the
block's intersection with the line. A block is convex, so that intersection is one
stretch and the clip is exact rather than a search.

*The bug this prevents:* walking the path forwards to find its end is only right
when the start is outside. A muon that entered the block and left it exited at the
first sample, which is at once, and every event reported zero photons.

### 2. The photon count is a Poisson draw

`source::expected_photons` gives the mean from the Cherenkov law,
`dN/dx = 2π α (1 − 1/(β²n²)) ∫dλ/λ²`, which for β ≈ 1 and n = 1.84 over 400–600 nm
is 26.9 photons per millimetre. The count is drawn from a Poisson with that mean,
then clamped to the smallest microcell count among the four sensors.

A scintillator does not go through this path: it is not a Cherenkov radiator, and
the transport's source is the Cherenkov law. `canlab_muon::scintillator` carries
the scintillation yield as a separate model, and no event simulation uses it yet.

### 3. Each photon is born on the cone

The wavelength is drawn from the `1/λ²` spectrum by inverting its CDF — one
divide, no rejection. The cone angle comes from `cos θ = 1/(βn(λ))` at *that*
wavelength, so the cone's width is the material's dispersion rather than a
constant. The azimuth is uniform.

*Why the wavelength is not uniform:* `n(λ)` falls from blue to red, so a uniform
draw would put too many red photons in and get the dispersion — and therefore the
timing — slightly wrong.

### 4. The photon is traced

`transport::trace` steps to the next face, absorbs by `exp(−d/L)`, and at the face
either reflects or escapes by a Fresnel draw. A photon that reflects is nudged
0.01 mm back inside along the face normal, and the next exit is asked for with the
face it just left excluded.

*The two bugs this prevents:* without the face filter a photon at normal incidence
bounces on one square nanometre of the same face until its budget runs out; and a
nudge along the *new heading* at normal incidence points straight back out, so the
photon lands outside the block it is inside.

The flight time is `Σ dᵢ·nᵢ/c` in the block and `d/c` in air. The path and the
displacement are both reported, because a photon that bounced twice arrives along a
straight line having travelled further, and the difference is the difference
between `t = L·n/c` and the answer a timing measurement gives.

### 5. The sensor decides

Three draws, in order: does the photon land on the sensor's fraction of the face
(15 % for a 6 × 6 mm sensor on a 236 mm² face), then does it convert (PDE 35 %),
then what is its jitter (a Box–Muller normal with σ = the sensor's `jitter_ps`).

The coverage draw is the one that dominates. Leaving it out is how the first
version of the study reported tens of photons per sensor where the geometry allows
a handful.

### 6. Every photon has exactly one fate

`lost + trapped + absorbed + missed + detected == photons_born`. A photon that is
unaccounted for is a bug in a test, not a rounding difference, and the identity is
asserted rather than trusted.

---

## The numbers one event produces

For the design block — a 20 mm tent, 10 mm deep, PMMA, four sensors, a muon across
the walls — averaged over 200 events:

| Quantity | Value |
|---|---|
| Cherenkov photons born | ~210 |
| Reaching a sensor after Fresnel, absorption and coverage | ~8 |
| Sensors with at least one photon | 2.95 of 4 |
| Photons lost to total internal reflection | ~11 % |
| Arrival-time spread within one sensor | ~18 ps |
| Muons through the plane in a 100 s flight | ~41–46 |

The last two are the study's problem. The spread within one sensor is 18 ps, which
is the *geometry's* own blur and is larger than the timer's 20 ps; and the timing
difference *between* sensors is 0.22 ps, which is 80 times smaller than the blur.
A timestamp cannot resolve a 0.22 ps difference when the thing it is timing is
smeared over 18 ps.

---

## What is not modelled

Each of these would move a number, and each is listed in
`canlab_muon::components::UNMODELLED` so a test can check the list is not a wish.

| Not modelled | Why it matters |
|---|---|
| A PDE curve | The model uses one mid-band 35 %; the part is 31 % at Vbr+2.5 V and 41 % at +5 V, and the curve peaks at 420 nm |
| SPTR | The vendor does not specify it, and the model's 100 ps is a placeholder |
| Crosstalk (7 %) | One photon can produce two timestamps |
| Dark counts (30–96 kHz) | A 100 ns window is not always empty |
| Vbr drift (21.5 mV/°C) | A can descending through 10 °C changes its own PDE and SPTR in flight |
| Afterpulsing (0.2 %) | Late timestamps at the per-mille level |
| Surface scattering | A machined face has ~1 µm of roughness and Fresnel assumes a mirror |
| Time walk | The comparator's 25 ps of overdrive dispersion tracks the pulse amplitude |
| Scintillation in the transport | The yield is a separate model; no event simulation uses it yet |

The omissions do not all point the same way, which is why they are listed rather
than corrected for: gain and pulse height do not change an arrival time,
afterpulsing and crosstalk would add late or extra timestamps, saturation caps the
count, and dark counts would add photons the model does not have.

---

## How the simulation is trusted

The physics crate has a validation suite that checks the *composition* against
numbers from outside it, not against itself:

- a million sampled Fresnel splits conserve energy;
- a bounced photon's path is longer than its chord while its time is the chord's;
- transmission brackets `e^{−d/L}`;
- the cone's blue and red ends arrive picoseconds apart;
- every photon of a crossing is accounted for exactly once;
- `can-sat/sim/muon/events.json` is a fixture generated by the same optics, so a
  second implementation cannot drift from it.

Seven bugs were found by writing that suite, and each is recorded in the test that
would have caught it. The lesson is in `docs/lessons.md`: a table that tells a
convincing story is not evidence, and every one of those bugs was found by asking
for a *number of photons* rather than by reading the table.

---

## Reproducing it

```bash
cargo run --release -p canlab-optics --bin muon_study
cargo run --release -p canlab-optics --bin muon_study -- --waveform
cargo run --release -p canlab-muon --bin direction_study
cargo run --release -p canlab-muon --bin design_study
cargo test -p canlab-optics -p canlab-muon
```

Every binary is seeded, so a rerun prints the same table.

---

## Se även

- [README.md](README.md) — the overview and the headline
- [designs.md](designs.md) — the five designs
- [components.md](components.md) — the parts and the jitter budget
- [constraints.md](constraints.md) — the mission's parameters
