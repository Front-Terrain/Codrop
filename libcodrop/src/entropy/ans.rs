use crate::entropy::bitstream::BitReader;
use crate::error::CodropError;

pub const ANS_TABLE_LOG: usize = 10;
pub const ANS_TABLE_SIZE: usize = 1 << ANS_TABLE_LOG; // 1024
pub const ANS_STEP: usize = (ANS_TABLE_SIZE >> 1) + (ANS_TABLE_SIZE >> 3) + 3; // 643

/// Decoding table entry for a single state x in [ANS_TABLE_SIZE, 2 * ANS_TABLE_SIZE - 1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnsDecodeEntry {
    pub symbol: u16,
    pub num_bits: u8,
    pub new_state_base: u16,
}

/// Encoding parameters for a single symbol s.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnsSymbolEncoder {
    pub norm_freq: u16,
    pub k: u8,
    pub threshold: u16,
    pub next_states: Vec<u16>,
}

/// Normalizes raw frequency counts to sum exactly to `table_size` (1024).
///
/// Guaranteed properties:
/// 1. Every symbol with count > 0 receives a normalized frequency >= 1.
/// 2. Symbols with count == 0 receive normalized frequency == 0.
/// 3. The sum of all normalized frequencies equals `table_size`.
/// 4. Completely deterministic.
pub fn normalize_frequencies(
    raw_counts: &[u32],
    max_symbols: usize,
    table_size: usize,
) -> Vec<u16> {
    let mut norm = vec![0u16; max_symbols];
    let total_count: u64 = raw_counts.iter().take(max_symbols).map(|&c| c as u64).sum();

    if total_count == 0 {
        return norm;
    }

    let non_zero_count = raw_counts
        .iter()
        .take(max_symbols)
        .filter(|&&c| c > 0)
        .count();

    if non_zero_count == 1 {
        for (s, &c) in raw_counts.iter().take(max_symbols).enumerate() {
            if c > 0 {
                norm[s] = table_size as u16;
                return norm;
            }
        }
    }

    // Step 1: Initial proportional allocation with a minimum of 1 for non-zero symbols
    let mut sum: usize = 0;
    for (s, &c) in raw_counts.iter().take(max_symbols).enumerate() {
        if c > 0 {
            let allocated =
                ((c as u64 * table_size as u64 + (total_count / 2)) / total_count).max(1) as usize;
            norm[s] = allocated as u16;
            sum += allocated;
        }
    }

    // Step 2: Adjust sum to exactly table_size
    if sum > table_size {
        let mut excess = sum - table_size;
        // Decrement symbols with norm_freq > 1, starting from the ones with largest norm_freq
        while excess > 0 {
            let mut best_sym = None;
            let mut best_val = 1u16;
            for (s, &n) in norm.iter().enumerate() {
                if n > best_val {
                    best_val = n;
                    best_sym = Some(s);
                }
            }
            if let Some(s) = best_sym {
                norm[s] -= 1;
                excess -= 1;
            } else {
                // If all symbols are 1, we can't reduce further (only possible if non_zero_count > table_size)
                break;
            }
        }
    } else if sum < table_size {
        let mut deficit = table_size - sum;
        // Increment symbols with highest raw count / discrepancy
        while deficit > 0 {
            let mut best_sym = None;
            let mut best_score = 0i64;
            for (s, &c) in raw_counts.iter().take(max_symbols).enumerate() {
                if c > 0 {
                    let expected =
                        (c as i64 * table_size as i64) - (norm[s] as i64 * total_count as i64);
                    if best_sym.is_none() || expected > best_score {
                        best_score = expected;
                        best_sym = Some(s);
                    }
                }
            }
            if let Some(s) = best_sym {
                norm[s] += 1;
                deficit -= 1;
            } else {
                break;
            }
        }
    }

    norm
}

/// Spreads symbols across table states [0..table_size - 1] using coprime step permutation.
pub fn spread_symbols(norm_freq: &[u16], table_size: usize) -> Vec<u16> {
    let mut table = vec![u16::MAX; table_size];
    let mut pos = 0usize;

    // Collect active symbols sorted by (symbol index) for 100% determinism
    for (s, &freq) in norm_freq.iter().enumerate() {
        for _ in 0..freq {
            while table[pos] != u16::MAX {
                pos = (pos + ANS_STEP) & (table_size - 1);
            }
            table[pos] = s as u16;
            pos = (pos + ANS_STEP) & (table_size - 1);
        }
    }

    table
}

