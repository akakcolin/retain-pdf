// Port of `services/mineru/document_v1.py` — the MinerU raw `layout.json`
// payload → normalized document.v1 adapter (the default OCR provider). The
// builder output is freshly constructed so defaults may mutate it in place.

use std::path::Path;

use serde_json::{json, Map, Value};

use super::version::{DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION};

pub const PROVIDER_MINERU: &str = "mineru";

/// `_MATH_CONTROL_CHAR_RE`: ASCII control chars except \t(\x09) \n(\x0a) \r(\x0d).
fn is_math_control_char(c: char) -> bool {
    let code = c as u32;
    matches!(code, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f)
}

fn math_control_indices(text: &str) -> Vec<usize> {
    text.chars()
        .enumerate()
        .filter_map(|(index, c)| if is_math_control_char(c) { Some(index) } else { None })
        .collect()
}

/// `_repair_math_control_chars`: control-char formula repair (MinerU emits raw
/// control bytes inside some formula text). Operates on code-point indices like
/// Python `re`. Control chars with a formula-ish neighbor context become
/// `\theta`, otherwise a space.
fn repair_math_control_chars(text: &str, next_text: &str) -> String {
    if text.is_empty() || math_control_indices(text).is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut pos = 0usize;
    for index in math_control_indices(text) {
        out.extend(chars[pos..index].iter());
        let before_start = index.saturating_sub(48);
        let before: String = chars[before_start..index].iter().collect::<String>().to_lowercase();
        let after_end = (index + 1 + 48).min(chars.len());
        let mut after: String = chars[index + 1..after_end].iter().collect::<String>();
        after.push(' ');
        after.push_str(&next_text.chars().take(48).collect::<String>());
        after = after.to_lowercase();
        if before_math_context(&before) || after_math_context(&after) {
            out.push_str(r"\theta");
        } else {
            out.push(' ');
        }
        pos = index + 1;
    }
    out.extend(chars[pos..].iter());
    out
}

fn before_math_context(before: &str) -> bool {
    const SUFFIXES: [&str; 7] = [
        "fixing",
        "rotation angle",
        "torsion angle",
        "dihedral angle",
        "angle",
        "angles",
        "function of",
    ];
    let trimmed_end = before.trim_end();
    SUFFIXES.iter().any(|suffix| trimmed_end.ends_with(suffix))
}

fn after_math_context(after: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "as a dihedral angle",
        "of the methyl group",
        "varying",
        "represents",
        "=",
        "and",
        "or",
        ")",
    ];
    let trimmed = after.trim_start();
    PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
}

