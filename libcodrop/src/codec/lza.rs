use crate::codec::lzh::{
    decode_distance, decode_length, encode_distance, encode_length, tokenize_lz, LzSymbol,
};
use crate::entropy::ans::{
    normalize_frequencies, AnsBitChunk, AnsDecodeTable, AnsEncodeTable, ANS_TABLE_SIZE,
};
use crate::entropy::bitstream::{BitReader, BitWriter};
use crate::error::CodropError;

pub const ANS_ALPHABET_SIZE: usize = 318;
pub const EOB_SYMBOL: usize = 256;

/// Serialize normalized frequencies using RLE encoding into `out`.
pub fn serialize_ans_descriptors(norm_freq: &[u16], out: &mut Vec<u8>) {
    let mut max_symbol = 0usize;
    for (s, &f) in norm_freq.iter().enumerate() {
        if f > 0 {
            max_symbol = s;
        }
    }

    out.push((max_symbol & 0xFF) as u8);
    out.push(((max_symbol >> 8) & 0xFF) as u8);

    let active = &norm_freq[..=max_symbol];
    let mut i = 0;
    while i < active.len() {
        let val = active[i];
        if val == 0 {
            let mut run = 1usize;
            while i + run < active.len() && active[i + run] == 0 && run < 255 {
                run += 1;
            }
            out.push(0);
            out.push(run as u8);
            i += run;
        } else {
            // Write non-zero value using 1 or 2 bytes
            if val < 128 {
                out.push(val as u8);
            } else {
                out.push(0x80 | ((val & 0x7F) as u8));
                out.push((val >> 7) as u8);
            }
            i += 1;
        }
    }
}

/// Deserialize normalized frequencies from `input[cursor..]`.
pub fn deserialize_ans_descriptors(
    input: &[u8],
    cursor: &mut usize,
) -> Result<Vec<u16>, CodropError> {
    if *cursor + 2 > input.len() {
        return Err(CodropError::UnexpectedEof);
    }

    let max_symbol = (input[*cursor] as usize) | ((input[*cursor + 1] as usize) << 8);
    *cursor += 2;

    if max_symbol >= ANS_ALPHABET_SIZE {
        return Err(CodropError::CorruptedEntropyStream(
            "Invalid max_symbol in LZA table descriptor".into(),
        ));
    }

    let total_symbols = max_symbol + 1;
    let mut norm_freq = vec![0u16; ANS_ALPHABET_SIZE];
    let mut decoded = 0usize;

    while decoded < total_symbols {
        if *cursor >= input.len() {
            return Err(CodropError::UnexpectedEof);
        }
        let b = input[*cursor];
        *cursor += 1;

        if b == 0 {
            if *cursor >= input.len() {
                return Err(CodropError::UnexpectedEof);
            }
            let run = input[*cursor] as usize;
            *cursor += 1;
            if run == 0 || decoded + run > total_symbols {
                return Err(CodropError::CorruptedEntropyStream(
                    "Invalid RLE zero run in LZA table descriptor".into(),
                ));
            }
            // norm_freq is already initialized to zeros
            decoded += run;
        } else if b < 128 {
            norm_freq[decoded] = b as u16;
            decoded += 1;
        } else {
            if *cursor >= input.len() {
                return Err(CodropError::UnexpectedEof);
            }
            let high = input[*cursor] as u16;
            *cursor += 1;
            let val = ((b & 0x7F) as u16) | (high << 7);
            if val > ANS_TABLE_SIZE as u16 {
                return Err(CodropError::CorruptedEntropyStream(
                    "Normalized frequency exceeds ANS table size".into(),
                ));
            }
            norm_freq[decoded] = val;
            decoded += 1;
        }
    }

    let sum: usize = norm_freq.iter().map(|&f| f as usize).sum();
    if sum != ANS_TABLE_SIZE {
        return Err(CodropError::CorruptedEntropyStream(format!(
            "Normalized frequency sum {} != expected {}",
            sum, ANS_TABLE_SIZE
        )));
    }

    Ok(norm_freq)
}

/// LZA compression codec (Block Type 4): LZ matching + table-based ANS / FSE entropy coding.
pub struct LzaCodec;

impl LzaCodec {
    /// Compresses input data into an LZA payload.
    pub fn encode(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }

