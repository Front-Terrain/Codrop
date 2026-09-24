use crate::entropy::huffman::{BitReader, BitWriter, CanonicalHuffman};
use crate::error::CodropError;

pub struct LzhCodec;

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const EOB_SYMBOL: usize = 256;
const NUM_SYMBOLS: usize = 286; // 0..255 literals, 256 EOB, 257..285 length codes
const HASH_SIZE: usize = 16384;
const HASH_MASK: usize = HASH_SIZE - 1;

// Length code base and extra bits table
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

// Offset code base and extra bits table
const OFFSET_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const OFFSET_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

enum Token {
    Literal(u8),
    Match { length: u16, offset: u16 },
}

impl LzhCodec {
    pub fn encode(input: &[u8]) -> Vec<u8> {
        if input.is_empty() {
            return Vec::new();
        }

        // 1. Pass 1: Match Finding and Token Generation
        let mut tokens = Vec::with_capacity(input.len() / 2);
        let mut freqs = [0u32; NUM_SYMBOLS];
        freqs[EOB_SYMBOL] = 1;

        let mut head = vec![u32::MAX; HASH_SIZE];
        let mut prev = vec![u32::MAX; input.len()];

        let mut ip = 0;
        let limit = if input.len() >= 4 { input.len() - 4 } else { 0 };

        while ip < limit {
            let val = u32::from_le_bytes([input[ip], input[ip + 1], input[ip + 2], input[ip + 3]]);
            let h = ((val.wrapping_mul(0x9E3779B1)) >> 18) as usize & HASH_MASK;

            let mut ref_pos = head[h];
            head[h] = ip as u32;
            prev[ip] = ref_pos;

            let mut best_len = 0;
            let mut best_offset = 0;
            let mut depth = 0;

            while ref_pos != u32::MAX && depth < 32 {
                let r = ref_pos as usize;
                let offset = ip - r;
                if offset > 32768 {
                    break;
                }

                if input[r] == input[ip] && input[r + 1] == input[ip + 1] && input[r + 2] == input[ip + 2] {
                    let mut match_len = 3;
                    while ip + match_len < input.len()
                        && input[r + match_len] == input[ip + match_len]
                        && match_len < MAX_MATCH
                    {
                        match_len += 1;
                    }

                    if match_len > best_len {
                        best_len = match_len;
                        best_offset = offset;
                        if match_len >= 32 {
                            break;
                        }
                    }
                }

                ref_pos = prev[r];
                depth += 1;
            }

            if best_len >= MIN_MATCH {
                tokens.push(Token::Match {
                    length: best_len as u16,
                    offset: best_offset as u16,
                });
                let len_code = Self::get_length_code(best_len as u16);
                freqs[257 + len_code] += 1;
                ip += best_len;
            } else {
                tokens.push(Token::Literal(input[ip]));
                freqs[input[ip] as usize] += 1;
                ip += 1;
            }
        }

        // Tail literals
        while ip < input.len() {
            tokens.push(Token::Literal(input[ip]));
            freqs[input[ip] as usize] += 1;
            ip += 1;
        }

        // 2. Build Canonical Huffman Tree
        let huffman = CanonicalHuffman::from_frequencies(&freqs);

        // 3. Serialize Table Preamble
        let mut bit_writer = BitWriter::new();
        // Write lengths of all 286 symbols (4 bits each, max length 15)
        for &len in &huffman.code_lengths {
            bit_writer.write_bits(len as u32, 4);
        }

        // 4. Encode Tokens
        for token in tokens {
            match token {
                Token::Literal(lit) => {
                    let sym = lit as usize;
                    Self::write_huffman_code(&mut bit_writer, &huffman, sym);
                }
                Token::Match { length, offset } => {
                    let len_idx = Self::get_length_code(length);
                    let sym = 257 + len_idx;
                    Self::write_huffman_code(&mut bit_writer, &huffman, sym);
                    let extra_bits = LENGTH_EXTRA[len_idx] as usize;
                    if extra_bits > 0 {
                        let base = LENGTH_BASE[len_idx];
                        bit_writer.write_bits((length - base) as u32, extra_bits);
                    }

                    // Write offset code
                    let off_idx = Self::get_offset_code(offset);
                    bit_writer.write_bits(off_idx as u32, 5); // 0..29 fits in 5 bits
                    let off_extra = OFFSET_EXTRA[off_idx] as usize;
                    if off_extra > 0 {
                        let off_base = OFFSET_BASE[off_idx];
                        bit_writer.write_bits((offset - off_base) as u32, off_extra);
                    }
                }
            }
        }

        // Write EOB symbol
        Self::write_huffman_code(&mut bit_writer, &huffman, EOB_SYMBOL);
        bit_writer.flush();

        bit_writer.bytes
    }

