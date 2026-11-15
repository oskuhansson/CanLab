---
tags: [cansat, muon, vmf, probability, calibration]
status: aktuell
updated: 2026-11-15
---

# The network as a distribution

*The direction head returns a distribution on the sphere, not a point: a mean
direction and a concentration κ. This is why, what κ means, and the measurement
that says whether it can be believed.*

Numbers from
`cargo run --release -p canlab-muon --bin train_realistic -- --hexagon --scale 28 --thickness 30 --vmf`.

---

## Why a point estimate is not enough

The regressor returns two numbers and no uncertainty. A median error of 28.4°
says how wrong it is *on average*; it says nothing about *this* event. Two
events with the same reconstruction can be 5° and 60° wrong, and a trigger that
has to decide whether to trust a reconstruction needs the second number.

## The distribution

A direction is a point on the unit sphere, so the natural distribution is the
sphere's Gaussian — the **von Mises–Fisher**:

```text
f(x) = κ / (4π sinh κ) · exp(κ v · x)
```

with `v` the mean direction and `κ` the concentration. `κ = 0` is uniform over
the sphere; large `κ` is a tight cluster. Its negative log-likelihood is

```text
NLL = −ln(κ / (4π sinh κ)) − κ (y · ŷ)
```

and the homoscedastic case — one `κ` for every event — reduces to the cosine
distance, which is what a plain direction regressor minimises. So it is a strict
generalisation: it learns the same direction and *also* a per-event width.

A Gaussian on the three components would be the wrong shape: its support runs off
the sphere, so its likelihood is not a density over directions and its
uncertainty is miscalibrated near the poles. A paper that compares the two on
exactly this problem — 3D direction prediction for a particle detector — finds
the Gaussian version less well calibrated
([arXiv:2403.15949](https://arxiv.org/html/2403.15949)).

## The numerical trap

`ln(κ / (4π sinh κ))` overflows for `κ ≳ 88` in `f32`, and `sinh` overflows
earlier in `f64`. The limit is `ln(κ/2π) − κ`, which agrees with the exact form to
32-bit precision for `κ ≥ 9`; below that a Taylor series about zero is used,
switching at `κ = 2.65`, where the two agree to 0.14 %.

The first version of this function had the **wrong sign**: the series
`ln(4π) + κ²/6 − …` is for `ln(4π sinh κ / κ)`, the reciprocal of what the
likelihood needs. The test that compares it with the exact form caught it at
`κ = 0.1`, where the two differed by 5.07 — twice the value.

## The measurement

Trained on the same 8 000 realistic events as the regressor and tested on the
same 1 500, for the hexagon at 30 mm with the 6 × 6 mm part behind the taper:

| Configuration | Median | 68th | within 30° | within 60° |
|---|---|---|---|---|
| vMF mean direction | 26.0° | 35.0° | 59.9 % | 90.3 % |
| regressor, nominal readout | 26.4° | 36.4° | 56.8 % | 90.5 % |

The distribution costs **nothing** in the mean direction: the two heads land
within half a degree and three points, which is the noise. The `κ` is one extra
output unit, not another network.

Mean `κ` is 8.6, which implies a mean angular error of 28.2° — the 26.0° it
actually makes is slightly better than its own prediction.

### Is the κ believable?

A calibrated network's own `κ` predicts the error it then makes. The test bins the
held-out events by predicted error and compares the median of each bin with the
prediction:

| bin (by predicted) | predicted | observed | ratio |
|---|---|---|---|
| 0–25 % | 20.2° | 19.0° | 0.94 |
| 25–50 % | 22.1° | 21.2° | 0.96 |
| 50–75 % | 30.2° | 31.0° | 1.03 |
| 75–100 % | 40.4° | 42.6° | 1.06 |

The ordering works: the events the network calls hard are harder, and the ratio
is within 6 % of 1 in every bin. The *absolute* probability at the mission's
criterion is close as well:

**P(θ ≤ 30°): 62.7 % predicted by the network's own κ, 59.9 % observed.**

The vMF tail is slightly light — the 30° probability is optimistic by about three
points — but the bin ratios show the shape is right.

## What it buys, and what it does not

- **Buys:** a per-event confidence that ranks events correctly, and a mean
  direction identical to the regressor's within the noise. At inference it is one
  more output unit.
- **Does not buy:** an absolute probability you can put in a trigger. The 30°
  number is 62.7 % against an observed 59.9 %.
- **The fix is cheap:** a single scale on `κ` fitted on held-out data
  (temperature scaling) moves the predicted 30° fraction to the observed one
  without touching the mean direction. A heavier-tailed distribution on the
  sphere would fix the shape rather than the scale; neither is done yet.

## What it does not change

The mission's criterion is the fraction within 30°, and that is a property of the
**timing resolution**, not of the output head. A calibrated distribution tells
you when to trust a reconstruction; it does not make a 26.4° detector a 20°
detector. The design still fails the criterion at 59.9 % within 30°, and
[cherenkov.md](cherenkov.md) says why.

## Reproducing it

```bash
cargo run --release -p canlab-muon --bin train_realistic -- --hexagon --scale 28 --thickness 30 --vmf
```

## Se även

- [overview.md](overview.md) — the design on one page
- [cherenkov.md](cherenkov.md) — the Cherenkov design in full
- `crates/canlab-muon/src/vmf.rs` — the implementation and its tests