        // 1. LZ Tokenization
        let symbols = tokenize_lz(data);

        // 2. Frequency counting
        let mut freq = [0u32; ANS_ALPHABET_SIZE];
        for &sym in &symbols {
            match sym {
                LzSymbol::Literal(b) => {
                    freq[b as usize] += 1;
                }
                LzSymbol::Match { length, distance } => {
                    let (len_sym, _, _) = encode_length(length);
                    freq[len_sym] += 1;
                    let (dist_code, _, _) = encode_distance(distance);
                    freq[286 + dist_code] += 1;
                }
            }
        }
        freq[EOB_SYMBOL] = 1;

        // 3. Frequency normalization & ANS table construction
        let norm_freq = normalize_frequencies(&freq, ANS_ALPHABET_SIZE, ANS_TABLE_SIZE);
        let enc_table = match AnsEncodeTable::build(&norm_freq) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        // 4. Reverse ANS encoding
        let mut state = ANS_TABLE_SIZE as u16;
        let mut chunks = Vec::with_capacity(symbols.len() * 2 + 16);

        // Step EOB
        let (nb, bits) = enc_table.encode_symbol(EOB_SYMBOL, &mut state);
        chunks.push(AnsBitChunk {
            bits: nb,
            val: bits,
        });

        // Iterate symbols in reverse
        for &sym in symbols.iter().rev() {
            match sym {
                LzSymbol::Literal(b) => {
                    let (nb, bits) = enc_table.encode_symbol(b as usize, &mut state);
                    chunks.push(AnsBitChunk {
                        bits: nb,
                        val: bits,
                    });
                }
                LzSymbol::Match { length, distance } => {
                    // Distance extra bits + symbol
                    let (dist_code, dist_extra_bits, dist_extra_val) = encode_distance(distance);
                    if dist_extra_bits > 0 {
                        chunks.push(AnsBitChunk {
                            bits: dist_extra_bits,
                            val: dist_extra_val,
                        });
                    }
                    let (nb, bits) = enc_table.encode_symbol(286 + dist_code, &mut state);
                    chunks.push(AnsBitChunk {
                        bits: nb,
                        val: bits,
                    });

                    // Length extra bits + symbol
                    let (len_sym, len_extra_bits, len_extra_val) = encode_length(length);
                    if len_extra_bits > 0 {
                        chunks.push(AnsBitChunk {
                            bits: len_extra_bits,
                            val: len_extra_val,
                        });
                    }
                    let (nb, bits) = enc_table.encode_symbol(len_sym, &mut state);
                    chunks.push(AnsBitChunk {
                        bits: nb,
                        val: bits,
                    });
                }
            }
        }

        let initial_state = state;

        // 5. Serialize table descriptors
        let mut payload = Vec::with_capacity(symbols.len() + 128);
        serialize_ans_descriptors(&norm_freq, &mut payload);

        // 6. Write initial state & bitstream in forward order
        let mut writer = BitWriter::new();
        writer.write_bits(initial_state as u32, 16);
        for chunk in chunks.iter().rev() {
            writer.write_bits(chunk.val, chunk.bits);
        }

