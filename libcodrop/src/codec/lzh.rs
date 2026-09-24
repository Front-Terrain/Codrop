use crate::entropy::bitstream::{BitReader, BitWriter};
use crate::entropy::huffman::{build_canonical_codes, build_code_lengths, HuffmanDecoderTable};
use crate::error::CodropError;

pub const LIT_LEN_ALPHABET_SIZE: usize = 286;
pub const DIST_ALPHABET_SIZE: usize = 32;
pub const EOB_SYMBOL: usize = 256;

const HASH_BITS: usize = 14;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: u32 = (HASH_SIZE - 1) as u32;
const MAX_OFFSET: usize = 65535;

/// Base match lengths for symbols 257..=285
pub const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Number of extra bits for length symbols 257..=285
pub const LENGTH_EXTRA_BITS: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 16,
];

/// Base match distances for symbols 0..=31
pub const DISTANCE_BASE: [usize; 32] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577, 32769, 49153,
];

/// Number of extra bits for distance symbols 0..=31
pub const DISTANCE_EXTRA_BITS: [u8; 32] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13, 14, 14,
];

/// Intermediate LZ representation before Huffman entropy coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LzSymbol {
    Literal(u8),
    Match { length: usize, distance: usize },
}

#[inline(always)]
fn hash4(bytes: &[u8]) -> usize {
    let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let h = val.wrapping_mul(0x9E37_79B1);
    ((h >> (32 - HASH_BITS)) & HASH_MASK) as usize
}

/// Encode match length (>= 3) into `(symbol, extra_bits, extra_val)`.
#[inline(always)]
pub fn encode_length(len: usize) -> (usize, u8, u32) {
    debug_assert!(len >= 3);
    let idx = LENGTH_BASE.partition_point(|&b| b <= len).saturating_sub(1);
    let extra_bits = LENGTH_EXTRA_BITS[idx];
    let extra_val = (len - LENGTH_BASE[idx]) as u32;
    (257 + idx, extra_bits, extra_val)
}

/// Decode match length from `symbol` and `reader`.
pub fn decode_length(symbol: usize, reader: &mut BitReader) -> Result<usize, CodropError> {
    if !(257..=285).contains(&symbol) {
        return Err(CodropError::CorruptedEntropyStream(
            "Invalid length symbol".into(),
        ));
    }
    let idx = symbol - 257;
    let base = LENGTH_BASE[idx];
    let extra_bits = LENGTH_EXTRA_BITS[idx];
    let extra = reader.read_bits(extra_bits)? as usize;
    Ok(base + extra)
}

/// Encode match distance (>= 1) into `(symbol, extra_bits, extra_val)`.
#[inline(always)]
pub fn encode_distance(dist: usize) -> (usize, u8, u32) {
    debug_assert!(dist >= 1);
    let idx = DISTANCE_BASE
        .partition_point(|&b| b <= dist)
        .saturating_sub(1);
    let extra_bits = DISTANCE_EXTRA_BITS[idx];
    let extra_val = (dist - DISTANCE_BASE[idx]) as u32;
    (idx, extra_bits, extra_val)
}

/// Decode match distance from `symbol` and `reader`.
pub fn decode_distance(symbol: usize, reader: &mut BitReader) -> Result<usize, CodropError> {
    if symbol >= DISTANCE_BASE.len() {
        return Err(CodropError::CorruptedEntropyStream(
            "Invalid distance symbol".into(),
        ));
    }
    let base = DISTANCE_BASE[symbol];
    let extra_bits = DISTANCE_EXTRA_BITS[symbol];
    let extra = reader.read_bits(extra_bits)? as usize;
    Ok(base + extra)
}

const MAX_MATCH_LEN: usize = 65535;

