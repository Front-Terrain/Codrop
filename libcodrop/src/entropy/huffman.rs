use crate::entropy::bitstream::BitReader;
use crate::error::CodropError;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Maximum allowable code length in Codrop canonical Huffman trees (15 bits).
pub const MAX_HUFFMAN_BITS: usize = 15;

/// Primary lookup table size: 2^10 = 1024 entries.
pub const PRIMARY_BITS: usize = 10;
pub const PRIMARY_SIZE: usize = 1 << PRIMARY_BITS;
pub const PRIMARY_MASK: usize = PRIMARY_SIZE - 1;

/// Secondary subtable bits: 15 - 10 = 5 bits (32 entries per subtable).
pub const SECONDARY_BITS: usize = MAX_HUFFMAN_BITS - PRIMARY_BITS;
pub const SECONDARY_SIZE: usize = 1 << SECONDARY_BITS;
pub const SECONDARY_MASK: usize = SECONDARY_SIZE - 1;

/// Represents a node in the Huffman construction priority queue.
#[derive(Debug, Clone, Eq, PartialEq)]
enum HeapNode {
    Leaf {
        freq: u32,
        symbol: usize,
    },
    Internal {
        freq: u32,
        left: usize,
        right: usize,
    },
}

impl HeapNode {
    fn freq(&self) -> u32 {
        match self {
            HeapNode::Leaf { freq, .. } => *freq,
            HeapNode::Internal { freq, .. } => *freq,
        }
    }
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap: reverse comparison so smallest frequency has highest priority
        other.freq().cmp(&self.freq())
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Computes code lengths from symbol frequencies, strictly bounded to `MAX_HUFFMAN_BITS` (15).
pub fn build_code_lengths(frequencies: &[u32], max_symbols: usize) -> Vec<u8> {
    let mut lengths = vec![0u8; frequencies.len()];

    // Count non-zero symbols
    let non_zero_count = frequencies
        .iter()
        .take(max_symbols)
        .filter(|&&f| f > 0)
        .count();
    if non_zero_count == 0 {
        return lengths;
    }
    if non_zero_count == 1 {
        for (sym, &f) in frequencies.iter().enumerate().take(max_symbols) {
            if f > 0 {
                lengths[sym] = 1;
                return lengths;
            }
        }
    }

    // Build Huffman tree using a min-heap with deterministic tie-breaking
    let mut arena: Vec<HeapNode> = Vec::with_capacity(non_zero_count * 2);
    let mut heap: BinaryHeap<(std::cmp::Reverse<u32>, std::cmp::Reverse<usize>)> =
        BinaryHeap::new();

    for (sym, &freq) in frequencies.iter().enumerate().take(max_symbols) {
        if freq > 0 {
            let idx = arena.len();
            arena.push(HeapNode::Leaf { freq, symbol: sym });
            heap.push((std::cmp::Reverse(freq), std::cmp::Reverse(idx)));
        }
    }

    while heap.len() > 1 {
        let (std::cmp::Reverse(freq1), std::cmp::Reverse(idx1)) = heap.pop().unwrap();
        let (std::cmp::Reverse(freq2), std::cmp::Reverse(idx2)) = heap.pop().unwrap();
        let parent_freq = freq1.saturating_add(freq2);
        let parent_idx = arena.len();
        arena.push(HeapNode::Internal {
            freq: parent_freq,
            left: idx1,
            right: idx2,
        });
        heap.push((
            std::cmp::Reverse(parent_freq),
            std::cmp::Reverse(parent_idx),
        ));
    }

    let root_idx = heap.pop().unwrap().1 .0;

    // Traverse tree to calculate depths
    let mut stack = vec![(root_idx, 0usize)];
    while let Some((node_idx, depth)) = stack.pop() {
        match arena[node_idx] {
            HeapNode::Leaf { symbol, .. } => {
                lengths[symbol] = depth.clamp(1, 255) as u8;
            }
            HeapNode::Internal { left, right, .. } => {
                stack.push((left, depth + 1));
                stack.push((right, depth + 1));
            }
        }
    }

    // Limit maximum code length to 15 bits using Kraft inequality rebalancing
    limit_code_lengths(&mut lengths, max_symbols, MAX_HUFFMAN_BITS);

    lengths
}

/// Limits code lengths to `max_bits` while preserving prefix-code Kraft inequality.
fn limit_code_lengths(lengths: &mut [u8], max_symbols: usize, max_bits: usize) {
    let mut max_found = 0;
    for &l in lengths.iter().take(max_symbols) {
        if l as usize > max_found {
            max_found = l as usize;
        }
    }

    if max_found <= max_bits {
        return;
    }

    // Clamp all lengths > max_bits to max_bits
    for l in lengths.iter_mut().take(max_symbols) {
        if *l as usize > max_bits {
            *l = max_bits as u8;
        }
    }

    // Calculate Kraft sum: target is <= 2^max_bits
    let target_sum = 1u32 << max_bits;
    loop {
        let mut kraft_sum = 0u32;
        for &l in lengths.iter().take(max_symbols) {
            if l > 0 {
                kraft_sum += 1 << (max_bits - l as usize);
            }
        }

        if kraft_sum <= target_sum {
            break;
        }

        // Tree is oversubscribed due to clamping: increment the length of the symbol
        // with the smallest code length (< max_bits) to reduce Kraft sum
        let mut best_sym = None;
        let mut best_len = 0;
        for (sym, &l) in lengths.iter().enumerate().take(max_symbols) {
            if l > 0 && (l as usize) < max_bits && (l as usize) > best_len {
                best_len = l as usize;
                best_sym = Some(sym);
            }
        }

        if let Some(sym) = best_sym {
            lengths[sym] += 1;
        } else {
            break;
        }
    }
}

/// Reverses the lowest `len` bits of a 16-bit code for LSB-first bitstream packing.
#[inline(always)]
pub fn reverse_bits(code: u16, len: u8) -> u16 {
    if len == 0 {
        0
    } else {
        code.reverse_bits() >> (16 - len)
    }
}

/// Builds canonical Huffman codes from code lengths.
/// Returns bit-reversed codes aligned with LSB-first bitstream representation.
pub fn build_canonical_codes(lengths: &[u8]) -> Vec<u16> {
    let mut bl_count = [0u16; 16];
    for &l in lengths {
        if l > 0 && l <= 15 {
            bl_count[l as usize] += 1;
        }
    }

    let mut next_code = [0u16; 16];
    let mut code = 0u16;
    for len in 1..=15 {
        code = (code + bl_count[len - 1]) << 1;
        next_code[len] = code;
    }

    let mut codes = vec![0u16; lengths.len()];
    for (sym, &l) in lengths.iter().enumerate() {
        if l > 0 {
            let canon = next_code[l as usize];
            next_code[l as usize] += 1;
            codes[sym] = reverse_bits(canon, l);
        }
    }

    codes
}

/// Fast table-based canonical Huffman decoder.
/// Uses a 1024-entry primary table for 1-cycle resolution of codes <= 10 bits,
/// and a secondary table for codes 11..15 bits.
#[derive(Debug)]
pub struct HuffmanDecoderTable {
    primary: [u16; PRIMARY_SIZE],
    secondary: Vec<u16>,
}

impl HuffmanDecoderTable {
    pub const EMPTY_ENTRY: u16 = 0xFFFF;
    pub const SECONDARY_FLAG: u16 = 0x8000;

