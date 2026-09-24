use crate::error::CodropError;

/// Standard 4-byte magic sequence for Codrop streams: "CDP1" (0x43, 0x44, 0x50, 0x31)
pub const CODROP_MAGIC: [u8; 4] = [0x43, 0x44, 0x50, 0x31];

/// Validate that the provided 4-byte slice matches the Codrop magic identifier.
pub fn validate_magic(bytes: &[u8]) -> Result<(), CodropError> {
    if bytes.len() < 4 {
        return Err(CodropError::UnexpectedEof);
    }
    let magic: [u8; 4] = bytes[0..4].try_into().unwrap();
    if magic != CODROP_MAGIC {
        return Err(CodropError::InvalidMagic(magic));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_magic() {
        assert!(validate_magic(&CODROP_MAGIC).is_ok());
    }

    #[test]
    fn test_invalid_magic() {
        assert_eq!(
            validate_magic(b"GZIP"),
            Err(CodropError::InvalidMagic(*b"GZIP"))
        );
    }
}
