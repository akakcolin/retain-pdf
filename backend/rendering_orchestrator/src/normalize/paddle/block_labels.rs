// Port of `provider_adapters/paddle/block_labels.py`.

use serde_json::{Map, Value};

/// The resolved block kind `(block_type, sub_type, tags, kind_metadata)` tuple.
#[derive(Debug, Clone)]
pub struct BlockKind {
    pub block_type: String,
    pub sub_type: String,
    pub tags: Vec<String>,
    pub metadata: Map<String, Value>,
}

/// `map_block_kind` — raw paddle block label → block kind.
pub fn map_block_kind(raw_label: &str, _text: &str) -> BlockKind {
    let label = raw_label.trim().to_lowercase();
    match label.as_str() {
        "doc_title" => kind("text", "title", vec!["title"], Map::new()),
        "abstract" => kind("text", "body", vec!["abstract"], map_one("source_text_role", "abstract")),
        "text" => kind("text", "body", vec![], Map::new()),
        "paragraph_title" => kind("text", "heading", vec!["heading"], Map::new()),
        "content" => kind(
            "text",
            "table_of_contents",
            vec!["table_of_contents", "toc"],
            Map::new(),
        ),
        "reference_content" => kind(
            "text",
            "reference_entry",
            vec!["reference_entry", "reference_zone", "skip_translation"],
            Map::new(),
        ),
        "formula_number" => kind(
            "text",
            "formula_number",
            vec!["formula_number", "skip_translation"],
            Map::new(),
        ),
        "header" => kind("text", "header", vec!["skip_translation"], Map::new()),
        "footer" => kind("text", "footer", vec!["skip_translation"], Map::new()),
        "footnote" => kind("text", "footnote", vec!["footnote", "skip_translation"], Map::new()),
        "aside_text" => kind("text", "metadata", vec!["metadata", "skip_translation"], Map::new()),
        "number" => kind("text", "page_number", vec!["skip_translation"], Map::new()),
        "figure_title" => kind(
            "text",
            "figure_caption",
            vec!["caption", "figure_caption"],
            map_one("caption_target", "figure"),
        ),
        "table" => kind("table", "table_html", vec!["table"], Map::new()),
        "chart" | "header_image" | "footer_image" => kind(
            "image",
            "image_body",
            vec!["image", "skip_translation"],
            Map::new(),
        ),
        "image" => kind("image", "image_body", vec!["image", "skip_translation"], Map::new()),
        "algorithm" => kind("code", "code_block", vec!["code"], Map::new()),
        "display_formula" | "formula" => kind("formula", "display_formula", vec!["formula"], Map::new()),
        "vision_footnote" => kind(
            "text",
            "footnote",
            vec!["footnote"],
            map_one("footnote_target", "unknown"),
        ),
        _ => kind("unknown", "", vec!["unknown"], Map::new()),
    }
}

fn kind(block_type: &str, sub_type: &str, tags: Vec<&str>, metadata: Map<String, Value>) -> BlockKind {
    BlockKind {
        block_type: block_type.to_string(),
        sub_type: sub_type.to_string(),
        tags: tags.into_iter().map(|s| s.to_string()).collect(),
        metadata,
    }
}

fn map_one(key: &str, value: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(key.to_string(), Value::String(value.to_string()));
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_block_kind_known_labels() {
        assert_eq!(map_block_kind("doc_title", "").block_type, "text");
        assert_eq!(map_block_kind("doc_title", "").sub_type, "title");
        assert_eq!(map_block_kind("abstract", "").metadata["source_text_role"], "abstract");
        assert_eq!(map_block_kind("text", "").sub_type, "body");
        assert_eq!(map_block_kind("paragraph_title", "").sub_type, "heading");
        assert_eq!(map_block_kind("content", "").sub_type, "table_of_contents");
        assert_eq!(map_block_kind("reference_content", "").tags, vec!["reference_entry", "reference_zone", "skip_translation"]);
        assert_eq!(map_block_kind("formula_number", "").sub_type, "formula_number");
        assert_eq!(map_block_kind("header", "").sub_type, "header");
        assert_eq!(map_block_kind("footer", "").sub_type, "footer");
        assert_eq!(map_block_kind("footnote", "").sub_type, "footnote");
        assert_eq!(map_block_kind("aside_text", "").sub_type, "metadata");
        assert_eq!(map_block_kind("number", "").sub_type, "page_number");
        assert_eq!(map_block_kind("figure_title", "").sub_type, "figure_caption");
        assert_eq!(map_block_kind("table", "").block_type, "table");
        assert_eq!(map_block_kind("chart", "").block_type, "image");
        assert_eq!(map_block_kind("header_image", "").sub_type, "image_body");
        assert_eq!(map_block_kind("image", "").block_type, "image");
        assert_eq!(map_block_kind("algorithm", "").block_type, "code");
        assert_eq!(map_block_kind("display_formula", "").block_type, "formula");
        assert_eq!(map_block_kind("formula", "").sub_type, "display_formula");
        assert_eq!(map_block_kind("vision_footnote", "").sub_type, "footnote");
    }

    #[test]
    fn map_block_kind_case_insensitive_and_unknown() {
        assert_eq!(map_block_kind("DOC_TITLE", "").sub_type, "title");
        let unknown = map_block_kind("bogus", "");
        assert_eq!(unknown.block_type, "unknown");
        assert_eq!(unknown.sub_type, "");
        assert_eq!(unknown.tags, vec!["unknown"]);
    }

    #[test]
    fn block_bbox_parses_numeric_and_string_entries() {
        let block = json!({"block_bbox": [1, 2.5, "300", 4]});
        assert_eq!(block_bbox(&block), Some(vec![1.0, 2.5, 300.0, 4.0]));
        assert_eq!(block_bbox(&json!({"block_bbox": [0, null, false, 1]})), Some(vec![0.0, 0.0, 0.0, 1.0]));
        assert_eq!(block_bbox(&json!({"block_bbox": [1, 2, 3]})), None);
        assert_eq!(block_bbox(&json!({"block_bbox": "nope"})), None);
        assert_eq!(block_bbox(&json!({})), None);
        assert_eq!(block_bbox(&json!({"block_bbox": [1, "x", 3, 4]})), None);
    }
}

/// `_bbox` — 4-element block_bbox list via `float(item or 0)`, else `None` (the
/// Python port returns `[]` on any invalid entry, which callers treat as absent).
pub fn block_bbox(block: &Value) -> Option<Vec<f64>> {
    let value = block.get("block_bbox").unwrap_or(&Value::Null);
    let list = value.as_array()?;
    if list.len() != 4 {
        return None;
    }
    let mut out = Vec::with_capacity(4);
    for item in list.iter().take(4) {
        if item.is_null() || item.as_bool() == Some(false) {
            out.push(0.0);
        } else if let Some(f) = item.as_f64() {
            out.push(f);
        } else if let Some(s) = item.as_str() {
            match s.trim().parse::<f64>() {
                Ok(f) => out.push(f),
                Err(_) => return None,
            }
        } else {
            return None;
        }
    }
    Some(out)
}
