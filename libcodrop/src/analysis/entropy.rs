/// Data profile containing statistical characteristics of an input block.
#[derive(Debug, Clone, PartialEq)]
pub struct DataProfile {
    pub shannon_entropy: f64,
    pub dominant_byte_ratio: f64,
    pub unique_symbols: usize,
    pub sample_size: usize,
}

/// Compute statistical characteristics over an input slice.
/// Samples up to 4096 bytes for maximum classification throughput.
pub fn analyze_profile(data: &[u8]) -> DataProfile {
    if data.is_empty() {
        return DataProfile {
            shannon_entropy: 0.0,
            dominant_byte_ratio: 0.0,
            unique_symbols: 0,
            sample_size: 0,
        };
    }

    let sample_len = data.len().min(4096);
    let sample = &data[..sample_len];

    let mut counts = [0u32; 256];
    for &b in sample {
        counts[b as usize] += 1;
    }

    let mut entropy = 0.0;
    let mut max_count = 0u32;
    let mut unique = 0;
    let total = sample_len as f64;

    for &c in &counts {
        if c > 0 {
            unique += 1;
            if c > max_count {
                max_count = c;
            }
            let p = (c as f64) / total;
            entropy -= p * p.log2();
        }
    }

    DataProfile {
        shannon_entropy: entropy,
        dominant_byte_ratio: (max_count as f64) / total,
        unique_symbols: unique,
        sample_size: sample_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_entropy() {
        let zeroes = vec![0u8; 1000];
        let profile = analyze_profile(&zeroes);
        assert_eq!(profile.shannon_entropy, 0.0);
        assert_eq!(profile.dominant_byte_ratio, 1.0);
        assert_eq!(profile.unique_symbols, 1);
    }

    #[test]
    fn test_high_entropy() {
        // Uniform distribution across 256 bytes has entropy ~8.0
        let mut uniform = Vec::new();
        for _ in 0..16 {
            for b in 0..=255 {
                uniform.push(b);
            }
        }
        let profile = analyze_profile(&uniform);
        assert!((profile.shannon_entropy - 8.0).abs() < 0.05);
        assert_eq!(profile.unique_symbols, 256);
    }
}
