/// Calculates the Hann window function for the given sample count
/// https://en.wikipedia.org/wiki/Hann_function
pub(crate) fn hann(sample_count: usize) -> Vec<f32> {
    match sample_count {
        0 => return vec![],
        1 => return vec![0.0],
        _ => (),
    }

    let mut window = Vec::with_capacity(sample_count);
    let scale = 2.0 * std::f32::consts::PI / (sample_count - 1) as f32;

    // for symmetricy reasons it's enough to calculate only half of the window
    let halfsize = (sample_count + 1) / 2;
    for i in 0..halfsize {
        let value = 0.5 - 0.5 * (scale * i as f32).cos();
        window.push(value);
    }

    // fill the rest of the window
    for i in halfsize..sample_count {
        window.push(window[sample_count - i - 1]);
    }

    window
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn hann_test() {
        assert_eq!(hann(0), vec![]);
        assert_eq!(hann(1), vec![0.0]);
        assert_eq!(hann(2), vec![0.0, 0.0]);
        assert_eq!(hann(3), vec![0.0, 1.0, 0.0]);
        assert_eq!(hann(4), vec![0.0, 0.75, 0.75, 0.0]);
        assert_eq!(hann(5), vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        assert_eq!(hann(7), vec![0.0, 0.25, 0.75, 1.0, 0.75, 0.25, 0.0]);

        // check how overlapping works
        let window_size = 1023;
        let window = hann(window_size);
        for i in 0..window_size / 2 {
            let value = window[i] + window[i + window_size / 2];
            assert!((value - 1.0).abs() < 1e-6);
        }

        let window_size = 1024;
        let window = hann(window_size);
        for i in 0..window_size / 2 {
            let value = window[i] + window[i + window_size / 2];
            assert!((value - 1.0).abs() < 1e-2);
        }
    }
}
