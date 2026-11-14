---
tags: [cansat, muon, physics, reference]
status: aktuell
updated: 2026-11-14
---

# The physics of the muon detector

*Every equation here is implemented somewhere in `canlab-optics` or
`canlab-muon`, and every constant is pinned by a test in
`crates/canlab-muon/tests/units.rs`. The worked numbers use the design block: a
20 mm tent, 10 mm deep, PMMA at n = 1.49.*

---

## 1. The muon

A cosmic-ray muon reaching sea level has a mean energy of about **4 GeV**, so
`β = v/c = 0.99996` and `γ = 39`. It is the most common charged particle at the
ground, arriving at about **1 per cm² per minute**.

| Quantity | Value |
|---|---|
| Mean energy at sea level | ~4 GeV |
| β | 0.99996 |
| Vertical flux | ~70 m⁻²s⁻¹sr⁻¹ |
| Integrated over the sky | ~127 m⁻²s⁻¹ |
| Angular distribution | `I(θ) ∝ cos²θ` |
| Energy loss in plastic | 1.95 MeV per (g/cm²) |

The `cos²θ` distribution is what makes the coincidence acceptance calculation
possible: the muon is more likely to come from near the vertical, so a two-plane
tracker keeps more crossings than a uniform-in-θ flux would give.

---

## 2. Cherenkov radiation

### 2.1 The condition

A charged particle radiates Cherenkov light when it travels faster than the phase
velocity of light in the medium:

```text
v > c/n     ⇔     β n > 1
```

The threshold for a muon in PMMA (n = 1.491) is `β = 1/1.491 = 0.671`, which
corresponds to a kinetic energy of about **157 MeV**. A 4 GeV muon is far above it,
so the threshold is a completeness check rather than a number that shapes a run.

### 2.2 The cone

The light is emitted along a cone at the Cherenkov angle:

```text
cos θ_c = 1 / (β n)
```

At β ≈ 1 and n = 1.491, `θ_c = 47.8°`. At n = 1.84 it is 57.1°.

The cone is not a single angle because `n` depends on wavelength. A plastic with
`dn/dλ ≈ −0.016/µm` across 400–600 nm spreads the cone by about **0.3°**, which is
5 ps of arrival-time spread over 100 mm of path — the same order as the timing
goal, and therefore not a detail.

### 2.3 The yield

The number of photons per unit path and wavelength is

```text
dN/dx dλ = 2π α z² / λ² · (1 − 1/(β²n²))
```

with `α = 1/137.036`. Integrating over a band `[λ_blue, λ_red]`:

```text
dN/dx = 2π α (1 − 1/(β²n²)) (1/λ_blue − 1/λ_red)
```

**The units matter.** If `λ` is in nanometres and `x` in millimetres, the bracket
carries a factor of `10⁶` (one nanometre is 10⁻⁶ mm). The crate writes it as
`1e6 * (1.0 / BLUE_NM - 1.0 / RED_NM)`.

| Material | n | Photons/mm | Photons/cm |
|---|---|---|---|
| PMMA | 1.491 | **21.0** | 210 |
| High-index plastic | 1.84 | 26.9 | 269 |

The textbook figure of 250–300 photons/cm is for the full visible range; 400–600 nm
is the band the sensors see.

### 2.4 The photon budget, worked

A 1 mm² sensor at 25 mm from the track subtends `1/25² = 1.6 × 10⁻³` sr. The
fraction of the 4π emission it sees is `1.6e-3 / (4π) = 1.27 × 10⁻⁴`. For a 10 mm
path at n = 1.84 that is

```text
269 photons × 1.27e-4 = 0.034 photons per muon
```

and with a 35 % PDE, **0.012 photoelectrons**. That is the number that makes the
Cherenkov design photon-starved: the sensor sees one photon every eighty muons.

A scintillator plane of the same size gives 19 950 photons, of which 25 % reach the
array and 35 % convert — **1 745 photoelectrons**. The factor is 95.

