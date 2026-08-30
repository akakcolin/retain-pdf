// Port of `document_schema/provider_adapters/paddle/content_extract.py` (the
// `build_segments`/`build_lines`/`tighten_text_bbox` helpers) plus the
// unconditional `post_rescale_rebuild_paddle_text_geometry` step from
// `ocr_provider/paddle_normalize.py`. The normalize worker runs the rebuild for
// every provider (mineru included), so these helpers drive line structure even
// when the source adapter is not paddle.

use std::collections::HashMap;

use rendering_core::payload::toc_document::build_toc_entries;
use rendering_core::text_flow::py_splitlines;
use rendering_core::util::py_round;
use serde_json::{json, Value};

use super::formula_protection::protect_inline_formulas;
use super::formula_protection::protected_token_re;

const APPROX_TEXT_CHAR_WIDTH_PT: f64 = 5.2;
const MIN_PSEUDO_LINE_PITCH_PT: f64 = 11.0;
const TARGET_PSEUDO_LINE_PITCH_PT: f64 = 12.0;
const PSEUDO_TEXT_HEIGHT_SLACK_RATIO: f64 = 1.08;
const BODYLIKE_SUBTYPES: [&str; 2] = ["body", "heading"];

fn is_bodylike(sub_type: &str) -> bool {
    BODYLIKE_SUBTYPES.contains(&sub_type.trim().to_lowercase().as_str())
}

fn segment_record(text: &str, raw_label: &str, segment_type: &str) -> Value {
    json!({
        "type": segment_type,
        "raw_type": raw_label,
        "text": text,
        "bbox": [0, 0, 0, 0],
        "score": null,
    })
}

fn build_text_segments(text: &str, raw_type: &str, segment_type: &str) -> Vec<Value> {
    if text.is_empty() {
        return vec![];
    }
    vec![segment_record(text, raw_type, segment_type)]
}

fn build_line_records(bbox: &Value, segments: &[Value]) -> Vec<Value> {
    if segments.is_empty() {
        return vec![];
    }
    vec![json!({ "bbox": bbox, "spans": segments })]
}

/// `_split_text_with_inline_formulas` — protect inline formulas, then re-split
/// the protected text into text/formula segments.
fn split_text_with_inline_formulas(text: &str, raw_label: &str) -> Vec<Value> {
    let (protected_text, formula_map) = protect_inline_formulas(text);
    if formula_map.is_empty() {
        return build_text_segments(text, raw_label, "text");
    }
    let lookup: HashMap<String, String> = formula_map
        .iter()
        .filter_map(|entry| {
            let placeholder = entry.get("placeholder").and_then(Value::as_str)?.to_string();
            let formula_text = entry.get("formula_text").and_then(Value::as_str)?.to_string();
            Some((placeholder, formula_text))
        })
        .collect();
    let mut segments: Vec<Value> = Vec::new();
    let mut cursor = 0usize;
    for m in protected_token_re().find_iter(&protected_text) {
        let start = m.start();
        let end = m.end();
        if start > cursor {
            let chunk = &protected_text[cursor..start];
            if !chunk.trim().is_empty() {
                segments.push(segment_record(chunk.trim(), raw_label, "text"));
            }
        }
        let placeholder = m.as_str().to_string();
        let formula_text = lookup
            .get(&placeholder)
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        if !formula_text.is_empty() {
            segments.push(segment_record(&formula_text, raw_label, "formula"));
        }
        cursor = end;
    }
    let tail = &protected_text[cursor..];
    if !tail.trim().is_empty() {
        segments.push(segment_record(tail.trim(), raw_label, "text"));
    }
    if segments.is_empty() {
        build_text_segments(text, raw_label, "text")
    } else {
        segments
    }
}

/// `build_segments` from content_extract.py.
fn build_segments(text: &str, raw_label: &str) -> Vec<Value> {
    let label = raw_label.trim().to_lowercase();
    if label == "display_formula" || label == "formula" {
        return build_text_segments(text, raw_label, "formula");
    }
    if label == "text" {
        return split_text_with_inline_formulas(text, raw_label);
    }
    build_text_segments(text, raw_label, "text")
}

fn bbox_width(bbox: &Value) -> f64 {
    match bbox.as_array() {
        Some(arr) if arr.len() == 4 => {
            let x0 = arr[0].as_f64().unwrap_or(0.0);
            let x1 = arr[2].as_f64().unwrap_or(0.0);
            (x1 - x0).max(0.0)
        }
        _ => 0.0,
    }
}

fn bbox_height(bbox: &Value) -> f64 {
    match bbox.as_array() {
        Some(arr) if arr.len() == 4 => {
            let y0 = arr[1].as_f64().unwrap_or(0.0);
            let y1 = arr[3].as_f64().unwrap_or(0.0);
            (y1 - y0).max(0.0)
        }
        _ => 0.0,
    }
}

