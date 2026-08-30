// Port of `provider_adapters/paddle/*` — the PaddleOCR `layoutParsingResults`
// payload → normalized document.v1 adapter (C5-N2c). Shares the common builders
// in `super::common` (block_record/page_record/document_record) plus the
// provider-continuation + previous-anchor helpers added for this port.

pub mod block_labels;
pub mod block_reader;
pub mod body_repair;
pub mod column_signals;
pub mod content_extract;
pub mod context;
pub mod continuation;
pub mod page_reader;
pub mod page_trace;
pub mod relations;
pub mod rich_content;
pub mod trace;

use std::path::Path;

use serde_json::{json, Value};

use super::common::build_page_record;
use super::version::{DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION};
use self::column_signals::summarize_document_column_signals;
use self::continuation::assign_paddle_continuation_hints;
use self::page_reader::iter_page_specs;

pub const PROVIDER_PADDLE: &str = "paddle";

/// `looks_like_paddle_layout` — payload has `layoutParsingResults` list + `dataInfo` dict.
pub fn looks_like_paddle_layout(payload: &Value) -> bool {
    payload.is_object()
        && payload.get("layoutParsingResults").map_or(false, Value::is_array)
        && payload.get("dataInfo").map_or(false, Value::is_object)
}

/// `build_paddle_document` — raw paddle payload → document.v1 dict.
pub fn build_paddle_document(
    payload: &Value,
    document_id: &str,
    source_json_path: &Path,
    provider_version: &str,
) -> Value {
    let mut pages: Vec<Value> = iter_page_specs(payload)
        .into_iter()
        .map(|spec| build_page_record(&spec))
        .collect();
    assign_paddle_continuation_hints(&mut pages);
    let mut document = json!({
        "schema": DOCUMENT_SCHEMA_NAME,
        "schema_version": DOCUMENT_SCHEMA_VERSION,
        "document_id": document_id,
        "doc_id": document_id,
        "source": {
            "provider": PROVIDER_PADDLE,
            "provider_version": provider_version,
            "raw_files": { "source_json": source_json_path.display().to_string() },
        },
        "page_count": pages.len(),
        "pages": pages,
        "assets": {},
        "derived": { "notes": "Adapted from PaddleOCR layoutParsingResults payload." },
        "markers": {},
    });
    let provider_signals = summarize_document_column_signals(
        document.get("pages").and_then(Value::as_array).map(|p| p.as_slice()).unwrap_or(&[]),
    );
    if let Some(derived) = document.get_mut("derived").and_then(Value::as_object_mut) {
        derived.insert("provider_signals".to_string(), provider_signals);
    }
    document
}

/// Raw-files key used by the paddle adapter (python `build_document_record`
/// default `raw_file_key="source_json"`).
pub const RAW_FILE_KEY: &str = "source_json";

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Value {
        json!({
            "layoutParsingResults": [
                {
                    "inputImage": "raw/page_0.png",
                    "prunedResult": {
                        "page_count": 1,
                        "model_settings": {"enable_body_repair": false},
                        "layout_det_res": {"boxes": []},
                        "parsing_res_list": [
                            {"block_label": "doc_title", "block_content": "Doc Title", "block_bbox": [40.0, 40.0, 560.0, 90.0], "group_id": "sec", "block_order": 0},
                            {"block_label": "text", "block_content": "Body text.", "block_bbox": [40.0, 100.0, 560.0, 160.0], "group_id": "sec", "block_order": 1},
                        ],
                    },
                },
            ],
            "dataInfo": {"pages": [{"width": 595.0, "height": 842.0}]},
            "preprocessedImages": ["pre/page_0.png"],
        })
    }

    #[test]
    fn looks_like_paddle_layout_requires_shape() {
        assert!(looks_like_paddle_layout(&payload()));
        // Shape check is structural: an empty list + empty dict still satisfies it.
        assert!(looks_like_paddle_layout(&json!({"layoutParsingResults": [], "dataInfo": {}})));
        assert!(!looks_like_paddle_layout(&json!({"dataInfo": {}})));
        assert!(!looks_like_paddle_layout(&json!({"layoutParsingResults": []})));
        assert!(!looks_like_paddle_layout(&json!("nope")));
    }

    #[test]
    fn build_paddle_document_full_shape() {
        let doc = build_paddle_document(&payload(), "job-1", Path::new("/src/layout.json"), "2025.11.1");
        assert_eq!(doc["schema"], "normalized_document_v1");
        assert_eq!(doc["document_id"], "job-1");
        assert_eq!(doc["doc_id"], "job-1");
        assert_eq!(doc["page_count"], 1);
        assert_eq!(doc["assets"], json!({}));
        assert_eq!(doc["markers"], json!({}));
        assert_eq!(doc["source"]["provider"], "paddle");
        assert_eq!(doc["source"]["provider_version"], "2025.11.1");
        assert_eq!(doc["source"]["raw_files"]["source_json"], "/src/layout.json");
        assert_eq!(doc["derived"]["notes"], "Adapted from PaddleOCR layoutParsingResults payload.");
        assert_eq!(doc["derived"]["provider_signals"]["provider"], "paddle");
        assert_eq!(doc["derived"]["provider_signals"]["pages"][0]["page_index"], 0);

        let page = &doc["pages"][0];
        assert_eq!(page["width"], 595.0);
        assert_eq!(page["height"], 842.0);
        let blocks = page["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["block_id"], "p001-b0000");
        assert_eq!(blocks[0]["continuation_hint"]["group_id"], "provider-paddle-page-001-group-sec");
        assert_eq!(blocks[0]["continuation_hint"]["role"], "head");
        assert_eq!(blocks[1]["continuation_hint"]["role"], "tail");
        assert_eq!(blocks[1]["continuation_hint"]["reading_order"], 1);
    }

    #[test]
    fn build_paddle_document_empty_pages() {
        let payload = json!({"layoutParsingResults": [], "dataInfo": {"pages": []}});
        let doc = build_paddle_document(&payload, "job-2", Path::new("/src/x.json"), "v1");
        assert_eq!(doc["page_count"], 0);
        assert_eq!(doc["pages"], json!([]));
        assert_eq!(doc["derived"]["provider_signals"]["suspicious_cross_column_merge_pages"], 0);
    }
}
