---
tags: [cansat, muon, components, availability]
status: aktuell
updated: 2026-11-14
---

# The parts, their numbers, and what is available

*The four parts from `can-sat`'s `komponenter` list, with the datasheet figures
that decide whether a design works. The jitter budget is
`cargo run --release -p canlab-muon --bin direction_study`.*

---

## The four parts

| Part | Role | Datasheet figure | Verdict |
|---|---|---|---|
| onsemi MICROFC-10035 | SiPM, 1 mm² | PDE 31 % at Vbr+2.5 V; 504 microcells; 30 kHz dark count; 7 % crosstalk; **SPTR not specified** | Right for Cherenkov, saturates on a scintillator |
| TI OPA858 | TIA | 5.5 GHz GBW, 2.5 nV/√Hz, 20.5 mA | Fine, but the anode's 100 pF collapses its bandwidth |
| ADI MAX40026 | comparator | 280 ps delay, **2 ps jitter**, 25 ps overdrive dispersion, LVDS | Good; the dispersion is the term to watch |
| ADI MAX35101 | "analog pulse timer" | **20 ps accuracy**, 3.8 ps resolution, 2 channels | **Wrong part for four SiPMs** |

---

## The MAX35101 is not a general-purpose timer

This is the finding that changed the design, and it is worth stating plainly
because the part is listed as an "analog pulse timer" and reads like a TDC.

- **Its START is internal.** It fires its own pulse launcher and measures
  launch-to-echo. There is no external START pin, so it cannot answer "when did
  this SiPM fire".
- **Its STOP inputs are analog** — 10 to 700 mVpp into a comparator built for
  1 MHz ultrasonic transducers, not for a 0.6 ns SiPM pulse.
- **Its 20 ps is a differential accuracy.** The datasheet says absolute
  measurements "include circuit delays and cannot be considered accurate".
- **It has two channels**, not four.

A general-purpose alternative would be a TDC with an external START and STOP, such
as TI's TDC7200 or ScioSense's TDC-GP22. Neither is in the component list, and
neither is needed: the bound says timing cannot give a direction at all, so
replacing the timer would buy a better measurement of nothing.

---

## The sensor is right for one light source and wrong for the other

| Part | Area | Microcells | Price estimate | Right for |
|---|---|---|---|---|
| MICROFC-10035 | 1 mm² | 504 | 10–25 € | Cherenkov (a handful of photons) |
| MICROFC-30035 | 3 mm² | 4 774 | 20–45 € | a scintillator with a counter's energy readout |
| MICROFC-60035 | 6 mm² | 18 980 | 40–90 € | a counter that needs the pulse height |

A 5 mm scintillator plane gives 873 photoelectrons. The 1 mm² part has 504
microcells, so it **saturates** — and that is not automatically a fault. A tracker
reads out *which* pixel fired, so a clipped pulse is still a hit, and the cheap
part is the correct choice. A counter reads the pulse *height*, so a clipped pulse
is a lost measurement, and the 6 mm part is the correct choice.

The component list cannot carry that distinction: the part is correct for one
design and wrong for another, and the list does not say which.

**Availability note:** all three are onsemi C-series parts in the same 4-side
tileable SMT package, so a board can be laid out for the family and the part chosen
at build time. Prices are estimates from the documented module range scaled by
area, not quotes — `KOSTNADER.md` puts a SiPM module at 10–50 € and says to verify
against the invoice.

---

## The fast output is the timing path, and the anode is not

The MICROFC-10035 has **100 pF** from anode to cathode and **1 pF** from the fast
terminal to cathode.

An OPA858 transimpedance amplifier behind 100 pF at a 10 kΩ feedback has a
bandwidth of

```text
f = sqrt(GBW / (2π · R_F · C_in)) = sqrt(5.5 GHz / (2π · 10 kΩ · 100 pF)) ≈ 30 MHz
```

which is a **33 ns rise time**. Any timing measurement is destroyed before the
comparator sees it. The fast output — 1 pF, 0.3 ns rise, 0.6 ns FWHM — is the
timing path; the anode is the energy path.

A design that needs both needs both taps, and that is a board change rather than a
component change.

---

## The optical adhesive

The block's faces are machined, and a machined acrylic face has about a micrometre
of roughness: light trapped by total internal reflection meets that roughness at
every bounce, and Fresnel reflection assumes a mirror. Optical adhesive fixes that
— it fills the scratches, so the interface becomes optically smooth.

**But that is the smaller half of what it does.** At a PMMA/air face the critical
angle is 42.2°, so only

```text
(1 − cos 42.2°) / 2 = 13 %
```

of the light arriving at that face escapes and **87 % is trapped**. An adhesive
with an index above PMMA's makes the interface go from lower to higher index, so
there is **no critical angle at all**.

