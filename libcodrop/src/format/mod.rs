pub mod block;
pub mod flags;
pub mod header;
pub mod magic;

pub use block::{BlockHeader, BlockType};
pub use flags::HeaderFlags;
pub use header::StreamHeader;
pub use magic::{validate_magic, CODROP_MAGIC};
