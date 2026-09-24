use crate::error::CodropError;

pub struct RleCodec;

impl RleCodec {
    /// Encode input slice as sequential runs: `[byte_value: 1B] [run_length: ULEB128]`
    pub fn encode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() / 2 + 16);
        let mut i = 0;
        while i < data.len() {
            let val = data[i];
            let mut run_len = 1usize;
            while i + run_len < data.len() && data[i + run_len] == val {
                run_len += 1;
            }
            i += run_len;

            // Emit byte value
            out.push(val);

            // Emit run_length as ULEB128
            let mut l = run_len;
            loop {
                let mut b = (l & 0x7F) as u8;
                l >>= 7;
                if l != 0 {
                    b |= 0x80;
                }
                out.push(b);
                if l == 0 {
                    break;
                }
            }
        }
        out
    }

    /// Decode compressed RLE byte slice into output buffer
    pub fn decode(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodropError> {
        let mut out = Vec::with_capacity(expected_len);
        let mut i = 0;

        while i < compressed.len() {
            let val = compressed[i];
            i += 1;

            // Decode ULEB128 run length
            let mut run_len: usize = 0;
            let mut shift = 0;
            let mut done = false;
            while i < compressed.len() {
                let b = compressed[i];
                i += 1;
                run_len |= ((b & 0x7F) as usize) << shift;
                if (b & 0x80) == 0 {
                    done = true;
                    break;
                }
                shift += 7;
                if shift >= 32 {
                    return Err(CodropError::CorruptedEntropyStream("RLE run length overflow".into()));
                }
            }

            if !done {
                return Err(CodropError::UnexpectedEof);
            }

            if run_len == 0 {
                return Err(CodropError::CorruptedEntropyStream("Zero run length in RLE".into()));
            }

            if out.len() + run_len > expected_len {
                return Err(CodropError::InvalidMatchLength {
                    length: run_len,
                    remaining: expected_len.saturating_sub(out.len()),
                });
            }

            out.resize(out.len() + run_len, val);
        }

        if out.len() != expected_len {
            return Err(CodropError::CorruptedEntropyStream(format!(
                "RLE decoded length mismatch: got {} expected {}",
                out.len(),
                expected_len
            )));
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_roundtrip() {
        let mut original = Vec::new();
        original.extend(vec![0xAA; 1000]);
        original.extend(vec![0xBB; 25]);
        original.extend(vec![0x00; 5000]);
        original.extend(b"Hello World!");

        let enc = RleCodec::encode(&original);
        assert!(enc.len() < original.len());

        let dec = RleCodec::decode(&enc, original.len()).unwrap();
        assert_eq!(dec, original);
    }

    #[test]
    fn test_rle_overflow_protection() {
        let mut bad_stream = Vec::new();
        bad_stream.push(0x42);
        // ULEB128 of 1000
        bad_stream.push(0xE8);
        bad_stream.push(0x07);

        // Expected length is only 500
        let err = RleCodec::decode(&bad_stream, 500).unwrap_err();
        assert!(matches!(err, CodropError::InvalidMatchLength { .. }));
    }
}
