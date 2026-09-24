use crate::error::CodropError;

/// Number of bits for the match finder hash table.
/// 14 bits = 16,384 entries (64 KB memory footprint, fits in CPU L1/L2 cache).
pub const HASH_BITS: usize = 14;
pub const HASH_SIZE: usize = 1 << HASH_BITS;
pub const HASH_MASK: u32 = (HASH_SIZE - 1) as u32;

/// Maximum offset for 16-bit displacement (65,535 bytes).
pub const MAX_OFFSET_16: usize = 0xFFFF;
/// Maximum offset for 24-bit displacement (16,777,215 bytes).
pub const MAX_OFFSET_24: usize = 0xFF_FFFF;

/// Configured offset representation for LZF matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OffsetMode {
    #[default]
    Offset16,
    Offset24,
}

/// Fast deterministic multiplicative hash for 4-byte sequences.
#[inline(always)]
fn hash4(bytes: &[u8]) -> usize {
    let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    // Knuth's multiplicative hash constant (golden ratio approximation 2^32 / phi)
    let h = val.wrapping_mul(0x9E37_79B1);
    ((h >> (32 - HASH_BITS)) & HASH_MASK) as usize
}

/// Encodes literal length into a 4-bit nibble (0..15) and appends any extended length bytes into `out`.
pub fn encode_literal_length(len: usize, out: &mut Vec<u8>) -> u8 {
    if len < 15 {
        len as u8
    } else {
        let mut rem = len - 15;
        while rem >= 255 {
            out.push(255);
            rem -= 255;
        }
        out.push(rem as u8);
        15
    }
}

/// Decodes literal length given the 4-bit nibble from the token and a cursor into the compressed buffer.
pub fn decode_literal_length(
    nibble: u8,
    cursor: &mut usize,
    input: &[u8],
    max_allowed: usize,
) -> Result<usize, CodropError> {
    if nibble < 15 {
        let len = nibble as usize;
        if len > max_allowed {
            return Err(CodropError::InvalidMatchLength {
                length: len,
                remaining: max_allowed,
            });
        }
        return Ok(len);
    }

    let mut len: usize = 15;
    loop {
        if *cursor >= input.len() {
            return Err(CodropError::UnexpectedEof);
        }
        let b = input[*cursor];
        *cursor += 1;
        len = len
            .checked_add(b as usize)
            .ok_or_else(|| CodropError::CorruptedEntropyStream("Literal length overflow".into()))?;
        if len > max_allowed {
            return Err(CodropError::MemoryLimitExceeded {
                limit: max_allowed,
                requested: len,
            });
        }
        if b < 255 {
            break;
        }
    }
    Ok(len)
}

/// Encodes match length (which must be >= 3) into a 4-bit nibble (0..15) and appends any extended length bytes into `out`.
pub fn encode_match_length(len: usize, out: &mut Vec<u8>) -> Result<u8, CodropError> {
    if len < 3 {
        return Err(CodropError::InvalidMatchLength {
            length: len,
            remaining: 3,
        });
    }
    let m = len - 3;
    if m < 15 {
        Ok(m as u8)
    } else {
        let mut rem = len - 18;
        while rem >= 255 {
            out.push(255);
            rem -= 255;
        }
        out.push(rem as u8);
        Ok(15)
    }
}

/// Decodes match length given the 4-bit nibble from the token and a cursor into the compressed buffer.
pub fn decode_match_length(
    nibble: u8,
    cursor: &mut usize,
    input: &[u8],
    max_allowed: usize,
) -> Result<usize, CodropError> {
    if nibble < 15 {
        let len = (nibble as usize) + 3;
        if len > max_allowed {
            return Err(CodropError::InvalidMatchLength {
                length: len,
                remaining: max_allowed,
            });
        }
        return Ok(len);
    }

    let mut len: usize = 18;
    loop {
        if *cursor >= input.len() {
            return Err(CodropError::UnexpectedEof);
        }
        let b = input[*cursor];
        *cursor += 1;
        len = len
            .checked_add(b as usize)
            .ok_or_else(|| CodropError::CorruptedEntropyStream("Match length overflow".into()))?;
        if len > max_allowed {
            return Err(CodropError::InvalidMatchLength {
                length: len,
                remaining: max_allowed,
            });
        }
        if b < 255 {
            break;
        }
    }
    Ok(len)
}

/// LZF byte-oriented fast LZ codec.
pub struct LzfCodec;