    /// Build a decoder lookup table from canonical code lengths.
    pub fn build(lengths: &[u8]) -> Result<Self, CodropError> {
        let mut bl_count = [0u16; 16];
        let mut kraft_sum = 0u32;

        for &l in lengths {
            if l > 15 {
                return Err(CodropError::CorruptedEntropyStream(
                    "Huffman code length exceeds 15 bits".into(),
                ));
            }
            if l > 0 {
                bl_count[l as usize] += 1;
                kraft_sum = kraft_sum.checked_add(1 << (15 - l)).ok_or_else(|| {
                    CodropError::CorruptedEntropyStream("Kraft sum overflow".into())
                })?;
            }
        }

        // Validate Kraft inequality: sum(2^-L) <= 1
        if kraft_sum > 32768 {
            return Err(CodropError::CorruptedEntropyStream(
                "Oversubscribed Huffman tree".into(),
            ));
        }

        // Compute canonical codes
        let mut next_code = [0u16; 16];
        let mut code = 0u16;
        for len in 1..=15 {
            code = (code + bl_count[len - 1]) << 1;
            next_code[len] = code;
        }

        let mut primary = [Self::EMPTY_ENTRY; PRIMARY_SIZE];
        let mut secondary = Vec::new();

        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }

            let canon = next_code[len as usize];
            next_code[len as usize] += 1;
            let rev_code = reverse_bits(canon, len) as usize;

            if (len as usize) <= PRIMARY_BITS {
                // Short code (<= 10 bits): fill all primary table suffixes
                let entry = ((len as u16) << 10) | (sym as u16);
                let step = 1 << len;
                let mut idx = rev_code;
                while idx < PRIMARY_SIZE {
                    primary[idx] = entry;
                    idx += step;
                }
            } else {
                // Long code (11..15 bits): setup primary link to secondary table
                let prefix = rev_code & PRIMARY_MASK;
                let sub_offset = if primary[prefix] == Self::EMPTY_ENTRY {
                    let off = secondary.len();
                    secondary.resize(off + SECONDARY_SIZE, Self::EMPTY_ENTRY);
                    primary[prefix] = Self::SECONDARY_FLAG | (off as u16);
                    off
                } else if (primary[prefix] & Self::SECONDARY_FLAG) != 0 {
                    (primary[prefix] & !Self::SECONDARY_FLAG) as usize
                } else {
                    return Err(CodropError::CorruptedEntropyStream(
                        "Huffman prefix collision".into(),
                    ));
                };

                let sub_code = rev_code >> PRIMARY_BITS;
                let sub_len = (len as usize) - PRIMARY_BITS;
                let step = 1 << sub_len;
                let entry = ((len as u16) << 10) | (sym as u16);

                let mut s_idx = sub_code;
                while s_idx < SECONDARY_SIZE {
                    secondary[sub_offset + s_idx] = entry;
                    s_idx += step;
                }
            }
        }

