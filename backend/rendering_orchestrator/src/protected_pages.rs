//! R-2: OCR normalized-document protected-pages loading.
//!
//! Mirror of `source_cleanup/protected_blocks.py` + the
//! `document_schema/consumer_reader.py` accessors it uses. Reads
//! `ocr/normalized/document.v1.json` and returns the pages whose text blocks
//! carry `policy.translate == false` — kept-origin items the redaction chain
//! must protect. Missing path / unreadable file / JSON parse failure yield an
//! empty map (matching Python); a wrong document schema is an error that
//! propagates, exactly like `ensure_normalized_document`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

const SCHEMA_PREFIX: &str = "normalized_document_v";

/// `protected_blocks.protected_pages_from_document_path` — None/missing path or
/// unreadable/invalid JSON yields `{}`; a wrong schema is an error.
pub fn protected_pages_from_document_path(
    document_path: Option<&Path>,
) -> Result<BTreeMap<i32, Vec<Value>>> {
    let Some(path) = document_path else {
        return Ok(BTreeMap::new());
    };
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Ok(BTreeMap::new()),
    };
    let data: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return Ok(BTreeMap::new()),
    };
    protected_pages_from_document(&data)
}

/// `protected_blocks.protected_pages_from_document`.
pub fn protected_pages_from_document(data: &Value) -> Result<BTreeMap<i32, Vec<Value>>> {
    let obj = data
        .as_object()
        .ok_or_else(|| anyhow!("expected normalized_document_v1 JSON data"))?;
    let schema = obj.get("schema").and_then(Value::as_str).unwrap_or("");
    if !schema.starts_with(SCHEMA_PREFIX) {
        bail!("expected normalized_document_v1 JSON data");
    }
    let pages = obj.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut protected_pages: BTreeMap<i32, Vec<Value>> = BTreeMap::new();
    for page in pages {
        let Some(page_obj) = page.as_object() else {
            continue;
        };
        let page_index = page_index_of(page_obj);
        let mut items: Vec<Value> = Vec::new();
        for block in page_obj.get("blocks").and_then(Value::as_array).into_iter().flatten() {
            if block_should_protect_source(block) {
                if let Some(item) = protected_item_from_block(block) {
                    items.push(item);
                }
            }
        }
        if !items.is_empty() {
            protected_pages.insert(page_index, items);
        }
    }
    Ok(protected_pages)
}

/// `consumer_reader.block_should_protect_source`: text kind, `policy.translate
/// == false`, non-degenerate bbox, non-empty text.
fn block_should_protect_source(block: &Value) -> bool {
    let Some(obj) = block.as_object() else {
        return false;
    };
    if content_kind(obj) != "text" {
        return false;
    }
    let policy_translate = obj
        .get("policy")
        .and_then(Value::as_object)
        .and_then(|policy| policy.get("translate"));
    if !matches!(policy_translate, Some(Value::Bool(false))) {
        return false;
    }
    let bbox = block_bbox(obj);
    let Some(coords) = bbox.as_array() else {
        return false;
    };
    if coords.len() != 4 || coords.iter().all(|v| as_f64(v).unwrap_or(0.0) == 0.0) {
        return false;
    }
    !block_text(obj).trim().is_empty()
}

/// `consumer_reader.block_bbox` — block `bbox` (len 4) else `geometry.bbox`
/// (len 4) else `[0, 0, 0, 0]`.
fn block_bbox(obj: &Map<String, Value>) -> Value {
    if let Some(Value::Array(bbox)) = obj.get("bbox") {
        if bbox.len() == 4 {
            return Value::Array(bbox.clone());
        }
    }
    if let Some(Value::Object(geometry)) = obj.get("geometry") {
        if let Some(Value::Array(bbox)) = geometry.get("bbox") {
            if bbox.len() == 4 {
                return Value::Array(bbox.clone());
            }
        }
    }
    json!([0, 0, 0, 0])
}

/// `protected_blocks.protected_item_from_block` — kept-origin item DTO.
fn protected_item_from_block(block: &Value) -> Option<Value> {
    let obj = block.as_object()?;
    let bbox = block_bbox(obj);
    if bbox.as_array()?.len() != 4 {
        return None;
    }
    let text = block_text(obj);
    Some(json!({
        "item_id": block_id(obj),
        "block_kind": "text",
        "block_type": "text",
        "bbox": bbox,
        "source_text": text,
        "protected_source_text": text,
        "final_status": "kept_origin",
    }))
}

/// `consumer_reader.block_kind`: `str(content.kind or "unknown").strip().lower()`.
fn content_kind(obj: &Map<String, Value>) -> String {
    let raw = obj
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("kind"));
    match raw {
        Some(value) if json_truthy(value) => json_to_string(value),
        _ => "unknown".to_string(),
    }
    .trim()
    .to_lowercase()
}

/// `consumer_reader.block_text`: `str(content.text or "")`.
fn block_text(obj: &Map<String, Value>) -> String {
    obj.get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("text"))
        .map(json_to_string)
        .unwrap_or_default()
}

