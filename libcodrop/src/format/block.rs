use std::io::{Read, Write};
use crate::checksum::Crc32c;
use crate::error::CodropError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Raw = 0,
    Rle = 1,
    Lzf = 2,
    Lzh = 3,
    Lza = 4,
    TextPrefilter = 5,
    Reserved = 6,
    EndOfStream = 7,
}

impl TryFrom<u8> for BlockType {
    type Error = CodropError;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(BlockType::Raw),
            1 => Ok(BlockType::Rle),
            2 => Ok(BlockType::Lzf),
            3 => Ok(BlockType::Lzh),
            4 => Ok(BlockType::Lza),
            5 => Ok(BlockType::TextPrefilter),
            6 => Ok(BlockType::Reserved),
            7 => Ok(BlockType::EndOfStream),
            _ => Err(CodropError::InvalidBlockType(val)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHeader {
    pub block_type: BlockType,
    pub has_checksum: bool,
    pub is_last: bool,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub checksum: Option<u32>,
}

impl BlockHeader {
    pub fn end_of_stream() -> Self {
        Self {
            block_type: BlockType::EndOfStream,
            has_checksum: false,
            is_last: true,
            compressed_size: 0,
            uncompressed_size: 0,
            checksum: None,
        }
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<usize, CodropError> {
        let mut written = 0;
        let mut byte0 = (self.block_type as u8) & 0x07;
        if self.has_checksum {
            byte0 |= 1 << 3;
        }
        if self.is_last {
            byte0 |= 1 << 4;
        }
        writer.write_all(&[byte0])?;
        written += 1;

        if self.block_type == BlockType::EndOfStream {
            return Ok(written);
        }

        // Compressed Size: 16-bit LE, or 0xFFFF followed by 32-bit LE
        if self.compressed_size < 0xFFFF {
            writer.write_all(&(self.compressed_size as u16).to_le_bytes())?;
            written += 2;
        } else {
            writer.write_all(&0xFFFFu16.to_le_bytes())?;
            writer.write_all(&self.compressed_size.to_le_bytes())?;
            written += 6;
        }

        // Uncompressed Size: ULEB128
        let mut u_size = self.uncompressed_size;
        loop {
            let mut byte = (u_size & 0x7F) as u8;
            u_size >>= 7;
            if u_size != 0 {
                byte |= 0x80;
            }
            writer.write_all(&[byte])?;
            written += 1;
            if u_size == 0 {
                break;
            }
        }

        // Block Checksum (CRC32c) if enabled
        if self.has_checksum {
            let chk = self.checksum.unwrap_or(0);
            writer.write_all(&chk.to_le_bytes())?;
            written += 4;
        }

        Ok(written)
    }

    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, CodropError> {
        let mut b0 = [0u8; 1];
        if let Err(e) = reader.read_exact(&mut b0) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(CodropError::UnexpectedEof);
            }
            return Err(CodropError::Io(e.to_string()));
        }

        let block_type = BlockType::try_from(b0[0] & 0x07)?;
        let has_checksum = (b0[0] & (1 << 3)) != 0;
        let is_last = (b0[0] & (1 << 4)) != 0;

        if block_type == BlockType::EndOfStream {
            return Ok(Self {
                block_type,
                has_checksum: false,
                is_last: true,
                compressed_size: 0,
                uncompressed_size: 0,
                checksum: None,
            });
        }

        // Compressed size
        let mut c_size_buf = [0u8; 2];
        reader.read_exact(&mut c_size_buf)?;
        let c_size_16 = u16::from_le_bytes(c_size_buf);
        let compressed_size = if c_size_16 < 0xFFFF {
            c_size_16 as u32
        } else {
            let mut ext_buf = [0u8; 4];
            reader.read_exact(&mut ext_buf)?;
            u32::from_le_bytes(ext_buf)
        };

        // Uncompressed size (ULEB128)
        let mut uncompressed_size: u32 = 0;
        let mut shift = 0;
        loop {
            let mut b = [0u8; 1];
            reader.read_exact(&mut b)?;
            uncompressed_size |= ((b[0] & 0x7F) as u32) << shift;
            if (b[0] & 0x80) == 0 {
                break;
            }
            shift += 7;
            if shift >= 32 {
                return Err(CodropError::CorruptedHeader("ULEB128 overflow in block uncompressed size".into()));
            }
        }

        // Checksum
        let checksum = if has_checksum {
            let mut chk_buf = [0u8; 4];
            reader.read_exact(&mut chk_buf)?;
            Some(u32::from_le_bytes(chk_buf))
        } else {
            None
        };

        Ok(Self {
            block_type,
            has_checksum,
            is_last,
            compressed_size,
            uncompressed_size,
            checksum,
        })
    }

    pub fn verify_checksum(&self, decompressed_data: &[u8]) -> Result<(), CodropError> {
        if self.has_checksum {
            let expected = self.checksum.ok_or(CodropError::CorruptedHeader("Missing block checksum".into()))?;
            let actual = Crc32c::compute(decompressed_data);
            if expected != actual {
                return Err(CodropError::BlockChecksumMismatch { expected, actual });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_block_header_roundtrip() {
        let header = BlockHeader {
            block_type: BlockType::Lzh,
            has_checksum: true,
            is_last: false,
            compressed_size: 4096,
            uncompressed_size: 16384,
            checksum: Some(0xDEADBEEF),
        };

        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let parsed = BlockHeader::read_from(&mut cursor).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_eos_block_roundtrip() {
        let eos = BlockHeader::end_of_stream();
        let mut buf = Vec::new();
        eos.write_to(&mut buf).unwrap();

        let mut cursor = Cursor::new(&buf);
        let parsed = BlockHeader::read_from(&mut cursor).unwrap();
        assert_eq!(parsed.block_type, BlockType::EndOfStream);
        assert!(parsed.is_last);
    }
}
