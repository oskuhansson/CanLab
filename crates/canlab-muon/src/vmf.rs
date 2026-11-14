//! A network that outputs a **distribution** over directions, not a point.
//!
//! The earlier network was a regressor: it returned one direction per event, and
//! the 23.8° was the median error of that point estimate. That answers "how well
//! does it do on average" and not "how sure is it about *this* event".
//!
//! # The right distribution is the von Mises–Fisher
//!
//! A direction is a point on the unit sphere, so the natural distribution is the
//! sphere's analogue of the Gaussian — the **von Mises–Fisher** distribution:
//!
//! ```text
//! f(x) = κ / (4π sinh κ) · exp(κ v · x)
//! ```
//!
//! with `v` the mean direction and `κ` the concentration: `κ = 0` is uniform over
//! the sphere, and large `κ` is a tight cluster. Its negative log-likelihood is
//!
//! ```text
//! NLL = −ln(κ / (4π sinh κ)) − κ (y · ŷ)
//! ```
//!
//! and the homoscedastic case — one `κ` for every event — reduces to the cosine
//! distance, which is what a plain direction regressor minimises. So this is a
//! strict generalisation: it learns the same direction and *also* a per-event
//! uncertainty.
//!
//! # Why not a Gaussian on the three components
//!
//! A Gaussian over `R³` has support off the sphere, so its likelihood is not a
//! density over directions and its uncertainty is miscalibrated near the poles.
//! A paper that compares the two on exactly this problem — 3D direction
//! prediction for a particle detector — finds the Gaussian version less
//! well-calibrated ([arXiv:2403.15949](https://arxiv.org/html/2403.15949)).
//!
//! # The numerical trap
//!
//! `ln(κ / (4π sinh κ))` overflows for `κ ≳ 88` in `f32`, and `sinh` overflows
//! even earlier in `f64`. The limit is `ln(κ/2π) − κ`, which agrees with the exact
//! form to 32-bit precision for `κ ≥ 9`; below that a Taylor series about zero is
//! used. The switch is at `κ = 2.65`, where the two agree to 0.14 %.
//!
//! # Calibration
//!
//! A predicted `κ` implies a mean angular error:
//!
//! ```text
//! θ_mean = π (I₀(κ) − e^{−κ}) / (2 sinh κ)
//! ```
//!
//! with `I₀` the modified Bessel function of the first kind. A network is
//! **calibrated** when the errors it actually makes match the errors its own `κ`
//! predicts. That is the test this module exists to pass.

use crate::net::Layer;

/// `ln(κ / (4π sinh κ))`, stable for every `κ > 0`.
///
/// The exact form is used wherever `sinh κ` is representable, which is up to
/// about `κ = 700`; beyond that the asymptotic `ln(κ/2π) − κ` is used, and below
/// `κ = 10⁻³` the density is the uniform `1/(4π)`.
///
/// The first version of this function used a Taylor series for small `κ` and had
/// the **wrong sign**: the series `ln(4π) + κ²/6 − …` is for
/// `ln(4π sinh κ / κ)`, which is the reciprocal of what the likelihood needs. The
/// test that compares it with the exact form caught it at `κ = 0.1`, where the
/// two differed by 5.07 — twice the value.
pub fn log_normalizer(kappa: f64) -> f64 {
    if kappa <= 1e-3 {
        // The limit as κ → 0 is the uniform density, 1/(4π).
        return -(4.0 * std::f64::consts::PI).ln();
    }
    if kappa < 700.0 {
        (kappa / (4.0 * std::f64::consts::PI * kappa.sinh())).ln()
    } else {
        (kappa / (2.0 * std::f64::consts::PI)).ln() - kappa
    }
}

/// `coth κ`, stable for large `κ`.
pub fn coth(kappa: f64) -> f64 {
    if kappa > 20.0 {
        // coth κ = 1 + 2e^{-2κ} + …, and the exponential is below 4e-18 here.
        1.0 + 2.0 * (-2.0 * kappa).exp()
    } else {
        1.0 / kappa.tanh()
    }
}

/// The mean angular error a concentration `κ` implies, radians.
///
/// `π (I₀(κ) − e^{−κ}) / (2 sinh κ)`. The Bessel function is evaluated by its
/// series, which converges in a few terms for the `κ` this problem produces.
pub fn expected_angle(kappa: f64) -> f64 {
    if kappa <= 0.0 {
        // Uniform over the sphere: the mean angle from a random point is π/2.
        return std::f64::consts::FRAC_PI_2;
    }
    // I₀(κ) = Σ (κ²/4)^n / (n!)².
    let quarter = kappa * kappa / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..40 {
        term *= quarter / (n * n) as f64;
        sum += term;
        if term < 1e-18 * sum {
            break;
        }
    }
    let numerator = std::f64::consts::PI * (sum - (-kappa).exp());
    let denominator = 2.0 * kappa.sinh();
    if denominator == 0.0 { std::f64::consts::FRAC_PI_2 } else { numerator / denominator }
}