/// ANS Decoding Table with O(1) state lookup.
#[derive(Debug, Clone)]
pub struct AnsDecodeTable {
    pub entries: Vec<AnsDecodeEntry>,
}

impl AnsDecodeTable {
    /// Builds the decoding table from normalized frequencies.
    pub fn build(norm_freq: &[u16]) -> Result<Self, CodropError> {
        let total: usize = norm_freq.iter().map(|&f| f as usize).sum();
        if total != ANS_TABLE_SIZE {
            return Err(CodropError::CorruptedEntropyStream(format!(
                "ANS normalized frequency sum {} != expected {}",
                total, ANS_TABLE_SIZE
            )));
        }

        let spread = spread_symbols(norm_freq, ANS_TABLE_SIZE);
        let mut entries = vec![AnsDecodeEntry::default(); ANS_TABLE_SIZE];

        // Track occurrence index j for each symbol
        let max_sym = norm_freq.len();
        let mut symbol_occ = vec![0usize; max_sym];

        for (state_idx, &sym) in spread.iter().enumerate() {
            let s = sym as usize;
            let n_s = norm_freq[s] as usize;
            let j = symbol_occ[s];
            symbol_occ[s] += 1;

            let sub_x = n_s + j;
            let k = 31 - (sub_x as u32).leading_zeros() as usize;
            let num_bits = (ANS_TABLE_LOG - k) as u8;
            let new_state_base = ((sub_x << num_bits) - ANS_TABLE_SIZE) as u16;

            entries[state_idx] = AnsDecodeEntry {
                symbol: sym,
                num_bits,
                new_state_base,
            };
        }

        Ok(Self { entries })
    }

    /// Decodes one symbol and updates state by reading bits from reader.
    #[inline(always)]
    pub fn decode_symbol(
        &self,
        state: &mut u16,
        reader: &mut BitReader,
    ) -> Result<u16, CodropError> {
        let idx = (*state as usize).saturating_sub(ANS_TABLE_SIZE);
        if idx >= ANS_TABLE_SIZE {
            return Err(CodropError::CorruptedEntropyStream(
                "ANS state out of bounds".into(),
            ));
        }

        let entry = self.entries[idx];
        let bits = if entry.num_bits > 0 {
            reader.read_bits(entry.num_bits)? as u16
        } else {
            0
        };

        *state = ANS_TABLE_SIZE as u16 + entry.new_state_base + bits;
        Ok(entry.symbol)
    }
}

/// ANS Encoding Table.
#[derive(Debug, Clone)]
pub struct AnsEncodeTable {
    pub symbols: Vec<AnsSymbolEncoder>,
}

impl AnsEncodeTable {
    /// Builds the encoding table from normalized frequencies.
    pub fn build(norm_freq: &[u16]) -> Result<Self, CodropError> {
        let total: usize = norm_freq.iter().map(|&f| f as usize).sum();
        if total != ANS_TABLE_SIZE {
            return Err(CodropError::CorruptedEntropyStream(
                "ANS frequency sum mismatch".into(),
            ));
        }

        let spread = spread_symbols(norm_freq, ANS_TABLE_SIZE);
        let max_sym = norm_freq.len();
        let mut symbols = vec![AnsSymbolEncoder::default(); max_sym];

        for (s, &freq) in norm_freq.iter().enumerate() {
            if freq == 0 {
                continue;
            }
            let n_s = freq as usize;
            let k = (31 - (n_s as u32).leading_zeros()) as u8;
            let threshold =
                ((n_s << (ANS_TABLE_LOG - k as usize)) as u16).min((2 * ANS_TABLE_SIZE) as u16);

            symbols[s] = AnsSymbolEncoder {
                norm_freq: freq,
                k,
                threshold,
                next_states: Vec::with_capacity(n_s),
            };
        }

        // Fill next_states for each symbol occurrence in the spread table
        for (state_idx, &sym) in spread.iter().enumerate() {
            let s = sym as usize;
            let state = (ANS_TABLE_SIZE + state_idx) as u16;
            symbols[s].next_states.push(state);
        }

        Ok(Self { symbols })
    }

