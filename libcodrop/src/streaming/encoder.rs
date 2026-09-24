use crate::checksum::{Crc32c, StreamHash64};
use crate::codec::{LzaCodec, LzfCodec, LzhCodec, RawCodec, RleCodec};
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

        // Decision Logic & Expansion Safeguard (Stage 4 / M2):
        // Fast profile: primarily LZF (with RLE/RAW fallback).
        // Balanced profile: evaluates LZH (along with LZF, RLE, and RAW fallback).
        // Auto / Compact: evaluates all available codecs (RAW, RLE, LZF, LZH).
        let (chosen_type, compressed) = match self.options.level {
            CompressionLevel::Fast => {
                let lzf_candidate = LzfCodec::encode(&self.buffer);
                let rle_candidate = RleCodec::encode(&self.buffer);
                if lzf_candidate.len() < self.buffer.len()
                    && lzf_candidate.len() <= rle_candidate.len()
                {
                    (BlockType::Lzf, lzf_candidate)
                } else if rle_candidate.len() < self.buffer.len() {
                    (BlockType::Rle, rle_candidate)
                } else {
                    (BlockType::Raw, RawCodec::encode(&self.buffer))
                }
            }
            CompressionLevel::Balanced => {
                let mut best_type = BlockType::Raw;
                let mut best_payload = RawCodec::encode(&self.buffer);

                let rle_candidate = RleCodec::encode(&self.buffer);
                if rle_candidate.len() < best_payload.len() {
                    best_type = BlockType::Rle;
                    best_payload = rle_candidate;
                }

                let lzf_candidate = LzfCodec::encode(&self.buffer);
                if lzf_candidate.len() < best_payload.len() {
                    best_type = BlockType::Lzf;
                    best_payload = lzf_candidate;
                }

                let lzh_candidate = LzhCodec::encode(&self.buffer);
                if lzh_candidate.len() < best_payload.len() {
                    best_type = BlockType::Lzh;
                    best_payload = lzh_candidate;
                }

                if let Some(cand) = crate::prefilter::try_encode_prefilter(
                    &self.buffer,
                    crate::prefilter::PrefilterBackend::Lzh,
                    5,
                ) {
                    if cand.encoded_payload.len() < best_payload.len() {
                        best_type = BlockType::TextPrefilter;
                        best_payload = cand.encoded_payload;
                    }
                }

                (best_type, best_payload)
            }
            CompressionLevel::Compact | CompressionLevel::Auto => {
                let mut best_type = BlockType::Raw;
                let mut best_payload = RawCodec::encode(&self.buffer);

                let rle_candidate = RleCodec::encode(&self.buffer);
                if rle_candidate.len() < best_payload.len() {
                    best_type = BlockType::Rle;
                    best_payload = rle_candidate;
                }

                let lzf_candidate = LzfCodec::encode(&self.buffer);
                if lzf_candidate.len() < best_payload.len() {
                    best_type = BlockType::Lzf;
                    best_payload = lzf_candidate;
                }

                let lzh_candidate = LzhCodec::encode(&self.buffer);
                if lzh_candidate.len() < best_payload.len() {
                    best_type = BlockType::Lzh;
                    best_payload = lzh_candidate;
                }

                let lza_candidate = LzaCodec::encode(&self.buffer);
                if !lza_candidate.is_empty() && lza_candidate.len() < best_payload.len() {
                    best_type = BlockType::Lza;
                    best_payload = lza_candidate;
                }

                if let Some(cand) = crate::prefilter::try_encode_prefilter(
                    &self.buffer,
                    crate::prefilter::PrefilterBackend::Lza,
                    9,
                ) {
                    if cand.encoded_payload.len() < best_payload.len() {
                        best_type = BlockType::TextPrefilter;
                        best_payload = cand.encoded_payload;
                    }
                }

                if let Some(cand) = crate::prefilter::try_encode_prefilter(
                    &self.buffer,
                    crate::prefilter::PrefilterBackend::Lzh,
                    9,
                ) {
                    if cand.encoded_payload.len() < best_payload.len() {
                        best_type = BlockType::TextPrefilter;
                        best_payload = cand.encoded_payload;
                    }
                }

                (best_type, best_payload)
            }
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
