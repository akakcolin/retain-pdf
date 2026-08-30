// Port of `services/document_schema/defaults.py` — `apply_document_defaults_with_report`,
// the mechanical missing-key filler applied after every provider adapter. Mutates
// the freshly-built document in place (the python reference uses `in_place=True`
// for adapter output), mirroring Python truthiness for `.get(key, default) or default`.

use serde_json::{json, Map, Value};

use super::common::{py_int, value_as_str};

pub fn default_block_geometry() -> Value {
    json!({ "bbox": [0, 0, 0, 0] })
}

pub fn default_block_content() -> Value {
    json!({ "kind": "unknown", "text": "" })
}

pub fn default_block_policy() -> Value {
    json!({ "translate": false, "translate_reason": "missing_contract_fields" })
}

pub fn default_block_provenance() -> Value {
    json!({ "provider": "", "raw_label": "", "raw_sub_type": "", "raw_bbox": [0, 0, 0, 0], "raw_path": "" })
}

pub fn default_block_derived() -> Value {
    json!({ "role": "", "by": "", "confidence": 0.0 })
}

pub fn default_block_continuation_hint() -> Value {
    json!({ "source": "", "group_id": "", "role": "", "scope": "", "reading_order": -1, "confidence": 0.0 })
}

/// `normalize_block_continuation_hint`: rebuild the hint from defaults, keeping
/// only well-typed fields (str keys stripped, int reading_order clamped >= -1,
/// numeric confidence clamped to 0..1, bools rejected).
pub fn normalize_block_continuation_hint(value: &Value) -> Value {
    let mut hint = match default_block_continuation_hint() {
        Value::Object(map) => map,
        _ => unreachable!(),
    };
    if let Some(source) = value.as_object() {
        for key in ["source", "group_id", "role", "scope"] {
            if let Some(raw) = source.get(key) {
                if let Some(s) = raw.as_str() {
                    hint.insert(key.to_string(), Value::String(s.trim().to_string()));
                } else {
                    hint.insert(key.to_string(), Value::String(String::new()));
                }
            }
        }
        if let Some(reading_order) = source.get("reading_order") {
            if let Some(ro) = py_int(reading_order) {
                if !is_bool(reading_order) {
                    hint.insert("reading_order".to_string(), Value::from(ro.max(-1)));
                }
            }
        }
        if let Some(confidence) = source.get("confidence") {
            if let Some(c) = confidence.as_f64() {
                if !is_bool(confidence) {
                    hint.insert("confidence".to_string(), Value::from(c.clamp(0.0, 1.0)));
                }
            }
        }
    }
    Value::Object(hint)
}

pub fn is_bool(value: &Value) -> bool {
    matches!(value, Value::Bool(_))
}

fn increment(counter: &mut Map<String, Value>, key: &str) {
    *counter
        .entry(key.to_string())
        .or_insert(Value::from(0)) = Value::from(
        counter
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + 1,
    );
}

fn apply_document_defaults(document: &mut Map<String, Value>, report: &mut Map<String, Value>) {
    let doc_defaults = report
        .get_mut("document_defaults")
        .and_then(Value::as_object_mut)
        .expect("document_defaults");
    for (key, default) in [("derived", json!({})), ("markers", json!({}))] {
        if !document.contains_key(key) {
            document.insert(key.to_string(), default);
            increment(doc_defaults, key);
        }
    }
    if !document.contains_key("page_count") {
        if let Some(pages) = document.get("pages").and_then(Value::as_array) {
            document.insert("page_count".to_string(), Value::from(pages.len()));
            increment(doc_defaults, "page_count");
        }
    }
}

fn apply_page_defaults(page: &mut Map<String, Value>, page_index: usize, report: &mut Map<String, Value>) {
    let page_defaults = report
        .get_mut("page_defaults")
        .and_then(Value::as_object_mut)
        .expect("page_defaults");
    if !page.contains_key("page_index") {
        page.insert("page_index".to_string(), Value::from(page_index));
        increment(page_defaults, "page_index");
    }
}

