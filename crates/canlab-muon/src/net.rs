//! A small multilayer perceptron, written out because the workspace is offline.
//!
//! No dependency is available, so the network is here: three dense layers, ReLU
//! in the hidden ones, a linear output, mean-squared error, and Adam. That is
//! about a hundred lines of arithmetic and it is enough for this problem, which
//! has 34 inputs and 2 outputs.
//!
//! # Why this shape
//!
//! The input is the two planes' pixel patterns, normalised and with their totals:
//! `2 × n² + 2` values. The output is the direction's two horizontal components,
//! with the vertical one reconstructed from the unit constraint. Two hidden
//! layers of 64 is the smallest size that can represent "the ratio of two
//! centroids" and "the width of the halo" at once, which is what the physics
//! asks for.
//!
//! # The loss, and why it is not the angle
//!
//! The loss is the mean squared error on `(dx, dy)`. The natural loss for an
//! angle is `1 − cos θ`, and for small angles the two agree to second order —
//! `1 − cos θ ≈ θ²/2`, and `|d − d̂|² ≈ θ²`. The squared error is used because
//! its gradient is linear and therefore does not vanish at the origin the way a
//! cosine's does; the *reported* metric is the angle, which is what a reader
//! wants.
//!
//! # What is not here
//!
//! No dropout, no batch normalisation, no learning-rate schedule. Each is a
//! reasonable thing to add and none is needed to answer "what accuracy does this
//! detector give": the domain randomisation is the regulariser, and it is a
//! stronger one than dropout because it changes the *data* rather than the
//! network.

use crate::tracker::Rng;

/// A dense layer: weights in row-major `[out][in]` order.
#[derive(Debug, Clone)]
pub(crate) struct Layer {
    pub(crate) weights: Vec<f64>,
    pub(crate) biases: Vec<f64>,
    pub(crate) inputs: usize,
    pub(crate) outputs: usize,
}

impl Layer {
    pub(crate) fn new(inputs: usize, outputs: usize, rng: &mut Rng) -> Layer {
        // He initialisation: a normal with a standard deviation of sqrt(2/fan_in),
        // which keeps the variance of the activations from collapsing through a
        // ReLU stack.
        let scale = (2.0 / inputs as f64).sqrt();
        let weights = (0..inputs * outputs).map(|_| rng.normal() * scale).collect();
        Layer { weights, biases: vec![0.0; outputs], inputs, outputs }
    }

    pub(crate) fn forward(&self, input: &[f64], output: &mut [f64]) {
        #[allow(clippy::needless_range_loop)]
        for unit in 0..self.outputs {
            let row = &self.weights[unit * self.inputs..(unit + 1) * self.inputs];
            let mut sum = self.biases[unit];
            for (weight, value) in row.iter().zip(input) {
                sum += weight * value;
            }
            output[unit] = sum;
        }
    }
}

/// The network: two hidden layers and a linear output.
#[derive(Debug, Clone)]
pub struct Network {
    input: Layer,
    hidden: Layer,
    output: Layer,
    hidden_width: usize,
}

/// One training sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The input features.
    pub features: Vec<f64>,
    /// The target direction's horizontal components.
    pub target: [f64; 2],
}

impl Network {
    /// A network with `hidden` units in each hidden layer.
    pub fn new(inputs: usize, hidden: usize, rng: &mut Rng) -> Network {
        Network {
            input: Layer::new(inputs, hidden, rng),
            hidden: Layer::new(hidden, hidden, rng),
            output: Layer::new(hidden, 2, rng),
            hidden_width: hidden,
        }
    }

    /// How many trainable parameters the network has.
    pub fn parameters(&self) -> usize {
        self.input.weights.len()
            + self.input.biases.len()
            + self.hidden.weights.len()
            + self.hidden.biases.len()
            + self.output.weights.len()
            + self.output.biases.len()
    }

