//! Static dictionary registry and token-substitution prefilter.
//!
//! Exposes repetitive structured tokens to downstream compression while preserving
//! 100% exact byte losslessness.
//!
//! # Escaping Mechanism
//! A dynamic escape byte `esc_byte` (the byte appearing with minimum frequency in `src`)
//! is chosen.
//! - A literal occurrence of `esc_byte` in `src` is encoded as `[esc_byte, 0xFF]`.
//! - A dictionary token reference at index `k` (< 254) is encoded as `[esc_byte, k]`.
//! - All other bytes are passed through unchanged.
//!
//! Decoding is deterministic, byte-exact, and linear in output size.

use crate::error::{CodropError, CodropResult};

pub const DICT_ID_JSON: u16 = 0x0001;
pub const DICT_ID_WEB: u16 = 0x0002;
pub const DICT_VERSION_CURRENT: u8 = 1;
pub const MAX_DICT_TOKENS: usize = 254;
pub const ESC_LITERAL_TAG: u8 = 0xFF;

/// Static JSON token dictionary (v1).
static JSON_DICT_V1: &[&[u8]] = &[
    b"\"id\":",
    b"\"name\":",
    b"\"type\":",
    b"\"status\":",
    b"\"value\":",
    b"\"description\":",
    b"\"title\":",
    b"\"data\":",
    b"\"items\":",
    b"\"error\":",
    b"\"message\":",
    b"\"code\":",
    b"\"success\":",
    b"\"created_at\":",
    b"\"updated_at\":",
    b"\"timestamp\":",
    b"\"version\":",
    b"\"enabled\":",
    b"\"count\":",
    b"\"total\":",
    b"\"results\":",
    b"\"properties\":",
    b"\"attributes\":",
    b"\"config\":",
    b"\"options\":",
    b"\"email\":",
    b"\"user\":",
    b"\"role\":",
    b"\"token\":",
    b"\"active\":",
    b"\"true\"",
    b"\"false\"",
    b"\"null\"",
    b": true",
    b": false",
    b": null",
    b": \"\"",
    b": []",
    b": {}",
    b": \"",
    b"\", \"",
    b"\",\n",
    b"\",\r\n",
    b"},\n",
    b"},\r\n",
    b"],\n",
    b"],\r\n",
    b"{\n  \"",
    b"{\r\n  \"",
    b"    \"",
    b"  \"",
    b"\"}",
    b"\"]",
    b"http://",
    b"https://",
    b"application/json",
];

/// Static Web (HTML/CSS/JS) token dictionary (v1).
static WEB_DICT_V1: &[&[u8]] = &[
    b"<!DOCTYPE html>",
    b"<html",
    b"</html>",
    b"<head>",
    b"</head>",
    b"<body>",
    b"</body>",
    b"<meta ",
    b"<title>",
    b"</title>",
    b"<link rel=\"stylesheet\" ",
    b"<script",
    b"</script>",
    b"<div class=\"",
    b"<div id=\"",
    b"<div>",
    b"</div>",
    b"<span class=\"",
    b"<span>",
    b"</span>",
    b"<a href=\"",
    b"</a>",
    b"<p>",
    b"</p>",
    b"<ul>",
    b"</ul>",
    b"<li>",
    b"</li>",
    b"<button ",
    b"</button>",
    b"<input type=\"",
    b"class=\"",
    b"style=\"",
    b"id=\"",
    b"href=\"",
    b"src=\"",
    b"alt=\"",
    b"width=\"",
    b"height=\"",
    b"display: flex;",
    b"display: none;",
    b"display: block;",
    b"position: absolute;",
    b"position: relative;",
    b"margin: 0;",
    b"padding: 0;",
    b"box-sizing: border-box;",
    b"text-align: center;",
    b"color: #",
    b"background-color: #",
    b"font-family: ",
    b"font-size: ",
    b"addEventListener(",
    b"querySelector(",
    b"querySelectorAll(",
    b"getElementById(",
    b"function(",
    b"return ",
    b"const ",
    b"let ",
    b"var ",
    b"console.log(",
    b"document.",
    b"window.",
];

/// Look up a registered static dictionary by ID and version.
pub fn lookup_static_dict(dict_id: u16, version: u8) -> CodropResult<&'static [&'static [u8]]> {
    match (dict_id, version) {
        (DICT_ID_JSON, DICT_VERSION_CURRENT) => Ok(JSON_DICT_V1),
        (DICT_ID_WEB, DICT_VERSION_CURRENT) => Ok(WEB_DICT_V1),
        _ => Err(CodropError::UnknownDictionary { dict_id, version }),
    }
}

