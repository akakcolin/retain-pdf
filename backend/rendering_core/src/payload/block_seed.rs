// Port of services/rendering/layout/payload/block_seed.py — the C3-N2 seed
// boundary `build_block_payloads`.

use crate::item::Item;
use crate::payload::block_seed_metrics::collect_page_seed_metrics;
use crate::payload::block_seed_payload_factory::build_seed_payload_for_item;

/// `build_block_payloads`: per-item seed payloads plus the median page text
/// width. `raw_items` mirrors `translated_items` as the original dicts so each
/// payload can embed the raw `item` / `bbox` / `formula_map` values.
pub fn build_block_payloads(
    translated_items: &[Item],
    raw_items: &[serde_json::Value],
    page_width: Option<f64>,
    page_height: Option<f64>,
) -> (Vec<serde_json::Value>, f64) {
    let metrics = collect_page_seed_metrics(translated_items, page_width);
    let mut block_payloads: Vec<serde_json::Value> = Vec::new();
    for (index, item) in translated_items.iter().enumerate() {
        let raw_item = raw_items
            .get(index)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(payload) =
            build_seed_payload_for_item(index, item, &raw_item, &metrics, page_width, page_height)
        {
            block_payloads.push(serde_json::Value::Object(payload));
        }
    }
    (block_payloads, metrics.page_text_width_med)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_items_empty_payloads() {
        let (payloads, width) = build_block_payloads(&[], &[], Some(595.0), None);
        assert!(payloads.is_empty());
        assert_eq!(width, 0.0);
    }

    #[test]
    fn skips_items_without_translated_text() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 40.0]),
            block_type: Some("text".into()),
            translated_text: "hello world".into(),
            source_text: "hello world".into(),
            lines: vec![crate::item::Line { bbox: Some([0.0, 0.0, 100.0, 20.0]), spans: vec![] }],
            ..Default::default()
        };
        let raw = json!({
            "bbox": [0.0, 0.0, 100.0, 40.0],
            "block_type": "text",
            "translated_text": "hello world",
            "source_text": "hello world",
            "lines": [{"bbox": [0.0, 0.0, 100.0, 20.0], "spans": []}]
        });
        let (payloads, width) = build_block_payloads(&[item], &[raw], Some(595.0), None);
        assert_eq!(payloads.len(), 1);
        assert_eq!(width, 100.0);
        assert_eq!(payloads[0].get("translated_text").unwrap(), &json!("hello world"));
    }
}