/// Tokenizes raw input into literals and matches using 4-byte hashing with lazy evaluation.
pub fn tokenize_lz(data: &[u8]) -> Vec<LzSymbol> {
    if data.is_empty() {
        return Vec::new();
    }
    if data.len() < 4 {
        return data.iter().map(|&b| LzSymbol::Literal(b)).collect();
    }

    let mut symbols = Vec::with_capacity(data.len() / 2 + 16);
    let mut table = vec![0u32; HASH_SIZE];
    let mut ip = 0;

    while ip + 4 <= data.len() {
        let h = hash4(&data[ip..ip + 4]);
        let prev = table[h];
        table[h] = (ip + 1) as u32;

        let mut match_found = false;
        let mut match_len = 0;
        let mut match_dist = 0;

        if prev != 0 {
            let ref_pos = (prev - 1) as usize;
            let dist = ip - ref_pos;
            if ref_pos < ip
                && dist <= MAX_OFFSET
                && data[ref_pos] == data[ip]
                && data[ref_pos + 1] == data[ip + 1]
                && data[ref_pos + 2] == data[ip + 2]
            {
                match_len = 3;
                while match_len + 8 <= MAX_MATCH_LEN
                    && ip + match_len + 8 <= data.len()
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
                while match_len < MAX_MATCH_LEN
                    && ip + match_len < data.len()
                    && data[ref_pos + match_len] == data[ip + match_len]
                {
                    match_len += 1;
                }
                match_dist = dist;
                match_found = true;
            }
        }

        if match_found {
            // Lazy evaluation: check if position ip + 1 has a longer match
            let mut better_lazy = false;
            if match_len < 32 && ip + 1 + 4 <= data.len() {
                let h_next = hash4(&data[ip + 1..ip + 5]);
                let prev_next = table[h_next];
                if prev_next != 0 {
                    let ref_next = (prev_next - 1) as usize;
                    let dist_next = (ip + 1) - ref_next;
                    if ref_next < (ip + 1)
                        && dist_next <= MAX_OFFSET
                        && data[ref_next] == data[ip + 1]
                        && data[ref_next + 1] == data[ip + 2]
                        && data[ref_next + 2] == data[ip + 3]
                    {
                        let mut next_len = 3;
                        while next_len < MAX_MATCH_LEN
                            && ip + 1 + next_len < data.len()
                            && data[ref_next + next_len] == data[ip + 1 + next_len]
                        {
                            next_len += 1;
                        }
                        if next_len > match_len {
                            better_lazy = true;
                        }
                    }
                }
            }

            if better_lazy {
                symbols.push(LzSymbol::Literal(data[ip]));
                ip += 1;
                continue;
            }

            symbols.push(LzSymbol::Match {
                length: match_len,
                distance: match_dist,
            });
            ip += match_len;
        } else {
            symbols.push(LzSymbol::Literal(data[ip]));
            ip += 1;
        }
    }

    while ip < data.len() {
        symbols.push(LzSymbol::Literal(data[ip]));
        ip += 1;
    }

    symbols
}

/// Serialize code lengths using run-length encoding into `out`.
pub fn serialize_table_descriptors(lit_len_lengths: &[u8], dist_lengths: &[u8], out: &mut Vec<u8>) {
    // Determine active sizes (trim trailing zero lengths)
    let num_lit_len = {
        let mut n = lit_len_lengths.len();
        while n > (EOB_SYMBOL + 1) && lit_len_lengths[n - 1] == 0 {
            n -= 1;
        }
        n
    };

    let num_dist = {
        let mut n = dist_lengths.len();
        while n > 0 && dist_lengths[n - 1] == 0 {
            n -= 1;
        }
        n
    };

    // Emit alphabet boundaries
    out.push((num_lit_len & 0xFF) as u8);
    out.push((num_lit_len >> 8) as u8);
    out.push(num_dist as u8);

    // Concatenate active code lengths
    let mut combined = Vec::with_capacity(num_lit_len + num_dist);
    combined.extend_from_slice(&lit_len_lengths[..num_lit_len]);
    combined.extend_from_slice(&dist_lengths[..num_dist]);

    // Simple robust RLE compression of code lengths
    let mut i = 0;
    while i < combined.len() {
        let val = combined[i];
        let mut run = 1usize;
        while i + run < combined.len() && combined[i + run] == val && run < 255 {
            run += 1;
        }
        i += run;

        if val == 0 {
            // Run of zeros: [0x00, count]
            out.push(0);
            out.push(run as u8);
        } else if run == 1 {
            // Single non-zero length: [val] (1..15)
            out.push(val);
        } else {
            // Run of non-zero lengths: [0x10 | val, count]
            out.push(0x10 | val);
            out.push(run as u8);
        }
    }
}

