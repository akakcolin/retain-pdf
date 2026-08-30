// Small JSON helpers shared by the normalize ports, mirroring Python coercion
// used across document_schema: `str(x or "")`-style string extraction, `int()`
// truncation, bbox normalization, provider-continuation grouping, and the
// previous-anchor block classifier shared by the paddle adapter.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::defaults::{default_block_continuation_hint, normalize_block_continuation_hint};

/// `str(value or "")`-style extraction: None/null/false → empty string, strings
/// verbatim, numbers formatted as JSON does, bools "true"/"false".
pub fn value_as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Coerce a value to a string like Python `str(x)`.
pub fn to_string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Python `int(x)` for JSON values: integers verbatim, floats truncated toward
/// zero, numeric strings parsed; everything else (including bool) → None.
pub fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(f) = n.as_f64() {
                Some(f.trunc() as i64)
            } else {
                None
            }
        }
        Value::String(s) => {
            if let Ok(parsed) = s.trim().parse::<f64>() {
                Some(parsed.trunc() as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `normalize_bbox`: a 4-element list passes through as floats; anything else →
/// `[0, 0, 0, 0]`.
pub fn normalize_bbox(value: Option<&Value>) -> Vec<f64> {
    match value {
        Some(Value::Array(items)) if items.len() == 4 => {
            items.iter().map(|item| item.as_f64().unwrap_or(0.0)).collect()
        }
        _ => vec![0.0, 0.0, 0.0, 0.0],
    }
}

/// `normalize_polygon`: a list of `[x, y]` pairs passes through as floats;
/// anything else → `[]`.
pub fn normalize_polygon(value: Option<&Value>) -> Vec<Vec<f64>> {
    match value {
        Some(Value::Array(items)) => {
            let mut out: Vec<Vec<f64>> = Vec::new();
            for item in items {
                if let Some(pts) = item.as_array() {
                    if pts.len() == 2 {
                        out.push(vec![
                            pts[0].as_f64().unwrap_or(0.0),
                            pts[1].as_f64().unwrap_or(0.0),
                        ]);
                    }
                }
            }
            out
        }
        _ => vec![],
    }
}

/// `build_text_segments`: a single `{type, raw_type, text, bbox=[0,0,0,0], score=None}`.
pub fn build_text_segments(text: &str, raw_type: &str, segment_type: &str) -> Vec<Value> {
    if text.is_empty() {
        return vec![];
    }
    vec![json!({
        "type": segment_type,
        "raw_type": raw_type,
        "text": text,
        "bbox": json!([0, 0, 0, 0]),
        "score": Value::Null,
    })]
}

/// `build_line_records`: a single line with `bbox` + segments as spans.
pub fn build_line_records(bbox: &Value, segments: &[Value]) -> Value {
    if segments.is_empty() {
        return Value::Array(vec![]);
    }
    json!([{ "bbox": bbox, "spans": segments }])
}

/// `build_block_record` (provider_adapters/common/block_builder.py) — the shared
/// block-record constructor; optional spec keys (layout_role/semantic_role/
/// structure_role/policy/provenance) are copied when present.
pub fn build_block_record(spec: &Value) -> Value {
    let bbox = spec
        .get("bbox")
        .cloned()
        .unwrap_or_else(|| json!([0.0, 0.0, 0.0, 0.0]));
    let block_type = spec
        .get("block_type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let order = spec.get("order").and_then(py_int).unwrap_or(0);
    let text = spec.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    let mut record = Map::new();
    record.insert("block_id".to_string(), spec.get("block_id").cloned().unwrap_or_default());
    record.insert(
        "page_index".to_string(),
        Value::from(spec.get("page_index").and_then(py_int).unwrap_or(0)),
    );
    record.insert("order".to_string(), Value::from(order));
    record.insert("type".to_string(), Value::String(block_type.clone()));
    record.insert("sub_type".to_string(), spec.get("sub_type").cloned().unwrap_or_default());
    record.insert("bbox".to_string(), bbox.clone());
    record.insert("text".to_string(), Value::String(text.clone()));
    record.insert(
        "lines".to_string(),
        spec.get("lines").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    record.insert(
        "segments".to_string(),
        spec.get("segments").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    record.insert(
        "tags".to_string(),
        spec.get("tags").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    record.insert(
        "derived".to_string(),
        spec.get("derived").cloned().unwrap_or_else(|| Value::Object(Map::new())),
    );
    record.insert(
        "continuation_hint".to_string(),
        normalize_block_continuation_hint(spec.get("continuation_hint").unwrap_or(&Value::Null)),
    );
    record.insert(
        "metadata".to_string(),
        spec.get("metadata").cloned().unwrap_or_else(|| Value::Object(Map::new())),
    );
    record.insert(
        "source".to_string(),
        spec.get("source").cloned().unwrap_or_else(|| Value::Object(Map::new())),
    );
    record.insert("reading_order".to_string(), Value::from(order));
    record.insert("geometry".to_string(), json!({ "bbox": bbox }));
    record.insert(
        "content".to_string(),
        spec.get("content")
            .cloned()
            .unwrap_or_else(|| json!({ "kind": block_type, "text": text })),
    );
    for key in ["layout_role", "semantic_role", "structure_role", "policy", "provenance"] {
        if let Some(value) = spec.get(key) {
            record.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(record)
}

/// `build_page_record` (provider_adapters/common/page_builder.py).
pub fn build_page_record(spec: &Value) -> Value {
    let page_index = spec.get("page_index").and_then(py_int).unwrap_or(0);
    let page = spec
        .get("page")
        .and_then(py_int)
        .unwrap_or(page_index + 1);
    json!({
        "page_index": page_index,
        "page": page,
        "width": spec.get("width").and_then(Value::as_f64).unwrap_or(0.0),
        "height": spec.get("height").and_then(Value::as_f64).unwrap_or(0.0),
        "unit": spec.get("unit").and_then(Value::as_str).unwrap_or("pt"),
        "blocks": spec.get("blocks").cloned().unwrap_or_else(|| Value::Array(vec![])),
        "metadata": spec.get("metadata").cloned().unwrap_or_else(|| Value::Object(Map::new())),
    })
}

/// `continuation_role_for(index, size)`.
pub fn continuation_role_for(index: usize, size: usize) -> &'static str {
    if size <= 1 {
        "single"
    } else if index == 0 {
        "head"
    } else if index == size - 1 {
        "tail"
    } else {
        "middle"
    }
}

/// `continuation_scope_for_blocks(blocks)`: `cross_page` if blocks span >1 page.
pub fn continuation_scope_for_blocks(blocks: &[Value]) -> &'static str {
    let mut page_indexes: BTreeSet<i64> = BTreeSet::new();
    for block in blocks {
        if let Some(pi) = block.get("page_index").and_then(py_int) {
            page_indexes.insert(pi);
        }
    }
    if page_indexes.len() > 1 {
        "cross_page"
    } else {
        "intra_page"
    }
}

/// `build_provider_continuation_hint` — empty group_id falls back to the default hint.
pub fn build_provider_continuation_hint(
    group_id: &str,
    role: &str,
    scope: &str,
    reading_order: i64,
    confidence: f64,
) -> Value {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return default_block_continuation_hint();
    }
    normalize_block_continuation_hint(&json!({
        "source": "provider",
        "group_id": group_id,
        "role": role,
        "scope": scope,
        "reading_order": reading_order,
        "confidence": confidence,
    }))
}

/// `assign_provider_group_continuation_hints` — writes `continuation_hint` onto each
/// block in place, using `reading_order_resolver` when provided (None → -1).
pub fn assign_provider_group_continuation_hints(
    group_id: &str,
    ordered_blocks: &mut [Value],
    confidence: f64,
    reading_order_resolver: Option<&dyn Fn(&Value) -> Option<i64>>,
) {
    if ordered_blocks.is_empty() {
        return;
    }
    let scope = continuation_scope_for_blocks(ordered_blocks);
    let size = ordered_blocks.len();
    for (index, block) in ordered_blocks.iter_mut().enumerate() {
        let reading_order = match reading_order_resolver {
            Some(resolver) => resolver(block).unwrap_or(-1),
            None => index as i64,
        };
        if let Some(obj) = block.as_object_mut() {
            obj.insert(
                "continuation_hint".to_string(),
                build_provider_continuation_hint(
                    group_id,
                    continuation_role_for(index, size),
                    scope,
                    reading_order,
                    confidence,
                ),
            );
        }
    }
}

/// `classify_with_previous_anchor`: run `resolver` over items, tracking the previous
/// anchor `(sub_type_or_type, index)` only for image/table/code/formula kinds.
pub fn classify_with_previous_anchor(
    items: &[Value],
    resolver: &dyn Fn(&Value, Option<(String, i64)>) -> Value,
    anchor_getter: &dyn Fn(&Value) -> Option<(String, String)>,
) -> Vec<Value> {
    let mut resolved: Vec<Value> = Vec::with_capacity(items.len());
    let mut previous_anchor: Option<(String, i64)> = None;
    for (index, item) in items.iter().enumerate() {
        let value = resolver(item, previous_anchor.clone());
        resolved.push(value.clone());
        if let Some((anchor_type, anchor_sub_type)) = anchor_getter(&value) {
            if matches!(anchor_type.as_str(), "image" | "table" | "code" | "formula") {
                let anchor = if anchor_sub_type.is_empty() {
                    anchor_type
                } else {
                    anchor_sub_type
                };
                previous_anchor = Some((anchor, index as i64));
            }
        }
    }
    resolved
}

/// `_normalize_tags`: lowercased, stripped, non-empty string tags as a set.
pub fn normalize_tags(value: Option<&Value>) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    match value {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    let t = s.trim().to_lowercase();
                    if !t.is_empty() {
                        out.insert(t);
                    }
                }
            }
        }
        _ => {}
    }
    out
}
