use crate::error::CodropError;

/// BitWriter for packing variable-length prefix codes into a byte buffer.
pub struct BitWriter {
    pub bytes: Vec<u8>,
    bit_buffer: u64,
    bits_in_buffer: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(512),
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    /// Write bits (least significant bits first)
    #[inline(always)]
    pub fn write_bits(&mut self, value: u32, count: usize) {
        self.bit_buffer |= (value as u64) << self.bits_in_buffer;
        self.bits_in_buffer += count;
        while self.bits_in_buffer >= 8 {
            self.bytes.push((self.bit_buffer & 0xFF) as u8);
            self.bit_buffer >>= 8;
            self.bits_in_buffer -= 8;
        }
    }

    /// Flush remaining bits, zero-padding to next byte boundary
    pub fn flush(&mut self) {
        if self.bits_in_buffer > 0 {
            self.bytes.push((self.bit_buffer & 0xFF) as u8);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        }
    }
}

/// BitReader for reading variable-length codes from a byte slice.
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buffer: u64,
    bits_in_buffer: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut reader = Self {
            data,
            byte_pos: 0,
            bit_buffer: 0,
            bits_in_buffer: 0,
        };
        reader.refill();
        reader
    }

    #[inline(always)]
    pub fn refill(&mut self) {
        while self.bits_in_buffer <= 56 && self.byte_pos < self.data.len() {
            self.bit_buffer |= (self.data[self.byte_pos] as u64) << self.bits_in_buffer;
            self.bits_in_buffer += 8;
            self.byte_pos += 1;
        }
    }

    #[inline(always)]
    pub fn peek_bits(&self, count: usize) -> u32 {
        (self.bit_buffer & ((1 << count) - 1)) as u32
    }

    #[inline(always)]
    pub fn consume_bits(&mut self, count: usize) {
        self.bit_buffer >>= count;
        self.bits_in_buffer -= count;
        self.refill();
    }

    #[inline(always)]
    pub fn read_bits(&mut self, count: usize) -> u32 {
        let val = self.peek_bits(count);
        self.consume_bits(count);
        val
    }
}

/// Canonical Huffman Code specification for an alphabet.
pub const MAX_HUFFMAN_BITS: usize = 15;

#[derive(Clone)]
pub struct CanonicalHuffman {
    pub num_symbols: usize,
    pub code_lengths: Vec<u8>,
    pub codes: Vec<u16>,
    // Fast decode table: 10 bits lookup table => (symbol: u16, length: u8)
    decode_table: Vec<u16>,
}

impl CanonicalHuffman {
    /// Build canonical Huffman codes from symbol frequencies
    pub fn from_frequencies(frequencies: &[u32]) -> Self {
        let num_symbols = frequencies.len();
        let mut active = Vec::new();
        for (sym, &freq) in frequencies.iter().enumerate() {
            if freq > 0 {
                active.push((freq, sym));
            }
        }

        if active.is_empty() {
            return Self {
                num_symbols,
                code_lengths: vec![0; num_symbols],
                codes: vec![0; num_symbols],
                decode_table: vec![0; 1024],
            };
        }

        if active.len() == 1 {
            let sym = active[0].1;
            let mut lengths = vec![0u8; num_symbols];
            lengths[sym] = 1;
            return Self::from_code_lengths(&lengths);
        }

        // Build priority queue tree
        #[derive(Eq, PartialEq)]
        struct Node {
            freq: u32,
            symbol: Option<usize>,
            left: Option<Box<Node>>,
            right: Option<Box<Node>>,
        }

        impl Ord for Node {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other.freq.cmp(&self.freq)
            }
        }
        impl PartialOrd for Node {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        use std::collections::BinaryHeap;
        let mut heap = BinaryHeap::new();
        for (freq, sym) in active {
            heap.push(Node {
                freq,
                symbol: Some(sym),
                left: None,
                right: None,
            });
        }

        while heap.len() > 1 {
            let left = heap.pop().unwrap();
            let right = heap.pop().unwrap();
            let parent_freq = left.freq.saturating_add(right.freq);
            heap.push(Node {
                freq: parent_freq,
                symbol: None,
                left: Some(Box::new(left)),
                right: Some(Box::new(right)),
            });
        }

        let root = heap.pop().unwrap();
        let mut lengths = vec![0u8; num_symbols];

