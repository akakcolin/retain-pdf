// Port of `provider_adapters/mineru_content_list_v2_adapter.py` plus the shared
// `provider_adapters/common` builders (block_builder/page_builder/document_builder/
// normalize) — the MinerU `content_list_v2.json` experimental payload →
// normalized document.v1 adapter. The builder output is freshly constructed so
// defaults may mutate it in place.

use std::path::Path;

use rendering_core::text_flow::{classify_text_flow, line_texts_from_lines, py_splitlines};
use serde_json::{json, Map, Value};

use super::common::{
    build_block_record, build_line_records, build_page_record, build_text_segments, normalize_bbox,
};
use super::defaults::default_block_derived;
use super::version::{DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION};

pub const PROVIDER_MINERU_CONTENT_LIST_V2: &str = "mineru_content_list_v2";

/// `MINERU_CONTENT_LIST_V2_FILE_NAME.removesuffix(".json")` — the raw-files key.
const RAW_FILE_KEY: &str = "content_list_v2";

const TEXTUAL_BLOCK_TYPES: [&str; 6] = [
    "title",
    "paragraph",
    "page_header",
    "page_footer",
    "page_number",
    "page_aside_text",
];

fn key_for(raw_type: &str) -> &str {
    match raw_type {
        "title" => "title_content",
        "paragraph" => "paragraph_content",
        "page_header" => "page_header_content",
        "page_footer" => "page_footer_content",
        "page_number" => "page_number_content",
        "page_aside_text" => "page_aside_text_content",
        _ => "",
    }
}

fn bbox_value(bbox: &[f64]) -> Value {
    Value::Array(bbox.iter().map(|f| Value::from(*f)).collect())
}

/// `map_block_kind`.
fn map_block_kind(raw_type: &str) -> (String, String) {
    match raw_type {
        "title" => ("text".into(), "title".into()),
        "paragraph" | "page_aside_text" | "list" => ("text".into(), "body".into()),
        "page_header" => ("text".into(), "header".into()),
        "page_footer" => ("text".into(), "footer".into()),
        "page_number" => ("text".into(), "page_number".into()),
        "image" => ("image".into(), "figure".into()),
        _ => ("unknown".into(), String::new()),
    }
}

/// `normalize_segments`: keep dict raw segments with non-empty stripped text.
fn normalize_segments(raw_segments: Option<&Value>) -> Vec<Value> {
    let mut segments: Vec<Value> = Vec::new();
    let Some(items) = raw_segments.and_then(Value::as_array) else {
        return segments;
    };
    for raw in items {
        if !raw.is_object() {
            continue;
        }
        let raw_type = raw.get("type").and_then(Value::as_str).unwrap_or("text").to_string();
        let raw_type = if raw_type.is_empty() { "text".to_string() } else { raw_type };
        let text = raw.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let seg_type = if raw_type == "equation_inline" { "formula" } else { "text" };
        segments.extend(build_text_segments(&text, &raw_type, seg_type));
    }
    segments
}

/// `extract_text_structure`: list vs textual block content → (lines, segments, text).
fn extract_text_structure(block: &Map<String, Value>) -> (Value, Value, String) {
    let raw_type = block.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let segments: Vec<Value>;
    if raw_type == "list" {
        let mut list_segments: Vec<Value> = Vec::new();
        if let Some(items) = block
            .get("content")
            .and_then(|c| c.get("list_items"))
            .and_then(Value::as_array)
        {
            for item in items {
                let item_content = item.get("item_content");
                for seg in normalize_segments(item_content) {
                    list_segments.push(seg);
                }
            }
        }
        segments = list_segments;
    } else if TEXTUAL_BLOCK_TYPES.contains(&raw_type.as_str()) {
        let raw_segments = block.get("content").and_then(|c| c.get(key_for(&raw_type)));
        segments = normalize_segments(raw_segments);
    } else {
        segments = Vec::new();
    }
    let text = merge_segment_texts(&segments);
    let line_bbox = bbox_value(&normalize_bbox(block.get("bbox")));
    let lines = build_line_records(&line_bbox, &segments);
    (lines, Value::Array(segments), text)
}