    /// Encodes symbol `s`, returning the number of bits to emit, the bits value, and updates state.
    #[inline(always)]
    pub fn encode_symbol(&self, s: usize, state: &mut u16) -> (u8, u32) {
        let enc = &self.symbols[s];
        debug_assert!(
            enc.norm_freq > 0,
            "Cannot encode symbol with zero frequency"
        );

        let cur_state = *state;
        let nb_bits = if cur_state >= enc.threshold {
            (ANS_TABLE_LOG as u8) - enc.k
        } else {
            (ANS_TABLE_LOG as u8) - enc.k - 1
        };

        let bits_out = if nb_bits > 0 {
            (cur_state & ((1 << nb_bits) - 1)) as u32
        } else {
            0
        };

        let sub_x = (cur_state >> nb_bits) as usize;
        let j = sub_x - (enc.norm_freq as usize);
        *state = enc.next_states[j];

        (nb_bits, bits_out)
    }
}

/// A recorded bit chunk emitted during reverse ANS encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnsBitChunk {
    pub bits: u8,
    pub val: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::bitstream::BitWriter;

    #[test]
    fn test_spread_coprime_step_covers_all_states() {
        let mut visited = [false; ANS_TABLE_SIZE];
        let mut pos = 0;
        for _ in 0..ANS_TABLE_SIZE {
            assert!(!visited[pos], "Position {} visited twice", pos);
            visited[pos] = true;
            pos = (pos + ANS_STEP) & (ANS_TABLE_SIZE - 1);
        }
        assert!(visited.iter().all(|&v| v));
    }

    #[test]
    fn test_frequency_normalization_invariants() {
        // Test uniform
        let raw = vec![10u32; 100];
        let norm = normalize_frequencies(&raw, 100, ANS_TABLE_SIZE);
        assert_eq!(
            norm.iter().map(|&f| f as usize).sum::<usize>(),
            ANS_TABLE_SIZE
        );
        assert!(norm.iter().all(|&f| f >= 1));

        // Test single symbol
        let mut raw_single = vec![0u32; 256];
        raw_single[42] = 1000;
        let norm_single = normalize_frequencies(&raw_single, 256, ANS_TABLE_SIZE);
        assert_eq!(norm_single[42] as usize, ANS_TABLE_SIZE);
        assert_eq!(
            norm_single.iter().map(|&f| f as usize).sum::<usize>(),
            ANS_TABLE_SIZE
        );

        // Test highly skewed
        let mut raw_skewed = vec![1u32; 200];
        raw_skewed[0] = 100_000;
        let norm_skewed = normalize_frequencies(&raw_skewed, 200, ANS_TABLE_SIZE);
        assert_eq!(
            norm_skewed.iter().map(|&f| f as usize).sum::<usize>(),
            ANS_TABLE_SIZE
        );
        assert!(norm_skewed[0] > 500);
        assert!(norm_skewed.iter().all(|&f| f >= 1));
    }

    #[test]
    fn test_ans_encode_decode_roundtrip_pure_symbols() {
        let alphabet_size = 50;
        let mut raw_counts = vec![0u32; alphabet_size];
        for (i, item) in raw_counts.iter_mut().enumerate() {
            *item = (i * 7 + 3) as u32;
        }

        let norm_freq = normalize_frequencies(&raw_counts, alphabet_size, ANS_TABLE_SIZE);
        let enc_table = AnsEncodeTable::build(&norm_freq).unwrap();
        let dec_table = AnsDecodeTable::build(&norm_freq).unwrap();

        // Generate a sequence of 500 symbols
        let mut symbols = Vec::with_capacity(500);
        for i in 0..500 {
            symbols.push((i * 17) % alphabet_size);
        }

        // Encode in reverse
        let mut state = ANS_TABLE_SIZE as u16;
        let mut chunks = Vec::new();
        for &s in symbols.iter().rev() {
            let (nb_bits, bits_out) = enc_table.encode_symbol(s, &mut state);
            chunks.push(AnsBitChunk {
                bits: nb_bits,
                val: bits_out,
            });
        }

        let final_state = state;

        // Write chunks into bitstream in forward order
        let mut writer = BitWriter::new();
        for chunk in chunks.iter().rev() {
            writer.write_bits(chunk.val, chunk.bits);
        }
        let bytes = writer.into_bytes();

        // Decode forward
        let mut reader = BitReader::new(&bytes);
        let mut dec_state = final_state;
        let mut decoded = Vec::with_capacity(symbols.len());

        for _ in 0..symbols.len() {
            let sym = dec_table
                .decode_symbol(&mut dec_state, &mut reader)
                .unwrap() as usize;
            decoded.push(sym);
        }

        assert_eq!(decoded, symbols);
        assert_eq!(dec_state, ANS_TABLE_SIZE as u16);
    }
}