    /// The forward pass, keeping the activations the backward pass needs.
    fn forward(&self, features: &[f64]) -> (Vec<f64>, Vec<f64>, [f64; 2]) {
        let mut first = vec![0.0; self.hidden_width];
        self.input.forward(features, &mut first);
        for value in &mut first {
            *value = value.max(0.0);
        }
        let mut second = vec![0.0; self.hidden_width];
        self.hidden.forward(&first, &mut second);
        for value in &mut second {
            *value = value.max(0.0);
        }
        let mut out = [0.0; 2];
        self.output.forward(&second, &mut out);
        (first, second, out)
    }

    /// The prediction for one input.
    pub fn predict(&self, features: &[f64]) -> [f64; 2] {
        let (_, _, out) = self.forward(features);
        out
    }

    /// The mean squared error over a set of samples.
    pub fn loss(&self, samples: &[Sample]) -> f64 {
        if samples.is_empty() {
            return f64::NAN;
        }
        let mut total = 0.0;
        for sample in samples {
            let prediction = self.predict(&sample.features);
            for (predicted, target) in prediction.iter().zip(&sample.target) {
                let difference = predicted - target;
                total += difference * difference;
            }
        }
        total / (samples.len() as f64 * 2.0)
    }

    /// One gradient step over a batch, with Adam.
    ///
    /// Returns the batch's loss before the step. The gradients are accumulated
    /// over the batch and divided by its size, so the step size does not depend on
    /// the batch.
    fn step(&mut self, batch: &[Sample], state: &mut Adam, learning_rate: f64) -> f64 {
        let count = batch.len() as f64;
        let mut grad_input = LayerGradient::new(&self.input);
        let mut grad_hidden = LayerGradient::new(&self.hidden);
        let mut grad_output = LayerGradient::new(&self.output);
        let mut loss = 0.0;

        for sample in batch {
            let (first, second, out) = self.forward(&sample.features);
            // dL/dout for mean squared error, with the 1/(2n) folded into the
            // batch division below.
            let mut delta_out = [0.0; 2];
            #[allow(clippy::needless_range_loop)]
            for index in 0..2 {
                let _ = index;
                let difference = out[index] - sample.target[index];
                loss += difference * difference;
                delta_out[index] = 2.0 * difference;
            }
            // The output layer's gradient.
            #[allow(clippy::needless_range_loop)]
            for unit in 0..2 {
                grad_output.biases[unit] += delta_out[unit];
                for (index, value) in second.iter().enumerate() {
                    grad_output.weights[unit * self.hidden_width + index] += delta_out[unit] * value;
                }
            }
            // Back through the second hidden layer.
            let mut delta_second = vec![0.0; self.hidden_width];
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.hidden_width {
                let mut sum = 0.0;
                for out_unit in 0..2 {
                    sum += self.output.weights[out_unit * self.hidden_width + unit] * delta_out[out_unit];
                }
                // ReLU's derivative: zero where the activation was clamped.
                delta_second[unit] = if second[unit] > 0.0 { sum } else { 0.0 };
            }
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.hidden_width {
                grad_hidden.biases[unit] += delta_second[unit];
                for (index, value) in first.iter().enumerate() {
                    grad_hidden.weights[unit * self.hidden_width + index] += delta_second[unit] * value;
                }
            }
            // Back through the first hidden layer.
            let mut delta_first = vec![0.0; self.hidden_width];
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.hidden_width {
                let mut sum = 0.0;
                for next in 0..self.hidden_width {
                    sum += self.hidden.weights[next * self.hidden_width + unit] * delta_second[next];
                }
                delta_first[unit] = if first[unit] > 0.0 { sum } else { 0.0 };
            }
            #[allow(clippy::needless_range_loop)]
            for unit in 0..self.hidden_width {
                grad_input.biases[unit] += delta_first[unit];
                for (index, value) in sample.features.iter().enumerate() {
                    grad_input.weights[unit * self.input.inputs + index] += delta_first[unit] * value;
                }
            }
        }

        // The mean over the batch, then Adam. The step count advances once per
        // batch and not once per layer, so all three layers share a bias
        // correction.
        state.step += 1;
        state.apply(0, &mut self.input, &grad_input, count, learning_rate);
        state.apply(1, &mut self.hidden, &grad_hidden, count, learning_rate);
        state.apply(2, &mut self.output, &grad_output, count, learning_rate);
        loss / (count * 2.0)
    }