/// `_normalize_text`: repair control chars, then collapse whitespace.
fn normalize_text(raw_text: &str, next_text: &str) -> String {
    repair_math_control_chars(raw_text, next_text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn iter_layout_pages(payload: &Value) -> Vec<Value> {
    payload
        .get("pdf_info")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn iter_page_blocks(page: &Map<String, Value>) -> Vec<Value> {
    page.get("para_blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn iter_child_blocks(block: &Map<String, Value>) -> Vec<Value> {
    block
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn block_segments(block: &Map<String, Value>) -> Vec<Value> {
    let mut segments: Vec<Value> = Vec::new();
    let lines = block.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
    for line in &lines {
        let Some(line_obj) = line.as_object() else { continue };
        let spans = line_obj.get("spans").and_then(Value::as_array).cloned().unwrap_or_default();
        for (index, span) in spans.iter().enumerate() {
            let content = span.get("content").and_then(Value::as_str).unwrap_or("");
            if content.trim().is_empty() {
                continue;
            }
            let next_content = spans
                .get(index + 1)
                .and_then(|s| s.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let span_type = span.get("type").and_then(Value::as_str).unwrap_or("text");
            let span_type = if span_type.is_empty() { "text" } else { span_type };
            segments.push(json!({
                "type": if span_type == "inline_equation" { "formula" } else { "text" },
                "raw_type": span_type,
                "text": normalize_text(content, next_content),
                "bbox": span.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
                "score": span.get("score").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    segments
}

fn block_lines(block: &Map<String, Value>) -> Vec<Value> {
    let mut lines_out: Vec<Value> = Vec::new();
    let lines = block.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
    for line in &lines {
        let Some(line_obj) = line.as_object() else { continue };
        let mut spans_out: Vec<Value> = Vec::new();
        let spans = line_obj.get("spans").and_then(Value::as_array).cloned().unwrap_or_default();
        for (index, span) in spans.iter().enumerate() {
            let content = span.get("content").and_then(Value::as_str).unwrap_or("");
            if content.trim().is_empty() {
                continue;
            }
            let next_content = spans
                .get(index + 1)
                .and_then(|s| s.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let span_type = span.get("type").and_then(Value::as_str).unwrap_or("text");
            let span_type = if span_type.is_empty() { "text" } else { span_type };
            spans_out.push(json!({
                "type": if span_type == "inline_equation" { "formula" } else { "text" },
                "raw_type": span_type,
                "text": normalize_text(content, next_content),
                "bbox": span.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
                "score": span.get("score").cloned().unwrap_or(Value::Null),
            }));
        }
        if !spans_out.is_empty() {
            lines_out.push(json!({
                "bbox": line_obj.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
                "spans": spans_out,
            }));
        }
    }
    lines_out
}

fn merge_segments_text(segments: &[Value]) -> String {
    segments
        .iter()
        .filter_map(|segment| segment.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn map_block_kind(raw_type: &str, raw_sub_type: &str, has_text: bool) -> (String, String) {
    let normalized_sub = raw_sub_type.trim().to_lowercase();
    match raw_type {
        "title" => ("text".into(), "title".into()),
        "interline_equation" => ("formula".into(), "display_formula".into()),
        "image" | "image_body" => ("image".into(), "figure".into()),
        "table" | "table_body" => ("table".into(), "table_body".into()),
        "code" | "code_body" => ("code".into(), "code_block".into()),
        _ if normalized_sub == "algorithm" => ("code".into(), "code_block".into()),
        "page_header" => ("text".into(), "header".into()),
        "page_footer" => ("text".into(), "footer".into()),
        "page_number" => ("text".into(), "page_number".into()),
        "image_footnote" | "table_footnote" => ("text".into(), "footnote".into()),
        "text" | "list" => ("text".into(), "body".into()),
        _ if has_text => ("text".into(), "metadata".into()),
        _ => ("unknown".into(), String::new()),
    }
}

fn default_derived() -> Value {
    json!({ "role": "", "by": "", "confidence": 0.0 })
}

fn caption_derived(raw_type: &str) -> (Vec<Value>, Value) {
    const CAPTION_TYPES: [&str; 4] = ["image_caption", "table_caption", "table_footnote", "image_footnote"];
    if !CAPTION_TYPES.contains(&raw_type) {
        return (vec![], default_derived());
    }
    (
        vec![Value::String("caption".to_string()), Value::String(raw_type.to_string())],
        json!({ "role": "caption", "by": "provider_rule", "confidence": 0.98 }),
    )
}

fn make_raw_path(page_idx: usize, parts: &[String]) -> String {
    let mut path = format!("/pdf_info/{page_idx}");
    for part in parts {
        path.push('/');
        path.push_str(part);
    }
    path
}

fn build_document_source(layout_json_path: &Path, provider_version: &str) -> Value {
    json!({
        "provider": PROVIDER_MINERU,
        "provider_version": provider_version,
        "raw_files": {
            "layout_json": layout_json_path.to_string_lossy(),
        },
    })
}

fn build_block_record(
    block: &Map<String, Value>,
    page_idx: usize,
    page_block_index: usize,
    raw_path_parts: &[String],
    parent_block_id: Option<&str>,
) -> Value {
    let raw_type = block.get("type").and_then(Value::as_str).unwrap_or("");
    let raw_sub_type = block.get("sub_type").and_then(Value::as_str).unwrap_or("");
    let segments = block_segments(block);
    let lines = block_lines(block);
    let text = merge_segments_text(&segments);
    let (block_type, sub_type) = map_block_kind(raw_type, raw_sub_type, !text.is_empty());
    let block_id = format!("p{:03}-b{:04}", page_idx + 1, page_block_index);
    let (tags, derived) = caption_derived(raw_type);
    let raw_bbox = block.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![]));
    json!({
        "block_id": block_id,
        "page_index": page_idx,
        "order": page_block_index,
        "type": block_type,
        "sub_type": sub_type,
        "bbox": raw_bbox.clone(),
        "text": text,
        "lines": lines,
        "segments": segments,
        "tags": tags,
        "derived": derived,
        "metadata": {
            "raw_index": block.get("index").cloned().unwrap_or(Value::Null),
            "raw_angle": block.get("angle").cloned().unwrap_or(Value::Null),
            "raw_sub_type": raw_sub_type,
            "parent_block_id": parent_block_id.unwrap_or(""),
        },
        "source": {
            "provider": PROVIDER_MINERU,
            "raw_page_index": page_idx,
            "raw_path": make_raw_path(page_idx, raw_path_parts),
            "raw_type": raw_type,
            "raw_sub_type": raw_sub_type,
            "raw_bbox": raw_bbox,
            "raw_text_excerpt": text.chars().take(200).collect::<String>(),
        },
    })
}

fn build_page_record(page: &Map<String, Value>, page_idx: usize) -> Value {
    let page_size = page.get("page_size").and_then(Value::as_array).cloned().unwrap_or_default();
    let width = page_size.get(0).and_then(Value::as_f64).unwrap_or(0.0);
    let height = page_size.get(1).and_then(Value::as_f64).unwrap_or(0.0);
    let mut blocks_out: Vec<Value> = Vec::new();
    let mut page_block_index = 0usize;

    fn visit_block(
        block: &Map<String, Value>,
        raw_path_parts: &[String],
        parent_block_id: Option<&str>,
        page_idx: usize,
        page_block_index: &mut usize,
        blocks_out: &mut Vec<Value>,
    ) {
        let record = build_block_record(block, page_idx, *page_block_index, raw_path_parts, parent_block_id);
        let current_block_id = record["block_id"].as_str().unwrap().to_string();
        blocks_out.push(record);
        *page_block_index += 1;
        let children = iter_child_blocks(block);
        for (child_idx, child) in children.iter().enumerate() {
            let mut child_parts = raw_path_parts.to_vec();
            child_parts.push(format!("blocks/{child_idx}"));
            if let Some(child_obj) = child.as_object() {
                visit_block(
                    child_obj,
                    &child_parts,
                    Some(&current_block_id),
                    page_idx,
                    page_block_index,
                    blocks_out,
                );
            }
        }
    }

    let blocks = iter_page_blocks(page);
    for (block_idx, block) in blocks.iter().enumerate() {
        if let Some(block_obj) = block.as_object() {
            let parts = vec![format!("para_blocks/{block_idx}")];
            visit_block(
                block_obj,
                &parts,
                None,
                page_idx,
                &mut page_block_index,
                &mut blocks_out,
            );
        }
    }

    json!({
        "page_index": page_idx,
        "width": width,
        "height": height,
        "unit": "pt",
        "blocks": blocks_out,
    })
}

/// `build_normalized_document` — raw MinerU layout payload → document.v1 dict.
pub fn build_mineru_document(
    payload: &Value,
    document_id: &str,
    source_json_path: &Path,
    provider_version: &str,
) -> Value {
    let pages_out: Vec<Value> = iter_layout_pages(payload)
        .iter()
        .enumerate()
        .filter_map(|(page_idx, page)| page.as_object().map(|obj| build_page_record(obj, page_idx)))
        .collect();
    json!({
        "schema": DOCUMENT_SCHEMA_NAME,
        "schema_version": DOCUMENT_SCHEMA_VERSION,
        "document_id": document_id,
        "source": build_document_source(source_json_path, provider_version),
        "page_count": pages_out.len(),
        "pages": pages_out,
        "derived": {
            "notes": "This layer stores post-OCR semantic conclusions from provider rules, local rules, or later LLM judgment.",
        },
    })
}