/// `_compact_text_len`: len(re.sub(r"\s+", "", text)).
fn compact_text_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

fn estimated_chars_per_line(width_pt: f64) -> usize {
    (width_pt / APPROX_TEXT_CHAR_WIDTH_PT) as usize
}

fn bbox_f64s(bbox: &Value) -> Option<(f64, f64, f64, f64)> {
    let arr = bbox.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    Some((
        arr[0].as_f64()?,
        arr[1].as_f64()?,
        arr[2].as_f64()?,
        arr[3].as_f64()?,
    ))
}

/// `_split_words_evenly`.
fn split_words_evenly(text: &str, line_count: usize, chars_per_line: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        return if compact.is_empty() { vec![] } else { vec![compact] };
    }
    let total_compact_len = compact_text_len(text);
    let target = ((total_compact_len as f64) / (line_count.max(1) as f64)).ceil().max(10.0) as usize;
    let break_threshold = chars_per_line.min(
        target.max((chars_per_line as f64 * 0.58) as usize),
    );
    let mut chunks: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_len = 0usize;
    let mut remaining_words = words.len();
    let mut remaining_lines = line_count.max(1);

    for word in words {
        remaining_words -= 1;
        let projected = current_len + (if current.is_empty() { 0 } else { 1 }) + word.chars().count();
        let force_break = !current.is_empty()
            && current_len >= break_threshold
            && remaining_lines > 1
            && remaining_words >= remaining_lines - 1;
        if force_break {
            chunks.push(current.join(" "));
            current = vec![word];
            current_len = word.chars().count();
            remaining_lines -= 1;
            continue;
        }
        current.push(word);
        current_len = projected;
    }
    if !current.is_empty() {
        chunks.push(current.join(" "));
    }
    chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect()
}

/// `_pseudo_line_count`.
fn pseudo_line_count(bbox: &Value, text: &str) -> usize {
    let width_pt = bbox_width(bbox);
    let height_pt = bbox_height(bbox);
    let text_len = compact_text_len(text);
    if width_pt <= 0.0 || height_pt <= 0.0 || text_len < 72 {
        return 0;
    }
    if height_pt < MIN_PSEUDO_LINE_PITCH_PT * 2.2 {
        return 0;
    }
    let chars_per_line = estimated_chars_per_line(width_pt).max(12);
    let predicted_by_width = ((text_len as f64) / (chars_per_line.max(1) as f64)).ceil().max(2.0) as usize;
    let max_lines_by_height = ((height_pt / MIN_PSEUDO_LINE_PITCH_PT) as usize).max(1);
    let desired_by_height = py_round(height_pt / TARGET_PSEUDO_LINE_PITCH_PT, 0).max(2.0) as usize;
    max_lines_by_height.min(predicted_by_width.max(desired_by_height))
}

/// `tighten_text_bbox`.
pub fn tighten_text_bbox(bbox: &Value, text: &str, block_type: &str, sub_type: &str) -> Vec<f64> {
    if block_type != "text" || !is_bodylike(sub_type) {
        return bbox_f64s(bbox).map(|v| vec![v.0, v.1, v.2, v.3]).unwrap_or_default();
    }
    let Some((x0, y0, x1, y1)) = bbox_f64s(bbox) else {
        return vec![];
    };
    let line_count = pseudo_line_count(bbox, text);
    if line_count <= 1 {
        return vec![x0, y0, x1, y1];
    }
    let original_height = (y1 - y0).max(0.0);
    let target_height = original_height.min(
        (TARGET_PSEUDO_LINE_PITCH_PT * 2.0)
            .max(line_count as f64 * TARGET_PSEUDO_LINE_PITCH_PT * PSEUDO_TEXT_HEIGHT_SLACK_RATIO),
    );
    let new_y1 = py_round(y1.min(y0 + target_height), 3);
    vec![x0, y0, x1, new_y1]
}

/// `_build_pseudo_lines`.
fn build_pseudo_lines(bbox: &Value, text: &str, raw_label: &str) -> Vec<Value> {
    let line_count = pseudo_line_count(bbox, text);
    if line_count <= 1 {
        return vec![];
    }
    let width_pt = bbox_width(bbox);
    let height_pt = bbox_height(bbox);
    let chars_per_line = estimated_chars_per_line(width_pt).max(12);
    let chunks = split_words_evenly(text, line_count, chars_per_line);
    if chunks.len() <= 1 {
        return vec![];
    }
    let Some((x0, y0, x1, y1)) = bbox_f64s(bbox) else {
        return vec![];
    };
    let total_lines = chunks.len();
    let line_height = height_pt / total_lines as f64;
    let mut lines: Vec<Value> = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let line_y0 = y0 + line_height * index as f64;
        let line_y1 = if index == total_lines - 1 {
            y1
        } else {
            y0 + line_height * (index + 1) as f64
        };
        lines.push(json!({
            "bbox": [x0, py_round(line_y0, 3), x1, py_round(line_y1, 3)],
            "spans": build_segments(chunk, raw_label),
        }));
    }
    lines
}

