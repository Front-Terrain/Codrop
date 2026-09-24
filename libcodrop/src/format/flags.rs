use crate::error::CodropError;

/// Bitfield representation of stream header flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderFlags {
    pub has_uncompressed_size: bool,
    pub has_stream_checksum: bool,
    pub has_dictionary: bool,
    pub independent_blocks: bool,
    pub window_code: u8, // 2 bits: 0 = 64KB, 1 = 1MB, 2 = 8MB, 3 = Custom
}

impl HeaderFlags {
    pub const FLAG_HAS_UNCOMPRESSED_SIZE: u16 = 1 << 0;
    pub const FLAG_HAS_STREAM_CHECKSUM: u16 = 1 << 1;
    pub const FLAG_HAS_DICTIONARY: u16 = 1 << 2;
    pub const FLAG_INDEPENDENT_BLOCKS: u16 = 1 << 3;
    pub const WINDOW_CODE_MASK: u16 = 0b11 << 4;
    pub const RESERVED_MASK: u16 = !0x003F; // Bits 6..15 must be 0

    pub fn to_u16(&self) -> u16 {
        let mut bits = 0u16;
        if self.has_uncompressed_size {
            bits |= Self::FLAG_HAS_UNCOMPRESSED_SIZE;
        }
        if self.has_stream_checksum {
            bits |= Self::FLAG_HAS_STREAM_CHECKSUM;
        }
        if self.has_dictionary {
            bits |= Self::FLAG_HAS_DICTIONARY;
        }
        if self.independent_blocks {
            bits |= Self::FLAG_INDEPENDENT_BLOCKS;
        }
        bits |= ((self.window_code & 0x03) as u16) << 4;
        bits
    }

    pub fn try_from_u16(bits: u16) -> Result<Self, CodropError> {
        if (bits & Self::RESERVED_MASK) != 0 {
            return Err(CodropError::CorruptedHeader(format!(
                "Non-zero reserved bits in stream header flags: 0x{:04X}",
                bits & Self::RESERVED_MASK
            )));
        }
        Ok(Self {
            has_uncompressed_size: (bits & Self::FLAG_HAS_UNCOMPRESSED_SIZE) != 0,
            has_stream_checksum: (bits & Self::FLAG_HAS_STREAM_CHECKSUM) != 0,
            has_dictionary: (bits & Self::FLAG_HAS_DICTIONARY) != 0,
            independent_blocks: (bits & Self::FLAG_INDEPENDENT_BLOCKS) != 0,
            window_code: ((bits >> 4) & 0x03) as u8,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_roundtrip() {
        let flags = HeaderFlags {
            has_uncompressed_size: true,
            has_stream_checksum: true,
            has_dictionary: false,
            independent_blocks: true,
            window_code: 2,
        };
        let val = flags.to_u16();
        let parsed = HeaderFlags::try_from_u16(val).unwrap();
        assert_eq!(flags, parsed);
    }

    #[test]
    fn test_flags_reject_reserved_bits() {
        let bad_bits = (1 << 6) | (1 << 0);
        let err = HeaderFlags::try_from_u16(bad_bits).unwrap_err();
        assert!(matches!(err, CodropError::CorruptedHeader(_)));
    }
}
