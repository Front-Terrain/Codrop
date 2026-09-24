use crate::error::CodropError;

/// LZF: Codrop Fast LZ Block Codec
/// High-throughput byte-aligned LZ compression with zero bit-level entropy operations.
pub struct LzfCodec;

const MIN_MATCH: usize = 3;
const HASH_BITS: usize = 14; // 16,384 entries
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const HASH_PRIME: u32 = 0x9E3779B1;

#[inline(always)]
fn hash_4(bytes: &[u8]) -> usize {
    let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    ((val.wrapping_mul(HASH_PRIME)) >> (32 - HASH_BITS)) as usize & HASH_MASK
}

impl LzfCodec {
    /// Compress an input slice into LZF format
    pub fn encode(input: &[u8]) -> Vec<u8> {
        if input.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(input.len());
        let mut table = vec![0u32; HASH_SIZE];

        let mut ip = 0;
        let mut anchor = 0;
        let limit = if input.len() >= 4 { input.len() - 4 } else { 0 };

        while ip < limit {
            let h = hash_4(&input[ip..ip + 4]);
            let ref_pos = table[h] as usize;
            table[h] = ip as u32;

            let offset = ip.saturating_sub(ref_pos);
            if offset > 0 && offset <= 65535 && ref_pos + 3 <= input.len() {
                // Check if at least MIN_MATCH bytes match
                if input[ref_pos] == input[ip]
                    && input[ref_pos + 1] == input[ip + 1]
                    && input[ref_pos + 2] == input[ip + 2]
                {
                    // Match found! Determine match length
                    let mut match_len = 3;
                    while ip + match_len < input.len()
                        && input[ref_pos + match_len] == input[ip + match_len]
                        && match_len < 65535
                    {
                        match_len += 1;
                    }

                    // Emit token: literals first, then match
                    let lit_len = ip - anchor;
                    Self::emit_sequence(&mut out, &input[anchor..ip], lit_len, offset, match_len);

                    ip += match_len;
                    anchor = ip;
                    continue;
                }
            }

            ip += 1;
        }

        // Emit trailing literals (match_len = 0, offset = 0)
        let lit_len = input.len() - anchor;
        if lit_len > 0 {
            Self::emit_sequence(&mut out, &input[anchor..input.len()], lit_len, 0, 0);
        }

        out
    }

    fn emit_sequence(out: &mut Vec<u8>, literals: &[u8], lit_len: usize, offset: usize, match_len: usize) {
        let token_lit = lit_len.min(15) as u8;
        let token_match = if match_len >= MIN_MATCH {
            (match_len - MIN_MATCH).min(15) as u8
        } else {
            0
        };

        // 1. Token byte
        out.push((token_lit << 4) | (token_match & 0x0F));

        // 2. Extended literal length if >= 15
        if lit_len >= 15 {
            let mut remaining = lit_len - 15;
            while remaining >= 255 {
                out.push(255);
                remaining -= 255;
            }
            out.push(remaining as u8);
        }

        // 3. Literals
        out.extend_from_slice(literals);

        // If this is a trailing sequence without a match, we are done
        if match_len == 0 {
            return;
        }

        // 4. Extended match length if >= 15
        if match_len - MIN_MATCH >= 15 {
            let mut remaining = match_len - MIN_MATCH - 15;
            while remaining >= 255 {
                out.push(255);
                remaining -= 255;
            }
            out.push(remaining as u8);
        }

        // 5. Offset (16-bit LE)
        out.extend_from_slice(&(offset as u16).to_le_bytes());
    }

    /// Decompress LZF encoded buffer
    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        let mut out = Vec::with_capacity(expected_len);
        let mut ip = 0;

        while ip < compressed.len() {
            let token = compressed[ip];
            ip += 1;

            let mut lit_len = (token >> 4) as usize;
            if lit_len == 15 {
                loop {
                    if ip >= compressed.len() {
                        return Err(CodropError::UnexpectedEof);
                    }
                    let extra = compressed[ip] as usize;
                    ip += 1;
                    lit_len += extra;
                    if extra < 255 {
                        break;
                    }
                }
            }

            // Copy literals
            if ip + lit_len > compressed.len() {
                return Err(CodropError::UnexpectedEof);
            }
            if out.len() + lit_len > expected_len {
                return Err(CodropError::InvalidMatchLength {
                    length: lit_len,
                    remaining: expected_len.saturating_sub(out.len()),
                });
            }
            out.extend_from_slice(&compressed[ip..ip + lit_len]);
            ip += lit_len;

            // If we have fulfilled expected_len and reached end of compressed stream, break
            if ip == compressed.len() && out.len() == expected_len {
                break;
            }

            let match_len_code = (token & 0x0F) as usize;
            if match_len_code == 0 && (token & 0x0F) == 0 && ip == compressed.len() {
                // Trailing literals sequence with no match
                break;
            }

            let mut match_len = match_len_code + MIN_MATCH;
            if match_len_code == 15 {
                loop {
                    if ip >= compressed.len() {
                        return Err(CodropError::UnexpectedEof);
                    }
                    let extra = compressed[ip] as usize;
                    ip += 1;
                    match_len += extra;
                    if extra < 255 {
                        break;
                    }
                }
            }

            // Read offset (16-bit LE)
            if ip + 2 > compressed.len() {
                return Err(CodropError::UnexpectedEof);
            }
            let offset = u16::from_le_bytes([compressed[ip], compressed[ip + 1]]) as usize;
            ip += 2;

            if offset == 0 || offset > out.len() {
                return Err(CodropError::InvalidOffset {
                    offset,
                    max_valid: out.len(),
                });
            }

            if out.len() + match_len > expected_len {
                return Err(CodropError::InvalidMatchLength {
                    length: match_len,
                    remaining: expected_len.saturating_sub(out.len()),
                });
            }

            // Copy overlapping match bytes safely
            let start = out.len() - offset;
            for i in 0..match_len {
                let byte = out[start + i];
                out.push(byte);
            }
        }

        if out.len() != expected_len {
            return Err(CodropError::CorruptedEntropyStream(format!(
                "LZF decoded length mismatch: got {} expected {}",
                out.len(),
                expected_len
            )));
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lzf_roundtrip_text() {
        let text = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog again and again!";
        let compressed = LzfCodec::encode(text);
        assert!(compressed.len() < text.len(), "Compressed size {} should be < {}", compressed.len(), text.len());
        let decompressed = LzfCodec::decode(&compressed, text.len()).unwrap();
        assert_eq!(decompressed, text);
    }

    #[test]
    fn test_lzf_roundtrip_repetitions() {
        let data = "ABCDABCDABCDABCDABCDABCDABCD12345678901234567890ABCDABCDABCDABCD".as_bytes();
        let compressed = LzfCodec::encode(data);
        let decompressed = LzfCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lzf_invalid_offset_protection() {
        // Construct a token that claims a match with offset > history
        let bad = vec![0x05, 0x10, 0x00]; // token with match_len 8, offset 16 on empty history
        let err = LzfCodec::decode(&bad, 8).unwrap_err();
        assert!(matches!(err, CodropError::InvalidOffset { .. }));
    }
}