fn apply_block_defaults(
    block: &mut Map<String, Value>,
    page_index: usize,
    order: usize,
    report: &mut Map<String, Value>,
) {
    let block_defaults = report
        .get_mut("block_defaults")
        .and_then(Value::as_object_mut)
        .expect("block_defaults");
    if !block.contains_key("page_index") {
        block.insert("page_index".to_string(), Value::from(page_index));
        increment(block_defaults, "page_index");
    }
    if !block.contains_key("order") {
        block.insert("order".to_string(), Value::from(order));
        increment(block_defaults, "order");
    }
    if !block.contains_key("reading_order") {
        let source = block.get("order").cloned().unwrap_or(Value::from(order));
        let reading_order = py_int(&source).unwrap_or(order as i64);
        block.insert("reading_order".to_string(), Value::from(reading_order));
        increment(block_defaults, "reading_order");
    }
    for (key, default) in [("tags", Value::Array(vec![])), ("metadata", Value::Object(Map::new())), ("source", Value::Object(Map::new()))] {
        if !block.contains_key(key) {
            block.insert(key.to_string(), default);
            increment(block_defaults, key);
        }
    }
    if !block.contains_key("geometry") {
        let bbox = block.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![]));
        let bbox_value = if let Some(list) = bbox.as_array() {
            if list.len() == 4 {
                bbox
            } else {
                Value::Array(vec![0.0, 0.0, 0.0, 0.0].into_iter().map(Value::from).collect())
            }
        } else {
            Value::Array(vec![0.0, 0.0, 0.0, 0.0].into_iter().map(Value::from).collect())
        };
        block.insert("geometry".to_string(), json!({ "bbox": bbox_value }));
        increment(block_defaults, "geometry");
    }
    if !block.contains_key("content") {
        let kind = value_as_str(block.get("type")).unwrap_or("unknown");
        let text = value_as_str(block.get("text")).unwrap_or("");
        block.insert(
            "content".to_string(),
            json!({ "kind": if kind.is_empty() { "unknown" } else { kind }, "text": text }),
        );
        increment(block_defaults, "content");
    }
    if !block.contains_key("layout_role") {
        block.insert("layout_role".to_string(), Value::String("unknown".to_string()));
        increment(block_defaults, "layout_role");
    }
    if !block.contains_key("semantic_role") {
        block.insert("semantic_role".to_string(), Value::String("unknown".to_string()));
        increment(block_defaults, "semantic_role");
    }
    if !block.contains_key("structure_role") {
        block.insert("structure_role".to_string(), Value::String(String::new()));
        increment(block_defaults, "structure_role");
    }
    if !block.contains_key("policy") {
        block.insert("policy".to_string(), default_block_policy());
        increment(block_defaults, "policy");
    }
    if !block.contains_key("provenance") {
        let mut provenance = match default_block_provenance() {
            Value::Object(map) => map,
            _ => unreachable!(),
        };
        let source = block.get("source").cloned().unwrap_or_else(|| Value::Object(Map::new()));
        let bbox = block.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![]));
        let provider = value_as_str(source.get("provider")).unwrap_or("");
        let raw_label = value_as_str(source.get("raw_type"))
            .or_else(|| value_as_str(source.get("raw_label")))
            .unwrap_or("");
        let raw_sub_type = value_as_str(source.get("raw_sub_type")).unwrap_or("");
        let raw_bbox = if bbox.as_array().map_or(false, |list| list.len() == 4) { bbox } else { Value::Array(vec![0.0, 0.0, 0.0, 0.0].into_iter().map(Value::from).collect()) };
        let raw_path = value_as_str(source.get("raw_path")).unwrap_or("");
        provenance.insert("provider".to_string(), Value::String(provider.to_string()));
        provenance.insert("raw_label".to_string(), Value::String(raw_label.to_string()));
        provenance.insert("raw_sub_type".to_string(), Value::String(raw_sub_type.to_string()));
        provenance.insert("raw_bbox".to_string(), raw_bbox);
        provenance.insert("raw_path".to_string(), Value::String(raw_path.to_string()));
        block.insert("provenance".to_string(), Value::Object(provenance));
        increment(block_defaults, "provenance");
    }
    if !block.contains_key("derived") {
        block.insert("derived".to_string(), default_block_derived());
        increment(block_defaults, "derived");
    }
    if !block.contains_key("continuation_hint") {
        block.insert("continuation_hint".to_string(), default_block_continuation_hint());
        increment(block_defaults, "continuation_hint");
    } else {
        let normalized = normalize_block_continuation_hint(&block["continuation_hint"]);
        if block["continuation_hint"] != normalized {
            block.insert("continuation_hint".to_string(), normalized);
            increment(block_defaults, "continuation_hint");
        }
    }
}

/// `apply_document_defaults_with_report(document, in_place=True)` — fills missing
/// document/page/block keys and returns the defaults summary report.
pub fn apply_document_defaults_with_report(document: &mut Value) -> Value {
    let mut report = json!({
        "document_defaults": {},
        "page_defaults": {},
        "block_defaults": {},
    });
    let doc_obj = document.as_object_mut().expect("document object");
    apply_document_defaults(doc_obj, report.as_object_mut().unwrap());
    let pages = document
        .get("pages")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    let pages_list = pages.as_array().cloned().unwrap_or_default();
    let mut blocks_seen = 0usize;
    for (page_index, page_value) in pages_list.iter().enumerate() {
        if let Some(page) = page_value.as_object() {
            let mut page = page.clone();
            apply_page_defaults(&mut page, page_index, report.as_object_mut().unwrap());
            if let Some(blocks) = page.get("blocks").and_then(Value::as_array) {
                blocks_seen += blocks.len();
                let mut blocks_out: Vec<Value> = Vec::with_capacity(blocks.len());
                for (order, block_value) in blocks.iter().enumerate() {
                    if let Some(block) = block_value.as_object() {
                        let mut block = block.clone();
                        apply_block_defaults(&mut block, page_index, order, report.as_object_mut().unwrap());
                        blocks_out.push(Value::Object(block));
                    } else {
                        blocks_out.push(block_value.clone());
                    }
                }
                page.insert("blocks".to_string(), Value::Array(blocks_out));
            }
            if let Some(page_obj) = document
                .get_mut("pages")
                .and_then(Value::as_array_mut)
                .and_then(|pages| pages.get_mut(page_index))
                .and_then(Value::as_object_mut)
            {
                *page_obj = page;
            }
        }
    }
    let mut summary = json!({
        "pages_seen": pages_list.len(),
        "blocks_seen": blocks_seen,
    });
    let report_obj = report.as_object_mut().unwrap();
    for key in ["document_defaults", "page_defaults", "block_defaults"] {
        summary.as_object_mut().unwrap().insert(
            key.to_string(),
            report_obj.get(key).cloned().unwrap_or_else(|| Value::Object(Map::new())),
        );
    }
    summary
}
