// Port of services/rendering/layout/typography_memory/features.py — the
// deterministic feature-hash builder. The learning / store modules feed
// `observe_payload_typography` (C3-N3) and are not ported here.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use crate::item::Item;
use crate::payload::text_common::{source_word_count, translated_zh_char_count, translation_density_ratio};
use crate::semantics::{block_kind, is_title_like_block, layout_role, semantic_role, structure_role};
use crate::typography::content::formula_ratio;
use crate::typography::line_count::source_visual_line_count;
use crate::typography::line_metrics::{bbox_height, bbox_width};
use crate::util::py_round;

pub const TYPOGRAPHY_MEMORY_FEATURE_VERSION: &str = "typography_memory_features_v1";

#[derive(Debug, Clone, PartialEq)]
pub struct TypographyFeature {
    pub key: String,
    pub payload: serde_json::Map<String, serde_json::Value>,
}

fn linear_bin(value: f64, step: f64) -> i64 {
    py_round(value.max(0.0) / step.max(0.1), 0) as i64
}

fn log_bin(value: usize) -> i64 {
    py_round((value as f64).max(0.0).ln_1p() * 4.0, 0) as i64
}

fn ratio_bin(value: f64) -> i64 {
    py_round(value.max(0.0).min(12.0) * 10.0, 0) as i64
}

fn blake2b_hex(data: &str, digest_size: usize) -> String {
    let mut hasher = Blake2bVar::new(digest_size).expect("valid digest size");
    hasher.update(data.as_bytes());
    let mut out = vec![0u8; digest_size];
    hasher.finalize_variable(&mut out).expect("hash output fits");
    let mut hex = String::with_capacity(digest_size * 2);
    for byte in out {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

pub fn build_typography_feature(
    item: &Item,
    translated_text: &str,
    font_size_pt: f64,
    leading_em: f64,
    page_width: Option<f64>,
    page_height: Option<f64>,
    page_text_width_med: f64,
    is_body: bool,
    dense_small_box: bool,
    heavy_dense_small_box: bool,
    wide_aspect_body_text: bool,
    preserve_line_breaks: bool,
) -> Option<TypographyFeature> {
    let width = bbox_width(item);
    let height = bbox_height(item);
    if width <= 0.0 || height <= 0.0 || font_size_pt <= 0.0 || leading_em <= 0.0 {
        return None;
    }
    let page_width = page_width.unwrap_or(0.0);
    let page_height = page_height.unwrap_or(0.0);
    let source_words = source_word_count(item);
    let zh_chars = translated_zh_char_count(translated_text);

    let mut payload = serde_json::Map::new();
    payload.insert("version".to_string(), serde_json::Value::String(TYPOGRAPHY_MEMORY_FEATURE_VERSION.to_string()));
    payload.insert("block_kind".to_string(), serde_json::Value::String(block_kind(item)));
    payload.insert("layout_role".to_string(), serde_json::Value::String(layout_role(item)));
    payload.insert("semantic_role".to_string(), serde_json::Value::String(semantic_role(item)));
    payload.insert("structure_role".to_string(), serde_json::Value::String(structure_role(item)));
    payload.insert("title".to_string(), serde_json::Value::Bool(is_title_like_block(item)));
    payload.insert("body".to_string(), serde_json::Value::Bool(is_body));
    payload.insert("dense".to_string(), serde_json::Value::Bool(dense_small_box));
    payload.insert("heavy_dense".to_string(), serde_json::Value::Bool(heavy_dense_small_box));
    payload.insert("wide_body".to_string(), serde_json::Value::Bool(wide_aspect_body_text));
    payload.insert("preserve_lines".to_string(), serde_json::Value::Bool(preserve_line_breaks));
    payload.insert("w_bin".to_string(), serde_json::Value::from(linear_bin(width, 12.0)));
    payload.insert("h_bin".to_string(), serde_json::Value::from(linear_bin(height, 8.0)));
    payload.insert("aspect_bin".to_string(), serde_json::Value::from(ratio_bin(width / height.max(1.0))));
    payload.insert(
        "page_w_bin".to_string(),
        if page_width > 0.0 {
            serde_json::Value::from(linear_bin(page_width, 40.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "page_h_bin".to_string(),
        if page_height > 0.0 {
            serde_json::Value::from(linear_bin(page_height, 40.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "rel_w_bin".to_string(),
        if page_width > 0.0 {
            serde_json::Value::from(ratio_bin(width / page_width.max(1.0)))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "rel_h_bin".to_string(),
        if page_height > 0.0 {
            serde_json::Value::from(ratio_bin(height / page_height.max(1.0)))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "text_w_bin".to_string(),
        if page_text_width_med > 0.0 {
            serde_json::Value::from(linear_bin(page_text_width_med, 12.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "source_lines_bin".to_string(),
        serde_json::Value::from(12.min(source_visual_line_count(item) as i64)),
    );
    payload.insert("source_words_bin".to_string(), serde_json::Value::from(log_bin(source_words)));
    payload.insert("zh_chars_bin".to_string(), serde_json::Value::from(log_bin(zh_chars)));
    payload.insert(
        "density_bin".to_string(),
        serde_json::Value::from(ratio_bin(translation_density_ratio(item, translated_text))),
    );
    payload.insert("formula_bin".to_string(), serde_json::Value::from(ratio_bin(formula_ratio(item))));

    let raw = serde_json::to_string(&payload).expect("feature payload serializes");
    let key = blake2b_hex(&raw, 16);
    Some(TypographyFeature { key, payload })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Item;

    fn feature_item() -> Item {
        Item {
            block_kind: Some("text".into()),
            layout_role: Some("paragraph".into()),
            semantic_role: Some("body".into()),
            structure_role: Some("body".into()),
            source_text: "A reasonably long source text block for feature hashing.".into(),
            bbox: Some([40.0, 100.0, 400.0, 160.0]),
            lines: vec![crate::item::Line { bbox: Some([40.0, 100.0, 390.0, 115.0]), spans: vec![] }],
            ..Default::default()
        }
    }

    #[test]
    fn degenerate_dimensions_return_none() {
        assert!(build_typography_feature(&Item::default(), "x", 10.0, 0.4, None, None, 0.0, false, false, false, false, false).is_none());
    }

    #[test]
    fn feature_key_is_deterministic_and_hex() {
        let item = feature_item();
        let a = build_typography_feature(&item, "翻译文本", 11.4, 0.48, Some(595.0), Some(842.0), 300.0, true, false, false, false, false).unwrap();
        let b = build_typography_feature(&item, "翻译文本", 11.4, 0.48, Some(595.0), Some(842.0), 300.0, true, false, false, false, false).unwrap();
        assert_eq!(a.key, b.key);
        assert_eq!(a.key.len(), 32);
        assert!(a.key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
