use crate::error::CodropError;

pub struct RawCodec;

impl RawCodec {
    pub fn encode(data: &[u8]) -> Vec<u8> {
        data.to_vec()
    }

    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        if compressed.len() != expected_len {
            return Err(CodropError::CorruptedHeader(format!(
                "Raw block length mismatch: compressed {} != expected {}",
                compressed.len(),
                expected_len
            )));
        }
        Ok(compressed.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_codec_roundtrip() {
        let original = b"Arbitrary uncompressible binary or text payload 12345";
        let enc = RawCodec::encode(original);
        assert_eq!(enc, original);
        let dec = RawCodec::decode(&enc, original.len()).unwrap();
        assert_eq!(dec, original);
    }
}