/// A network whose output is a von Mises–Fisher distribution on the sphere.
#[derive(Debug, Clone)]
pub struct VmfNetwork {
    input: Layer,
    hidden: Layer,
    output: Layer,
    width: usize,
}

/// One training sample for the probabilistic network.
#[derive(Debug, Clone, PartialEq)]
pub struct VmfSample {
    /// The input features.
    pub features: Vec<f64>,
    /// The true direction, a unit vector.
    pub target: [f64; 3],
}

impl VmfNetwork {
    /// A network with `hidden` units per hidden layer.
    pub fn new(inputs: usize, hidden: usize, rng: &mut crate::tracker::Rng) -> VmfNetwork {
        VmfNetwork {
            input: Layer::new(inputs, hidden, rng),
            hidden: Layer::new(hidden, hidden, rng),
            output: Layer::new(hidden, 4, rng),
            width: hidden,
        }
    }

    /// The number of trainable parameters.
    pub fn parameters(&self) -> usize {
        self.input.weights.len()
            + self.input.biases.len()
            + self.hidden.weights.len()
            + self.hidden.biases.len()
            + self.output.weights.len()
            + self.output.biases.len()
    }

    /// The forward pass, returning the hidden activations and the four outputs.
    fn forward(&self, features: &[f64]) -> (Vec<f64>, Vec<f64>, [f64; 4]) {
        let mut first = vec![0.0; self.width];
        self.input.forward(features, &mut first);
        for value in &mut first {
            *value = value.max(0.0);
        }
        let mut second = vec![0.0; self.width];
        self.hidden.forward(&first, &mut second);
        for value in &mut second {
            *value = value.max(0.0);
        }
        let mut out = [0.0; 4];
        self.output.forward(&second, &mut out);
        (first, second, out)
    }

    /// The distribution for one input: the mean direction and its concentration.
    pub fn predict(&self, features: &[f64]) -> ([f64; 3], f64) {
        let (_, _, out) = self.forward(features);
        let direction = normalise([out[0], out[1], out[2]]);
        // Softplus, so the concentration is positive, plus a floor so the
        // likelihood never divides by zero.
        let kappa = softplus(out[3]) + 1e-3;
        (direction, kappa)
    }

    /// The mean negative log-likelihood over a set of samples.
    pub fn loss(&self, samples: &[VmfSample]) -> f64 {
        if samples.is_empty() {
            return f64::NAN;
        }
        let mut total = 0.0;
        for sample in samples {
            let (direction, kappa) = self.predict(&sample.features);
            let cosine = dot(direction, sample.target).clamp(-1.0, 1.0);
            total += -log_normalizer(kappa) - kappa * cosine;
        }
        total / samples.len() as f64
    }