        Ok(Self { primary, secondary })
    }

    /// Decode the next symbol from the bitstream.
    #[inline(always)]
    pub fn decode_symbol(&self, reader: &mut BitReader) -> Result<u16, CodropError> {
        let peek10 = (reader.peek_bits(10) as usize) & PRIMARY_MASK;
        let entry = self.primary[peek10];

        if entry == Self::EMPTY_ENTRY {
            return Err(CodropError::CorruptedEntropyStream(
                "Invalid Huffman code encountered".into(),
            ));
        }

        if (entry & Self::SECONDARY_FLAG) == 0 {
            // Direct hit in primary table (<= 10 bits)
            let sym = entry & 0x03FF;
            let len = (entry >> 10) as u8;
            reader.drop_bits(len);
            Ok(sym)
        } else {
            // Secondary table hit (11..15 bits)
            let sub_offset = (entry & !Self::SECONDARY_FLAG) as usize;
            let peek15 = reader.peek_bits(15) as usize;
            let extra5 = (peek15 >> PRIMARY_BITS) & SECONDARY_MASK;
            let sec_entry = self.secondary[sub_offset + extra5];

            if sec_entry == Self::EMPTY_ENTRY {
                return Err(CodropError::CorruptedEntropyStream(
                    "Invalid long Huffman code encountered".into(),
                ));
            }

            let sym = sec_entry & 0x03FF;
            let len = (sec_entry >> 10) as u8;
            reader.drop_bits(len);
            Ok(sym)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::bitstream::BitWriter;

    #[test]
    fn test_canonical_huffman_roundtrip() {
        // Skewed frequencies
        let mut freqs = [0u32; 8];
        freqs[0] = 1000;
        freqs[1] = 500;
        freqs[2] = 200;
        freqs[3] = 100;
        freqs[4] = 50;
        freqs[5] = 20;
        freqs[6] = 10;
        freqs[7] = 5;

        let lengths = build_code_lengths(&freqs, 8);
        for &l in &lengths {
            assert!(l > 0 && l <= 15);
        }
        let codes = build_canonical_codes(&lengths);
        let decoder_table = HuffmanDecoderTable::build(&lengths).unwrap();

        // Encode symbols
        let mut writer = BitWriter::new();
        let test_symbols = [0, 1, 2, 7, 3, 0, 0, 4, 5, 6, 7, 1];
        for &sym in &test_symbols {
            writer.write_bits(codes[sym] as u32, lengths[sym]);
        }
        let bytes = writer.into_bytes();

        // Decode symbols
        let mut reader = BitReader::new(&bytes);
        for &expected in &test_symbols {
            let decoded = decoder_table.decode_symbol(&mut reader).unwrap();
            assert_eq!(decoded, expected as u16);
        }
    }

    #[test]
    fn test_single_symbol_tree() {
        let mut freqs = [0u32; 4];
        freqs[2] = 42; // only symbol 2

        let lengths = build_code_lengths(&freqs, 4);
        assert_eq!(lengths[2], 1);
        let codes = build_canonical_codes(&lengths);
        let decoder_table = HuffmanDecoderTable::build(&lengths).unwrap();

        let mut writer = BitWriter::new();
        writer.write_bits(codes[2] as u32, lengths[2]);
        writer.write_bits(codes[2] as u32, lengths[2]);
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        assert_eq!(decoder_table.decode_symbol(&mut reader).unwrap(), 2);
        assert_eq!(decoder_table.decode_symbol(&mut reader).unwrap(), 2);
    }

    #[test]
    fn test_pathological_fibonacci_depth_bounded_to_15() {
        // Fibonacci frequencies create deep trees
        let mut freqs = [0u32; 25];
        let mut a = 1u32;
        let mut b = 1u32;
        for f in freqs.iter_mut() {
            *f = a;
            let next = a.saturating_add(b);
            a = b;
            b = next;
        }

        let lengths = build_code_lengths(&freqs, 25);
        for &l in &lengths {
            assert!(l <= 15, "Code length {} exceeded 15 bits", l);
        }

        // Must build valid decoder table without oversubscription error
        let table = HuffmanDecoderTable::build(&lengths).unwrap();

        let codes = build_canonical_codes(&lengths);
        let mut writer = BitWriter::new();
        for sym in 0..25 {
            writer.write_bits(codes[sym] as u32, lengths[sym]);
        }
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        for sym in 0..25 {
            assert_eq!(table.decode_symbol(&mut reader).unwrap(), sym as u16);
        }
    }

    #[test]
    fn test_oversubscribed_tree_rejected() {
        // Two symbols of length 1 is already sum = 1. Adding a third must fail.
        let invalid_lengths = [1u8, 1, 1];
        let err = HuffmanDecoderTable::build(&invalid_lengths).unwrap_err();
        match err {
            CodropError::CorruptedEntropyStream(msg) => {
                assert!(msg.contains("Oversubscribed"));
            }
            other => panic!("Expected CorruptedEntropyStream, got {:?}", other),
        }
    }
}