    /// Train for `epochs` over `samples`, in mini-batches, and return the loss
    /// curve.
    pub fn train(
        &mut self,
        samples: &[Sample],
        epochs: usize,
        batch_size: usize,
        learning_rate: f64,
        rng: &mut Rng,
    ) -> Vec<f64> {
        let mut state = Adam::new(self);
        let mut curve = Vec::with_capacity(epochs);
        let mut order: Vec<usize> = (0..samples.len()).collect();
        for _ in 0..epochs {
            // Shuffle, so a batch is not a contiguous slice of the data: the
            // generator produces events in a fixed order and a contiguous batch
            // would be one region of the parameter space.
            for index in (1..order.len()).rev() {
                let swap = (rng.uniform() * (index as f64 + 1.0)) as usize;
                order.swap(index, swap.min(index));
            }
            let mut total = 0.0;
            let mut batches = 0;
            for chunk in order.chunks(batch_size) {
                let batch: Vec<Sample> = chunk.iter().map(|index| samples[*index].clone()).collect();
                total += self.step(&batch, &mut state, learning_rate);
                batches += 1;
            }
            curve.push(total / batches.max(1) as f64);
        }
        curve
    }
}

/// A layer's accumulated gradients.
pub(crate) struct LayerGradient {
    pub(crate) weights: Vec<f64>,
    pub(crate) biases: Vec<f64>,
}

impl LayerGradient {
    pub(crate) fn new(layer: &Layer) -> LayerGradient {
        LayerGradient {
            weights: vec![0.0; layer.weights.len()],
            biases: vec![0.0; layer.biases.len()],
        }
    }
}

/// Adam's state for one layer: a first and a second moment for every parameter.
pub(crate) struct LayerState {
    pub(crate) weight_moment: Vec<f64>,
    pub(crate) weight_velocity: Vec<f64>,
    pub(crate) bias_moment: Vec<f64>,
    pub(crate) bias_velocity: Vec<f64>,
}

impl LayerState {
    pub(crate) fn new(layer: &Layer) -> LayerState {
        LayerState {
            weight_moment: vec![0.0; layer.weights.len()],
            weight_velocity: vec![0.0; layer.weights.len()],
            bias_moment: vec![0.0; layer.biases.len()],
            bias_velocity: vec![0.0; layer.biases.len()],
        }
    }
}

/// Adam over the three layers, with the step count shared so the bias
/// corrections are the same for every parameter.
struct Adam {
    layers: [LayerState; 3],
    step: u32,
}

impl Adam {
    fn new(network: &Network) -> Adam {
        Adam {
            layers: [
                LayerState::new(&network.input),
                LayerState::new(&network.hidden),
                LayerState::new(&network.output),
            ],
            step: 0,
        }
    }

