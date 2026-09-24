use crate::checksum::{Crc32c, StreamHash64};
use crate::codec::{RawCodec, RleCodec};
use crate::error::CodropError;
use crate::format::{BlockHeader, BlockType, StreamHeader};
use std::io::Write;

pub const DEFAULT_BLOCK_SIZE: usize = 128 * 1024; // 128 KB

/// Compression levels supported by the Codrop public API.
/// For M0, all levels utilize the core RAW / RLE decision pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    Fast,
    #[default]
    Balanced,
    Compact,
    Auto,
}

pub struct EncoderOptions {
    pub level: CompressionLevel,
    pub block_size: usize,
    pub include_block_checksum: bool,
    pub include_stream_checksum: bool,
    pub known_uncompressed_size: Option<u64>,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        Self {
            level: CompressionLevel::Balanced,
            block_size: DEFAULT_BLOCK_SIZE,
            include_block_checksum: true,
            include_stream_checksum: true,
            known_uncompressed_size: None,
        }
    }
}

pub struct Encoder<W: Write> {
    writer: W,
    options: EncoderOptions,
    buffer: Vec<u8>,
    stream_hasher: StreamHash64,
    header_written: bool,
    finished: bool,
}

impl<W: Write> Encoder<W> {
    pub fn new(writer: W, options: EncoderOptions) -> Self {
        let block_size = options.block_size;
        Self {
            writer,
            options,
            buffer: Vec::with_capacity(block_size),
            stream_hasher: StreamHash64::new(),
            header_written: false,
            finished: false,
        }
    }

    fn ensure_header_written(&mut self) -> Result<(), CodropError> {
        if !self.header_written {
            let mut header = StreamHeader::default();
            header.flags.has_stream_checksum = self.options.include_stream_checksum;
            if let Some(size) = self.options.known_uncompressed_size {
                header.flags.has_uncompressed_size = true;
                header.uncompressed_size = Some(size);
            }
            header.write_to(&mut self.writer)?;
            self.header_written = true;
        }
        Ok(())
    }

    pub fn write_chunk(&mut self, mut data: &[u8]) -> Result<usize, CodropError> {
        if self.finished {
            return Err(CodropError::Io("Encoder already finished".into()));
        }
        self.ensure_header_written()?;

        let total_written = data.len();
        self.stream_hasher.update(data);

        while !data.is_empty() {
            let space = self.options.block_size - self.buffer.len();
            let to_copy = data.len().min(space);
            self.buffer.extend_from_slice(&data[..to_copy]);
            data = &data[to_copy..];

            if self.buffer.len() == self.options.block_size {
                self.flush_block(false)?;
            }
        }

        Ok(total_written)
    }

    fn flush_block(&mut self, is_last: bool) -> Result<(), CodropError> {
        if self.buffer.is_empty() && !is_last {
            return Ok(());
        }

        let uncompressed_len = self.buffer.len() as u32;

        // M0 Decision Logic & Expansion Safeguard:
        // Try RLE. If RLE produces a strictly smaller result, use RLE; otherwise use RAW.
        let rle_candidate = RleCodec::encode(&self.buffer);
        let (chosen_type, compressed) = if rle_candidate.len() < self.buffer.len() {
            (BlockType::Rle, rle_candidate)
        } else {
            (BlockType::Raw, RawCodec::encode(&self.buffer))
        };

        let checksum = if self.options.include_block_checksum {
            Some(Crc32c::compute(&self.buffer))
        } else {
            None
        };

        let block_header = BlockHeader {
            block_type: chosen_type,
            has_checksum: self.options.include_block_checksum,
            is_last,
            compressed_size: compressed.len() as u32,
            uncompressed_size: uncompressed_len,
            checksum,
        };

        block_header.write_to(&mut self.writer)?;
        self.writer.write_all(&compressed)?;
        self.buffer.clear();

        Ok(())
    }

    pub fn finish(mut self) -> Result<W, CodropError> {
        if self.finished {
            return Ok(self.writer);
        }
        self.ensure_header_written()?;

        // Flush remaining buffered data as the last block
        if !self.buffer.is_empty() {
            self.flush_block(true)?;
        }

        // Emit EndOfStream sentinel block
        let eos = BlockHeader::end_of_stream();
        eos.write_to(&mut self.writer)?;

        // Write stream checksum if enabled
        if self.options.include_stream_checksum {
            let stream_hash = self.stream_hasher.finalize();
            self.writer.write_all(&stream_hash.to_le_bytes())?;
        }

        self.writer.flush()?;
        self.finished = true;
        Ok(self.writer)
    }
}
