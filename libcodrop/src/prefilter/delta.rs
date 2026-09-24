//! Byte-wise delta prefilter for Codrop.
//!
//! Provides strictly lossless 1D byte-difference transformation:
//! `output[0] = input[0]`
//! `output[i] = input[i] - input[i - stride] (mod 256)`
//!
//! The inverse transformation exactly recovers the original byte sequence:
//! `input[0] = output[0]`
//! `input[i] = input[i - stride] + output[i] (mod 256)`

use crate::error::{CodropError, CodropResult};

/// Maximum supported stride for delta encoding.
pub const MAX_DELTA_STRIDE: u8 = 8;

/// Encodes `src` using modulo-256 byte difference with the given `stride`.
pub fn delta_encode(src: &[u8], stride: u8) -> CodropResult<Vec<u8>> {
    if stride == 0 || stride > MAX_DELTA_STRIDE {
        return Err(CodropError::CorruptedPrefilterData(format!(
            "invalid delta stride: {stride} (must be 1..={MAX_DELTA_STRIDE})"
        )));
    }
    let s = stride as usize;
    let mut out = Vec::with_capacity(src.len());
    for (i, &b) in src.iter().enumerate() {
        if i < s {
            out.push(b);
        } else {
            out.push(b.wrapping_sub(src[i - s]));
        }
    }
    Ok(out)
}

/// Inverses delta encoding to recover the exact original bytes.
pub fn delta_decode(src: &[u8], stride: u8, max_output_size: usize) -> CodropResult<Vec<u8>> {
    if stride == 0 || stride > MAX_DELTA_STRIDE {
        return Err(CodropError::CorruptedPrefilterData(format!(
            "invalid delta stride: {stride} (must be 1..={MAX_DELTA_STRIDE})"
        )));
    }
    if src.len() > max_output_size {
        return Err(CodropError::DecompressionBombDetected {
            limit: max_output_size as u64,
            requested: src.len() as u64,
        });
    }
    let s = stride as usize;
    let mut out = Vec::with_capacity(src.len());
    for (i, &b) in src.iter().enumerate() {
        if i < s {
            out.push(b);
        } else {
            let prev = out[i - s];
            out.push(prev.wrapping_add(b));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_round_trip_strides() {
        for stride in 1..=4 {
            let data: Vec<u8> = (0..500).map(|i| ((i * 3 + 7) % 256) as u8).collect();
            let encoded = delta_encode(&data, stride).unwrap();
            let decoded = delta_decode(&encoded, stride, 1024).unwrap();
            assert_eq!(decoded, data);
        }
    }

    #[test]
    fn test_delta_wraparound() {
        let data = vec![0u8, 255, 0, 1, 254, 255, 0];
        let encoded = delta_encode(&data, 1).unwrap();
        let decoded = delta_decode(&encoded, 1, 64).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_delta_empty() {
        let encoded = delta_encode(&[], 1).unwrap();
        assert!(encoded.is_empty());
        let decoded = delta_decode(&encoded, 1, 64).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_delta_invalid_stride() {
        assert!(delta_encode(&[1, 2, 3], 0).is_err());
        assert!(delta_encode(&[1, 2, 3], 9).is_err());
        assert!(delta_decode(&[1, 2, 3], 0, 64).is_err());
        assert!(delta_decode(&[1, 2, 3], 9, 64).is_err());
    }
}