/// Deserialize table descriptors from `input[cursor..]`.
pub fn deserialize_table_descriptors(
    input: &[u8],
    cursor: &mut usize,
) -> Result<(Vec<u8>, Vec<u8>), CodropError> {
    if *cursor + 3 > input.len() {
        return Err(CodropError::UnexpectedEof);
    }

    let num_lit_len = (input[*cursor] as usize) | ((input[*cursor + 1] as usize) << 8);
    let num_dist = input[*cursor + 2] as usize;
    *cursor += 3;

    if num_lit_len > LIT_LEN_ALPHABET_SIZE || num_dist > DIST_ALPHABET_SIZE {
        return Err(CodropError::CorruptedEntropyStream(
            "Invalid Huffman alphabet size in table descriptor".into(),
        ));
    }

    let total = num_lit_len + num_dist;
    let mut combined = Vec::with_capacity(total);

    while combined.len() < total {
        if *cursor >= input.len() {
            return Err(CodropError::UnexpectedEof);
        }
        let b = input[*cursor];
        *cursor += 1;

        if b == 0 {
            // Run of zeros
            if *cursor >= input.len() {
                return Err(CodropError::UnexpectedEof);
            }
            let count = input[*cursor] as usize;
            *cursor += 1;
            if count == 0 || combined.len() + count > total {
                return Err(CodropError::CorruptedEntropyStream(
                    "Invalid RLE zero count in table descriptor".into(),
                ));
            }
            combined.resize(combined.len() + count, 0);
        } else if b <= 15 {
            // Single non-zero length
            combined.push(b);
        } else {
            // Run of identical non-zero length
            let val = b & 0x0F;
            if *cursor >= input.len() {
                return Err(CodropError::UnexpectedEof);
            }
            let count = input[*cursor] as usize;
            *cursor += 1;
            if count == 0 || combined.len() + count > total {
                return Err(CodropError::CorruptedEntropyStream(
                    "Invalid RLE repeat count in table descriptor".into(),
                ));
            }
            for _ in 0..count {
                combined.push(val);
            }
        }
    }

    let mut lit_len_lengths = vec![0u8; LIT_LEN_ALPHABET_SIZE];
    lit_len_lengths[..num_lit_len].copy_from_slice(&combined[..num_lit_len]);

    let mut dist_lengths = vec![0u8; DIST_ALPHABET_SIZE];
    dist_lengths[..num_dist].copy_from_slice(&combined[num_lit_len..]);

    Ok((lit_len_lengths, dist_lengths))
}

/// LZH compression codec (Block Type 3): LZ matching + Canonical Huffman entropy coding.
pub struct LzhCodec;

impl LzhCodec {
    /// Compress an input byte slice into an LZH payload.
    pub fn encode(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }

        // 1. LZ Tokenization
        let symbols = tokenize_lz(data);

        // 2. Frequency Counting
        let mut lit_len_freq = [0u32; LIT_LEN_ALPHABET_SIZE];
        let mut dist_freq = [0u32; DIST_ALPHABET_SIZE];

        for &sym in &symbols {
            match sym {
                LzSymbol::Literal(b) => {
                    lit_len_freq[b as usize] += 1;
                }
                LzSymbol::Match { length, distance } => {
                    let (len_sym, _, _) = encode_length(length);
                    lit_len_freq[len_sym] += 1;
                    let (dist_sym, _, _) = encode_distance(distance);
                    dist_freq[dist_sym] += 1;
                }
            }
        }
        // Always include End-Of-Block symbol
        lit_len_freq[EOB_SYMBOL] += 1;