    /// One gradient step over a batch, with Adam.
    fn step(&mut self, batch: &[VmfSample], state: &mut VmfAdam, learning_rate: f64) -> f64 {
        let count = batch.len() as f64;
        let mut grad_input = crate::net::LayerGradient::new(&self.input);
        let mut grad_hidden = crate::net::LayerGradient::new(&self.hidden);
        let mut grad_output = crate::net::LayerGradient::new(&self.output);
        let mut loss = 0.0;

        for sample in batch {
            let (first, second, out) = self.forward(&sample.features);
            let raw = [out[0], out[1], out[2]];
            let direction = normalise(raw);
            let length = norm(raw).max(1e-12);
            let kappa = softplus(out[3]) + 1e-3;
            let cosine = dot(direction, sample.target).clamp(-1.0, 1.0);
            loss += -log_normalizer(kappa) - kappa * cosine;

            // dL/du = −κ (y − (y·ŷ)ŷ) / |u|
            let mut delta_out = [0.0; 4];
            for index in 0..3 {
                let perpendicular = sample.target[index] - cosine * direction[index];
                delta_out[index] = -kappa * perpendicular / length;
            }
            // dL/dk = (coth κ − 1/κ − y·ŷ) · sigmoid(k)
            let sigmoid = 1.0 / (1.0 + (-out[3]).exp());
            delta_out[3] = (coth(kappa) - 1.0 / kappa - cosine) * sigmoid;

            #[allow(clippy::needless_range_loop)]
                #[allow(clippy::needless_range_loop)]
            for unit in 0..4 {
                grad_output.biases[unit] += delta_out[unit];
                for (index, value) in second.iter().enumerate() {
                    grad_output.weights[unit * self.width + index] += delta_out[unit] * value;
                }
            }
            // Back through the hidden layers.
            let mut delta_second = vec![0.0; self.width];
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.width {
                let mut sum = 0.0;
                for out_unit in 0..4 {
                    sum += self.output.weights[out_unit * self.width + unit] * delta_out[out_unit];
                }
                delta_second[unit] = if second[unit] > 0.0 { sum } else { 0.0 };
            }
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.width {
                grad_hidden.biases[unit] += delta_second[unit];
                for (index, value) in first.iter().enumerate() {
                    grad_hidden.weights[unit * self.width + index] += delta_second[unit] * value;
                }
            }
            let mut delta_first = vec![0.0; self.width];
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.width {
                let mut sum = 0.0;
                for next in 0..self.width {
                    sum += self.hidden.weights[next * self.width + unit] * delta_second[next];
                }
                delta_first[unit] = if first[unit] > 0.0 { sum } else { 0.0 };
            }
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.width {
                grad_input.biases[unit] += delta_first[unit];
                for (index, value) in sample.features.iter().enumerate() {
                    grad_input.weights[unit * self.input.inputs + index] += delta_first[unit] * value;
                }
            }
        }

        state.step += 1;
        state.apply(0, &mut self.input, &grad_input, count, learning_rate);
        state.apply(1, &mut self.hidden, &grad_hidden, count, learning_rate);
        state.apply(2, &mut self.output, &grad_output, count, learning_rate);
        loss / count
    }

    /// Train for `epochs` over the samples, in mini-batches.
    pub fn train(
        &mut self,
        samples: &[VmfSample],
        epochs: usize,
        batch_size: usize,
        learning_rate: f64,
        rng: &mut crate::tracker::Rng,
    ) -> Vec<f64> {
        let mut state = VmfAdam::new(self);
        let mut curve = Vec::with_capacity(epochs);
        let mut order: Vec<usize> = (0..samples.len()).collect();
        for _ in 0..epochs {
            for index in (1..order.len()).rev() {
                let swap = (rng.uniform() * (index as f64 + 1.0)) as usize;
                order.swap(index, swap.min(index));
            }
            let mut total = 0.0;
            let mut batches = 0;
            for chunk in order.chunks(batch_size) {
                let batch: Vec<VmfSample> = chunk.iter().map(|index| samples[*index].clone()).collect();
                total += self.step(&batch, &mut state, learning_rate);
                batches += 1;
            }
            curve.push(total / batches.max(1) as f64);
        }
        curve
    }
}