---

## 3. Scintillation

### 3.1 The yield

A plastic scintillator (PVT, such as EJ-200) emits about **10 000 photons per MeV**
of deposited energy. A minimum-ionising muon loses 1.95 MeV per (g/cm²), and PVT's
density is 1.023 g/cm³, so

```text
dE/dx = 1.95 × 1.023 = 1.995 MeV/cm
```

and one centimetre emits `10 000 × 1.995 = 19 950` photons, or **1 995 per
millimetre**.

### 3.2 The inclined path

A muon crossing a plane at zenith angle θ travels `t / cos θ` through it, so an
inclined muon deposits more energy and makes more light:

```text
N(θ) = N(0) / cos θ
```

At 45° that is 41 % more light. Without this the network could not tell a bright
vertical muon from a dim inclined one, and the total charge would be a misleading
feature.

### 3.3 Saturation

A SiPM has a finite number of microcells, and one microcell reports at most one
photoelectron per event. A signal larger than the cell count is clipped.

| Part | Area | Microcells | A 5 mm plane gives | Saturates? |
|---|---|---|---|---|
| MICROFC-10035 | 1 mm² | 504 | 873 pe | **yes** |
| MICROFC-30035 | 3 mm² | 4 774 | 873 pe | no |
| MICROFC-60035 | 6 mm² | 18 980 | 873 pe | no |

Saturation is not automatically bad. A **tracker** reads out *which* pixel fired, so
a clipped pulse is still a hit, and the 1 mm² part is the correct cheap choice. A
**counter** reads the pulse height for energy, so a clipped pulse is a lost
measurement, and the 6 mm part is correct.

---

## 4. Photon transport

### 4.1 The three fates

A photon born inside the block travels in a straight line until it meets a face.
There it either escapes, reflects, or is absorbed.

**Total internal reflection.** Above the critical angle `sin θ_c = 1/n` the photon
cannot refract out and reflects with unit probability. At a PMMA/air face with
n = 1.491, `θ_c = 42.2°`.

The fraction of an isotropic distribution that is *within* the critical angle — and
therefore escapes — is

```text
(1 − cos θ_c) / 2 = (1 − 0.741) / 2 = 0.13
```

So **13 % escapes and 87 % is trapped** at each face.

**Fresnel reflection.** Below the critical angle the photon reflects or transmits
with probabilities from the Fresnel equations, computed for both polarisations.

**Absorption.** Along the path, `exp(−d/L)` with `L` the absorption length. A good
plastic has `L` of metres, so a 10 mm block absorbs almost nothing.

### 4.2 Path versus displacement

The transport reports both, because they are different quantities:

- **Path** is the total distance travelled, including every bounce.
- **Displacement** is the straight-line distance from birth to exit.

The flight time is `Σ dᵢ·nᵢ/c` along the *path*, so a photon that bounced twice
arrives later than one that did not, even though both travelled the same
displacement. A timing measurement sees the path, not the displacement.

### 4.3 Scattering

A machined acrylic face has about **1 µm of roughness**. Against 500 nm light that
is a rough surface, and a photon trapped by total internal reflection meets it at
every bounce. Fresnel reflection assumes a mirror, so a machined block's light yield
is an **upper** bound until a scattering model exists.

In the tracker's light model the scattering is a **kernel**, not a transport: the
expected fraction of light reaching pixel `i` from a hit at `(x, y)` is

```text
w_i = f_b · [pixel contains the hit] + (1 − f_b) · exp(−r_i / λ)
```

normalised over the array, with `λ` the spreading length and `f_b` the ballistic
fraction. The physically right kernel for a thin sheet is a diffusion kernel that
decays as `exp(−r/λ)/√r`; the `1/√r` is a 15 % effect over the 40 mm a plane covers
and is left out rather than approximated badly.

The kernel is the right trade because what the network can learn from is the
*distribution* — its centroid and its width — and a kernel gives both exactly,
where a photon Monte Carlo gives them with noise.

