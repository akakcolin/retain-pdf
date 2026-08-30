// Port of `provider_adapters/paddle/trace.py`.

use serde_json::{json, Map, Value};

use super::super::common::normalize_polygon;
use super::super::defaults::default_block_derived;
use super::PROVIDER_PADDLE;

/// `build_derived` — provider-rule role/confidence defaults, overridden by label.
pub fn build_derived(raw_label: &str, sub_type: &str) -> Value {
    let mut derived = match default_block_derived() {
        Value::Object(map) => map,
        _ => unreachable!(),
    };
    let label = raw_label.trim().to_lowercase();
    match label.as_str() {
        "doc_title" => {
            derived.insert("role".into(), "title".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.98));
        }
        "abstract" => {
            derived.insert("role".into(), "abstract".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.98));
        }
        "figure_title" => {
            let mut role = "figure_caption".to_string();
            if matches!(
                sub_type,
                "figure_caption" | "table_caption" | "image_caption" | "code_caption"
            ) {
                role = sub_type.to_string();
            }
            derived.insert("role".into(), Value::String(role));
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.95));
        }
        "header" | "footer" => {
            derived.insert("role".into(), Value::String(label));
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.98));
        }
        "reference_content" => {
            derived.insert("role".into(), "reference_entry".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.98));
        }
        "formula_number" => {
            derived.insert("role".into(), "formula_number".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.98));
        }
        "number" => {
            derived.insert("role".into(), "page_number".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.95));
        }
        "aside_text" => {
            derived.insert("role".into(), "metadata".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.95));
        }
        "footnote" => {
            derived.insert("role".into(), "footnote".into());
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.9));
        }
        "vision_footnote" => {
            let role = if sub_type.is_empty() { "footnote" } else { sub_type };
            derived.insert("role".into(), Value::String(role.to_string()));
            derived.insert("by".into(), "provider_rule".into());
            derived.insert("confidence".into(), json!(0.9));
        }
        _ => {}
    }
    Value::Object(derived)
}

/// `build_metadata` — raw paddle provenance fields + kind metadata. Missing keys
/// serialize as null (Python `.get()` returns None).
pub fn build_metadata(block: &Value, kind_metadata: &Map<String, Value>) -> Value {
    let mut metadata = Map::new();
    for key in ["group_id", "global_block_id", "global_group_id", "block_order"] {
        metadata.insert(
            format!("raw_{key}"),
            block.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    metadata.insert(
        "raw_polygon".to_string(),
        Value::Array(
            normalize_polygon(block.get("block_polygon_points"))
                .into_iter()
                .map(|pair| Value::Array(pair.into_iter().map(Value::from).collect()))
                .collect(),
        ),
    );
    for (key, value) in kind_metadata {
        metadata.insert(key.clone(), value.clone());
    }
    Value::Object(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_derived_provider_roles() {
        assert_eq!(build_derived("doc_title", "")["role"], "title");
        assert_eq!(build_derived("abstract", "")["role"], "abstract");
        assert_eq!(build_derived("figure_title", "figure_caption")["role"], "figure_caption");
        assert_eq!(build_derived("figure_title", "table_caption")["role"], "table_caption");
        assert_eq!(build_derived("header", "")["role"], "header");
        assert_eq!(build_derived("footer", "")["role"], "footer");
        assert_eq!(build_derived("reference_content", "")["role"], "reference_entry");
        assert_eq!(build_derived("formula_number", "")["role"], "formula_number");
        assert_eq!(build_derived("number", "")["role"], "page_number");
        assert_eq!(build_derived("aside_text", "")["role"], "metadata");
        assert_eq!(build_derived("footnote", "")["role"], "footnote");
        assert_eq!(build_derived("vision_footnote", "image_footnote")["role"], "image_footnote");
        // Unlabeled body text keeps the default empty role.
        assert_eq!(build_derived("text", "body")["role"], "");
        assert_eq!(build_derived("doc_title", "")["confidence"], 0.98);
        assert_eq!(build_derived("number", "")["by"], "provider_rule");
    }

    #[test]
    fn build_metadata_raw_fields_and_kind_metadata() {
        let block = json!({
            "group_id": "g1",
            "global_block_id": "gb1",
            "global_group_id": "gg1",
            "block_order": 7,
            "block_polygon_points": [[1, 2], [3, 2], [3, 4], [1, 4]],
        });
        let mut kind = Map::new();
        kind.insert("source_text_role".to_string(), Value::String("abstract".to_string()));
        let metadata = build_metadata(&block, &kind);
        assert_eq!(metadata["raw_group_id"], "g1");
        assert_eq!(metadata["raw_global_block_id"], "gb1");
        assert_eq!(metadata["raw_global_group_id"], "gg1");
        assert_eq!(metadata["raw_block_order"], 7);
        assert_eq!(metadata["raw_polygon"], json!([[1.0, 2.0], [3.0, 2.0], [3.0, 4.0], [1.0, 4.0]]));
        assert_eq!(metadata["source_text_role"], "abstract");
        // Missing raw fields serialize as null.
        let bare = build_metadata(&json!({}), &Map::new());
        assert!(bare["raw_group_id"].is_null());
    }

    #[test]
    fn build_source_trace_fields() {
        let source = build_source(&json!({"block_id": "b9"}), 2, "text", &[1.0, 2.0, 3.0, 4.0], "hello world", 5);
        assert_eq!(source["provider"], "paddle");
        assert_eq!(source["raw_page_index"], 2);
        assert_eq!(source["raw_type"], "text");
        assert_eq!(source["raw_bbox"], json!([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(source["raw_text_excerpt"], "hello world");
        assert_eq!(source["raw_block_id"], "b9");
        assert_eq!(source["raw_path"], "/layoutParsingResults/2/prunedResult/parsing_res_list/5");
    }
}

/// `build_source`.
pub fn build_source(
    block: &Value,
    page_index: i64,
    raw_label: &str,
    bbox: &[f64],
    text: &str,
    order: usize,
) -> Value {
    json!({
        "provider": PROVIDER_PADDLE,
        "raw_page_index": page_index,
        "raw_type": raw_label,
        "raw_sub_type": "",
        "raw_bbox": Value::Array(bbox.iter().map(|f| Value::from(*f)).collect()),
        "raw_text_excerpt": text.chars().take(200).collect::<String>(),
        "raw_block_id": block.get("block_id").cloned().unwrap_or(Value::Null),
        "raw_path": format!(
            "/layoutParsingResults/{page_index}/prunedResult/parsing_res_list/{order}"
        ),
    })
}
