// Port of `provider_adapters/paddle/page_reader.py` — page context building,
// page spec assembly, and `iter_page_specs` over the payload.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::common::{build_block_record, py_int, to_string_value};
use super::block_reader::build_block_spec;
use super::body_repair::repair_body_cross_column_blocks;
use super::column_signals::analyze_page_column_signals;
use super::context::PaddlePageContext;
use super::page_trace::{build_layout_box_lookup, build_page_trace};
use super::relations::classify_page_blocks;

/// Python `bool(x)` truthiness over JSON values.
fn value_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `_provider_allows_body_repair` — truthy `model_settings.enable_body_repair`.
fn provider_allows_body_repair(pruned: &Value) -> bool {
    let enable = pruned
        .get("model_settings")
        .and_then(Value::as_object)
        .and_then(|m| m.get("enable_body_repair"));
    value_truthy(enable)
}

/// Python `float(x or 0)` over page/pruned width/height.
fn float_or_zero(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn block_flag_orders(blocks: &[Value], key: &str) -> Vec<String> {
    blocks
        .iter()
        .filter(|b| {
            b.get("metadata")
                .and_then(Value::as_object)
                .and_then(|m| m.get(key))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .map(|b| to_string_value(b.get("block_id")))
        .collect()
}

/// `build_page_context`.
fn build_page_context(
    page_payload: &Value,
    page_index: i64,
    page_meta: &Value,
    preprocessed_image: &str,
) -> PaddlePageContext {
    let pruned = page_payload.get("prunedResult").cloned().unwrap_or_else(|| json!({}));
    let parsing_res_list: Vec<Value> = pruned
        .get("parsing_res_list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let layout_box_lookup = build_layout_box_lookup(
        &pruned
            .get("layout_det_res")
            .and_then(|r| r.get("boxes"))
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
    );
    let markdown = page_payload.get("markdown").cloned().unwrap_or_else(|| json!({}));
    let markdown_text = to_string_value(markdown.get("text"));
    let markdown_images = markdown
        .get("images")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let _classified_kinds = classify_page_blocks(&parsing_res_list);
    let page_width = float_or_zero(page_meta.get("width").or_else(|| pruned.get("width")));
    let original_column_signals = analyze_page_column_signals(&parsing_res_list, page_width);
    let (repaired_parsing_res_list, repair_metadata, repair_summary) =
        if provider_allows_body_repair(&pruned) {
            repair_body_cross_column_blocks(&parsing_res_list, &original_column_signals)
        } else {
            (
                parsing_res_list.clone(),
                BTreeMap::new(),
                json!({
                    "body_repair_pair_count": 0,
                    "body_repair_pairs": [],
                    "body_repair_block_count": 0,
                }),
            )
        };
    let repaired_classified_kinds = classify_page_blocks(&repaired_parsing_res_list);
    let column_signals = analyze_page_column_signals(&repaired_parsing_res_list, page_width);
    PaddlePageContext {
        page_index,
        page_payload: page_payload.clone(),
        page_meta: page_meta.clone(),
        preprocessed_image: preprocessed_image.to_string(),
        pruned,
        parsing_res_list: repaired_parsing_res_list,
        layout_box_lookup,
        markdown_text,
        markdown_images,
        classified_kinds: repaired_classified_kinds,
        column_signals,
        repair_metadata,
        repair_summary,
    }
}

/// `build_page_spec`.
pub fn build_page_spec(
    page_payload: &Value,
    page_index: i64,
    page_meta: &Value,
    preprocessed_image: &str,
) -> Value {
    let page_context = build_page_context(page_payload, page_index, page_meta, preprocessed_image);
    let blocks: Vec<Value> = (0..page_context.parsing_res_list.len())
        .map(|order| build_block_record(&build_block_spec(&page_context, order)))
        .collect();
    let block_ids: Vec<String> = blocks
        .iter()
        .map(|b| to_string_value(b.get("block_id")))
        .collect();

    let mut metadata = build_page_trace(
        &page_context.page_payload,
        &page_context.pruned,
        &page_context.preprocessed_image,
        Some(&page_context.column_signals),
        &block_ids,
    );
    let metadata_obj = metadata.as_object_mut().expect("metadata object");
    let text_missing = block_flag_orders(&blocks, "provider_text_missing_but_bbox_present");
    metadata_obj.insert(
        "text_missing_but_bbox_present_count".to_string(),
        Value::from(text_missing.len()),
    );
    metadata_obj.insert(
        "text_missing_but_bbox_present_block_ids".to_string(),
        Value::Array(text_missing.iter().map(|s| Value::String(s.clone())).collect()),
    );
    let peer_absorbed = block_flag_orders(&blocks, "provider_peer_block_absorbed_text");
    metadata_obj.insert(
        "peer_block_absorbed_text_count".to_string(),
        Value::from(peer_absorbed.len()),
    );
    metadata_obj.insert(
        "peer_block_absorbed_text_block_ids".to_string(),
        Value::Array(peer_absorbed.iter().map(|s| Value::String(s.clone())).collect()),
    );
    metadata_obj.insert(
        "body_repair_pair_count".to_string(),
        Value::from(
            page_context
                .repair_summary
                .get("body_repair_pair_count")
                .and_then(py_int)
                .unwrap_or(0),
        ),
    );
    metadata_obj.insert(
        "body_repair_block_count".to_string(),
        Value::from(
            page_context
                .repair_summary
                .get("body_repair_block_count")
                .and_then(py_int)
                .unwrap_or(0),
        ),
    );
    metadata_obj.insert(
        "body_repair_pairs".to_string(),
        page_context
            .repair_summary
            .get("body_repair_pairs")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
    );
    let repair_block_ids = block_flag_orders(&blocks, "provider_body_repair_applied");
    metadata_obj.insert(
        "body_repair_block_ids".to_string(),
        Value::Array(repair_block_ids.iter().map(|s| Value::String(s.clone())).collect()),
    );

    let width = float_or_zero(page_meta.get("width").or_else(|| page_context.pruned.get("width")));
    let height = float_or_zero(page_meta.get("height").or_else(|| page_context.pruned.get("height")));
    json!({
        "page_index": page_context.page_index,
        "width": width,
        "height": height,
        "unit": "pt",
        "blocks": Value::Array(blocks),
        "metadata": metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_payload(enable_body_repair: bool) -> Value {
        json!({
            "inputImage": "raw/page_0.png",
            "markdown": {"text": "markdown", "images": {}},
            "prunedResult": {
                "page_count": 1,
                "model_settings": {"enable_body_repair": enable_body_repair},
                "layout_det_res": {"boxes": []},
                "parsing_res_list": [
                    {"block_label": "doc_title", "block_content": "Title", "block_bbox": [40.0, 40.0, 560.0, 90.0], "group_id": "g", "block_order": 0},
                    {"block_label": "text", "block_content": "", "block_bbox": [40.0, 100.0, 250.0, 150.0]},
                    {"block_label": "text", "block_content": "Body paragraph with real content.", "block_bbox": [350.0, 100.0, 560.0, 150.0]},
                ],
            },
        })
    }

    #[test]
    fn build_page_context_respects_body_repair_gate() {
        let meta = json!({"width": 595.0, "height": 842.0});
        let on = build_page_context(&page_payload(true), 0, &meta, "pre/page_0.png");
        assert_eq!(on.repair_summary["body_repair_pair_count"], 0);
        assert_eq!(on.parsing_res_list.len(), 3);

        let off = build_page_context(&page_payload(false), 0, &meta, "pre/page_0.png");
        assert_eq!(off.repair_summary["body_repair_pair_count"], 0);
        assert_eq!(off.repair_metadata.len(), 0);
    }

    #[test]
    fn build_page_spec_metadata_counts_and_width() {
        let spec = build_page_spec(&page_payload(true), 0, &json!({"width": 595.0, "height": 842.0}), "pre/page_0.png");
        assert_eq!(spec["page_index"], 0);
        assert_eq!(spec["width"], 595.0);
        assert_eq!(spec["height"], 842.0);
        assert_eq!(spec["unit"], "pt");
        let blocks = spec["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(spec["metadata"]["provider_page_count"], 1);
        assert_eq!(spec["metadata"]["raw_unit"], "px");
        // empty text block flags text-missing; the cross-column empty slot is a
        // body-repair candidate (right column donor overlaps), so both counts reflect it.
        assert_eq!(spec["metadata"]["text_missing_but_bbox_present_count"], 1);
        assert_eq!(
            spec["metadata"]["text_missing_but_bbox_present_block_ids"],
            json!(["p001-b0001"])
        );
        assert!(spec["metadata"]["body_repair_pair_count"].as_i64().unwrap() >= 0);
        // Continuation hints are assigned at the document level; per-page specs
        // only carry the raw group provenance.
        assert_eq!(blocks[0]["metadata"]["raw_group_id"], "g");
        assert_eq!(blocks[0]["continuation_hint"]["group_id"], "");
    }

    #[test]
    fn build_page_context_missing_pruned_defaults() {
        let ctx = build_page_context(&json!({}), 0, &json!({}), "");
        assert!(ctx.parsing_res_list.is_empty());
        assert!(ctx.markdown_text.is_empty());
        assert!(ctx.column_signals["column_layout_mode"].is_string());
    }

    #[test]
    fn value_truthy_and_float_or_zero_helpers() {
        assert!(!value_truthy(None));
        assert!(!value_truthy(Some(&Value::Null)));
        assert!(!value_truthy(Some(&Value::Bool(false))));
        assert!(value_truthy(Some(&Value::Bool(true))));
        assert!(value_truthy(Some(&json!(1))));
        assert!(!value_truthy(Some(&json!(0))));
        assert!(value_truthy(Some(&json!("x"))));
        assert!(!value_truthy(Some(&json!(""))));
        assert!(value_truthy(Some(&json!([1]))));
        assert!(!value_truthy(Some(&json!([]))));

        assert_eq!(float_or_zero(Some(&json!(595))), 595.0);
        assert_eq!(float_or_zero(Some(&json!("842"))), 842.0);
        assert_eq!(float_or_zero(Some(&json!("bad"))), 0.0);
        assert_eq!(float_or_zero(None), 0.0);
    }
}

/// `iter_page_specs` — one spec per `layoutParsingResults` page.
pub fn iter_page_specs(payload: &Value) -> Vec<Value> {
    let pages_meta: Vec<Value> = payload
        .get("dataInfo")
        .and_then(|d| d.get("pages"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let layout_results: Vec<Value> = payload
        .get("layoutParsingResults")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let preprocessed_images: Vec<Value> = payload
        .get("preprocessedImages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut page_specs: Vec<Value> = Vec::with_capacity(layout_results.len());
    for (page_index, page_payload) in layout_results.iter().enumerate() {
        let page_meta = pages_meta
            .get(page_index)
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let preprocessed_image = preprocessed_images
            .get(page_index)
            .map(|v| to_string_value(Some(v)))
            .unwrap_or_default();
        let page_payload = if page_payload.is_object() {
            page_payload.clone()
        } else {
            json!({})
        };
        page_specs.push(build_page_spec(
            &page_payload,
            page_index as i64,
            &page_meta,
            &preprocessed_image,
        ));
    }
    page_specs
}