---

## 5. Detection

### 5.1 The three draws

A photon that reaches a sensor's face becomes a photoelectron only if:

1. **Coverage** — it lands on the sensor's fraction of the face. A 6 × 6 mm sensor
   on a 236 mm² face sees **15 %**. This is the loss that dominates everything
   else.
2. **PDE** — it converts. 31 % at Vbr + 2.5 V, 41 % at +5 V, peaking at 420 nm. The
   model uses one mid-band **35 %**.
3. **Jitter** — it arrives at a time smeared by the sensor's SPTR.

### 5.2 The jitter budget

| Part | ps | Random | Source |
|---|---|---|---|
| MICROFC-10035 | 42.5 | yes | **not in the datasheet**; 70–120 ps FWHM published, 100 ps FWHM used |
| MAX35101 | 20.0 | yes | 20 ps accuracy, 3.8 ps resolution |
| OPA858 | 6.0 | yes | 2.5 nV/√Hz over a 0.6 ns edge |
| MAX40026 | 2.0 | yes | 2 ps jitter |
| **Total** | **47.4** | | quadrature |

Three details decide whether the budget means anything:

1. **The largest term has no datasheet figure.** The vendor specifies the fast
   output's rise time and pulse width and no SPTR at all.
2. **FWHM is not a standard deviation.** 100 ps FWHM is 42.5 ps σ; using 100 ps as
   a σ is 2.355 times too pessimistic.
3. **The comparator's 25 ps is a dispersion, not a jitter.** It is the change in
   propagation delay with overdrive, so it is a systematic walk that tracks the
   pulse amplitude — and therefore the photon count. Adding it in quadrature would
   be wrong.

### 5.3 The fast output

The MICROFC-10035 has **100 pF** from anode to cathode and **1 pF** from the fast
terminal to cathode.

An OPA858 transimpedance amplifier behind 100 pF at a 10 kΩ feedback has a
bandwidth of

```text
f = √(GBW / (2π · R_F · C_in)) = √(5.5 GHz / (2π · 10 kΩ · 100 pF)) ≈ 30 MHz
```

which is a **33 ns rise time**. Any timing measurement is destroyed before the
comparator sees it. The fast output — 1 pF, 0.3 ns rise, 0.6 ns FWHM — is the
timing path; the anode is the energy path.

---

## 6. The two observables

### 6.1 Timing

The model is

```text
t_i = t₀ + (r_i · d) / v
```

Three unknowns (`t₀` and two angles of `d`) against four timestamps, so a
least-squares fit recovers the heading. The fit solves for `s = d/v` and normalises,
so the speed is a by-product rather than an input.

**The signal.** The spread of `(r_i · d)/v` across the sensors. Light moves
`c/n = 0.2998/1.491 = 0.2012 mm/ps` in PMMA, so the 54.1 mm between the two
furthest sensors is **269 ps**. The observed spread over the four sensors is
**224 ps**.

**The bound.** The Fisher information for `(t₀, d)` with Gaussian jitter σ is

```text
J = (1/σ²) · AᵀA,     A_i = [1, (u_i · e₁)/v, (u_i · e₂)/v]
```

with `u_i` the centred sensor positions and `e₁, e₂` an orthonormal basis of the
plane perpendicular to the heading. The bound on the total angular variance is
`J⁻¹₁₁ + J⁻¹₂₂`, because a small rotation by `(a, b)` moves the heading by
`√(a² + b²)`.

The result depends on where the sensors are, because the lever arm does:

| Placement | Worst | Best |
|---|---|---|
| four face centres | 81° | 48° |
| four block corners | 56° | 16° |
| a symmetric 54 mm tetrahedron | 17.5° | 17.5° |

The face-centre placement is poor because the two end sensors sit at the centres of
the ±z faces, only 10 mm apart — the block's thickness — so a heading along that
axis has almost no lever arm.

### 6.2 Position

