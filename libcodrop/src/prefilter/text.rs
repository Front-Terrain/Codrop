//! Conservative text detection and repeated-token prefilter.
//!
//! Identifies repeated word/token sequences in textual data, replaces them with
//! compact references, and preserves exact byte-for-byte fidelity upon decompression.

use std::collections::HashMap;

use crate::error::{CodropError, CodropResult};
use crate::prefilter::dict::{
    find_least_frequent_byte, invert_tokens, substitute_tokens, MAX_DICT_TOKENS,
};

/// Maximum dynamic tokens extracted in a single block.
pub const MAX_DYNAMIC_TOKENS: usize = 64;
/// Minimum token length for dynamic extraction.
pub const MIN_TOKEN_LEN: usize = 4;
/// Maximum token length for dynamic extraction.
pub const MAX_TOKEN_LEN: usize = 32;

/// Conservatively determines whether `data` is likely human-readable or structured text.
pub fn is_text(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    // Inspect a sample up to 4096 bytes for speed
    let sample = if data.len() > 4096 {
        &data[..4096]
    } else {
        data
    };

    // Check for null bytes or excessive non-printable control characters
    let mut printable_count = 0usize;
    let mut whitespace_count = 0usize;

    for &b in sample {
        if b == 0 {
            return false; // Binary indicator
        }
        if b.is_ascii_graphic() {
            printable_count += 1;
        } else if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            printable_count += 1;
            whitespace_count += 1;
        } else if b >= 0x80 {
            // Potential UTF-8 multi-byte
            printable_count += 1;
        }
    }

    let printable_ratio = printable_count as f64 / sample.len() as f64;
    let whitespace_ratio = whitespace_count as f64 / sample.len() as f64;

    // Must be overwhelmingly printable and have reasonable whitespace distribution
    if printable_ratio < 0.92 || whitespace_ratio < 0.05 {
        return false;
    }

    // Verify UTF-8 validity if non-ASCII bytes are present
    std::str::from_utf8(sample).is_ok()
}

/// Identifies repeated words/tokens that yield a net size reduction.
pub fn extract_repeated_tokens(src: &[u8], max_tokens: usize) -> Vec<Vec<u8>> {
    let mut candidates: HashMap<&[u8], usize> = HashMap::new();

    let mut start = 0;
    while start < src.len() {
        // Find token boundaries (alphanumeric sequences or punctuation chains)
        while start < src.len()
            && (src[start] == b' '
                || src[start] == b'\n'
                || src[start] == b'\r'
                || src[start] == b'\t')
        {
            start += 1;
        }
        if start >= src.len() {
            break;
        }

        let mut end = start;
        while end < src.len()
            && src[end] != b' '
            && src[end] != b'\n'
            && src[end] != b'\r'
            && src[end] != b'\t'
        {
            if end - start >= MAX_TOKEN_LEN {
                break;
            }
            end += 1;
        }

        let token = &src[start..end];
        if token.len() >= MIN_TOKEN_LEN {
            *candidates.entry(token).or_insert(0) += 1;
        }
        start = end;
    }

    // Score candidates by estimated savings:
    // Savings = freq * (len - 2) - (1 + len) [table overhead]
    let mut scored: Vec<(&[u8], i64)> = candidates
        .into_iter()
        .filter(|(token, freq)| *freq >= 3 && token.len() >= MIN_TOKEN_LEN)
        .map(|(token, freq)| {
            let savings = (freq as i64) * (token.len() as i64 - 2) - (1 + token.len() as i64);
            (token, savings)
        })
        .filter(|&(_, savings)| savings > 8)
        .collect();

    // Sort descending by savings, break ties by token bytes for determinism
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let limit = max_tokens.min(MAX_DYNAMIC_TOKENS).min(MAX_DICT_TOKENS);
    scored
        .into_iter()
        .take(limit)
        .map(|(t, _)| t.to_vec())
        .collect()
}