    /// One Adam update of one layer.
    ///
    /// `m ← β₁m + (1−β₁)g`, `v ← β₂v + (1−β₂)g²`, then
    /// `θ ← θ − lr · m̂ / (√v̂ + ε)` with the bias corrections `m̂ = m/(1−β₁ᵗ)`.
    fn apply(
        &mut self,
        index: usize,
        layer: &mut Layer,
        gradient: &LayerGradient,
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
            let m = *moment / correction1;
            let v = *velocity / correction2;
            layer.weights[position] -= learning_rate * m / (v.sqrt() + EPSILON);
        }
        for position in 0..layer.biases.len() {
            let value = gradient.biases[position] / count;
            let moment = &mut state.bias_moment[position];
            let velocity = &mut state.bias_velocity[position];
            *moment = BETA1 * *moment + (1.0 - BETA1) * value;
            *velocity = BETA2 * *velocity + (1.0 - BETA2) * value * value;
            let m = *moment / correction1;
            let v = *velocity / correction2;
            layer.biases[position] -= learning_rate * m / (v.sqrt() + EPSILON);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_learns_a_linear_map() {
        // The smallest thing a network must be able to do: fit a linear function
        // of its input. If this fails, nothing downstream means anything.
        let mut rng = Rng::new(1);
        let mut network = Network::new(4, 16, &mut rng);
        let samples: Vec<Sample> = (0..500)
            .map(|_| {
                let features: Vec<f64> = (0..4).map(|_| rng.normal()).collect();
                Sample {
                    target: [features[0] * 0.5 + features[2] * 0.5, features[1] - features[3]],
                    features,
                }
            })
            .collect();
        let before = network.loss(&samples);
        let curve = network.train(&samples, 200, 32, 0.01, &mut rng);
        let after = network.loss(&samples);
        assert!(after < before * 0.05, "loss {before:.4} -> {after:.4}");
        assert!(curve.len() == 200);
        assert!(curve[199] < curve[0], "{} -> {}", curve[0], curve[199]);
    }

    #[test]
    fn the_network_learns_a_nonlinear_map() {
        // A linear map can be fitted without a hidden layer at all, so it does
        // not test the depth. This one cannot: the target is a product.
        let mut rng = Rng::new(2);
        let mut network = Network::new(3, 32, &mut rng);
        let samples: Vec<Sample> = (0..1_000)
            .map(|_| {
                let features: Vec<f64> = (0..3).map(|_| rng.range(-1.0, 1.0)).collect();
                Sample {
                    target: [features[0] * features[1], features[2].abs()],
                    features,
                }
            })
            .collect();
        let before = network.loss(&samples);
        network.train(&samples, 300, 32, 0.01, &mut rng);
        let after = network.loss(&samples);
        assert!(after < before * 0.1, "loss {before:.4} -> {after:.4}");
    }

    #[test]
    fn a_trained_network_predicts_a_held_out_sample() {
        // Generalisation, not memorisation: the test samples are new draws from
        // the same distribution.
        let mut rng = Rng::new(3);
        let mut network = Network::new(2, 16, &mut rng);
        let make = |rng: &mut Rng| Sample {
            features: vec![rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)],
            target: [0.0, 0.0],
        };
        let mut samples: Vec<Sample> = (0..800).map(|_| make(&mut rng)).collect();
        for sample in &mut samples {
            sample.target = [sample.features[0].tanh(), sample.features[1].tanh()];
        }
        network.train(&samples, 300, 32, 0.01, &mut rng);
        let mut held_out: Vec<Sample> = (0..200).map(|_| make(&mut rng)).collect();
        for sample in &mut held_out {
            sample.target = [sample.features[0].tanh(), sample.features[1].tanh()];
        }
        let loss = network.loss(&held_out);
        assert!(loss < 0.01, "{loss}");
    }

    #[test]
    fn the_parameter_count_is_the_layers_own() {
        // 4 -> 8 -> 8 -> 2: (4x8 + 8) + (8x8 + 8) + (8x2 + 2).
        let mut rng = Rng::new(4);
        let network = Network::new(4, 8, &mut rng);
        assert_eq!(network.parameters(), 40 + 72 + 18);
    }

    #[test]
    fn a_prediction_is_finite_and_the_loss_of_an_empty_set_is_not_a_number() {
        let mut rng = Rng::new(5);
        let network = Network::new(3, 4, &mut rng);
        let prediction = network.predict(&[1.0, -1.0, 0.5]);
        assert!(prediction.iter().all(|value| value.is_finite()));
        assert!(network.loss(&[]).is_nan());
    }
}
