# Muon direction from Cherenkov light — overview and first iteration

*Status: first iteration proposed, nothing simulated yet. Written 2026-11-14.*

## 1. What is being measured, and why it is hard

A muon crosses an acrylic slab. It emits Cherenkov light along its track at a fixed
angle to that track. Photons reach SiPMs; the **differences between their arrival
times** encode the track's direction, because light from a track that leans towards
one SiPM reaches it earlier.

The binding constraint is not timing. It is **photon count**.

Numbers, so the design is argued from them and not from intuition:

| Quantity | Value | Source |
|---|---|---|
| Muon flux, sea level | ~1 cm⁻² min⁻¹ (~0.017 cm⁻² s⁻¹) | can-sat `MUONDETEKTOR.md` |
| Acrylic refractive index | 1.49 | standard |
| Cherenkov angle in acrylic | `acos(1/1.49)` = **47.8°** | derived |
| Muon threshold energy in acrylic | ~157 MeV (`γ > 1.35`) | derived |
| Cherenkov yield | ~490·`sin²θc` ≈ **270 photons/cm** in 400–700 nm | derived |
| SiPM | onsemi **MICROFC-10035**, **1 mm²** | `stumped-potential/komponenter` |
| Readout | OPA858 TIA → MAX40026 comparator → pulse timer, **4 channels, ~10 ps** | same |
| Can | **115 × 66 mm** | can-sat `KOPPLING.md` |

A 20 mm track makes ~540 photons. A 1 mm² SiPM at 25 mm from the track sees a solid
angle of `1/(4π·25²)` ≈ `1.3·10⁻⁴` sr, so it collects **~0.07 photons per muon**.

That is the whole problem in one line: **an unguided 1 mm² SiPM is expected to see
nothing.** Every design decision below follows from it.

## 2. The three ways to get photons, and their costs

1. **Guide the light.** Total internal reflection in the acrylic keeps photons inside
   until they reach a face; a SiPM on that face collects from a much larger solid
   angle. This is nearly free (it is the material doing it) and it is the first thing
   to get right.
2. **More SiPM area.** Cost and jitter both grow with area, and the brief says jitter
   is the thing to keep small. Four 1 mm² SiPMs is 4 mm², which is still 0.16 % of a
   50 × 50 mm face.
3. **More path length.** A thicker slab makes more photons per muon, at the cost of
   mass and of blurring the arrival times (a longer track has a longer emission
   window). The can allows ~20 mm of acrylic without difficulty.

The first iteration below uses all three, modestly, and the simulation is what will
say which one to spend on.

## 3. Direction from four timestamps — this is better than it looks

The brief says two directional axes are hard with four channels. They are not, and
this is worth stating before the design is fixed, because it changes what to
optimise.

A track's direction `d` (unit) and a reference time `t₀` predict each SiPM's arrival
time as

```
t_i = t₀ + (r_i · d) / v_light
```

where `r_i` is the SiPM's position relative to the track's closest approach. That is
**three unknowns** (`t₀`, and `d` with `|d| = 1`), and four SiPMs give **four
equations**. The system is over-determined by one, so a least-squares fit returns a
full 3D direction — both axes — *provided the four SiPMs are not coplanar*. Four
SiPMs in one plane determine the direction only up to a mirror reflection.

So the geometry requirement is not "more channels", it is **non-coplanar SiPM
placement**. Two on the top face and two on the bottom, or four on the side faces at
two different depths, is enough.

What four channels cannot do is reject a bad fit. With one spare equation the
residual is a weak check; a fifth SiPM would make it a real one. That is a trade to
measure, not to assume.

## 4. First iteration — the thing to simulate

**Geometry.** A rectangular acrylic slab, `50 × 50 × 20 mm`, with the muon entering
the top face. Photons are collected at four SiPMs placed for non-coplanarity:

