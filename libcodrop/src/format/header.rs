use std::io::{Read, Write};
use crate::checksum::Crc8;
use crate::error::CodropError;
use crate::format::flags::HeaderFlags;
use crate::format::magic::{CODROP_MAGIC, validate_magic};

pub const FORMAT_VERSION_MAJOR: u8 = 1;
pub const FORMAT_VERSION_MINOR: u8 = 0;
pub const FORMAT_VERSION_BYTE: u8 = (FORMAT_VERSION_MAJOR << 4) | (FORMAT_VERSION_MINOR & 0x0F);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamHeader {
    pub version_major: u8,
    pub version_minor: u8,
    pub flags: HeaderFlags,
    pub window_size: u32,
    pub uncompressed_size: Option<u64>,
    pub dictionary_id: Option<u32>,
}

impl Default for StreamHeader {
    fn default() -> Self {
        Self {
            version_major: FORMAT_VERSION_MAJOR,
            version_minor: FORMAT_VERSION_MINOR,
            flags: HeaderFlags {
                has_uncompressed_size: false,
                has_stream_checksum: true,
                has_dictionary: false,
                independent_blocks: false,
                window_code: 1, // 1 MB default
            },
            window_size: 1024 * 1024,
            uncompressed_size: None,
            dictionary_id: None,
        }
    }
}

impl StreamHeader {
    /// Calculate window size from code or descriptor
    pub fn compute_window_size(window_code: u8, custom_exponent: Option<u8>) -> Result<u32, CodropError> {
        match window_code {
            0 => Ok(64 * 1024),         // 64 KB
            1 => Ok(1024 * 1024),       // 1 MB
            2 => Ok(8 * 1024 * 1024),   // 8 MB
            3 => {
                let exp = custom_exponent.ok_or_else(|| CodropError::CorruptedHeader("Missing custom window exponent".into()))?;
                if !(16..=27).contains(&exp) {
                    return Err(CodropError::InvalidWindowSize(1 << exp));
                }
                Ok(1 << exp)
            }
            _ => Err(CodropError::CorruptedHeader("Invalid window code".into())),
        }
    }

    /// Serialize header to writer, computing and appending the CRC-8 checksum
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<usize, CodropError> {
        let mut buf = Vec::with_capacity(32);
        // 1. Magic
        buf.extend_from_slice(&CODROP_MAGIC);

        // 2. Version
        let version_byte = (self.version_major << 4) | (self.version_minor & 0x0F);
        buf.push(version_byte);

        // 3. Flags
        let flags_u16 = self.flags.to_u16();
        buf.extend_from_slice(&flags_u16.to_le_bytes());

        // 4. Custom Window Exponent if window_code == 3
        if self.flags.window_code == 3 {
            let exp = (31 - self.window_size.leading_zeros()) as u8;
            buf.push(exp);
        }

        // 5. Uncompressed Size if flagged (encoded as ULEB128)
        if self.flags.has_uncompressed_size {
            let mut val = self.uncompressed_size.unwrap_or(0);
            loop {
                let mut byte = (val & 0x7F) as u8;
                val >>= 7;
                if val != 0 {
                    byte |= 0x80;
                }
                buf.push(byte);
                if val == 0 {
                    break;
                }
            }
        }

        // 6. Dictionary ID if flagged
        if self.flags.has_dictionary {
            let dict_id = self.dictionary_id.unwrap_or(0);
            buf.extend_from_slice(&dict_id.to_le_bytes());
        }

        // 7. CRC-8 computed over bytes 4 through end of header payload
        let crc = Crc8::compute(&buf[4..]);
        buf.push(crc);

        writer.write_all(&buf)?;
        Ok(buf.len())
    }

    /// Parse header from reader, verifying magic, version, and CRC-8 checksum
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, CodropError> {
        // 1. Magic (4 bytes)
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        validate_magic(&magic)?;

        let mut header_bytes = Vec::with_capacity(32);

        // 2. Version (1 byte)
        let mut version_buf = [0u8; 1];
        reader.read_exact(&mut version_buf)?;
        header_bytes.push(version_buf[0]);

        let major = version_buf[0] >> 4;
        let minor = version_buf[0] & 0x0F;
        if major != FORMAT_VERSION_MAJOR {
            return Err(CodropError::UnsupportedVersion { major, minor });
        }

        // 3. Flags (2 bytes)
        let mut flags_buf = [0u8; 2];
        reader.read_exact(&mut flags_buf)?;
        header_bytes.extend_from_slice(&flags_buf);
        let flags_u16 = u16::from_le_bytes(flags_buf);
        let flags = HeaderFlags::from_u16(flags_u16);

        // 4. Custom Window Exponent if window_code == 3
        let custom_exponent = if flags.window_code == 3 {
            let mut exp_buf = [0u8; 1];
            reader.read_exact(&mut exp_buf)?;
            header_bytes.push(exp_buf[0]);
            Some(exp_buf[0])
        } else {
            None
        };
        let window_size = Self::compute_window_size(flags.window_code, custom_exponent)?;

        // 5. Uncompressed Size if flagged (ULEB128)
        let uncompressed_size = if flags.has_uncompressed_size {
            let mut val: u64 = 0;
            let mut shift = 0;
            loop {
                let mut b = [0u8; 1];
                reader.read_exact(&mut b)?;
                header_bytes.push(b[0]);
                val |= ((b[0] & 0x7F) as u64) << shift;
                if (b[0] & 0x80) == 0 {
                    break;
                }
                shift += 7;
                if shift >= 64 {
                    return Err(CodropError::CorruptedHeader("ULEB128 overflow in uncompressed size".into()));
                }
            }
            Some(val)
        } else {
            None
        };

        // 6. Dictionary ID if flagged
        let dictionary_id = if flags.has_dictionary {
            let mut dict_buf = [0u8; 4];
            reader.read_exact(&mut dict_buf)?;
            header_bytes.extend_from_slice(&dict_buf);
            Some(u32::from_le_bytes(dict_buf))
        } else {
            None
        };

        // 7. CRC-8 (1 byte)
        let mut expected_crc_buf = [0u8; 1];
        reader.read_exact(&mut expected_crc_buf)?;
        let expected_crc = expected_crc_buf[0];
        let actual_crc = Crc8::compute(&header_bytes);
        if expected_crc != actual_crc {
            return Err(CodropError::HeaderChecksumMismatch {
                expected: expected_crc,
                actual: actual_crc,
            });
        }

        Ok(Self {
            version_major: major,
            version_minor: minor,
            flags,
            window_size,
            uncompressed_size,
            dictionary_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_header_roundtrip_default() {
        let header = StreamHeader::default();
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let parsed = StreamHeader::read_from(&mut cursor).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_header_roundtrip_with_size_and_custom_window() {
        let mut header = StreamHeader::default();
        header.flags.has_uncompressed_size = true;
        header.flags.window_code = 3;
        header.window_size = 4 * 1024 * 1024; // 2^22
        header.uncompressed_size = Some(123456789);

        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let parsed = StreamHeader::read_from(&mut cursor).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_header_corrupted_crc() {
        let header = StreamHeader::default();
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();

        // Corrupt CRC-8 byte at the end
        let last = buf.len() - 1;
        buf[last] ^= 0xFF;

        let mut cursor = Cursor::new(&buf);
        let err = StreamHeader::read_from(&mut cursor).unwrap_err();
        assert!(matches!(err, CodropError::HeaderChecksumMismatch { .. }));
    }
}