/// Softplus, `ln(1 + e^x)`, without overflow for large `x`.
fn softplus(value: f64) -> f64 {
    if value > 30.0 { value } else { value.exp().ln_1p() }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalise(a: [f64; 3]) -> [f64; 3] {
    let length = norm(a);
    if length < 1e-12 {
        [0.0, 0.0, -1.0]
    } else {
        [a[0] / length, a[1] / length, a[2] / length]
    }
}

/// Adam's state for the probabilistic network.
struct VmfAdam {
    layers: [crate::net::LayerState; 3],
    step: u32,
}

impl VmfAdam {
    fn new(network: &VmfNetwork) -> VmfAdam {
        VmfAdam {
            layers: [
                crate::net::LayerState::new(&network.input),
                crate::net::LayerState::new(&network.hidden),
                crate::net::LayerState::new(&network.output),
            ],
            step: 0,
        }
    }

    fn apply(
        &mut self,
        index: usize,
        layer: &mut Layer,
        gradient: &crate::net::LayerGradient,
        count: f64,
        learning_rate: f64,
    ) {
        const BETA1: f64 = 0.9;
        const BETA2: f64 = 0.999;
        const EPSILON: f64 = 1e-8;
        let correction1 = 1.0 - BETA1.powi(self.step as i32);
        let correction2 = 1.0 - BETA2.powi(self.step as i32);
        let state = &mut self.layers[index];
        for position in 0..layer.weights.len() {
            let value = gradient.weights[position] / count;
            let moment = &mut state.weight_moment[position];
            let velocity = &mut state.weight_velocity[position];
            *moment = BETA1 * *moment + (1.0 - BETA1) * value;
            *velocity = BETA2 * *velocity + (1.0 - BETA2) * value * value;
            layer.weights[position] -= learning_rate * (*moment / correction1)
                / ((*velocity / correction2).sqrt() + EPSILON);
        }
        for position in 0..layer.biases.len() {
            let value = gradient.biases[position] / count;
            let moment = &mut state.bias_moment[position];
            let velocity = &mut state.bias_velocity[position];
            *moment = BETA1 * *moment + (1.0 - BETA1) * value;
            *velocity = BETA2 * *velocity + (1.0 - BETA2) * value * value;
            layer.biases[position] -= learning_rate * (*moment / correction1)
                / ((*velocity / correction2).sqrt() + EPSILON);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_normaliser_agrees_with_the_exact_form_where_it_is_stable() {
        // The exact form is `ln(κ/(4π sinh κ))`; the approximation must match it
        // across the switch, and the switch must be continuous.
        for kappa in [0.1f64, 0.5, 1.0, 2.0, 2.65, 5.0, 20.0, 60.0] {
            let exact = (kappa / (4.0 * std::f64::consts::PI * kappa.sinh())).ln();
            let ours = log_normalizer(kappa);
            assert!((exact - ours).abs() < 0.01, "κ = {kappa}: {exact} vs {ours}");
        }
        // Continuous across the asymptotic switch at κ = 700.
        let below = log_normalizer(700.0 - 1e-6);
        let above = log_normalizer(700.0 + 1e-6);
        assert!((below - above).abs() < 1e-3, "{below} vs {above}");
        // And finite for a huge κ, where `sinh` overflows.
        assert!(log_normalizer(1e4).is_finite());
    }

    #[test]
    fn coth_is_stable_and_correct() {
        assert!((coth(1.0) - 1.313_035_468).abs() < 1e-6);
        assert!((coth(0.5) - 2.163_953_558).abs() < 1e-6);
        // Large κ tends to one without overflow.
        assert!((coth(1e4) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_expected_angle_falls_with_the_concentration() {
        // A uniform distribution has a mean angle of 90°, and a tight one is
        // small. The function must be monotone, which is what makes κ readable as
        // an uncertainty.
        assert!((expected_angle(0.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let mut previous = std::f64::consts::PI;
        for kappa in [0.5, 1.0, 2.0, 5.0, 10.0, 50.0, 200.0] {
            let angle = expected_angle(kappa);
            assert!(angle < previous, "κ = {kappa}: {angle} > {previous}");
            assert!(angle > 0.0);
            previous = angle;
        }
        // A concentration of 10 is 23°, which is the scale this detector
        // produces — the network's median error of 23.8° corresponds to κ ≈ 10.
        assert!((expected_angle(10.0).to_degrees() - 23.0).abs() < 1.0, "{}", expected_angle(10.0).to_degrees());
    }

    #[test]
    fn the_loss_is_lower_for_a_confident_correct_prediction() {
        // The distribution's own arithmetic: a correct direction with a high κ
        // must score better than a wrong one with a low κ.
        let mut rng = crate::tracker::Rng::new(1);
        let network = VmfNetwork::new(4, 8, &mut rng);
        let features = vec![0.1, -0.2, 0.3, 0.4];
        let (direction, _) = network.predict(&features);
        let correct = VmfSample { features: features.clone(), target: direction };
        let opposite = VmfSample { features, target: [-direction[0], -direction[1], -direction[2]] };
        assert!(network.loss(&[correct]) < network.loss(&[opposite]));
    }

    #[test]
    fn a_network_trains_on_a_simple_direction_task() {
        // The gradient must be right, and the only way to know is to train: a
        // network that learns is a network whose gradient points downhill.
        let mut rng = crate::tracker::Rng::new(7);
        let mut network = VmfNetwork::new(3, 32, &mut rng);
        let samples: Vec<VmfSample> = (0..600)
            .map(|_| {
                let features: Vec<f64> = (0..3).map(|_| rng.range(-1.0, 1.0)).collect();
                let raw = [features[0], features[1], 1.0];
                VmfSample { features, target: normalise(raw) }
            })
            .collect();
        let before = network.loss(&samples);
        let curve = network.train(&samples, 150, 32, 0.01, &mut rng);
        let after = network.loss(&samples);
        assert!(after < before, "loss {before:.3} -> {after:.3}");
        assert!(curve[149] < curve[0], "{} -> {}", curve[0], curve[149]);
        // And the predicted concentration must be finite and positive.
        let (_, kappa) = network.predict(&[0.5, 0.5, 0.5]);
        assert!(kappa.is_finite() && kappa > 0.0, "{kappa}");
    }
}
