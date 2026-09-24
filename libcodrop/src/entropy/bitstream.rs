use crate::error::CodropError;

/// Bitstream writer that packs bits into bytes (LSB-first).
#[derive(Debug, Default)]
pub struct BitWriter {
    buffer: Vec<u8>,
    bit_buf: u64,
    bits_in_buf: u8,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            bit_buf: 0,
            bits_in_buf: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            bit_buf: 0,
            bits_in_buf: 0,
        }
    }

    /// Write up to 32 bits into the bitstream (least-significant bit first).
    #[inline(always)]
    pub fn write_bits(&mut self, value: u32, count: u8) {
        if count == 0 {
            return;
        }
        let mask = if count == 32 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        self.bit_buf |= ((value as u64) & mask) << self.bits_in_buf;
        self.bits_in_buf += count;
        while self.bits_in_buf >= 8 {
            self.buffer.push(self.bit_buf as u8);
            self.bit_buf >>= 8;
            self.bits_in_buf -= 8;
        }
    }

    /// Flush any remaining bits in the bit buffer, padding the final byte with zero bits.
    pub fn flush_align(&mut self) {
        if self.bits_in_buf > 0 {
            self.buffer.push(self.bit_buf as u8);
            self.bit_buf = 0;
            self.bits_in_buf = 0;
        }
    }

    /// Complete bitstream emission and return the underlying byte vector.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.flush_align();
        self.buffer
    }
}

/// Bitstream reader that extracts bits from bytes (LSB-first).
pub struct BitReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    bit_buf: u64,
    bits_in_buf: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let mut reader = Self {
            bytes,
            cursor: 0,
            bit_buf: 0,
            bits_in_buf: 0,
        };
        reader.fill_buffer();
        reader
    }

    #[inline(always)]
    fn fill_buffer(&mut self) {
        while self.bits_in_buf <= 56 && self.cursor < self.bytes.len() {
            self.bit_buf |= (self.bytes[self.cursor] as u64) << self.bits_in_buf;
            self.cursor += 1;
            self.bits_in_buf += 8;
        }
    }

    /// Peek up to 16 bits without advancing the bitstream.
    #[inline(always)]
    pub fn peek_bits(&mut self, count: u8) -> u32 {
        self.fill_buffer();
        let mask = if count == 32 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        (self.bit_buf & mask) as u32
    }

    /// Drop `count` bits from the bit buffer.
    #[inline(always)]
    pub fn drop_bits(&mut self, count: u8) {
        let c = count.min(self.bits_in_buf);
        self.bit_buf >>= c;
        self.bits_in_buf -= c;
    }

    /// Read `count` bits from the bitstream.
    pub fn read_bits(&mut self, count: u8) -> Result<u32, CodropError> {
        if count == 0 {
            return Ok(0);
        }
        self.fill_buffer();
        if self.bits_in_buf < count {
            return Err(CodropError::UnexpectedEof);
        }
        let mask = if count == 32 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        let val = (self.bit_buf & mask) as u32;
        self.bit_buf >>= count;
        self.bits_in_buf -= count;
        Ok(val)
    }

    /// Check if all bits and bytes have been fully consumed.
    pub fn is_empty(&mut self) -> bool {
        self.fill_buffer();
        self.bits_in_buf == 0 && self.cursor >= self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitstream_single_and_multi_bits() {
        let mut writer = BitWriter::new();
        writer.write_bits(1, 1); // bit 0 = 1
        writer.write_bits(2, 2); // bits 1..2 = 10
        writer.write_bits(13, 5); // bits 3..7 = 01101
                                  // byte 0 = 1 | (2 << 1) | (13 << 3) = 1 | 4 | 104 = 109 = 0x6D
        writer.write_bits(0xABCD, 16);
        writer.write_bits(7, 3);
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.read_bits(1).unwrap(), 1);
        assert_eq!(reader.read_bits(2).unwrap(), 2);
        assert_eq!(reader.read_bits(5).unwrap(), 13);
        assert_eq!(reader.read_bits(16).unwrap(), 0xABCD);
        assert_eq!(reader.read_bits(3).unwrap(), 7);
    }

    #[test]
    fn test_bitstream_peek_and_drop() {
        let mut writer = BitWriter::new();
        writer.write_bits(42, 8);
        writer.write_bits(999, 10);
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.peek_bits(8), 42);
        assert_eq!(reader.peek_bits(8), 42); // repeated peek produces same value
        reader.drop_bits(8);
        assert_eq!(reader.peek_bits(10), 999);
        assert_eq!(reader.read_bits(10).unwrap(), 999);
    }

    #[test]
    fn test_bitstream_truncated_read() {
        let bytes = [0x55];
        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.read_bits(8).unwrap(), 0x55);
        assert_eq!(reader.read_bits(1).unwrap_err(), CodropError::UnexpectedEof);
    }
}