/// Serializes dynamic tokens into metadata bytes:
/// `[num_tokens: u8, for each token: (len: u8, bytes)]`
pub fn serialize_token_table(tokens: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(tokens.len() as u8);
    for token in tokens {
        out.push(token.len() as u8);
        out.extend_from_slice(token);
    }
    out
}

/// Deserializes dynamic tokens from metadata bytes.
pub fn deserialize_token_table(src: &[u8], offset: &mut usize) -> CodropResult<Vec<Vec<u8>>> {
    if *offset >= src.len() {
        return Err(CodropError::CorruptedPrefilterData(
            "truncated token table header".to_string(),
        ));
    }
    let num_tokens = src[*offset] as usize;
    *offset += 1;

    let mut tokens = Vec::with_capacity(num_tokens);
    for _ in 0..num_tokens {
        if *offset >= src.len() {
            return Err(CodropError::CorruptedPrefilterData(
                "truncated token table entry length".to_string(),
            ));
        }
        let t_len = src[*offset] as usize;
        *offset += 1;

        if t_len == 0 || t_len > MAX_TOKEN_LEN {
            return Err(CodropError::CorruptedPrefilterData(format!(
                "invalid token length in table: {t_len}"
            )));
        }

        if *offset + t_len > src.len() {
            return Err(CodropError::CorruptedPrefilterData(
                "truncated token table entry data".to_string(),
            ));
        }
        tokens.push(src[*offset..*offset + t_len].to_vec());
        *offset += t_len;
    }

    Ok(tokens)
}

/// Encodes text data using dynamic token substitution.
/// Returns (metadata_bytes, transformed_payload).
pub fn text_prefilter_encode(src: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let tokens = extract_repeated_tokens(src, MAX_DYNAMIC_TOKENS);
    if tokens.is_empty() {
        return None;
    }

    let esc_byte = find_least_frequent_byte(src);
    let token_slices: Vec<&[u8]> = tokens.iter().map(|t| t.as_slice()).collect();
    let transformed = substitute_tokens(src, &token_slices, esc_byte);

    let mut meta = Vec::new();
    meta.push(esc_byte);
    meta.extend_from_slice(&serialize_token_table(&tokens));

    // Only proceed if transformed + metadata is genuinely smaller than src
    if transformed.len() + meta.len() >= src.len() {
        return None;
    }

    Some((meta, transformed))
}

/// Inverses text prefilter to recover original text bytes.
pub fn text_prefilter_decode(
    meta: &[u8],
    transformed: &[u8],
    max_output_size: usize,
) -> CodropResult<Vec<u8>> {
    if meta.is_empty() {
        return Err(CodropError::CorruptedPrefilterData(
            "empty text prefilter metadata".to_string(),
        ));
    }
    let esc_byte = meta[0];
    let mut offset = 1;
    let tokens = deserialize_token_table(meta, &mut offset)?;
    let token_slices: Vec<&[u8]> = tokens.iter().map(|t| t.as_slice()).collect();

    invert_tokens(transformed, &token_slices, esc_byte, max_output_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_text() {
        assert!(is_text(b"The quick brown fox jumps over the lazy dog."));
        assert!(is_text(b"{\n  \"message\": \"Hello, World!\"\n}\n"));
        assert!(!is_text(&[0x00, 0xFF, 0x02, 0x03, 0x80]));
        assert!(!is_text(b""));
    }

    #[test]
    fn test_text_prefilter_round_trip() {
        let input = b"Codrop is a universal compression codec. Codrop provides lossless compression. Codrop is fast, and Codrop is portable. Codrop preserves exact bytes.";
        let (meta, transformed) = text_prefilter_encode(input).expect("tokens extracted");
        assert!(transformed.len() + meta.len() < input.len());

        let restored = text_prefilter_decode(&meta, &transformed, 1024).unwrap();
        assert_eq!(restored, input);
    }
}
