use libcodrop::error::CodropError;
use libcodrop::prefilter::delta::{delta_decode, delta_encode};
use libcodrop::prefilter::dict::{
    find_least_frequent_byte, invert_tokens, lookup_static_dict, substitute_tokens, DICT_ID_JSON,
    DICT_ID_WEB, DICT_VERSION_CURRENT,
};
use libcodrop::prefilter::json::{is_json, json_prefilter_decode, json_prefilter_encode};
use libcodrop::prefilter::text::{is_text, text_prefilter_decode, text_prefilter_encode};
use libcodrop::prefilter::{decode_prefiltered_payload, try_encode_prefilter, PrefilterBackend};
use libcodrop::{compress, decompress, CompressionLevel};

// ==========================================
// 1. Text Prefilter Dedicated Tests
// ==========================================

#[test]
fn test_text_plain_ascii_and_multilingual_utf8() {
    let ascii_text = b"The architecture of Codrop enables high efficiency compression. Codrop is portable. Codrop is reliable.";
    assert!(is_text(ascii_text));

    let utf8_multilingual = "Codrop 支持多种语言压缩。Codrop is fast! Codrop est rapide! Codrop ist schnell! Codrop 高速です。".as_bytes();
    assert!(is_text(utf8_multilingual));

    let compressed = compress(utf8_multilingual, CompressionLevel::Compact).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, utf8_multilingual);
}

#[test]
fn test_text_repeated_words_and_phrases() {
    let phrase =
        "lossless compression algorithm with data-aware prefilters and verifiable round-trips. ";
    let repeated = phrase.repeat(50).into_bytes();

    let (meta, transformed) =
        text_prefilter_encode(&repeated).expect("repeated text should encode");
    assert!(transformed.len() + meta.len() < repeated.len());

    let restored = text_prefilter_decode(&meta, &transformed, repeated.len() * 2).unwrap();
    assert_eq!(restored, repeated);

    // Full CDP stream roundtrip under Balanced and Compact
    for level in [CompressionLevel::Balanced, CompressionLevel::Compact] {
        let cdp = compress(&repeated, level).unwrap();
        let dec = decompress(&cdp).unwrap();
        assert_eq!(dec, repeated);
    }
}

#[test]
fn test_text_whitespace_and_punctuation_heavy() {
    let input = b"    \t\t\n\n  --- [KEY: VALUE] ---   \t\n  --- [KEY: VALUE] ---   \t\n  --- [KEY: VALUE] ---   \t\n";
    assert!(is_text(input));

    let compressed = compress(input, CompressionLevel::Compact).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, input);
}

#[test]
fn test_text_classifier_rejects_binary_random() {
    let random_binary: Vec<u8> = (0..512).map(|i| ((i * 73 + 19) % 256) as u8).collect();
    assert!(!is_text(&random_binary));
}

// ==========================================
// 2. JSON Prefilter Dedicated Tests
// ==========================================

#[test]
fn test_json_empty_and_minimal_structures() {
    assert!(is_json(b"{}"));
    assert!(is_json(b"[]"));
    assert!(is_json(b"  {   }  "));
    assert!(is_json(b"  [   ]  "));

    for empty in [b"{}" as &[u8], b"[]", b"  {\n}\n"] {
        let compressed = compress(empty, CompressionLevel::Compact).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, empty);
    }
}

#[test]
fn test_json_nested_structures_and_arrays() {
    let json = br#"{
  "store": {
    "book": [
      {
        "category": "reference",
        "author": "Nigel Rees",
        "title": "Sayings of the Century",
        "price": 8.95
      },
      {
        "category": "fiction",
        "author": "Evelyn Waugh",
        "title": "Sword of Honour",
        "price": 12.99
      },
      {
        "category": "fiction",
        "author": "Herman Melville",
        "title": "Moby Dick",
        "isbn": "0-553-21311-3",
        "price": 8.99
      }
    ],
    "bicycle": {
      "color": "red",
      "price": 19.95
    }
  },
  "expensive": 10
}"#;

    assert!(is_json(json));

    // Test explicit JSON key prefilter encode & decode
    if let Some((meta, transformed)) = json_prefilter_encode(json) {
        let restored = json_prefilter_decode(&meta, &transformed, json.len() * 2).unwrap();
        assert_eq!(restored, json);
    }

    // Test try_encode_prefilter candidate creation
    let cand = try_encode_prefilter(json, PrefilterBackend::Lzh, 5);
    assert!(cand.is_some());

    for level in [
        CompressionLevel::Fast,
        CompressionLevel::Balanced,
        CompressionLevel::Compact,
    ] {
        let compressed = compress(json, level).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, json);
    }
}

#[test]
fn test_json_escaped_strings_unicode_and_numbers() {
    let json = br#"{"escaped": "quote: \" and newline: \n and unicode: \u0041", "pi": 3.141592653589793, "exp": -1.25e+10, "neg": -42, "zero": 0}"#;
    assert!(is_json(json));

    let compressed = compress(json, CompressionLevel::Compact).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, json);
}

#[test]
fn test_json_classifier_rejects_code_and_pseudo_json() {
    assert!(!is_json(b"int main() { printf(\"hello\"); return 0; }"));
    assert!(!is_json(b"{ unquoted_key: not_json }"));
    assert!(!is_json(b"{ broken braces"));
    assert!(!is_json(b"[ unbalanced bracket }"));
}

// ==========================================
// 3. Delta Prefilter Dedicated Tests
// ==========================================