impl LzfCodec {
    /// Encode data using standard 16-bit relative match offsets.
    pub fn encode(data: &[u8]) -> Vec<u8> {
        Self::encode_with_offset_mode(data, OffsetMode::Offset16)
    }

    /// Encode data using the specified offset mode (16-bit or 24-bit).
    pub fn encode_with_offset_mode(data: &[u8], offset_mode: OffsetMode) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }

        let max_offset = match offset_mode {
            OffsetMode::Offset16 => MAX_OFFSET_16,
            OffsetMode::Offset24 => MAX_OFFSET_24,
        };

        if data.len() < 4 {
            let mut out = Vec::with_capacity(data.len() + 4);
            let mut extra_lit = Vec::new();
            let lit_nibble = encode_literal_length(data.len(), &mut extra_lit);
            let token = lit_nibble << 4;
            out.push(token);
            out.extend_from_slice(&extra_lit);
            out.extend_from_slice(data);
            return out;
        }

        let mut out = Vec::with_capacity(data.len() / 2 + 16);
        let mut table = vec![0u32; HASH_SIZE];
        let mut ip = 0;
        let mut anchor = 0;

        while ip + 4 <= data.len() {
            let h = hash4(&data[ip..ip + 4]);
            let prev_val = table[h];
            table[h] = (ip + 1) as u32;

            if prev_val != 0 {
                let ref_pos = (prev_val - 1) as usize;
                let dist = ip - ref_pos;
                if ref_pos < ip && dist <= max_offset {
                    // Check if at least 3 bytes match (format allows min match length 3)
                    if data[ref_pos] == data[ip]
                        && data[ref_pos + 1] == data[ip + 1]
                        && data[ref_pos + 2] == data[ip + 2]
                    {
                        let mut match_len = 3;
                        while ip + match_len + 8 <= data.len()
                            && ref_pos + match_len + 8 <= data.len()
                        {
                            let w1 = u64::from_le_bytes(
                                data[ref_pos + match_len..ref_pos + match_len + 8]
                                    .try_into()
                                    .unwrap(),
                            );
                            let w2 = u64::from_le_bytes(
                                data[ip + match_len..ip + match_len + 8].try_into().unwrap(),
                            );
                            if w1 == w2 {
                                match_len += 8;
                            } else {
                                let diff = w1 ^ w2;
                                match_len += (diff.trailing_zeros() / 8) as usize;
                                break;
                            }
                        }
                        while ip + match_len < data.len()
                            && data[ref_pos + match_len] == data[ip + match_len]
                        {
                            match_len += 1;
                        }

                        // Emit pending literals and match without temporary allocations
                        let lit_len = ip - anchor;
                        let lit_nibble = if lit_len < 15 { lit_len as u8 } else { 15 };
                        let match_nibble = if match_len < 18 {
                            (match_len - 3) as u8
                        } else {
                            15
                        };

                        let token = (lit_nibble << 4) | match_nibble;
                        out.push(token);

                        if lit_nibble == 15 {
                            let mut rem = lit_len - 15;
                            while rem >= 255 {
                                out.push(255);
                                rem -= 255;
                            }
                            out.push(rem as u8);
                        }
                        out.extend_from_slice(&data[anchor..ip]);

                        if match_nibble == 15 {
                            let mut rem = match_len - 18;
                            while rem >= 255 {
                                out.push(255);
                                rem -= 255;
                            }
                            out.push(rem as u8);
                        }

                        match offset_mode {
                            OffsetMode::Offset16 => {
                                let offset = dist as u16;
                                out.extend_from_slice(&offset.to_le_bytes());
                            }
                            OffsetMode::Offset24 => {
                                let offset = dist as u32;
                                out.push(offset as u8);
                                out.push((offset >> 8) as u8);
                                out.push((offset >> 16) as u8);
                            }
                        }

                        ip += match_len;
                        anchor = ip;
                        continue;
                    }
                }
            }
            ip += 1;
        }

        // Emit any remaining literals
        let remaining_lit_len = data.len() - anchor;
        if remaining_lit_len > 0 {
            let lit_nibble = if remaining_lit_len < 15 {
                remaining_lit_len as u8
            } else {
                15
            };
            let token = lit_nibble << 4;
            out.push(token);
            if lit_nibble == 15 {
                let mut rem = remaining_lit_len - 15;
                while rem >= 255 {
                    out.push(255);
                    rem -= 255;
                }
                out.push(rem as u8);
            }
            out.extend_from_slice(&data[anchor..]);
        }

        out
    }

    /// Decode compressed LZF payload with standard 16-bit match offsets.
    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        Self::decode_with_offset_mode(compressed, expected_len, OffsetMode::Offset16)
    }

    /// Decode compressed LZF payload with the specified offset mode (16-bit or 24-bit).
    pub fn decode_with_offset_mode(
        compressed: &[u8],
        expected_len: usize,
        offset_mode: OffsetMode,
    ) -> Result<Vec<u8>, CodropError> {
        if expected_len == 0 {
            if compressed.is_empty() {
                return Ok(Vec::new());
            } else {
                return Err(CodropError::CorruptedEntropyStream(
                    "Non-empty compressed data for 0 expected length".into(),
                ));
            }
        }

        let mut out = Vec::with_capacity(expected_len);
        let mut cursor = 0;

        while cursor < compressed.len() {
            // Step 1: Read token
            let token = compressed[cursor];
            cursor += 1;

            let lit_nibble = (token >> 4) & 0x0F;
            let match_nibble = token & 0x0F;

            // Step 2: Decode literal length
            let max_lit_allowed = expected_len.saturating_sub(out.len());
            let lit_len =
                decode_literal_length(lit_nibble, &mut cursor, compressed, max_lit_allowed)?;

            // Step 3: Copy literals
            if cursor + lit_len > compressed.len() {
                return Err(CodropError::UnexpectedEof);
            }
            out.extend_from_slice(&compressed[cursor..cursor + lit_len]);
            cursor += lit_len;

            // Check if block has completed (trailing literals at end of block)
            if out.len() == expected_len {
                if cursor == compressed.len() {
                    break;
                } else {
                    return Err(CodropError::CorruptedEntropyStream(
                        "Trailing bytes after expected uncompressed size reached".into(),
                    ));
                }
            }

            // If we still need uncompressed bytes, but compressed input is exhausted:
            if cursor >= compressed.len() {
                return Err(CodropError::UnexpectedEof);
            }

            // Step 4: Decode match length
            let max_match_allowed = expected_len.saturating_sub(out.len());
            let match_len =
                decode_match_length(match_nibble, &mut cursor, compressed, max_match_allowed)?;

            // Step 5: Decode match offset
            let offset = match offset_mode {
                OffsetMode::Offset16 => {
                    if cursor + 2 > compressed.len() {
                        return Err(CodropError::UnexpectedEof);
                    }
                    let off =
                        u16::from_le_bytes([compressed[cursor], compressed[cursor + 1]]) as usize;
                    cursor += 2;
                    off
                }
                OffsetMode::Offset24 => {
                    if cursor + 3 > compressed.len() {
                        return Err(CodropError::UnexpectedEof);
                    }
                    let off = (compressed[cursor] as usize)
                        | ((compressed[cursor + 1] as usize) << 8)
                        | ((compressed[cursor + 2] as usize) << 16);
                    cursor += 3;
                    off
                }
            };

            // Step 6: Validate offset
            if offset == 0 || offset > out.len() {
                return Err(CodropError::InvalidOffset {
                    offset,
                    max_valid: out.len(),
                });
            }

            // Step 7 & 8: Copy match from history with safe overlapping support
            let start = out.len() - offset;
            if offset >= match_len {
                out.extend_from_within(start..start + match_len);
            } else {
                for i in 0..match_len {
                    let b = out[start + i];
                    out.push(b);
                }
            }

            // Check if block has completed after match
            if out.len() == expected_len {
                if cursor == compressed.len() {
                    break;
                } else {
                    return Err(CodropError::CorruptedEntropyStream(
                        "Trailing bytes after expected uncompressed size reached".into(),
                    ));
                }
            }
        }

        if out.len() != expected_len {
            return Err(CodropError::UnexpectedEof);
        }
        if cursor != compressed.len() {
            return Err(CodropError::CorruptedEntropyStream(
                "Unconsumed trailing bytes in LZF block".into(),
            ));
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_length_boundaries() {
        let test_cases = [0, 1, 14, 15, 16, 254, 255, 269, 270, 524, 525, 1000];
        for &len in &test_cases {
            let mut extra = Vec::new();
            let nibble = encode_literal_length(len, &mut extra);
            let mut cursor = 0;
            let decoded = decode_literal_length(nibble, &mut cursor, &extra, len + 10).unwrap();
            assert_eq!(decoded, len, "Failed for literal length {}", len);
            assert_eq!(cursor, extra.len());
        }
    }

    #[test]
    fn test_match_length_boundaries() {
        assert!(encode_match_length(0, &mut Vec::new()).is_err());
        assert!(encode_match_length(1, &mut Vec::new()).is_err());
        assert!(encode_match_length(2, &mut Vec::new()).is_err());

        let test_cases = [3, 4, 17, 18, 19, 255, 272, 273, 527, 528, 1000];
        for &len in &test_cases {
            let mut extra = Vec::new();
            let nibble = encode_match_length(len, &mut extra).unwrap();
            let mut cursor = 0;
            let decoded = decode_match_length(nibble, &mut cursor, &extra, len + 10).unwrap();
            assert_eq!(decoded, len, "Failed for match length {}", len);
            assert_eq!(cursor, extra.len());
        }
    }

    #[test]
    fn test_empty_and_small_inputs() {
        for size in 0..=16 {
            let data = vec![0x42; size];
            let compressed = LzfCodec::encode(&data);
            let decompressed = LzfCodec::decode(&compressed, data.len()).unwrap();
            assert_eq!(decompressed, data, "Failed for size {}", size);
        }
    }

    #[test]
    fn test_overlapping_matches() {
        // Distance 1 (repeated single byte)
        let data = vec![b'A'; 200];
        let compressed = LzfCodec::encode(&data);
        assert!(compressed.len() < data.len());
        let decompressed = LzfCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);

        // Distance 2 (repeated pattern "AB")
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"AB");
        }
        let compressed = LzfCodec::encode(&data);
        assert!(compressed.len() < data.len());
        let decompressed = LzfCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);

        // Distance 3 (repeated pattern "ABC")
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"ABC");
        }
        let compressed = LzfCodec::encode(&data);
        assert!(compressed.len() < data.len());
        let decompressed = LzfCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_mixed_literals_and_matches() {
        let text = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog!";
        let compressed = LzfCodec::encode(text);
        assert!(compressed.len() < text.len());
        let decompressed = LzfCodec::decode(&compressed, text.len()).unwrap();
        assert_eq!(decompressed, text);
    }

    #[test]
    fn test_24bit_offset_roundtrip() {
        let mut data = Vec::new();
        for _ in 0..50 {
            data.extend_from_slice(b"Codrop Universal Adaptive Compression System! ");
        }
        let compressed = LzfCodec::encode_with_offset_mode(&data, OffsetMode::Offset24);
        let decompressed =
            LzfCodec::decode_with_offset_mode(&compressed, data.len(), OffsetMode::Offset24)
                .unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_malformed_offset_zero() {
        // Construct token with lit=0, match=3, offset=0
        let token = 0x00; // lit=0, match=0 (len 3)
        let offset = 0u16.to_le_bytes();
        let payload = [token, offset[0], offset[1]];
        let err = LzfCodec::decode(&payload, 3).unwrap_err();
        match err {
            CodropError::InvalidOffset { offset: 0, .. } => {}
            other => panic!("Expected InvalidOffset with 0, got {:?}", other),
        }
    }

    #[test]
    fn test_malformed_offset_beyond_history() {
        // Construct token with lit=1 (byte 'A'), match=3, offset=5 (history has only 1 byte!)
        let token = 1 << 4; // lit=1, match=0 (len 3)
        let offset = 5u16.to_le_bytes();
        let payload = [token, b'A', offset[0], offset[1]];
        let err = LzfCodec::decode(&payload, 4).unwrap_err();
        match err {
            CodropError::InvalidOffset {
                offset: 5,
                max_valid: 1,
            } => {}
            other => panic!("Expected InvalidOffset with 5 > 1, got {:?}", other),
        }
    }

    #[test]
    fn test_malformed_truncated_offset() {
        let token = 0x00; // lit=0, match=0
        let payload = [token, 0x01]; // only 1 offset byte instead of 2
        let err = LzfCodec::decode(&payload, 3).unwrap_err();
        assert_eq!(err, CodropError::UnexpectedEof);
    }

    #[test]
    fn test_malformed_trailing_bytes() {
        let data = b"hello";
        let mut compressed = LzfCodec::encode(data);
        compressed.push(0xFF); // trailing garbage
        let err = LzfCodec::decode(&compressed, data.len()).unwrap_err();
        match err {
            CodropError::CorruptedEntropyStream(_) => {}
            other => panic!("Expected CorruptedEntropyStream, got {:?}", other),
        }
    }

    #[test]
    fn test_deterministic_output() {
        let data = b"Codrop deterministic LZF output test across repeated runs 123456789!";
        let run1 = LzfCodec::encode(data);
        let run2 = LzfCodec::encode(data);
        assert_eq!(run1, run2);
    }
}
