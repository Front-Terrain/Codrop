//! Conservative JSON detection and byte-preserving JSON prefilter.
//!
//! Exposes repetitive JSON structure (object keys, repeated string fields,
//! delimiters) to downstream compression without changing a single byte of formatting,
//! indentation, quotes, or numbers.

use std::collections::HashMap;

use crate::error::{CodropError, CodropResult};
use crate::prefilter::dict::{
    find_least_frequent_byte, invert_tokens, substitute_tokens, MAX_DICT_TOKENS,
};
use crate::prefilter::text::{deserialize_token_table, serialize_token_table};

/// Conservatively determines whether `data` is likely JSON.
pub fn is_json(data: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(data);
    if trimmed.len() < 2 {
        return false;
    }
    let first = trimmed[0];
    let last = trimmed[trimmed.len() - 1];

    let is_object = first == b'{' && last == b'}';
    let is_array = first == b'[' && last == b']';

    if !is_object && !is_array {
        return false;
    }

    // Quick structural check: must contain colons or quotes or balanced brackets
    let mut quote_count = 0usize;
    let mut colon_count = 0usize;
    let mut comma_count = 0usize;
    let mut depth = 0i32;

    for &b in trimmed {
        match b {
            b'"' => quote_count += 1,
            b':' => colon_count += 1,
            b',' => comma_count += 1,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return false;
    }

    let interior = trim_ascii_whitespace(&trimmed[1..trimmed.len() - 1]);
    if is_object {
        // If non-empty, must have quotes and at least one colon
        if !interior.is_empty() && (quote_count < 2 || colon_count == 0) {
            return false;
        }
    } else if is_array && !interior.is_empty() && interior.len() > 2 {
        // Multi-element array should have commas or colons or quotes
        if comma_count == 0 && quote_count == 0 && colon_count == 0 {
            return false;
        }
    }

    // Must be valid UTF-8
    std::str::from_utf8(trimmed).is_ok()
}

fn trim_ascii_whitespace(mut slice: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = slice.split_first() {
        if first.is_ascii_whitespace() {
            slice = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = slice.split_last() {
        if last.is_ascii_whitespace() {
            slice = rest;
        } else {
            break;
        }
    }
    slice
}

/// Extracts repeated JSON keys, e.g. `"property": ` or `"name":`.
pub fn extract_json_keys(src: &[u8], max_tokens: usize) -> Vec<Vec<u8>> {
    let mut candidates: HashMap<&[u8], usize> = HashMap::new();
    let mut i = 0;
    let n = src.len();

    while i < n {
        if src[i] == b'"' {
            let start = i;
            i += 1;
            while i < n && src[i] != b'"' {
                if src[i] == b'\\' && i + 1 < n {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < n && src[i] == b'"' {
                i += 1;
                // Check if followed by colon (a JSON key)
                let key_quote_end = i;
                let mut post = i;
                while post < n && (src[post] == b' ' || src[post] == b'\t') {
                    post += 1;
                }
                if post < n && src[post] == b':' {
                    post += 1;
                    while post < n && (src[post] == b' ' || src[post] == b'\t') {
                        post += 1;
                    }
                    // candidate is the whole `"key": ` pattern
                    let token = &src[start..post];
                    if token.len() >= 4 && token.len() <= 48 {
                        *candidates.entry(token).or_insert(0) += 1;
                    }
                } else {
                    // Regular string value
                    let token = &src[start..key_quote_end];
                    if token.len() >= 6 && token.len() <= 48 {
                        *candidates.entry(token).or_insert(0) += 1;
                    }
                }
            }
        } else {
            i += 1;
        }
    }

    // Score candidates by estimated savings:
    let mut scored: Vec<(&[u8], i64)> = candidates
        .into_iter()
        .filter(|(token, freq)| *freq >= 2 && token.len() >= 4)
        .map(|(token, freq)| {
            let savings = (freq as i64) * (token.len() as i64 - 2) - (1 + token.len() as i64);
            (token, savings)
        })
        .filter(|&(_, savings)| savings > 6)
        .collect();

    // Sort descending by savings, break ties deterministically
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let limit = max_tokens.min(MAX_DICT_TOKENS);
    scored
        .into_iter()
        .take(limit)
        .map(|(t, _)| t.to_vec())
        .collect()
}

/// Encodes JSON data by extracting repeated keys and tokens.
/// Returns `Some((metadata_bytes, transformed_payload))` if compression is advantageous.
pub fn json_prefilter_encode(src: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let tokens = extract_json_keys(src, 64);
    if tokens.is_empty() {
        return None;
    }

    let esc_byte = find_least_frequent_byte(src);
    let token_slices: Vec<&[u8]> = tokens.iter().map(|t| t.as_slice()).collect();
    let transformed = substitute_tokens(src, &token_slices, esc_byte);

    let mut meta = Vec::new();
    meta.push(esc_byte);
    meta.extend_from_slice(&serialize_token_table(&tokens));

    if transformed.len() + meta.len() >= src.len() {
        return None;
    }

    Some((meta, transformed))
}

/// Inverses JSON prefilter to recover exact original bytes.
pub fn json_prefilter_decode(
    meta: &[u8],
    transformed: &[u8],
    max_output_size: usize,
) -> CodropResult<Vec<u8>> {
    if meta.is_empty() {
        return Err(CodropError::CorruptedPrefilterData(
            "empty json prefilter metadata".to_string(),
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
    fn test_is_json() {
        assert!(is_json(b"{\"id\": 1, \"name\": \"test\"}"));
        assert!(is_json(b"[\n  {\"id\": 1},\n  {\"id\": 2}\n]"));
        assert!(!is_json(b"{ this is not valid json }"));
        assert!(!is_json(b"int main() { return 0; }"));
        assert!(!is_json(b""));
    }

    #[test]
    fn test_json_prefilter_round_trip() {
        let json_data = br#"[
  {"product_id": 1001, "product_name": "Keyboard", "category_tag": "accessories", "in_stock": true},
  {"product_id": 1002, "product_name": "Mouse", "category_tag": "accessories", "in_stock": true},
  {"product_id": 1003, "product_name": "Monitor", "category_tag": "displays", "in_stock": false},
  {"product_id": 1004, "product_name": "Headphones", "category_tag": "audio", "in_stock": true}
]"#;
        let (meta, transformed) = json_prefilter_encode(json_data).expect("keys extracted");
        assert!(transformed.len() + meta.len() < json_data.len());

        let restored = json_prefilter_decode(&meta, &transformed, 2048).unwrap();
        assert_eq!(restored, json_data);
    }
}
