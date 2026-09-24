//! Data-aware prefilters and static dictionary transformations.
//!
//! Codrop prefilters transform structured data (JSON, text, sequential streams)
//! before entropy compression (LZH or LZA), then invert the transform during decoding
//! to guarantee 100% byte-exact losslessness.

pub mod delta;
pub mod dict;
pub mod json;
pub mod text;

use crate::codec::lza::LzaCodec;
use crate::codec::lzh::LzhCodec;
use crate::error::{CodropError, CodropResult};
use crate::prefilter::delta::{delta_decode, delta_encode};
use crate::prefilter::dict::{
    find_least_frequent_byte, invert_tokens, lookup_static_dict, substitute_tokens,
};
use crate::prefilter::json::{is_json, json_prefilter_decode, json_prefilter_encode};
use crate::prefilter::text::{is_text, text_prefilter_decode, text_prefilter_encode};

/// Sub-codec backend used to compress transformed prefilter data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PrefilterBackend {
    Lzh = 0,
    Lza = 1,
}

impl TryFrom<u8> for PrefilterBackend {
    type Error = CodropError;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(Self::Lzh),
            1 => Ok(Self::Lza),
            other => Err(CodropError::CorruptedPrefilterData(format!(
                "invalid prefilter sub-codec backend: {other}"
            ))),
        }
    }
}

/// Identifiers for supported prefilter transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PrefilterType {
    Delta = 1,
    StaticDictionary = 2,
    Json = 3,
    Text = 4,
}

impl TryFrom<u8> for PrefilterType {
    type Error = CodropError;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            1 => Ok(Self::Delta),
            2 => Ok(Self::StaticDictionary),
            3 => Ok(Self::Json),
            4 => Ok(Self::Text),
            other => Err(CodropError::UnsupportedPrefilter(other)),
        }
    }
}

/// Result of evaluating a prefilter candidate.
pub struct PrefilterCandidate {
    pub backend: PrefilterBackend,
    pub prefilter_type: PrefilterType,
    pub encoded_payload: Vec<u8>,
}

/// Decodes a complete prefiltered block payload into original uncompressed bytes.
pub fn decode_prefiltered_payload(
    payload: &[u8],
    uncompressed_len: usize,
    max_output_size: usize,
) -> CodropResult<Vec<u8>> {
    // Header format:
    // [0]: sub_codec (1 byte)
    // [1]: prefilter_type (1 byte)
    // [2..6]: transformed_len (4 bytes LE)
    if payload.len() < 6 {
        return Err(CodropError::CorruptedPrefilterData(
            "truncated prefilter block header".to_string(),
        ));
    }

    let sub_codec = PrefilterBackend::try_from(payload[0])?;
    let prefilter_type = PrefilterType::try_from(payload[1])?;
    let transformed_len =
        u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;

    if transformed_len > max_output_size {
        return Err(CodropError::DecompressionBombDetected {
            limit: max_output_size as u64,
            requested: transformed_len as u64,
        });
    }

    let mut cursor = 6;

    match prefilter_type {
        PrefilterType::Delta => {
            if cursor >= payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated delta stride in prefilter payload".to_string(),
                ));
            }
            let stride = payload[cursor];
            cursor += 1;

            let compressed_backend = &payload[cursor..];
            let transformed = decompress_backend(sub_codec, compressed_backend, transformed_len)?;
            let decoded = delta_decode(&transformed, stride, max_output_size)?;
            if decoded.len() != uncompressed_len {
                return Err(CodropError::CorruptedPrefilterData(format!(
                    "decoded length mismatch: expected {uncompressed_len}, got {}",
                    decoded.len()
                )));
            }
            Ok(decoded)
        }
        PrefilterType::StaticDictionary => {
            if cursor + 4 > payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated static dictionary metadata in prefilter payload".to_string(),
                ));
            }
            let dict_id = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]);
            cursor += 2;
            let version = payload[cursor];
            cursor += 1;
            let esc_byte = payload[cursor];
            cursor += 1;

            let tokens = lookup_static_dict(dict_id, version)?;
            let compressed_backend = &payload[cursor..];
            let transformed = decompress_backend(sub_codec, compressed_backend, transformed_len)?;
            let decoded = invert_tokens(&transformed, tokens, esc_byte, max_output_size)?;
            if decoded.len() != uncompressed_len {
                return Err(CodropError::CorruptedPrefilterData(format!(
                    "decoded length mismatch: expected {uncompressed_len}, got {}",
                    decoded.len()
                )));
            }
            Ok(decoded)
        }
        PrefilterType::Json => {
            if cursor + 2 > payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated json prefilter metadata length".to_string(),
                ));
            }
            let meta_len = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
            cursor += 2;

            if cursor + meta_len > payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated json prefilter metadata payload".to_string(),
                ));
            }
            let meta = &payload[cursor..cursor + meta_len];
            cursor += meta_len;

            let compressed_backend = &payload[cursor..];
            let transformed = decompress_backend(sub_codec, compressed_backend, transformed_len)?;
            let decoded = json_prefilter_decode(meta, &transformed, max_output_size)?;
            if decoded.len() != uncompressed_len {
                return Err(CodropError::CorruptedPrefilterData(format!(
                    "decoded length mismatch: expected {uncompressed_len}, got {}",
                    decoded.len()
                )));
            }
            Ok(decoded)
        }
        PrefilterType::Text => {
            if cursor + 2 > payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated text prefilter metadata length".to_string(),
                ));
            }
            let meta_len = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
            cursor += 2;

            if cursor + meta_len > payload.len() {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated text prefilter metadata payload".to_string(),
                ));
            }
            let meta = &payload[cursor..cursor + meta_len];
            cursor += meta_len;

            let compressed_backend = &payload[cursor..];
            let transformed = decompress_backend(sub_codec, compressed_backend, transformed_len)?;
            let decoded = text_prefilter_decode(meta, &transformed, max_output_size)?;
            if decoded.len() != uncompressed_len {
                return Err(CodropError::CorruptedPrefilterData(format!(
                    "decoded length mismatch: expected {uncompressed_len}, got {}",
                    decoded.len()
                )));
            }
            Ok(decoded)
        }
    }
}

