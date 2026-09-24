use crate::analysis::entropy::analyze_profile;
use crate::format::BlockType;

/// High-level compression strategy level requested by caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// High-throughput linear speed (LZF)
    Fast,
    /// Balanced ratio and throughput (LZH, default)
    #[default]
    Balanced,
    /// Archival high-ratio compression
    Compact,
    /// Maximum effort compression
    Max,
    /// Dynamic heuristic auto-selection
    Auto,
}

pub struct AdaptiveClassifier;

impl AdaptiveClassifier {
    /// Analyze input and select optimal block compression strategy
    pub fn select_strategy(input: &[u8], level: CompressionLevel) -> BlockType {
        if input.is_empty() {
            return BlockType::Raw;
        }

        let profile = analyze_profile(input);

        // 1. Check for extreme repetition -> RLE
        if profile.dominant_byte_ratio >= 0.65 || profile.unique_symbols <= 3 {
            return BlockType::Rle;
        }

        // 2. Check for high-entropy incompressible data -> RAW bypass
        if profile.shannon_entropy >= 7.85 && profile.unique_symbols > 240 {
            return BlockType::Raw;
        }

        // 3. Dispatch based on level and profile
        match level {
            CompressionLevel::Fast => BlockType::Lzf,
            CompressionLevel::Balanced => BlockType::Lzh,
            CompressionLevel::Compact | CompressionLevel::Max => BlockType::Lzh,
            CompressionLevel::Auto => {
                if profile.shannon_entropy > 7.4 {
                    // Borderline high entropy: fast LZF avoids wasting cycles
                    BlockType::Lzf
                } else {
                    // Standard text/structured data: LZH provides great ratio
                    BlockType::Lzh
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_repetitive_as_rle() {
        let repeated = vec![0x55; 5000];
        let chosen = AdaptiveClassifier::select_strategy(&repeated, CompressionLevel::Auto);
        assert_eq!(chosen, BlockType::Rle);
    }

    #[test]
    fn test_classify_random_as_raw() {
        // High entropy pseudo-random sequence
        let mut pseudo_random = Vec::with_capacity(4096);
        let mut seed: u32 = 0x12345678;
        for _ in 0..4096 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            pseudo_random.push((seed >> 24) as u8);
        }
        let chosen = AdaptiveClassifier::select_strategy(&pseudo_random, CompressionLevel::Auto);
        assert_eq!(chosen, BlockType::Raw);
    }

    #[test]
    fn test_classify_text_as_lzh() {
        let text = b"The architecture of modern compression algorithms requires balancing speed and ratio.";
        let chosen = AdaptiveClassifier::select_strategy(text, CompressionLevel::Balanced);
        assert_eq!(chosen, BlockType::Lzh);
    }
}