        // 3. Build Canonical Huffman Trees (bounded to 15 bits)
        let lit_len_lengths = build_code_lengths(&lit_len_freq, LIT_LEN_ALPHABET_SIZE);
        let dist_lengths = build_code_lengths(&dist_freq, DIST_ALPHABET_SIZE);

        let lit_len_codes = build_canonical_codes(&lit_len_lengths);
        let dist_codes = build_canonical_codes(&dist_lengths);

        // 4. Serialize Table Descriptors Preamble
        let mut out = Vec::with_capacity(data.len() / 2 + 64);
        serialize_table_descriptors(&lit_len_lengths, &dist_lengths, &mut out);

        // 5. Encode Symbols into Bitstream
        let mut bitwriter = BitWriter::with_capacity(data.len() / 2 + 32);
        for sym in symbols {
            match sym {
                LzSymbol::Literal(b) => {
                    let code = lit_len_codes[b as usize];
                    let len = lit_len_lengths[b as usize];
                    bitwriter.write_bits(code as u32, len);
                }
                LzSymbol::Match { length, distance } => {
                    let (len_sym, len_extra_bits, len_extra_val) = encode_length(length);
                    let code = lit_len_codes[len_sym];
                    let len = lit_len_lengths[len_sym];
                    bitwriter.write_bits(code as u32, len);
                    bitwriter.write_bits(len_extra_val, len_extra_bits);

                    let (dist_sym, dist_extra_bits, dist_extra_val) = encode_distance(distance);
                    let dist_code = dist_codes[dist_sym];
                    let dist_len = dist_lengths[dist_sym];
                    bitwriter.write_bits(dist_code as u32, dist_len);
                    bitwriter.write_bits(dist_extra_val, dist_extra_bits);
                }
            }
        }

        // Emit End-Of-Block symbol
        let eob_code = lit_len_codes[EOB_SYMBOL];
        let eob_len = lit_len_lengths[EOB_SYMBOL];
        bitwriter.write_bits(eob_code as u32, eob_len);