fn block_id(obj: &Map<String, Value>) -> String {
    json_to_string(obj.get("block_id").unwrap_or(&Value::String(String::new())))
}

/// `int(page.get("page_index", page.get("page", 1) - 1) or 0)`.
fn page_index_of(page: &Map<String, Value>) -> i32 {
    let raw = match page.get("page_index") {
        Some(value) if json_truthy(value) => value,
        _ => {
            let p = match page.get("page") {
                Some(value) if json_truthy(value) => as_f64(value).unwrap_or(1.0),
                _ => 1.0,
            };
            return (p - 1.0) as i32;
        }
    };
    as_f64(raw).unwrap_or(0.0) as i32
}

/// Python `bool(value)` for JSON: null/empty-string/zero/false are falsy.
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Python `str(value)` for a JSON value (compact for non-strings).
fn json_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python `float(value or 0.0)` — numeric JSON or numeric string.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let unique = format!(
                "{label}-{}-{:?}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn text_block(id: &str, text: &str, policy_translate: bool) -> Value {
        json!({
            "block_id": id,
            "bbox": [10.0, 20.0, 30.0, 40.0],
            "content": {"kind": "text", "text": text},
            "policy": {"translate": policy_translate},
        })
    }

    fn doc_with_pages(pages: Vec<Value>) -> Value {
        json!({"schema": "normalized_document_v1", "pages": pages})
    }

    #[test]
    fn extracts_only_policy_translate_false_text_blocks() {
        let doc = doc_with_pages(vec![json!({
            "page_index": 0,
            "page": 1,
            "blocks": [
                text_block("p001-b0000", "protected title", false),
                text_block("p001-b0001", "normal paragraph", true),
                json!({"block_id": "p001-b0002", "bbox": [1.0, 2.0, 3.0, 4.0],
                       "content": {"kind": "image"}, "policy": {"translate": false}}),
            ],
        })]);
        let pages = protected_pages_from_document(&doc).unwrap();
        assert_eq!(pages.len(), 1);
        let items = &pages[&0];
        assert_eq!(items.len(), 1);
        let first = &items[0];
        assert_eq!(first["item_id"], "p001-b0000");
        assert_eq!(first["final_status"], "kept_origin");
        assert_eq!(first["source_text"], "protected title");
        assert_eq!(first["bbox"], json!([10.0, 20.0, 30.0, 40.0]));
    }

    #[test]
    fn skips_degenerate_bbox_and_empty_text() {
        let doc = doc_with_pages(vec![json!({
            "page_index": 0,
            "blocks": [
                json!({"block_id": "z1", "bbox": [0, 0, 0, 0],
                       "content": {"kind": "text", "text": "text"}, "policy": {"translate": false}}),
                json!({"block_id": "z2", "bbox": [1.0, 2.0, 3.0, 4.0],
                       "content": {"kind": "text", "text": "   "}, "policy": {"translate": false}}),
            ],
        })]);
        let pages = protected_pages_from_document(&doc).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn uses_geometry_bbox_fallback_and_page_number_default() {
        let doc = doc_with_pages(vec![json!({
            "page": 3,
            "blocks": [
                json!({"block_id": "g1",
                       "geometry": {"bbox": [5.0, 6.0, 7.0, 8.0]},
                       "content": {"kind": "text", "text": "kept"},
                       "policy": {"translate": false}}),
            ],
        })]);
        let pages = protected_pages_from_document(&doc).unwrap();
        assert_eq!(pages.keys().copied().collect::<Vec<_>>(), vec![2]);
        assert_eq!(pages[&2][0]["bbox"], json!([5.0, 6.0, 7.0, 8.0]));
    }

    #[test]
    fn rejects_wrong_schema() {
        let doc = json!({"schema": "other_v1", "pages": []});
        let err = protected_pages_from_document(&doc).unwrap_err();
        assert!(err.to_string().contains("normalized_document_v1"));
    }

    #[test]
    fn missing_path_is_empty() {
        let dir = TempDir::new("r2-missing");
        let pages = protected_pages_from_document_path(Some(&dir.path().join("nope.json"))).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn invalid_json_is_empty() {
        let dir = TempDir::new("r2-badjson");
        let path = dir.path().join("doc.json");
        fs::write(&path, "not json").unwrap();
        let pages = protected_pages_from_document_path(Some(&path)).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn loads_from_path() {
        let dir = TempDir::new("r2-path");
        let path = dir.path().join("document.v1.json");
        fs::write(
            &path,
            serde_json::to_string(&doc_with_pages(vec![json!({
                "page_index": 0,
                "blocks": [text_block("p001-b0000", "kept", false)],
            })])).unwrap(),
        )
        .unwrap();
        let pages = protected_pages_from_document_path(Some(&path)).unwrap();
        assert_eq!(pages[&0][0]["item_id"], "p001-b0000");
    }
}
