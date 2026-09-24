use crate::format::BlockType;
use std::fmt;

/// Result alias for Codrop operations.
pub type CodropResult<T> = Result<T, CodropError>;

/// Primary error type for all Codrop operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodropError {
    /// Stream does not begin with the valid "CDP1" magic identifier.
    InvalidMagic([u8; 4]),

    /// Stream specifies a major/minor version unsupported by this decoder.
    UnsupportedVersion { major: u8, minor: u8 },

    /// Header checksum failed verification.
    HeaderChecksumMismatch { expected: u8, actual: u8 },

    /// Header contains corrupted or structurally contradictory fields.
    CorruptedHeader(String),

    /// Encountered an unknown or illegal block type value.
    InvalidBlockType(u8),

    /// Block type is valid according to format specification, but unsupported in this version/build (e.g. LZF/LZH/LZA in M0).
    UnsupportedBlockType(BlockType),

    /// Backward match offset references data outside the initialized history window.
    InvalidOffset { offset: usize, max_valid: usize },

    /// Match length exceeds the remaining block or buffer boundary.
    InvalidMatchLength { length: usize, remaining: usize },

    /// Entropy stream (Huffman / ANS) contains illegal prefix codes or bitstream corruption.
    CorruptedEntropyStream(String),

    /// Block checksum (CRC32c) does not match decompressed block contents.
    BlockChecksumMismatch { expected: u32, actual: u32 },

    /// Whole-stream checksum does not match decoded data.
    StreamChecksumMismatch { expected: u64, actual: u64 },

    /// Stream ended prematurely before the `EndOfStream` terminator block.
    UnexpectedEof,

    /// Uncompressed size exceeds the configured safety threshold (decompression bomb protection).
    DecompressionBombDetected { limit: u64, requested: u64 },

    /// Configured window size exceeds the maximum allowable threshold.
    InvalidWindowSize(u32),

    /// Memory allocation request exceeds the configured safety limit.
    MemoryLimitExceeded { limit: usize, requested: usize },

    /// Prefilter type is unknown or unsupported.
    UnsupportedPrefilter(u8),

    /// Static dictionary identifier or version is unrecognized.
    UnknownDictionary { dict_id: u16, version: u8 },

    /// Prefilter stream data or token descriptor is corrupted.
    CorruptedPrefilterData(String),

    /// Generic I/O error during reading or writing.
    Io(String),
}

impl fmt::Display for CodropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodropError::InvalidMagic(m) => {
                write!(
                    f,
                    "Invalid Codrop magic: {:02X} {:02X} {:02X} {:02X}",
                    m[0], m[1], m[2], m[3]
                )
            }
            CodropError::UnsupportedVersion { major, minor } => {
                write!(f, "Unsupported format version: {}.{}", major, minor)
            }
            CodropError::HeaderChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Header CRC-8 mismatch: expected 0x{:02X}, found 0x{:02X}",
                    expected, actual
                )
            }
            CodropError::CorruptedHeader(msg) => write!(f, "Corrupted header: {}", msg),
            CodropError::InvalidBlockType(t) => write!(f, "Invalid block type: {}", t),
            CodropError::UnsupportedBlockType(t) => {
                write!(f, "Unsupported block type for current codec: {:?}", t)
            }
            CodropError::InvalidOffset { offset, max_valid } => {
                write!(
                    f,
                    "Invalid match offset {} (max valid: {})",
                    offset, max_valid
                )
            }
            CodropError::InvalidMatchLength { length, remaining } => {
                write!(
                    f,
                    "Invalid match length {} (remaining: {})",
                    length, remaining
                )
            }
            CodropError::CorruptedEntropyStream(msg) => {
                write!(f, "Corrupted entropy stream: {}", msg)
            }
            CodropError::BlockChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Block CRC32c mismatch: expected 0x{:08X}, found 0x{:08X}",
                    expected, actual
                )
            }
            CodropError::StreamChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Stream checksum mismatch: expected 0x{:016X}, found 0x{:016X}",
                    expected, actual
                )
            }
            CodropError::UnexpectedEof => write!(f, "Unexpected end of stream"),
            CodropError::DecompressionBombDetected { limit, requested } => {
                write!(
                    f,
                    "Decompression bomb detected: requested {} exceeds limit {}",
                    requested, limit
                )
            }
            CodropError::InvalidWindowSize(size) => write!(f, "Invalid window size: {}", size),
            CodropError::MemoryLimitExceeded { limit, requested } => {
                write!(
                    f,
                    "Memory limit exceeded: requested {} exceeds limit {}",
                    requested, limit
                )
            }
            CodropError::UnsupportedPrefilter(id) => {
                write!(f, "Unsupported prefilter type: {}", id)
            }
            CodropError::UnknownDictionary { dict_id, version } => {
                write!(
                    f,
                    "Unknown dictionary ID 0x{:04X} version {}",
                    dict_id, version
                )
            }
            CodropError::CorruptedPrefilterData(msg) => {
                write!(f, "Corrupted prefilter data: {}", msg)
            }
            CodropError::Io(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for CodropError {}

impl From<std::io::Error> for CodropError {
    fn from(err: std::io::Error) -> Self {
        CodropError::Io(err.to_string())
    }
}