| Product | n cured | Δn vs PMMA (1.491) | Plastic adhesion | Price |
|---|---|---|---|---|
| Norland NOA 65 | 1.52 | +0.029 | "Fair" | $38 / 1 oz |
| **Norland NOA 68** | **1.54** | **+0.049** | **"improved adhesion to acrylic"** | **€32.70 / 1 oz** |
| Norland NOA 88 | 1.56 | +0.069 | not stated | — |
| 3M LOCA 2321 | 1.49 | 0.000 | glass-to-glass | bulk only |

All four remove the critical angle and all four are index-matched well enough that
the Fresnel loss is a part in 10 000 — `((1.54 − 1.491)/(1.54 + 1.491))² = 2.6 ×
10⁻⁴`. So the choice is **adhesion to acrylic**, which is what NOA 68's datasheet
says it was formulated for, plus availability in a one-ounce bottle in Europe.

3M's LOCA 2321 is the only perfect index match and it is a display-bonding product
sold by the litre, so it is not a candidate for one detector.

### What it costs

| Item | Cost | Note |
|---|---|---|
| Norland NOA 68, 1 oz | **€32.70** | The smallest purchase; the material used is 0.05 ml |
| UV source, 365–395 nm | to be quoted | Not researched; a 4.5 J/cm² dose is needed |

The material is negligible — a 40 × 39 mm bond at 25 µm is 0.039 ml of a 30 ml
bottle — so the budget carries the *purchase*, not the usage.

### The curing problem, which is the real risk

PMMA's transmission band starts at **380 nm** and the adhesive cures at **350–380
nm** with a peak at 365. So **you cannot cure through the block**: the acrylic
absorbs the light that would set the adhesive behind it.

Three ways round it, in order of preference:

1. **Cure from the bond line's edge.** The bond is at the face's perimeter, and
   the adhesive is exposed there. A 395–405 nm LED penetrates PMMA and the
   adhesive's absorption tail still cures at that wavelength — slower, and the
   dose needs checking with a radiometer.
2. **Bond the sensor to a thin carrier first**, cure that in the open, then bond
   the carrier to the block and cure the second interface from its edge.
3. **Use a two-part epoxy** that cures without light. EPO-TEK 301 is the standard
   choice at `n = 1.5385` and roughly ten times the price.

The first is what the design assumes. It is a process risk, not a physics one, and
it is the thing to try on scrap acrylic before committing.

---

## Reading many SiPMs on few channels

The Cherenkov timing design needs four timestamps from four sensors, and four
channels of picosecond timing is the expensive part. Three techniques put more
sensors on fewer channels, and they are not equivalent: two of them throw the
timing away.

| Technique | Ratio | Timing kept? | Measured cost |
|---|---|---|---|
| **Series connection** | n → 1 | No — the sum, earliest photon wins | Capacitance ÷ n, gain ÷ n, bias × n |
| **Delay-line multiplexing** | 16 → 1 | **Yes**, +30 ps FWHM | Splitters, combiners, cable lengths |
| **Resistive charge division** | 16 → 4 | No — position only | Resistor ladder, 300 Ω–10 kΩ |

### Series connection