/// `_build_explicit_text_lines`.
fn build_explicit_text_lines(bbox: &Value, text: &str, raw_label: &str) -> Vec<Value> {
    let chunks: Vec<String> = py_splitlines(text)
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if chunks.len() <= 1 {
        return vec![];
    }
    let Some((x0, y0, x1, y1)) = bbox_f64s(bbox) else {
        return vec![];
    };
    let total_lines = chunks.len();
    let line_height = ((y1 - y0) / total_lines as f64).max(1.0);
    let mut lines: Vec<Value> = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let line_y0 = y0 + line_height * index as f64;
        let line_y1 = if index == total_lines - 1 {
            y1
        } else {
            y0 + line_height * (index + 1) as f64
        };
        lines.push(json!({
            "bbox": [x0, py_round(line_y0, 3), x1, py_round(line_y1, 3)],
            "spans": build_segments(chunk, raw_label),
        }));
    }
    lines
}

/// `build_lines` from content_extract.py.
fn build_lines(
    bbox: &Value,
    segments: &[Value],
    text: &str,
    raw_label: &str,
    block_type: &str,
    sub_type: &str,
) -> Vec<Value> {
    let explicit_lines = build_explicit_text_lines(bbox, text, raw_label);
    if !explicit_lines.is_empty() {
        return explicit_lines;
    }
    if block_type == "text" && is_bodylike(sub_type) {
        let pseudo_bbox_value = tighten_text_bbox(bbox, text, block_type, sub_type);
        let pseudo_bbox = json!(pseudo_bbox_value);
        let pseudo_lines = build_pseudo_lines(&pseudo_bbox, text, raw_label);
        if !pseudo_lines.is_empty() {
            return pseudo_lines;
        }
    }
    build_line_records(bbox, segments)
}

/// `post_rescale_rebuild_paddle_text_geometry` — unconditional rebuild of every
/// block's lines (and bodylike bbox tightening) after geometry rescale.
pub fn post_rescale_rebuild_paddle_text_geometry(document: &mut Value) {
    let Some(pages) = document.get_mut("pages").and_then(Value::as_array_mut) else {
        return;
    };
    for page in pages.iter_mut() {
        let Some(blocks) = page.get_mut("blocks").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks.iter_mut() {
            let block_type = block.get("type").and_then(Value::as_str).unwrap_or("").to_string();
            let sub_type = block.get("sub_type").and_then(Value::as_str).unwrap_or("").to_string();
            let text = block.get("text").and_then(Value::as_str).unwrap_or("").to_string();
            let raw_label = block
                .get("source")
                .and_then(|s| s.get("raw_type"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let original_bbox: Vec<f64> = block
                .get("bbox")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
                .unwrap_or_default();
            let tightened_bbox = tighten_text_bbox(
                &block.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
                &text,
                &block_type,
                &sub_type,
            );
            if tightened_bbox != original_bbox {
                block["bbox"] = json!(tightened_bbox);
                if let Some(source) = block.get_mut("source") {
                    if source.is_object() {
                        source["raw_bbox"] = json!(tightened_bbox);
                    }
                }
                let metadata = block
                    .get_mut("metadata")
                    .and_then(Value::as_object_mut);
                if let Some(metadata) = metadata {
                    metadata.insert("provider_bbox_tightened".to_string(), Value::Bool(true));
                    metadata.insert("provider_bbox_original".to_string(), json!(original_bbox));
                }
            }
            let segments = block
                .get("segments")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let rebuilt_lines = build_lines(
                &block.get("bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
                &segments,
                &text,
                &raw_label,
                &block_type,
                &sub_type,
            );
            if rebuilt_lines.is_empty() {
                continue;
            }
            block["lines"] = Value::Array(rebuilt_lines);
            let structure_role = block
                .get("structure_role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if structure_role != "table_of_contents" {
                continue;
            }
            let lines_value = block
                .get("lines")
                .cloned()
                .unwrap_or_else(|| Value::Array(vec![]));
            let content_obj = block.get_mut("content").and_then(Value::as_object_mut);
            let Some(content) = content_obj else {
                continue;
            };
            let line_texts = content.get("line_texts").cloned().unwrap_or_else(|| Value::Array(vec![]));
            let Some(line_texts_arr) = line_texts.as_array() else {
                continue;
            };
            let line_texts_value = Value::Array(line_texts_arr.clone());
            let toc_entries = build_toc_entries(&lines_value, &line_texts_value);
            if !toc_entries.is_empty() {
                content.insert("toc_entries".to_string(), Value::Array(toc_entries));
                content.insert("text_flow".to_string(), Value::String("preserve_lines".to_string()));
            }
        }
    }
}