        out.extend_from_slice(&bitwriter.into_bytes());
        out
    }

    /// Decompress an LZH payload into uncompressed bytes.
    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        if expected_len == 0 {
            if compressed.is_empty() {
                return Ok(Vec::new());
            } else {
                return Err(CodropError::CorruptedEntropyStream(
                    "Non-empty compressed data for 0 expected length".into(),
                ));
            }
        }

        let mut cursor = 0;
        let (lit_len_lengths, dist_lengths) =
            deserialize_table_descriptors(compressed, &mut cursor)?;

        let lit_len_table = HuffmanDecoderTable::build(&lit_len_lengths)?;
        let dist_table = HuffmanDecoderTable::build(&dist_lengths)?;

        let mut bitreader = BitReader::new(&compressed[cursor..]);
        let mut out = Vec::with_capacity(expected_len);

        loop {
            let sym = lit_len_table.decode_symbol(&mut bitreader)? as usize;
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

                let dist_sym = dist_table.decode_symbol(&mut bitreader)? as usize;
                let distance = decode_distance(dist_sym, &mut bitreader)?;

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
                    "Illegal symbol in LZH stream".into(),
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
    fn test_lzh_roundtrip_simple() {
        let text = b"The quick brown fox jumps over the lazy dog! The quick brown fox jumps over the lazy dog!";
        let compressed = LzhCodec::encode(text);
        let decompressed = LzhCodec::decode(&compressed, text.len()).unwrap();
        assert_eq!(decompressed, text);
    }

    #[test]
    fn test_lzh_roundtrip_empty_and_small() {
        for len in 0..=20 {
            let data = vec![b'x'; len];
            let compressed = LzhCodec::encode(&data);
            let decompressed = LzhCodec::decode(&compressed, data.len()).unwrap();
            assert_eq!(decompressed, data, "Failed for length {}", len);
        }
    }

    #[test]
    fn test_lzh_overlapping_matches() {
        let data = vec![b'Z'; 500];
        let compressed = LzhCodec::encode(&data);
        assert!(compressed.len() < data.len());
        let decompressed = LzhCodec::decode(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lzh_length_and_distance_bounds() {
        for len in [3, 4, 10, 11, 18, 19, 34, 35, 257, 258, 1000, 65535] {
            let (sym, extra_bits, extra_val) = encode_length(len);
            let mut writer = BitWriter::new();
            writer.write_bits(extra_val, extra_bits);
            let bytes = writer.into_bytes();
            let mut reader = BitReader::new(&bytes);
            let decoded_len = decode_length(sym, &mut reader).unwrap();
            assert_eq!(decoded_len, len, "Failed length {}", len);
        }

        for dist in [1, 2, 4, 5, 8, 9, 16, 17, 32, 33, 1024, 1025, 65535] {
            let (sym, extra_bits, extra_val) = encode_distance(dist);
            let mut writer = BitWriter::new();
            writer.write_bits(extra_val, extra_bits);
            let bytes = writer.into_bytes();
            let mut reader = BitReader::new(&bytes);
            let decoded_dist = decode_distance(sym, &mut reader).unwrap();
            assert_eq!(decoded_dist, dist, "Failed dist {}", dist);
        }
    }

    #[test]
    fn test_lzh_malformed_offset_beyond_history() {
        // Build an LZH stream with a match pointing to offset 5 when history only has 1 byte
        let mut lit_len_freq = [0u32; LIT_LEN_ALPHABET_SIZE];
        let mut dist_freq = [0u32; DIST_ALPHABET_SIZE];
        lit_len_freq[b'A' as usize] = 1;
        let (len_sym, _, _) = encode_length(3);
        lit_len_freq[len_sym] = 1;
        lit_len_freq[EOB_SYMBOL] = 1;
        dist_freq[4] = 1; // base distance 5

        let lit_len_lengths = build_code_lengths(&lit_len_freq, LIT_LEN_ALPHABET_SIZE);
        let dist_lengths = build_code_lengths(&dist_freq, DIST_ALPHABET_SIZE);
        let lit_len_codes = build_canonical_codes(&lit_len_lengths);
        let dist_codes = build_canonical_codes(&dist_lengths);

        let mut payload = Vec::new();
        serialize_table_descriptors(&lit_len_lengths, &dist_lengths, &mut payload);

        let mut bitwriter = BitWriter::new();
        // Emit literal 'A'
        bitwriter.write_bits(
            lit_len_codes[b'A' as usize] as u32,
            lit_len_lengths[b'A' as usize],
        );
        // Emit match length 3
        bitwriter.write_bits(lit_len_codes[len_sym] as u32, lit_len_lengths[len_sym]);
        // Distance symbol 4 (distance 5 > history 1)
        bitwriter.write_bits(dist_codes[4] as u32, dist_lengths[4]);
        bitwriter.write_bits(0, 1); // 1 extra bit for symbol 4
                                    // EOB
        bitwriter.write_bits(
            lit_len_codes[EOB_SYMBOL] as u32,
            lit_len_lengths[EOB_SYMBOL],
        );

        payload.extend_from_slice(&bitwriter.into_bytes());

        let err = LzhCodec::decode(&payload, 4).unwrap_err();
        match err {
            CodropError::InvalidOffset {
                offset: 5,
                max_valid: 1,
            } => {}
            other => panic!("Expected InvalidOffset with 5 > 1, got {:?}", other),
        }
    }

    #[test]
    fn test_debug_large_buf() {
        for &size in &[1024, 64 * 1024, 128 * 1024] {
            let mut buf = Vec::with_capacity(size);
            for i in 0..size {
                buf.push((i % 251) as u8);
            }
            let c = LzhCodec::encode(&buf);
            let d = match LzhCodec::decode(&c, buf.len()) {
                Ok(v) => v,
                Err(e) => panic!("Failed for size {}: {:?}", size, e),
            };
            assert_eq!(d.len(), buf.len());
        }
    }
}