fn decompress_backend(
    backend: PrefilterBackend,
    src: &[u8],
    transformed_len: usize,
) -> CodropResult<Vec<u8>> {
    match backend {
        PrefilterBackend::Lzh => LzhCodec::decode(src, transformed_len),
        PrefilterBackend::Lza => LzaCodec::decode(src, transformed_len),
    }
}

fn compress_backend(backend: PrefilterBackend, transformed: &[u8]) -> Vec<u8> {
    match backend {
        PrefilterBackend::Lzh => LzhCodec::encode(transformed),
        PrefilterBackend::Lza => LzaCodec::encode(transformed),
    }
}

/// Tries to encode `src` with a suitable prefilter + entropy backend.
/// Returns `Some(candidate)` if any prefilter was applicable and produced an encoded payload.
#[allow(unused_assignments)]
pub fn try_encode_prefilter(
    src: &[u8],
    backend: PrefilterBackend,
    _level: u8,
) -> Option<PrefilterCandidate> {
    if src.len() < 32 {
        return None;
    }

    let mut best_candidate: Option<PrefilterCandidate> = None;
    let mut best_len = usize::MAX;

    // 1. Try JSON prefilter if detected
    if is_json(src) {
        // Try static JSON dictionary first
        if let Ok(dict_tokens) = lookup_static_dict(dict::DICT_ID_JSON, dict::DICT_VERSION_CURRENT)
        {
            let esc = find_least_frequent_byte(src);
            let transformed = substitute_tokens(src, dict_tokens, esc);
            if transformed.len() < src.len() {
                let comp = compress_backend(backend, &transformed);
                if !comp.is_empty() {
                    let mut payload = Vec::with_capacity(10 + comp.len());
                    payload.push(backend as u8);
                    payload.push(PrefilterType::StaticDictionary as u8);
                    payload.extend_from_slice(&(transformed.len() as u32).to_le_bytes());
                    payload.extend_from_slice(&dict::DICT_ID_JSON.to_le_bytes());
                    payload.push(dict::DICT_VERSION_CURRENT);
                    payload.push(esc);
                    payload.extend_from_slice(&comp);

                    if payload.len() < best_len {
                        best_len = payload.len();
                        best_candidate = Some(PrefilterCandidate {
                            backend,
                            prefilter_type: PrefilterType::StaticDictionary,
                            encoded_payload: payload,
                        });
                    }
                }
            }
        }

        // Try dynamic JSON key prefilter
        if let Some((meta, transformed)) = json_prefilter_encode(src) {
            let comp = compress_backend(backend, &transformed);
            if !comp.is_empty() && meta.len() <= u16::MAX as usize {
                let mut payload = Vec::with_capacity(8 + meta.len() + comp.len());
                payload.push(backend as u8);
                payload.push(PrefilterType::Json as u8);
                payload.extend_from_slice(&(transformed.len() as u32).to_le_bytes());
                payload.extend_from_slice(&(meta.len() as u16).to_le_bytes());
                payload.extend_from_slice(&meta);
                payload.extend_from_slice(&comp);

                if payload.len() < best_len {
                    best_len = payload.len();
                    best_candidate = Some(PrefilterCandidate {
                        backend,
                        prefilter_type: PrefilterType::Json,
                        encoded_payload: payload,
                    });
                }
            }
        }
    } else if is_text(src) {
        // 2. Try Web static dictionary or dynamic text prefilter
        if let Ok(web_tokens) = lookup_static_dict(dict::DICT_ID_WEB, dict::DICT_VERSION_CURRENT) {
            let esc = find_least_frequent_byte(src);
            let transformed = substitute_tokens(src, web_tokens, esc);
            if transformed.len() + 16 < src.len() {
                let comp = compress_backend(backend, &transformed);
                if !comp.is_empty() {
                    let mut payload = Vec::with_capacity(10 + comp.len());
                    payload.push(backend as u8);
                    payload.push(PrefilterType::StaticDictionary as u8);
                    payload.extend_from_slice(&(transformed.len() as u32).to_le_bytes());
                    payload.extend_from_slice(&dict::DICT_ID_WEB.to_le_bytes());
                    payload.push(dict::DICT_VERSION_CURRENT);
                    payload.push(esc);
                    payload.extend_from_slice(&comp);

                    if payload.len() < best_len {
                        best_len = payload.len();
                        best_candidate = Some(PrefilterCandidate {
                            backend,
                            prefilter_type: PrefilterType::StaticDictionary,
                            encoded_payload: payload,
                        });
                    }
                }
            }
        }

        if let Some((meta, transformed)) = text_prefilter_encode(src) {
            let comp = compress_backend(backend, &transformed);
            if !comp.is_empty() && meta.len() <= u16::MAX as usize {
                let mut payload = Vec::with_capacity(8 + meta.len() + comp.len());
                payload.push(backend as u8);
                payload.push(PrefilterType::Text as u8);
                payload.extend_from_slice(&(transformed.len() as u32).to_le_bytes());
                payload.extend_from_slice(&(meta.len() as u16).to_le_bytes());
                payload.extend_from_slice(&meta);
                payload.extend_from_slice(&comp);

                if payload.len() < best_len {
                    best_len = payload.len();
                    best_candidate = Some(PrefilterCandidate {
                        backend,
                        prefilter_type: PrefilterType::Text,
                        encoded_payload: payload,
                    });
                }
            }
        }
    } else {
        // 3. Try Delta prefilter for binary / sequential data
        for stride in [1u8, 2, 4] {
            if let Ok(transformed) = delta_encode(src, stride) {
                let comp = compress_backend(backend, &transformed);
                if !comp.is_empty() {
                    let mut payload = Vec::with_capacity(7 + comp.len());
                    payload.push(backend as u8);
                    payload.push(PrefilterType::Delta as u8);
                    payload.extend_from_slice(&(transformed.len() as u32).to_le_bytes());
                    payload.push(stride);
                    payload.extend_from_slice(&comp);

                    if payload.len() < best_len {
                        best_len = payload.len();
                        best_candidate = Some(PrefilterCandidate {
                            backend,
                            prefilter_type: PrefilterType::Delta,
                            encoded_payload: payload,
                        });
                    }
                }
            }
        }
    }

    best_candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefilter_json_roundtrip_lzh() {
        let json_data = br#"[
            {"user_id": 100, "status": "active", "created_at": "2026-09-24", "role": "admin"},
            {"user_id": 101, "status": "active", "created_at": "2026-09-24", "role": "user"},
            {"user_id": 102, "status": "active", "created_at": "2026-09-24", "role": "user"},
            {"user_id": 103, "status": "active", "created_at": "2026-09-24", "role": "editor"}
        ]"#;

        let candidate = try_encode_prefilter(json_data, PrefilterBackend::Lzh, 5)
            .expect("should produce candidate");
        let decoded =
            decode_prefiltered_payload(&candidate.encoded_payload, json_data.len(), 4096).unwrap();
        assert_eq!(decoded, json_data);
    }

    #[test]
    fn test_prefilter_delta_roundtrip_lza() {
        let mut ramp = Vec::new();
        for i in 0..500 {
            ramp.push((i * 2) as u8);
        }

        let candidate = try_encode_prefilter(&ramp, PrefilterBackend::Lza, 5)
            .expect("delta candidate should be found");
        assert_eq!(candidate.prefilter_type, PrefilterType::Delta);

        let decoded =
            decode_prefiltered_payload(&candidate.encoded_payload, ramp.len(), 4096).unwrap();
        assert_eq!(decoded, ramp);
    }
}