| SiPM | Face | Position | Why |
|---|---|---|---|
| S0 | top | (+20, 0) mm | early for a track leaning +x |
| S1 | top | (−20, 0) mm | the opposite lean |
| S2 | bottom | (0, +20) mm | early for a track leaning +y |
| S3 | bottom | (0, −20) mm | the opposite lean |

Two on top and two on the bottom is what makes the four positions non-coplanar, and
it is the minimum that can resolve a 3D direction.

**Materials.** Acrylic `n = 1.49`, attenuation length 1–3 m (a parameter to vary —
the datasheet value is optimistic), surface polish perfect for the first iteration
and roughened later, because a real cut face scatters.

**SiPM.** 1 mm², photon detection efficiency 25–40 % over 400–700 nm (a parameter to
vary: the MICROFC-10035 curve peaks around 420 nm at ~40 %), dark count rate
~100 kHz/mm² at room temperature, and **single-photon time resolution ~100–300 ps**
— this is the number the brief calls jitter, and it is the one that limits angular
resolution, not the 10 ps TDC.

**Readout.** Four channels, each `t_i` timestamped at 10 ps. The simulation should
model the timestamp as `t_true + jitter + TDC quantisation`, with jitter drawn from a
Gaussian whose width is a varied parameter.

**What the simulation must produce per muon:** how many photons reached each SiPM,
and each SiPM's first-photon arrival time (or "none", which is the common case and
must be modelled as such — a channel with no photon is not a channel with a late
photon).

## 5. The NN, and the concern about simulated-to-real

The brief's concern is right and it is the main risk. A network trained on one
geometry with one set of material properties learns *that* detector, not the
physics, and a real slab will differ in index, attenuation, polish, SiPM gain and
jitter.

**My recommendation is the brief's own first instinct: vary the parameters per
sample (domain randomisation), and do it more aggressively than feels comfortable.**
The reasoning, and why the alternatives are weaker here:

- **Dropout** regularises the *network*, but the distribution shift here is in the
  *input*, not in the weights. A network that has never seen `n = 1.47` will not
  handle it because some units were dropped during training.
- **A smaller model** reduces overfitting to the training distribution but does not
  widen it. It helps, and it is cheap, so do both — but it is not the remedy.
- **Domain randomisation** is the only one of the three that changes the data the
  network sees, which is the thing that is wrong.

The honest caveat: randomisation cannot cover a parameter that was never varied in
the simulation, and it cannot cover a *structural* difference (a real SiPM's
afterpulsing, a scratched face). So the simulation should randomise the parameters we
know we cannot control, and the first iteration should be built to be *calibratable*
— the material properties should be measurable on the real slab with a LED and a
single SiPM, so the simulation's priors can be narrowed before the flight.

## 6. What is deliberately not decided yet

- Whether the readout is 4 SiPMs on 4 channels, or 8 SiPMs paired onto 4 channels.
  Pairing halves the channel count and destroys the per-SiPM timing, so it is only
  worth it if photon count is the binding constraint — which the simulation will say.
- Whether the slab is one piece or a stack of thin layers. A stack gives depth
  information and costs optical interfaces.
- Whether the Cherenkov medium is acrylic at all, versus a higher-index glass with a
  larger `sin²θc` and a shorter attenuation length.

## 7. Questions

1. **Is the 10 ps TDC a fixed part?** If it is the analog pulse timer in
   `komponenter`, its jitter may dominate the SiPM's, and the whole timing budget
   changes. A part number would let the simulation model it instead of guessing.
2. **How many muons per flight are acceptable?** The flux above gives ~0.8 muons/s
   through a 50 cm² face, so a 120 s descent is ~100 muons *crossing* — but with
   photon starvation the number that actually trigger four SiPMs will be far smaller,
   and that number is the one that decides whether the mission is worth flying.
3. **Is the detector's orientation fixed relative to the can?** If the can tumbles,
   the direction is in the can's frame, not the sky's, and the NN's target must be
   defined accordingly.
4. **Is a 5th SiPM affordable?** It would turn the fit's residual from a weak check
   into a real one, and the brief's 200–300 € has room if the TDC is 50 €.