fn merge_segment_texts(segments: &[Value]) -> String {
    segments
        .iter()
        .filter_map(|s| s.get("text").and_then(Value::as_str))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// `build_block_spec` — the `NormalizedBlockSpec` consumed by `build_block_record`.
fn build_block_spec(block: &Value, page_idx: usize, order: usize) -> Value {
    let raw_type = block.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let bbox = bbox_value(&normalize_bbox(block.get("bbox")));
    let (block_type, sub_type) = map_block_kind(&raw_type);
    let (lines, segments, text) = extract_text_structure(block.as_object().expect("block object"));
    let explicit_line_texts: Vec<String> = py_splitlines(&text)
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let line_texts = if explicit_line_texts.len() >= 2 {
        explicit_line_texts
    } else {
        line_texts_from_lines(&lines)
    };
    let text_flow = classify_text_flow(&text, &lines);
    let mut content = Map::new();
    content.insert("kind".to_string(), Value::String(block_type.clone()));
    content.insert("text".to_string(), Value::String(text.clone()));
    if !line_texts.is_empty() {
        content.insert(
            "line_texts".to_string(),
            Value::Array(line_texts.iter().map(|s| Value::String(s.clone())).collect()),
        );
        content.insert("text_flow".to_string(), Value::String(text_flow.to_string()));
    }
    json!({
        "block_id": format!("p{:03}-b{:04}", page_idx + 1, order),
        "page_index": page_idx,
        "order": order,
        "block_type": block_type,
        "sub_type": sub_type,
        "bbox": bbox,
        "content": Value::Object(content),
        "text": text,
        "lines": lines,
        "segments": segments,
        "tags": Value::Array(vec![]),
        "derived": default_block_derived(),
        "metadata": json!({ "raw_sub_type": "", "parent_block_id": "" }),
        "source": json!({
            "provider": PROVIDER_MINERU_CONTENT_LIST_V2,
            "raw_page_index": page_idx,
            "raw_type": raw_type,
            "raw_sub_type": "",
            "raw_bbox": bbox,
            "raw_text_excerpt": text.chars().take(200).collect::<String>(),
        }),
    })
}

/// `build_page_spec` — width/height = max block bbox x1/y1.
fn build_page_spec(page: &Value, page_idx: usize) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let mut x1_max = 0.0f64;
    let mut y1_max = 0.0f64;
    if let Some(page_arr) = page.as_array() {
        for (order, block) in page_arr.iter().enumerate() {
            let record = build_block_record(&build_block_spec(block, page_idx, order));
            if let Some(arr) = record.get("bbox").and_then(Value::as_array) {
                if arr.len() == 4 {
                    if let Some(x1) = arr[2].as_f64() {
                        x1_max = x1_max.max(x1);
                    }
                    if let Some(y1) = arr[3].as_f64() {
                        y1_max = y1_max.max(y1);
                    }
                }
            }
            blocks.push(record);
        }
    }
    json!({
        "page_index": page_idx,
        "width": x1_max,
        "height": y1_max,
        "unit": "pt",
        "blocks": Value::Array(blocks),
    })
}

/// `build_document_record` (provider_adapters/common/document_builder.py).
fn build_document_record(
    document_id: &str,
    provider_version: &str,
    source_json_path: &Path,
    pages: Vec<Value>,
) -> Value {
    json!({
        "schema": DOCUMENT_SCHEMA_NAME,
        "schema_version": DOCUMENT_SCHEMA_VERSION,
        "document_id": document_id,
        "doc_id": document_id,
        "source": {
            "provider": PROVIDER_MINERU_CONTENT_LIST_V2,
            "provider_version": provider_version,
            "raw_files": { RAW_FILE_KEY: source_json_path.display().to_string() },
        },
        "page_count": pages.len(),
        "pages": pages,
        "assets": {},
        "derived": { "notes": "Adapted from MinerU content_list_v2 experimental payload." },
        "markers": {},
    })
}