    fn write_huffman_code(writer: &mut BitWriter, huffman: &CanonicalHuffman, sym: usize) {
        let len = huffman.code_lengths[sym] as usize;
        let code = huffman.codes[sym];
        let mut rev_code = 0u32;
        for i in 0..len {
            if (code & (1 << (len - 1 - i))) != 0 {
                rev_code |= 1 << i;
            }
        }
        writer.write_bits(rev_code, len);
    }

    fn get_length_code(length: u16) -> usize {
        for i in (0..29).rev() {
            if length >= LENGTH_BASE[i] {
                return i;
            }
        }
        0
    }

    fn get_offset_code(offset: u16) -> usize {
        for i in (0..30).rev() {
            if offset >= OFFSET_BASE[i] {
                return i;
            }
        }
        0
    }

    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        let mut reader = BitReader::new(compressed);

        // 1. Read Huffman lengths (286 symbols * 4 bits)
        let mut lengths = vec![0u8; NUM_SYMBOLS];
        for i in 0..NUM_SYMBOLS {
            lengths[i] = reader.read_bits(4) as u8;
        }

        let huffman = CanonicalHuffman::from_code_lengths(&lengths);
        let mut out = Vec::with_capacity(expected_len);

        loop {
            let sym = huffman.decode_symbol(&mut reader)?;
            if sym == EOB_SYMBOL {
                break;
            }

            if sym < 256 {
                if out.len() >= expected_len {
                    return Err(CodropError::InvalidMatchLength {
                        length: 1,
                        remaining: 0,
                    });
                }
                out.push(sym as u8);
            } else {
                let len_idx = sym - 257;
                if len_idx >= 29 {
                    return Err(CodropError::CorruptedEntropyStream("Invalid length symbol".into()));
                }
                let mut match_len = LENGTH_BASE[len_idx] as usize;
                let extra_len_bits = LENGTH_EXTRA[len_idx] as usize;
                if extra_len_bits > 0 {
                    match_len += reader.read_bits(extra_len_bits) as usize;
                }

                // Read offset
                let off_idx = reader.read_bits(5) as usize;
                if off_idx >= 30 {
                    return Err(CodropError::CorruptedEntropyStream("Invalid offset symbol".into()));
                }
                let mut offset = OFFSET_BASE[off_idx] as usize;
                let off_extra = OFFSET_EXTRA[off_idx] as usize;
                if off_extra > 0 {
                    offset += reader.read_bits(off_extra) as usize;
                }

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

                let start = out.len() - offset;
                for i in 0..match_len {
                    let byte = out[start + i];
                    out.push(byte);
                }
            }
        }

        if out.len() != expected_len {
            return Err(CodropError::CorruptedEntropyStream(format!(
                "LZH decoded length mismatch: got {} expected {}",
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
    fn test_lzh_roundtrip() {
        let data = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Lorem ipsum dolor sit amet!";
        let compressed = LzhCodec::encode(data);
        let decompressed = LzhCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }
}