        fn traverse(node: &Node, depth: u8, lengths: &mut [u8]) {
            if let Some(sym) = node.symbol {
                lengths[sym] = depth.max(1).min(MAX_HUFFMAN_BITS as u8);
                return;
            }
            if let Some(ref left) = node.left {
                traverse(left, depth + 1, lengths);
            }
            if let Some(ref right) = node.right {
                traverse(right, depth + 1, lengths);
            }
        }

        traverse(&root, 0, &mut lengths);
        Self::from_code_lengths(&lengths)
    }

    /// Construct canonical codes and fast lookup table from lengths
    pub fn from_code_lengths(lengths: &[u8]) -> Self {
        let num_symbols = lengths.len();
        let mut bl_count = [0u16; MAX_HUFFMAN_BITS + 1];
        for &len in lengths {
            if len > 0 && (len as usize) <= MAX_HUFFMAN_BITS {
                bl_count[len as usize] += 1;
            }
        }

        let mut next_code = [0u16; MAX_HUFFMAN_BITS + 1];
        let mut code = 0u16;
        for bits in 1..=MAX_HUFFMAN_BITS {
            code = (code + bl_count[bits - 1]) << 1;
            next_code[bits] = code;
        }

        let mut codes = vec![0u16; num_symbols];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 && (len as usize) <= MAX_HUFFMAN_BITS {
                codes[sym] = next_code[len as usize];
                next_code[len as usize] += 1;
            }
        }

        // Build 10-bit direct lookup table: packed as (symbol << 4) | length
        let mut decode_table = vec![0u16; 1024];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 && len <= 10 {
                let sym_code = codes[sym];
                // Mirror bits for LSB bitstream reading
                let mut rev_code = 0u16;
                for b in 0..len {
                    if (sym_code & (1 << (len - 1 - b))) != 0 {
                        rev_code |= 1 << b;
                    }
                }

                let step = 1 << len;
                let mut idx = rev_code as usize;
                let entry = ((sym as u16) << 4) | (len as u16);
                while idx < 1024 {
                    decode_table[idx] = entry;
                    idx += step;
                }
            }
        }

        Self {
            num_symbols,
            code_lengths: lengths.to_vec(),
            codes,
            decode_table,
        }
    }

    /// Decode the next symbol from the bit reader
    #[inline(always)]
    pub fn decode_symbol(&self, reader: &mut BitReader) -> Result<usize, CodropError> {
        let peek = reader.peek_bits(10) as usize;
        let entry = self.decode_table[peek];
        let len = (entry & 0x0F) as usize;
        if len > 0 {
            reader.consume_bits(len);
            return Ok((entry >> 4) as usize);
        }

        // Fallback for codes > 10 bits: bit-by-bit tree traversal
        let mut cur_code = 0u16;
        for bits in 1..=MAX_HUFFMAN_BITS {
            cur_code = (cur_code << 1) | (reader.read_bits(1) as u16);
            for (sym, &l) in self.code_lengths.iter().enumerate() {
                if l as usize == bits && self.codes[sym] == cur_code {
                    return Ok(sym);
                }
            }
        }

        Err(CodropError::CorruptedEntropyStream("Invalid Huffman prefix code".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_huffman_roundtrip() {
        let mut freqs = [0u32; 256];
        freqs[b'A' as usize] = 100;
        freqs[b'B' as usize] = 50;
        freqs[b'C' as usize] = 20;
        freqs[b'D' as usize] = 5;
        freqs[b'R' as usize] = 10;

        let huff = CanonicalHuffman::from_frequencies(&freqs);
        let mut writer = BitWriter::new();

        // Encode "ABACADABRA"
        let msg = b"ABACADABRA";
        for &b in msg {
            let sym = b as usize;
            let len = huff.code_lengths[sym] as usize;
            let code = huff.codes[sym];
            // Write MSB code mirrored for LSB bitstream
            let mut rev_code = 0u32;
            for i in 0..len {
                if (code & (1 << (len - 1 - i))) != 0 {
                    rev_code |= 1 << i;
                }
            }
            writer.write_bits(rev_code, len);
        }
        writer.flush();

        let mut reader = BitReader::new(&writer.bytes);
        let mut decoded = Vec::new();
        for _ in 0..msg.len() {
            let sym = huff.decode_symbol(&mut reader).unwrap();
            decoded.push(sym as u8);
        }

        assert_eq!(decoded, msg);
    }
}