/// `build_mineru_content_list_v2_document` — raw content_list_v2 payload → document.v1 dict.
pub fn build_content_list_v2_document(
    payload: &Value,
    document_id: &str,
    source_json_path: &Path,
    provider_version: &str,
) -> Value {
    let pages: Vec<Value> = payload
        .as_array()
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(page_idx, page)| build_page_record(&build_page_spec(page, page_idx)))
                .collect()
        })
        .unwrap_or_default();
    build_document_record(document_id, provider_version, source_json_path, pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::defaults::default_block_continuation_hint;

    fn seg(type_: &str, content: &str) -> Value {
        json!({ "type": type_, "content": content })
    }

    #[test]
    fn map_block_kind_matches_python() {
        assert_eq!(map_block_kind("title"), ("text".to_string(), "title".to_string()));
        assert_eq!(map_block_kind("paragraph"), ("text".to_string(), "body".to_string()));
        assert_eq!(map_block_kind("list"), ("text".to_string(), "body".to_string()));
        assert_eq!(map_block_kind("page_aside_text"), ("text".to_string(), "body".to_string()));
        assert_eq!(map_block_kind("page_header"), ("text".to_string(), "header".to_string()));
        assert_eq!(map_block_kind("page_footer"), ("text".to_string(), "footer".to_string()));
        assert_eq!(map_block_kind("page_number"), ("text".to_string(), "page_number".to_string()));
        assert_eq!(map_block_kind("image"), ("image".to_string(), "figure".to_string()));
        assert_eq!(map_block_kind("table"), ("unknown".to_string(), String::new()));
    }

    #[test]
    fn normalize_segments_skips_empty_and_keeps_equation_inline_as_formula() {
        let raw = json!([
            seg("text", "  hello world  "),
            seg("equation_inline", "x = 1"),
            seg("text", "   "),
            json!({"type": "text"}), // missing content -> empty text -> skipped
            "not-a-dict",
        ]);
        let out = normalize_segments(Some(&raw));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["type"], "text");
        assert_eq!(out[0]["raw_type"], "text");
        assert_eq!(out[0]["text"], "hello world");
        assert_eq!(out[0]["bbox"], json!([0, 0, 0, 0]));
        assert!(out[0]["score"].is_null());
        assert_eq!(out[1]["type"], "formula");
        assert_eq!(out[1]["raw_type"], "equation_inline");
    }

    #[test]
    fn normalize_segments_empty_or_non_array() {
        assert!(normalize_segments(None).is_empty());
        assert!(normalize_segments(Some(&Value::Array(vec![]))).is_empty());
        assert!(normalize_segments(Some(&json!("nope"))).is_empty());
    }

    #[test]
    fn build_block_spec_title_produces_text_title_content() {
        let block = json!({
            "type": "title",
            "bbox": [10, 20, 300, 60],
            "content": { "title_content": [seg("text", "Understanding Systems")] },
        });
        let spec = build_block_spec(&block, 0, 2);
        assert_eq!(spec["block_id"], "p001-b0002");
        assert_eq!(spec["page_index"], 0);
        assert_eq!(spec["order"], 2);
        assert_eq!(spec["block_type"], "text");
        assert_eq!(spec["sub_type"], "title");
        assert_eq!(spec["bbox"], json!([10.0, 20.0, 300.0, 60.0]));
        assert_eq!(spec["text"], "Understanding Systems");
        assert_eq!(spec["segments"][0]["text"], "Understanding Systems");
        assert_eq!(spec["content"]["kind"], "text");
        assert_eq!(spec["content"]["text"], "Understanding Systems");
        assert_eq!(spec["metadata"], json!({ "raw_sub_type": "", "parent_block_id": "" }));
        assert_eq!(spec["source"]["raw_type"], "title");
        assert_eq!(spec["source"]["raw_bbox"], json!([10.0, 20.0, 300.0, 60.0]));
        // single line -> flow (no explicit 2+ lines, one line record)
        assert_eq!(spec["content"]["text_flow"], "flow");
        assert_eq!(spec["lines"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_block_spec_list_joins_item_content() {
        let block = json!({
            "type": "list",
            "bbox": [10, 80, 300, 140],
            "content": {
                "list_items": [
                    { "item_content": [seg("text", "first item")] },
                    { "item_content": [seg("equation_inline", "a + b"), seg("text", " second")] },
                ]
            },
        });
        let spec = build_block_spec(&block, 1, 0);
        assert_eq!(spec["block_type"], "text");
        assert_eq!(spec["sub_type"], "body");
        assert_eq!(spec["text"], "first item a + b second");
        let segs = spec["segments"].as_array().unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[1]["type"], "formula");
    }

    #[test]
    fn build_block_spec_image_has_no_segments() {
        let block = json!({ "type": "image", "bbox": [10, 10, 100, 100] });
        let spec = build_block_spec(&block, 0, 1);
        assert_eq!(spec["block_type"], "image");
        assert_eq!(spec["sub_type"], "figure");
        assert!(spec["segments"].as_array().unwrap().is_empty());
        assert!(spec["lines"].as_array().unwrap().is_empty());
        assert_eq!(spec["text"], "");
        assert_eq!(spec["content"]["kind"], "image");
        assert!(spec["content"].get("line_texts").is_none());
    }

    #[test]
    fn build_block_record_adds_continuation_geometry_content() {
        let spec = build_block_spec(
            &json!({ "type": "paragraph", "bbox": [0, 0, 100, 30], "content": { "paragraph_content": [seg("text", "hello")] } }),
            0,
            0,
        );
        let record = build_block_record(&spec);
        assert_eq!(record["continuation_hint"], default_block_continuation_hint());
        assert_eq!(record["reading_order"], 0);
        assert_eq!(record["geometry"]["bbox"], record["bbox"]);
        assert_eq!(record["content"]["kind"], "text");
        assert_eq!(record["type"], "text");
    }

    #[test]
    fn build_page_spec_computes_max_bbox_extents() {
        let page = json!([
            { "type": "title", "bbox": [10, 20, 300, 60], "content": { "title_content": [seg("text", "a")] } },
            { "type": "paragraph", "bbox": [10, 80, 500, 140], "content": { "paragraph_content": [seg("text", "b")] } },
        ]);
        let spec = build_page_spec(&page, 0);
        assert_eq!(spec["page_index"], 0);
        assert_eq!(spec["width"], 500.0);
        assert_eq!(spec["height"], 140.0);
        assert_eq!(spec["unit"], "pt");
        assert_eq!(spec["blocks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn build_content_list_v2_document_full_shape() {
        let payload = json!([
            [
                { "type": "title", "bbox": [10, 20, 300, 60], "content": { "title_content": [seg("text", "Doc Title")] } },
                { "type": "image", "bbox": [10, 80, 200, 200] },
            ],
            [
                { "type": "paragraph", "bbox": [10, 20, 400, 70], "content": { "paragraph_content": [seg("text", "Body text here")] } },
            ],
        ]);
        let doc = build_content_list_v2_document(&payload, "job-x", Path::new("/src/layout.json"), "2025.11.1");
        assert_eq!(doc["schema"], "normalized_document_v1");
        assert_eq!(doc["document_id"], "job-x");
        assert_eq!(doc["doc_id"], "job-x");
        assert_eq!(doc["page_count"], 2);
        assert_eq!(doc["assets"], json!({}));
        assert_eq!(doc["markers"], json!({}));
        assert_eq!(doc["source"]["provider"], "mineru_content_list_v2");
        assert_eq!(doc["source"]["raw_files"]["content_list_v2"], "/src/layout.json");
        let pages = doc["pages"].as_array().unwrap();
        assert_eq!(pages[0]["page"], 1);
        assert_eq!(pages[0]["width"], 300.0);
        assert_eq!(pages[0]["height"], 200.0);
        assert_eq!(pages[0]["blocks"].as_array().unwrap().len(), 2);
        assert_eq!(pages[1]["page"], 2);
        assert_eq!(pages[1]["blocks"].as_array().unwrap()[0]["type"], "text");
        assert_eq!(
            pages[0]["blocks"].as_array().unwrap()[0]["block_id"],
            "p001-b0000"
        );
    }
}