/// Find the byte value (0..=255) that occurs least frequently in `src`.
pub fn find_least_frequent_byte(src: &[u8]) -> u8 {
    let mut freqs = [0usize; 256];
    for &b in src {
        freqs[b as usize] += 1;
    }
    let mut min_idx = 0usize;
    let mut min_count = freqs[0];
    for (i, count) in freqs.iter().copied().enumerate().skip(1) {
        if count < min_count {
            min_count = count;
            min_idx = i;
            if min_count == 0 {
                break;
            }
        }
    }
    min_idx as u8
}

/// Substitute dictionary tokens in `src` using the specified `esc_byte`.
pub fn substitute_tokens(src: &[u8], tokens: &[&[u8]], esc_byte: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let mut pos = 0;
    let n = src.len();

    while pos < n {
        // Try greedy match on dictionary tokens
        let mut best_idx = None;
        let mut best_len = 0;

        for (idx, token) in tokens.iter().enumerate() {
            if idx >= MAX_DICT_TOKENS {
                break;
            }
            let t_len = token.len();
            if t_len > best_len && pos + t_len <= n && &src[pos..pos + t_len] == *token {
                best_idx = Some(idx);
                best_len = t_len;
            }
        }

        // We only substitute if the token length is >= 3 (saves at least 1 byte: len >= 3 replaced by 2 bytes)
        if let Some(idx) = best_idx {
            if best_len >= 3 {
                out.push(esc_byte);
                out.push(idx as u8);
                pos += best_len;
                continue;
            }
        }

        let b = src[pos];
        if b == esc_byte {
            out.push(esc_byte);
            out.push(ESC_LITERAL_TAG);
        } else {
            out.push(b);
        }
        pos += 1;
    }

    out
}

/// Reverse token substitution to recover exact original bytes.
pub fn invert_tokens(
    src: &[u8],
    tokens: &[&[u8]],
    esc_byte: u8,
    max_output_size: usize,
) -> CodropResult<Vec<u8>> {
    let mut out = Vec::with_capacity(src.len());
    let mut i = 0;
    let n = src.len();

    while i < n {
        let b = src[i];
        if b == esc_byte {
            i += 1;
            if i >= n {
                return Err(CodropError::CorruptedPrefilterData(
                    "truncated escape sequence in dictionary stream".to_string(),
                ));
            }
            let tag = src[i];
            i += 1;

            if tag == ESC_LITERAL_TAG {
                if out.len() >= max_output_size {
                    return Err(CodropError::DecompressionBombDetected {
                        limit: max_output_size as u64,
                        requested: (out.len() + 1) as u64,
                    });
                }
                out.push(esc_byte);
            } else {
                let token_idx = tag as usize;
                if token_idx >= tokens.len() {
                    return Err(CodropError::CorruptedPrefilterData(format!(
                        "out of bounds dictionary token index: {token_idx} (max: {})",
                        tokens.len().saturating_sub(1)
                    )));
                }
                let token = tokens[token_idx];
                if out.len() + token.len() > max_output_size {
                    return Err(CodropError::DecompressionBombDetected {
                        limit: max_output_size as u64,
                        requested: (out.len() + token.len()) as u64,
                    });
                }
                out.extend_from_slice(token);
            }
        } else {
            if out.len() >= max_output_size {
                return Err(CodropError::DecompressionBombDetected {
                    limit: max_output_size as u64,
                    requested: (out.len() + 1) as u64,
                });
            }
            out.push(b);
            i += 1;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_static_dict() {
        assert!(lookup_static_dict(DICT_ID_JSON, 1).is_ok());
        assert!(lookup_static_dict(DICT_ID_WEB, 1).is_ok());
        assert!(lookup_static_dict(DICT_ID_JSON, 2).is_err());
        assert!(lookup_static_dict(999, 1).is_err());
    }

    #[test]
    fn test_dict_token_substitution_roundtrip() {
        let text =
            br#"{"id": 101, "name": "Codrop", "status": "active", "value": true, "data": null}"#;
        let tokens = lookup_static_dict(DICT_ID_JSON, 1).unwrap();
        let esc = find_least_frequent_byte(text);
        let transformed = substitute_tokens(text, tokens, esc);
        // Transformed should be smaller than original
        assert!(transformed.len() < text.len());
        let restored = invert_tokens(&transformed, tokens, esc, 1024).unwrap();
        assert_eq!(restored, text);
    }

    #[test]
    fn test_dict_with_literal_escape_byte() {
        let tokens: &[&[u8]] = &[b"hello", b"world"];
        let esc = b'X';
        let original = b"hello X world X hello";
        let transformed = substitute_tokens(original, tokens, esc);
        let restored = invert_tokens(&transformed, tokens, esc, 1024).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn test_dict_corrupted_stream() {
        let tokens: &[&[u8]] = &[b"hello"];
        let esc = 0xAA;
        // Truncated escape
        assert!(invert_tokens(&[esc], tokens, esc, 100).is_err());
        // Invalid token index
        assert!(invert_tokens(&[esc, 5], tokens, esc, 100).is_err());
    }
}