        payload.extend_from_slice(&writer.into_bytes());
        payload
    }

    /// Decompresses an LZA payload into original uncompressed bytes.
    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        if expected_len == 0 {
            return Ok(Vec::new());
        }
        if compressed.is_empty() {
            return Err(CodropError::UnexpectedEof);
        }

        let mut cursor = 0;
        let norm_freq = deserialize_ans_descriptors(compressed, &mut cursor)?;
        let dec_table = AnsDecodeTable::build(&norm_freq)?;

        let mut bitreader = BitReader::new(&compressed[cursor..]);
        let mut state = bitreader.read_bits(16)? as u16;
        if state < ANS_TABLE_SIZE as u16 || state >= (2 * ANS_TABLE_SIZE) as u16 {
            return Err(CodropError::CorruptedEntropyStream(
                "Invalid initial ANS state".into(),
            ));
        }

        let mut out = Vec::with_capacity(expected_len);

        loop {
            let sym = dec_table.decode_symbol(&mut state, &mut bitreader)? as usize;
            if sym < 256 {
                if out.len() >= expected_len {
                    return Err(CodropError::InvalidMatchLength {
                        length: 1,
                        remaining: 0,
                    });
                }
                out.push(sym as u8);
            } else if sym == EOB_SYMBOL {
                break;
            } else if (257..=285).contains(&sym) {
                let match_len = decode_length(sym, &mut bitreader)?;
                let remaining = expected_len.saturating_sub(out.len());
                if match_len > remaining {
                    return Err(CodropError::InvalidMatchLength {
                        length: match_len,
                        remaining,
                    });
                }

                let dist_sym = dec_table.decode_symbol(&mut state, &mut bitreader)? as usize;
                if !(286..=317).contains(&dist_sym) {
                    return Err(CodropError::CorruptedEntropyStream(
                        "Invalid distance symbol in LZA stream".into(),
                    ));
                }
                let dist_code = dist_sym - 286;
                let distance = decode_distance(dist_code, &mut bitreader)?;

                if distance == 0 || distance > out.len() {
                    return Err(CodropError::InvalidOffset {
                        offset: distance,
                        max_valid: out.len(),
                    });
                }

                // Copy match from history with safe overlapping support
                let start = out.len() - distance;
                if distance >= match_len {
                    out.extend_from_within(start..start + match_len);
                } else {
                    for i in 0..match_len {
                        let b = out[start + i];
                        out.push(b);
                    }
                }
            } else {
                return Err(CodropError::CorruptedEntropyStream(
                    "Unexpected symbol in LZA stream".into(),
                ));
            }
        }

        if out.len() != expected_len {
            return Err(CodropError::UnexpectedEof);
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lza_roundtrip_simple() {
        let text = b"The quick brown fox jumps over the lazy dog! The quick brown fox jumps over the lazy dog!";
        let compressed = LzaCodec::encode(text);
        let decompressed = LzaCodec::decode(&compressed, text.len()).unwrap();
        assert_eq!(decompressed, text);
    }

    #[test]
    fn test_lza_roundtrip_empty_and_small() {
        for len in 0..=20 {
            let data = vec![b'y'; len];
            let compressed = LzaCodec::encode(&data);
            let decompressed = LzaCodec::decode(&compressed, data.len()).unwrap();
            assert_eq!(decompressed, data, "Failed for length {}", len);
        }
    }

    #[test]
    fn test_lza_overlapping_matches() {
        let data = vec![b'K'; 500];
        let compressed = LzaCodec::encode(&data);
        assert!(compressed.len() < data.len());
        let decompressed = LzaCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lza_golden_vector() {
        let original = b"CODROP_LZA_GOLDEN_VECTOR_VERIFICATION_2026";
        let compressed = LzaCodec::encode(original);
        let decompressed = LzaCodec::decode(&compressed, original.len()).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_lza_malformed_initial_state() {
        let text = b"Testing initial ANS state boundaries";
        let mut compressed = LzaCodec::encode(text);
        // Find cursor where bitstream begins (after table descriptors)
        let mut cursor = 0;
        let _ = deserialize_ans_descriptors(&compressed, &mut cursor).unwrap();

        // Mutate the 16 bits of initial state to be 0 (which is < ANS_TABLE_SIZE)
        compressed[cursor] = 0;
        compressed[cursor + 1] = 0;

        let err = LzaCodec::decode(&compressed, text.len()).unwrap_err();
        assert!(matches!(err, CodropError::CorruptedEntropyStream(_)));
    }

    #[test]
    fn test_lza_malformed_frequency_sum_mismatch() {
        let text = b"Frequency sum corruption test";
        let mut compressed = LzaCodec::encode(text);
        // Mutate a byte in the table descriptor so the sum changes
        compressed[3] = compressed[3].wrapping_add(5);
        let err = LzaCodec::decode(&compressed, text.len()).unwrap_err();
        assert!(matches!(
            err,
            CodropError::CorruptedEntropyStream(_) | CodropError::UnexpectedEof
        ));
    }

    #[test]
    fn test_lza_large_buffers() {
        for &size in &[1024, 32 * 1024, 64 * 1024] {
            let mut buf = Vec::with_capacity(size);
            for i in 0..size {
                buf.push((i % 251) as u8);
            }
            let c = LzaCodec::encode(&buf);
            let d = LzaCodec::decode(&c, buf.len()).unwrap();
            assert_eq!(d, buf, "Failed for size {}", size);
        }
    }
}
