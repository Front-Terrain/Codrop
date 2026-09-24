use crate::checksum::StreamHash64;
use crate::codec::{LzaCodec, LzfCodec, LzhCodec, RawCodec, RleCodec};
use crate::error::CodropError;
use crate::format::{BlockHeader, BlockType, StreamHeader};
use std::io::{Read, Write};

pub struct DecoderOptions {
    pub max_output_bytes: Option<u64>,
    pub verify_block_checksums: bool,
    pub verify_stream_checksum: bool,
}

impl Default for DecoderOptions {
    fn default() -> Self {
        Self {
            max_output_bytes: Some(1024 * 1024 * 1024), // 1 GB default safety clamp
            verify_block_checksums: true,
            verify_stream_checksum: true,
        }
    }
}

pub struct Decoder<R: Read> {
    reader: R,
    options: DecoderOptions,
    header: StreamHeader,
    stream_hasher: StreamHash64,
    total_decompressed: u64,
    stream_finished: bool,
}

impl<R: Read> Decoder<R> {
    pub fn new(mut reader: R, options: DecoderOptions) -> Result<Self, CodropError> {
        let header = StreamHeader::read_from(&mut reader)?;
        Ok(Self {
            reader,
            options,
            header,
            stream_hasher: StreamHash64::new(),
            total_decompressed: 0,
            stream_finished: false,
        })
    }

    pub fn header(&self) -> &StreamHeader {
        &self.header
    }

    /// Decode the next block and return its decompressed bytes.
    /// Returns Ok(None) when stream is cleanly completed via EndOfStream.
    pub fn decode_next_block(&mut self) -> Result<Option<Vec<u8>>, CodropError> {
        if self.stream_finished {
            return Ok(None);
        }

        let block_header = BlockHeader::read_from(&mut self.reader)?;

        if block_header.block_type == BlockType::EndOfStream {
            self.stream_finished = true;
            if self.header.flags.has_stream_checksum && self.options.verify_stream_checksum {
                let mut expected_hash_bytes = [0u8; 8];
                if let Err(e) = self.reader.read_exact(&mut expected_hash_bytes) {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Err(CodropError::UnexpectedEof);
                    }
                    return Err(CodropError::Io(e.to_string()));
                }
                let expected = u64::from_le_bytes(expected_hash_bytes);
                let actual = self.stream_hasher.finalize();
                if expected != actual {
                    return Err(CodropError::StreamChecksumMismatch { expected, actual });
                }
            }
            return Ok(None);
        }

        // Decompression bomb check against accumulated total
        let uncompressed_len = block_header.uncompressed_size as u64;
        let new_total = self
            .total_decompressed
            .checked_add(uncompressed_len)
            .ok_or(CodropError::MemoryLimitExceeded {
                limit: usize::MAX,
                requested: usize::MAX,
            })?;

        if let Some(limit) = self.options.max_output_bytes {
            if new_total > limit {
                return Err(CodropError::DecompressionBombDetected {
                    limit,
                    requested: new_total,
                });
            }
        }

        // Read raw compressed block payload
        let mut compressed_buf = vec![0u8; block_header.compressed_size as usize];
        if let Err(e) = self.reader.read_exact(&mut compressed_buf) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(CodropError::UnexpectedEof);
            }
            return Err(CodropError::Io(e.to_string()));
        }

        // Decode based on block type (RAW, RLE, LZF, LZH, and LZA)
        let decompressed = match block_header.block_type {
            BlockType::Raw => {
                RawCodec::decode(&compressed_buf, block_header.uncompressed_size as usize)?
            }
            BlockType::Rle => {
                RleCodec::decode(&compressed_buf, block_header.uncompressed_size as usize)?
            }
            BlockType::Lzf => {
                LzfCodec::decode(&compressed_buf, block_header.uncompressed_size as usize)?
            }
            BlockType::Lzh => {
                LzhCodec::decode(&compressed_buf, block_header.uncompressed_size as usize)?
            }
            BlockType::Lza => {
                LzaCodec::decode(&compressed_buf, block_header.uncompressed_size as usize)?
            }
            BlockType::TextPrefilter => crate::prefilter::decode_prefiltered_payload(
                &compressed_buf,
                block_header.uncompressed_size as usize,
                block_header.uncompressed_size as usize,
            )?,
            unsupported => {
                return Err(CodropError::UnsupportedBlockType(unsupported));
            }
        };

        // Verify block CRC32c
        if self.options.verify_block_checksums {
            block_header.verify_checksum(&decompressed)?;
        }

        self.stream_hasher.update(&decompressed);
        self.total_decompressed = new_total;

        Ok(Some(decompressed))
    }

    /// Decompress all blocks directly into a writer
    pub fn decompress_to<W: Write>(&mut self, writer: &mut W) -> Result<u64, CodropError> {
        let mut total = 0u64;
        while let Some(block) = self.decode_next_block()? {
            writer.write_all(&block)?;
            total += block.len() as u64;
        }
        writer.flush()?;
        Ok(total)
    }
}