Two hit points a distance `L` apart, each with perpendicular uncertainty `σ_h`,
give an angle

```text
θ = (h₂ − h₁) / L
```

and therefore

```text
σ_θ = √2 · σ_h / L
```

For a pixel of width `p`, a hit is localised to the pixel with uniform uncertainty,
so `σ_h = p/√12`.

| Pixel | Separation | Bound |
|---|---|---|
| 20 mm | 20 mm | 23.4° |
| 20 mm | 40 mm | 11.7° |
| 10 mm | 60 mm | 3.9° |
| 1 mm | 54.1 mm | 1.5° |

### 6.3 The coincidence

A two-plane tracker only measures a direction if the muon crosses **both** planes.
A muon entering the first plane at `(x, y)` with zenith θ and azimuth φ reaches the
second at

```text
(x + d·tanθ·cosφ,  y + d·tanθ·sinφ)
```

which is inside the plane only if that displacement is smaller than the room left on
each side. Averaged over a uniform crossing point and the `cos²θ` flux, the
acceptance is

| Gap | Acceptance | Angle | Directed muons |
|---|---|---|---|
| 20 mm | 0.51 | 23.4° | 10.1 |
| 40 mm | 0.27 | 11.7° | 5.3 |
| 60 mm | 0.15 | 3.9° | 3.1 |

**The gap buys the angle and costs the sample.** A muon at 30° drifts
`40 × tan 30° = 23 mm` sideways over a 40 mm gap, which is most of the plane's
40 mm width.

---

## 7. Worked budgets

### 7.1 The Cherenkov block

| Step | Value |
|---|---|
| Path through the block | 40 mm |
| Photons born | 40 × 21 = 840 |
| Escaping a face after TIR and Fresnel | 11 % trapped at n = 1.49, much of the rest leaves uncovered faces |
| Reaching a sensor's 15 % coverage | ~55 |
| After 35 % PDE | **19 photoelectrons** measured by the transport |

The transport measures 19; a born-times-collection estimate gives 74, because the
model does not know about total internal reflection or the uncovered faces. The
transport is right.

### 7.2 The scintillator plane

| Step | Value |
|---|---|
| Path through a 5 mm plane at 0° | 5 mm |
| Photons born | 5 × 1 995 = 9 975 |
| After 25 % collection | 2 494 |
| After 35 % PDE | **873 photoelectrons** |

At 45° the path is 7.07 mm and the count is 1 234.

---

## 8. The unit audit

The study's first version had the speed of light written as **299.79 mm/ps**,
which is millimetres per *nanosecond*. Light was a thousand times too fast, every
timing signal a thousand times too small, and a direction measurable at tens of
degrees was reported as unmeasurable at eighty thousand.

The tests did not catch it because they asserted the code's own arithmetic:
`signal == 54.1 / 201.2` passes whether 201.2 is millimetres per picosecond or per
nanosecond.

`crates/canlab-muon/tests/units.rs` now states every physical fact **independently**:

| Fact | Independent statement |
|---|---|
| `c` | a metre is 3.34 ns, and a nanosecond is a foot |
| the transport's clock | it computes from `c` in metres per second |
| Cherenkov yield | the textbook formula worked by hand |
| scintillation yield | 10 000 photons/MeV × 2 MeV/cm |
| the can | `π × 33²` |
| the flux | 127 m⁻²s⁻¹ × 36 cm² × 100 s |
| the mass | 30 cm³ × 1.19 g/cm³ |
| the position bound | `√2 × 1 mm / 54.1 mm` |
| the Cramér–Rao bound | a closed form for a symmetric tetrahedron |

The lesson: **a test that uses the same constant on both sides of an assertion
tests nothing about the constant.**

---

## Se även

- [README.md](README.md) — the overview
- [designs.md](designs.md) — the six designs
- [components.md](components.md) — the parts and the adhesive
- [simulation.md](simulation.md) — how the simulation is built
- [constraints.md](constraints.md) — the mission's requirements