#[test]
fn test_delta_monotonic_and_repeating() {
    for stride in 1..=4 {
        // Monotonic ramp
        let ramp: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let enc_ramp = delta_encode(&ramp, stride).unwrap();
        let dec_ramp = delta_decode(&enc_ramp, stride, 2048).unwrap();
        assert_eq!(dec_ramp, ramp);

        // Repeating sequence
        let repeating: Vec<u8> = (0..1000)
            .map(|i| ((i % stride as usize) * 40) as u8)
            .collect();
        let enc_rep = delta_encode(&repeating, stride).unwrap();
        let dec_rep = delta_decode(&enc_rep, stride, 2048).unwrap();
        assert_eq!(dec_rep, repeating);
    }
}

#[test]
fn test_delta_constant_values() {
    for val in [0u8, 127, 255] {
        let block = vec![val; 500];
        let enc = delta_encode(&block, 1).unwrap();
        let dec = delta_decode(&enc, 1, 1024).unwrap();
        assert_eq!(dec, block);
    }
}

#[test]
fn test_delta_wraparound_extremes() {
    let data = vec![0, 255, 0, 255, 1, 254, 2, 253];
    let enc = delta_encode(&data, 2).unwrap();
    let dec = delta_decode(&enc, 2, 64).unwrap();
    assert_eq!(dec, data);
}

// ==========================================
// 4. Static Dictionary Tests
// ==========================================

#[test]
fn test_static_dictionary_known_registrations() {
    let json_dict = lookup_static_dict(DICT_ID_JSON, DICT_VERSION_CURRENT).unwrap();
    assert!(!json_dict.is_empty());

    let web_dict = lookup_static_dict(DICT_ID_WEB, DICT_VERSION_CURRENT).unwrap();
    assert!(!web_dict.is_empty());

    assert!(lookup_static_dict(9999, DICT_VERSION_CURRENT).is_err());
    assert!(lookup_static_dict(DICT_ID_JSON, 99).is_err());
}

#[test]
fn test_static_dictionary_web_html_roundtrip() {
    let html = b"<!DOCTYPE html><html><head><title>Test Page</title><meta name=\"viewport\" content=\"width=device-width\"></head><body><div class=\"container\"><p>Hello Codrop!</p></div></body></html>";
    let dict = lookup_static_dict(DICT_ID_WEB, DICT_VERSION_CURRENT).unwrap();
    let esc = find_least_frequent_byte(html);

    let transformed = substitute_tokens(html, dict, esc);
    let restored = invert_tokens(&transformed, dict, esc, 1024).unwrap();
    assert_eq!(restored, html);
}

// ==========================================
// 5. Malformed Stream & Safety Limits Tests
// ==========================================

#[test]
fn test_malformed_prefilter_header_too_short() {
    // Less than 6 bytes
    let bad_payload = vec![0, 1, 2];
    assert!(decode_prefiltered_payload(&bad_payload, 10, 100).is_err());
}

#[test]
fn test_malformed_prefilter_unknown_type() {
    // sub_codec = 0, prefilter_type = 99 (unknown)
    let mut bad_payload = vec![0, 99, 10, 0, 0, 0];
    bad_payload.extend_from_slice(b"some bytes");
    let err = decode_prefiltered_payload(&bad_payload, 10, 100).unwrap_err();
    match err {
        CodropError::UnsupportedPrefilter(99) => {}
        other => panic!("expected UnsupportedPrefilter(99), got {:?}", other),
    }
}

#[test]
fn test_malformed_prefilter_unknown_dict() {
    // sub_codec = 0, prefilter_type = StaticDictionary(2), transformed_len = 10
    // dict_id = 0xbeef, version = 1, esc = 0
    let mut bad_payload = vec![0, 2, 10, 0, 0, 0];
    bad_payload.extend_from_slice(&0xbeefu16.to_le_bytes());
    bad_payload.push(1); // version
    bad_payload.push(0); // esc_byte
    bad_payload.extend_from_slice(b"fakecomp");

    let err = decode_prefiltered_payload(&bad_payload, 10, 100).unwrap_err();
    match err {
        CodropError::UnknownDictionary { dict_id, version } => {
            assert_eq!(dict_id, 0xbeef);
            assert_eq!(version, 1);
        }
        other => panic!("expected UnknownDictionary, got {:?}", other),
    }
}

#[test]
fn test_malformed_prefilter_decompression_bomb_limit() {
    // transformed_len = 1,000,000 but limit = 1,000
    let mut bomb = vec![0, 1]; // sub_codec = 0, prefilter = 1 (Delta)
    bomb.extend_from_slice(&1_000_000u32.to_le_bytes());
    bomb.push(1); // stride

    let err = decode_prefiltered_payload(&bomb, 1_000_000, 1_000).unwrap_err();
    match err {
        CodropError::DecompressionBombDetected { limit, requested } => {
            assert_eq!(limit, 1_000);
            assert_eq!(requested, 1_000_000);
        }
        other => panic!("expected DecompressionBombDetected, got {:?}", other),
    }
}

// ==========================================
// 6. Randomized Round-Trip Testing
// ==========================================

#[test]
fn test_randomized_json_like_roundtrips() {
    for seed in 1..=20 {
        let mut json = String::from("[\n");
        for i in 0..15 {
            json.push_str(&format!(
                "  {{\"index\": {}, \"hash\": \"h_{:x}\", \"active\": {}, \"value\": {}}},\n",
                i,
                (i * 37 + seed * 991),
                if i % 2 == 0 { "true" } else { "false" },
                (i * seed) as f64 * 1.5
            ));
        }
        json.push_str("  {\"final\": null}\n]");

        let bytes = json.into_bytes();
        let compressed = compress(&bytes, CompressionLevel::Compact).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, bytes, "Failed for seed {seed}");
    }
}
