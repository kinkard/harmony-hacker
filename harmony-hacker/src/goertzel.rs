//! Minimalistic implementation of the Goertzel algorithm.
//! https://en.wikipedia.org/wiki/Goertzel_algorithm

/// Stateless Goertzel algorithm
/// Example:
/// ```
/// // check the sine wave with the target frequency
/// let sample_rate = 44100;
/// let frequency = 440.0;
/// let samples: Vec<f32> = (0..44100)
///     .map(|i| {
///         let t = i as f32 / sample_rate as f32;
///         (2.0 * std::f32::consts::PI * frequency * t).sin()
///     })
///     .collect();
/// let magnitude = goertzel(&samples, sample_rate, frequency);
/// assert!(0.99 < magnitude && magnitude < 1.01);
/// // Another frequency
/// let magnitude = goertzel(&samples, sample_rate, 293.66484);
/// assert!(magnitude < 0.01);
/// ```
#[allow(dead_code)]
pub(crate) fn goertzel(samples: &[f32], sample_rate: u32, target_frequency: f32) -> f32 {
    let k = target_frequency / sample_rate as f32;
    let w = 2.0 * std::f32::consts::PI * k;
    let coeff = 2.0 * w.cos();

    let mut q0;
    let mut q1 = 0.0f32;
    let mut q2 = 0.0f32;

    // s[n] = x[n] + 2 * cos(2 * pi * k) * s[n-1] - s[n-2]
    for sample in samples {
        q0 = sample + coeff * q1 - q2;
        q2 = q1;
        q1 = q0;
    }

    let magnitude = ((q1 * q1) + (q2 * q2) - (q1 * q2 * coeff)).sqrt();
    let normalized_magnitude = 2.0 * magnitude / samples.len() as f32;
    normalized_magnitude
}

/// Stateful Goertzel algorithm. Might be orders of magnitude faster when multiple filters are needed.
/// Example:
/// ```
/// // combine two sine waves
/// let sample_rate = 44100;
/// let c4 = 261.6256;
/// let e4 = 329.6276;
/// let samples: Vec<f32> = (0..44100)
///     .map(|i| {
///         let t = i as f32 / sample_rate as f32;
///         (2.0 * std::f32::consts::PI * c4 * t).sin()
///             + (2.0 * std::f32::consts::PI * e4 * t).sin()
///     })
///     .collect();
/// let mut c4_goertzel = Goertzel::new(sample_rate, c4);
/// let mut d4_goertzel = Goertzel::new(sample_rate, 293.66484);
/// let mut e4_goertzel = Goertzel::new(sample_rate, e4);
/// for sample in &samples {
///     c4_goertzel.process(*sample);
///     d4_goertzel.process(*sample);
///     e4_goertzel.process(*sample);
/// }
/// let magnitude = c4_goertzel.magnitude(samples.len() as u32);
/// assert!(0.99 < magnitude && magnitude < 1.01);
/// let magnitude = d4_goertzel.magnitude(samples.len() as u32);
/// assert!(magnitude < 0.01);
/// let magnitude = e4_goertzel.magnitude(samples.len() as u32);
/// assert!(0.99 < magnitude && magnitude < 1.01);
/// ```
pub(crate) struct Goertzel {
    q0: f32,
    q1: f32,
    q2: f32,
    coeff: f32,
}

impl Goertzel {
    /// Create a new Goertzel filter
    pub(crate) fn new(sample_rate: u32, target_frequency: f32) -> Self {
        let k = target_frequency / sample_rate as f32;
        let w = 2.0 * std::f32::consts::PI * k;
        let coeff = 2.0 * w.cos();

        Self {
            q0: 0.0,
            q1: 0.0,
            q2: 0.0,
            coeff,
        }
    }

    /// Process a single sample
    /// s[n] = x[n] + 2 * cos(2 * pi * k) * s[n-1] - s[n-2]
    pub(crate) fn process(&mut self, sample: f32) {
        self.q0 = sample + self.coeff * self.q1 - self.q2;
        self.q2 = self.q1;
        self.q1 = self.q0;
    }

    /// Get the magnitude of the signal sampled before
    pub(crate) fn magnitude(&self, block_size: u32) -> f32 {
        let magnitude =
            ((self.q1 * self.q1) + (self.q2 * self.q2) - (self.q1 * self.q2 * self.coeff)).sqrt();
        let normalized_magnitude = 2.0 * magnitude / block_size as f32;
        normalized_magnitude
    }

    /// Reset the filter's state
    pub(crate) fn reset(&mut self) {
        self.q0 = 0.0;
        self.q1 = 0.0;
        self.q2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateless_goertzel_test() {
        // silence
        let samples = vec![0.0; 44100];
        let magnitude = goertzel(&samples, 44100, 440.0);
        assert_eq!(magnitude, 0.0);

        // check the sine wave with the target frequency
        let sample_rate = 44100;
        let target_frequency = 440.0;
        let samples: Vec<f32> = (0..44100)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * target_frequency * t).sin()
            })
            .collect();
        let magnitude = goertzel(&samples, sample_rate, target_frequency);
        assert!(0.99 < magnitude && magnitude < 1.01);

        // combine two sine waves
        let c4 = 261.6256;
        let e4 = 329.6276;
        let samples: Vec<f32> = (0..44100)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * c4 * t).sin()
                    + (2.0 * std::f32::consts::PI * e4 * t).sin()
            })
            .collect();
        let magnitude = goertzel(&samples, sample_rate, c4);
        assert!(0.99 < magnitude && magnitude < 1.01);

        // 293.66484 is D4
        let magnitude = goertzel(&samples, sample_rate, 293.66484);
        assert!(magnitude < 0.01);

        let magnitude = goertzel(&samples, sample_rate, e4);
        assert!(0.99 < magnitude && magnitude < 1.01);
    }

    #[test]
    fn statefull_goertzel_test() {
        // combine two sine waves
        let sample_rate = 44100;
        let c4 = 261.6256;
        let e4 = 329.6276;
        let samples: Vec<f32> = (0..44100)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * c4 * t).sin()
                    + (2.0 * std::f32::consts::PI * e4 * t).sin()
            })
            .collect();

        // and detect it using 3 stateful Goertzel filters
        let mut c4_goertzel = Goertzel::new(sample_rate, c4);
        let mut d4_goertzel = Goertzel::new(sample_rate, 293.66484);
        let mut e4_goertzel = Goertzel::new(sample_rate, e4);

        for sample in &samples {
            c4_goertzel.process(*sample);
            d4_goertzel.process(*sample);
            e4_goertzel.process(*sample);
        }

        let magnitude = c4_goertzel.magnitude(samples.len() as u32);
        assert!(0.99 < magnitude && magnitude < 1.01);

        let magnitude = d4_goertzel.magnitude(samples.len() as u32);
        assert!(magnitude < 0.01);

        let magnitude = e4_goertzel.magnitude(samples.len() as u32);
        assert!(0.99 < magnitude && magnitude < 1.01);
    }
}