`n` SiPMs in series reduce the total capacitance by `n` and the gain by `n`, and
need `n` times the bias. The MEG II pTC uses **six series-connected SiPMs per
side** of a 120 × 40 × 5 mm scintillator tile, sees 50–100 photoelectrons per
side, and reaches **70–90 ps** timing resolution with a two-sided readout
([arXiv:2005.05027](https://arxiv.org/abs/2005.05027)).

The series connection is what a **counter** wants: it sums the light and gives one
timestamp per side. It cannot give four timestamps, because the four sensors' pulses
are one pulse.

### Delay-line multiplexing

Each SiPM goes to a preamplifier, then a 1:2 splitter. One branch goes to a combiner
directly; the other goes through a **different cable length per channel** to a second
combiner. The *time difference between the two outputs* identifies which SiPM fired,
and the pulse's own arrival time is preserved.

Vinke et al. multiplex **16 SiPMs into one channel** this way and measure the
coincidence timing resolution degrading from **243 to 272 ps FWHM** — about 30 ps,
or 13 ps in quadrature ([PMC4386692](https://pmc.ncbi.nlm.nih.gov/articles/PMC4386692/)).
The delay is 2 ns per channel, which is three pulse widths: the pulses do not
overlap.

This is the technique that matters here. Four SiPMs on **one** delay-line channel
give four timestamps from one digitizer, at the cost of four comparators and a
length of PCB trace.

### Resistive charge division

A resistor ladder combines 16 SiPMs into 4 outputs, and the position comes from the
normalised charge ratios. A recent characterisation of a 16→4 network finds the
position recoverable in an intermediate amplitude range and **not** the timing: the
authors say the timing would be taken "through a separate parallel signal path"
([MDPI Sensors](https://www.mdpi.com/1424-8220/26/20/6531)).

So charge division is a **tracker** technique, not a timing one. For the tracker's
4 × 4 pixel array it would cut 16 channels to 4 and the cost by a factor of four.

### What it costs

| Item | Four separate channels | One delay-line channel |
|---|---|---|
| Comparators | 4 × MAX40026 ≈ 20 € | 4 × MAX40026 ≈ 20 € |
| Timing channels | 4 TDC channels | **1** multi-hit TDC channel |
| Delay lines | — | PCB traces, free |
| Splitters/combiners | — | ~10 € (or PCB couplers) |
| **Added timing jitter** | — | **~13 ps σ**, in quadrature |

The splitter/combiner parts are the same class as the ones in the published setup
(Mini-Circuits SBTC-2-10+ and JEPS-16-1W); a PCB coupler would be cheaper and is
what a CanSat board should use.

### Why it does not rescue the timing design on its own

The simulation found that four timestamps give a **120°** error with the linear
model and **81°** with the correct Cherenkov model *when the track position is
known*. The gap is not the channel count: it is that the correct arrival time
depends on **where along the track the photon was emitted**, and four timestamps
cannot determine both the direction and the track position.

More timestamps would. The correct model has five unknowns — two angles, the start
time, and the track's two transverse coordinates — so **six or more timestamps**
make it identifiable, and the bound of 81° then applies instead of the linear
model's 120°.

That is where multiplexing earns its place: **eight SiPMs on two delay-line
channels** give eight timestamps for the price of two, and the exact model becomes
fittable. Whether 81° is worth having is a separate question — the face-centre
placement is poor, and the block's corners give 16°.

---

## The jitter budget

| Part | ps sigma | Random | Source |
|---|---|---|---|
| MICROFC-10035 | 42.5 | yes | **not in the datasheet**; 70–120 ps FWHM is the published range, 100 ps FWHM is the middle |
| MAX35101 | 20.0 | yes | datasheet: 20 ps accuracy, 3.8 ps resolution |
| OPA858 | 6.0 | yes | 2.5 nV/√Hz over a 0.6 ns edge at a 10 mV pulse |
| MAX40026 | 2.0 | yes | datasheet: 2 ps jitter, 150 ps edge, 100 mV overdrive |
| **total** | **47.4** | | quadrature of the random terms |

Three details decide whether the budget means anything:

1. **The largest term is the one with no datasheet figure.** The vendor specifies
   the fast output's rise time and pulse width and no single-photon time
   resolution at all. Every other term is smaller than the uncertainty on this one,
   so the budget cannot be closed without measuring the part.
2. **FWHM is not a standard deviation.** The transport's `jitter_ps` is a standard
   deviation, and SPTR is quoted as a FWHM. 100 ps FWHM is 42.5 ps σ, and using
   100 ps as a σ is 2.355 times too pessimistic.
3. **The comparator's 25 ps is a dispersion, not a jitter.** It is the change in
   propagation delay with overdrive, so it is a systematic walk that tracks the
   pulse amplitude — and therefore the photon count. Adding it in quadrature would
   be wrong; it belongs in the model as an amplitude-dependent bias.

---

## What the simulation does not carry

| Not modelled | The number | Why it matters |
|---|---|---|
| A PDE curve | 31 % at Vbr+2.5 V, 41 % at +5 V, peak at 420 nm | The model uses one mid-band 35 % |
| SPTR | not specified | The largest term in the budget |
| Crosstalk | 7 % at 35 µm cells | One photon can produce two timestamps |
| Dark count | 30 kHz typ, 96 kHz max | A 100 ns window is not always empty |
| Vbr drift | 21.5 mV/°C, gain −0.8 %/°C | A descending can changes its own PDE and SPTR in flight |
| Afterpulsing | 0.2 % | Late timestamps at the per-mille level |

The fifth is the one a mission has to answer: a bias-tracking loop, a temperature
model, or an acceptance that the count drifts during the flight.

---

## What is genuinely missing from the component list

- **A timer that can take an external START.** The MAX35101 cannot.
- **A sensor with a specified SPTR.** None of the C-series parts has one.
- **A temperature sensor at the SiPM.** Vbr drifts 21.5 mV/°C, and the can flies
  through 0–10 °C, so the bias needs to track the temperature or the gain drifts.
- **A light-tight enclosure.** `MUONDETEKTOR.md` says black tape and a housing; a
  SiPM in daylight is a saturated photodiode.

---

## Se även

- [README.md](README.md) — the overview
- [designs.md](designs.md) — which part each design needs
- [simulation.md](simulation.md) — how the transport uses these numbers
- [constraints.md](constraints.md) — the budget the parts have to fit
