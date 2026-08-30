// Port of `provider_adapters/paddle/content_extract.py` — segment/line building
// with inline-formula re-splitting and pseudo-line reconstruction for bodylike
// text blocks.

use std::collections::HashMap;

use rendering_core::rect::round_to_digits;
use rendering_core::text_flow::py_splitlines;
use serde_json::{json, Value};

use super::super::common::{build_line_records, build_text_segments};
use super::super::formula_protection::{protect_inline_formulas, protected_token_re};

const APPROX_TEXT_CHAR_WIDTH_PT: f64 = 5.2;
const MIN_PSEUDO_LINE_PITCH_PT: f64 = 11.0;
const TARGET_PSEUDO_LINE_PITCH_PT: f64 = 12.0;
const PSEUDO_TEXT_HEIGHT_SLACK_RATIO: f64 = 1.08;

fn is_bodylike_subtype(sub_type: &str) -> bool {
    matches!(sub_type.trim().to_lowercase().as_str(), "body" | "heading")
}

/// `_segment_record`.
fn segment_record(text: &str, raw_label: &str, segment_type: &str) -> Value {
    json!({
        "type": segment_type,
        "raw_type": raw_label,
        "text": text,
        "bbox": json!([0, 0, 0, 0]),
        "score": Value::Null,
    })
}

/// `_split_text_with_inline_formulas`.
fn split_text_with_inline_formulas(text: &str, raw_label: &str) -> Vec<Value> {
    let (protected_text, formula_map) = protect_inline_formulas(text);
    if formula_map.is_empty() {
        return build_text_segments(text, raw_label, "text");
    }
    let mut lookup: HashMap<String, String> = HashMap::new();
    for entry in &formula_map {
        let placeholder = entry.get("placeholder").and_then(Value::as_str).unwrap_or("");
        let formula_text = entry.get("formula_text").and_then(Value::as_str).unwrap_or("");
        lookup.insert(placeholder.to_string(), formula_text.to_string());
    }
    let re = protected_token_re();
    let mut segments: Vec<Value> = Vec::new();
    let mut cursor = 0usize;
    for caps in re.captures_iter(&protected_text) {
        let m = caps.get(0).expect("protected token match");
        let (start, end) = (m.start(), m.end());
        if start > cursor {
            let chunk = protected_text[cursor..start].trim();
            if !chunk.is_empty() {
                segments.push(segment_record(chunk, raw_label, "text"));
            }
        }
        let placeholder = m.as_str();
        let formula_text = lookup.get(placeholder).cloned().unwrap_or_default();
        let formula_text = formula_text.trim();
        if !formula_text.is_empty() {
            segments.push(segment_record(formula_text, raw_label, "formula"));
        }
        cursor = end;
    }
    let tail = protected_text[cursor..].trim();
    if !tail.is_empty() {
        segments.push(segment_record(tail, raw_label, "text"));
    }
    if segments.is_empty() {
        return build_text_segments(text, raw_label, "text");
    }
    segments
}

/// `build_segments`.
pub fn build_segments(text: &str, raw_label: &str) -> Vec<Value> {
    let label = raw_label.trim().to_lowercase();
    if label == "display_formula" || label == "formula" {
        return build_text_segments(text, raw_label, "formula");
    }
    if label == "text" {
        return split_text_with_inline_formulas(text, raw_label);
    }
    build_text_segments(text, raw_label, "text")
}

fn bbox_width(bbox: &[f64]) -> f64 {
    if bbox.len() == 4 {
        (bbox[2] - bbox[0]).max(0.0)
    } else {
        0.0
    }
}

fn bbox_height(bbox: &[f64]) -> f64 {
    if bbox.len() == 4 {
        (bbox[3] - bbox[1]).max(0.0)
    } else {
        0.0
    }
}

/// `_compact_text_len` — Python `len(re.sub(r"\s+", "", text))`.
fn compact_text_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

fn estimated_chars_per_line(width_pt: f64) -> i64 {
    ((width_pt / APPROX_TEXT_CHAR_WIDTH_PT).trunc() as i64).max(12)
}

/// `_split_words_evenly`.
fn split_words_evenly(text: &str, line_count: i64, chars_per_line: i64) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        return if compact.is_empty() { vec![] } else { vec![compact] };
    }
    let total_compact_len = compact_text_len(text) as f64;
    let target_chars_per_line =
        (total_compact_len / line_count.max(1) as f64).ceil().max(10.0);
    let int_break = (chars_per_line as f64 * 0.58).trunc();
    let break_threshold = (chars_per_line as f64).min(target_chars_per_line.max(int_break));

    let mut chunks: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_len: i64 = 0;
    let mut remaining_words = words.len() as i64;
    let mut remaining_lines = line_count.max(1);

    for word in words {
        remaining_words -= 1;
        let projected = current_len + if current.is_empty() { 0 } else { 1 } + word.chars().count() as i64;
        let force_break = !current.is_empty()
            && current_len as f64 >= break_threshold
            && remaining_lines > 1
            && remaining_words >= remaining_lines - 1;
        if force_break {
            chunks.push(current.join(" "));
            current = vec![word.to_string()];
            current_len = word.chars().count() as i64;
            remaining_lines -= 1;
            continue;
        }
        current.push(word.to_string());
        current_len = projected;
    }
    if !current.is_empty() {
        chunks.push(current.join(" "));
    }
    chunks.into_iter().filter(|c| !c.trim().is_empty()).collect()
}

/// `_pseudo_line_count`.
fn pseudo_line_count(bbox: &[f64], text: &str) -> i64 {
    let width_pt = bbox_width(bbox);
    let height_pt = bbox_height(bbox);
    let text_len = compact_text_len(text);
    if width_pt <= 0.0 || height_pt <= 0.0 || text_len < 72 {
        return 0;
    }
    if height_pt < MIN_PSEUDO_LINE_PITCH_PT * 2.2 {
        return 0;
    }
    let chars_per_line = estimated_chars_per_line(width_pt);
    let predicted_by_width = ((text_len as f64 / chars_per_line.max(1) as f64).ceil() as i64).max(2);
    let max_lines_by_height = ((height_pt / MIN_PSEUDO_LINE_PITCH_PT).trunc() as i64).max(1);
    let desired_by_height = round_to_digits(height_pt / TARGET_PSEUDO_LINE_PITCH_PT, 0).trunc() as i64;
    let desired_by_height = desired_by_height.max(2);
    max_lines_by_height.min(predicted_by_width.max(desired_by_height))
}

/// `tighten_text_bbox`.
pub fn tighten_text_bbox(bbox: &[f64], text: &str, block_type: &str, sub_type: &str) -> Vec<f64> {
    if block_type != "text" || !is_bodylike_subtype(sub_type) || bbox.len() != 4 {
        return bbox.to_vec();
    }
    let line_count = pseudo_line_count(bbox, text);
    if line_count <= 1 {
        return bbox.to_vec();
    }
    let (x0, y0, x1, y1) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let original_height = (y1 - y0).max(0.0);
    let target_height = original_height.min(
        (TARGET_PSEUDO_LINE_PITCH_PT * 2.0).max(
            line_count as f64 * TARGET_PSEUDO_LINE_PITCH_PT * PSEUDO_TEXT_HEIGHT_SLACK_RATIO,
        ),
    );
    vec![x0, y0, x1, round_to_digits(y1.min(y0 + target_height), 3)]
}

/// `_build_pseudo_lines`.
fn build_pseudo_lines(bbox: &[f64], text: &str, raw_label: &str) -> Vec<Value> {
    let line_count = pseudo_line_count(bbox, text);
    if line_count <= 1 {
        return vec![];
    }
    let width_pt = bbox_width(bbox);
    let height_pt = bbox_height(bbox);
    let chars_per_line = estimated_chars_per_line(width_pt);
    let chunks = split_words_evenly(text, line_count, chars_per_line);
    if chunks.len() <= 1 {
        return vec![];
    }
    let (x0, y0, x1, y1) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let total_lines = chunks.len() as f64;
    let line_height = height_pt / total_lines;
    let mut lines: Vec<Value> = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        let line_y0 = y0 + line_height * index as f64;
        let line_y1 = if index == chunks.len() - 1 {
            y1
        } else {
            y0 + line_height * (index as f64 + 1.0)
        };
        lines.push(json!({
            "bbox": [x0, round_to_digits(line_y0, 3), x1, round_to_digits(line_y1, 3)],
            "spans": build_segments(chunk, raw_label),
        }));
    }
    lines
}

/// `_build_explicit_text_lines`.
fn build_explicit_text_lines(bbox: &[f64], text: &str, raw_label: &str) -> Vec<Value> {
    let chunks: Vec<String> = py_splitlines(text)
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if chunks.len() <= 1 || bbox.len() != 4 {
        return vec![];
    }
    let (x0, y0, x1, y1) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let total_lines = chunks.len() as f64;
    let line_height = ((y1 - y0) / total_lines).max(1.0);
    let mut lines: Vec<Value> = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        let line_y0 = y0 + line_height * index as f64;
        let line_y1 = if index == chunks.len() - 1 {
            y1
        } else {
            y0 + line_height * (index as f64 + 1.0)
        };
        lines.push(json!({
            "bbox": [x0, round_to_digits(line_y0, 3), x1, round_to_digits(line_y1, 3)],
            "spans": build_segments(chunk, raw_label),
        }));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG_BODY: &str = "The dynamics of layout parsing require rich spatial context; \
        we formalize the coupling between text blocks and their geometry and show how the \
        reading order can be recovered across the page.";

    #[test]
    fn build_segments_splits_inline_formula() {
        // Python reference: the whole-text latex match is prose-heavy (skipped);
        // GREEK_RUN then matches `\beta gives the result`, leaving a single text
        // segment "the angle \alpha +" before it.
        let segments = build_segments(r"the angle \alpha + \beta gives the result", "text");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["type"], "text");
        assert_eq!(segments[0]["text"], r"the angle \alpha +");
        assert_eq!(segments[1]["type"], "formula");
        assert_eq!(segments[1]["text"], r"\beta gives the result");
    }

    #[test]
    fn build_segments_display_formula_is_single_formula() {
        let segments = build_segments(r"E = mc^2", "display_formula");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0]["type"], "formula");
    }

    #[test]
    fn tighten_text_bbox_noop_for_short_and_non_bodylike() {
        let short = tighten_text_bbox(&[40.0, 100.0, 560.0, 150.0], "short", "text", "body");
        assert_eq!(short, vec![40.0, 100.0, 560.0, 150.0]);
        let non_body = tighten_text_bbox(&[40.0, 100.0, 560.0, 220.0], LONG_BODY, "text", "title");
        assert_eq!(non_body, vec![40.0, 100.0, 560.0, 220.0]);
    }

    #[test]
    fn tighten_text_bbox_narrows_tall_body_bbox() {
        // Python reference: bbox height 40 -> pseudo line count 3 -> target height
        // 38.88, so [3] tightens to 138.88. A 220.0-high bbox is untouched.
        let tightened = tighten_text_bbox(&[40.0, 100.0, 560.0, 140.0], LONG_BODY, "text", "body");
        assert_eq!(tightened[0], 40.0);
        assert_eq!(tightened[1], 100.0);
        assert!(tightened[3] < 140.0);
        assert!(tightened[3] >= 100.0);
        assert!((tightened[3] - 138.88).abs() < 1e-9);
        let untouched = tighten_text_bbox(&[40.0, 100.0, 560.0, 220.0], LONG_BODY, "text", "body");
        assert_eq!(untouched[3], 220.0);
    }

    #[test]
    fn build_lines_explicit_for_multi_line_text() {
        let bbox = [40.0, 100.0, 560.0, 140.0];
        let segments = build_segments("line one\nline two\nline three", "text");
        let lines = build_lines(&bbox, &segments, "line one\nline two\nline three", "text", "text", "title");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["bbox"], json!([40.0, 100.0, 560.0, 113.333]));
    }

    #[test]
    fn build_lines_pseudo_for_long_bodylike() {
        let bbox = [40.0, 100.0, 560.0, 220.0];
        let segments = build_segments(LONG_BODY, "text");
        let lines = build_lines(&bbox, &segments, LONG_BODY, "text", "text", "body");
        assert!(lines.len() >= 2, "expected pseudo lines, got {}", lines.len());
        let first = lines[0].as_object().unwrap();
        assert!(first["bbox"][0] == json!(40.0));
        assert!(!first["spans"].as_array().unwrap().is_empty());
    }

    #[test]
    fn pseudo_line_count_gates_on_length_and_height() {
        assert_eq!(pseudo_line_count(&[40.0, 100.0, 560.0, 150.0], "tiny"), 0);
        assert_eq!(pseudo_line_count(&[40.0, 100.0, 560.0, 15.0], LONG_BODY), 0);
        assert!(pseudo_line_count(&[40.0, 100.0, 560.0, 220.0], LONG_BODY) >= 2);
    }

    #[test]
    fn split_words_evenly_keeps_word_count() {
        // Python reference: LONG_BODY at line_count=4 yields 3 chunks (the target
        // break threshold caps below the requested line count).
        let words = split_words_evenly(LONG_BODY, 4, 100);
        assert!(words.len() >= 2 && words.len() <= 4);
        let joined = words.join(" ");
        assert_eq!(joined.split_whitespace().count(), LONG_BODY.split_whitespace().count());
    }
}

/// `build_lines`.
pub fn build_lines(
    bbox: &[f64],
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
    if block_type == "text" && is_bodylike_subtype(sub_type) {
        let pseudo_bbox = tighten_text_bbox(bbox, text, block_type, sub_type);
        let pseudo_lines = build_pseudo_lines(&pseudo_bbox, text, raw_label);
        if !pseudo_lines.is_empty() {
            return pseudo_lines;
        }
    }
    let bbox_value = Value::Array(bbox.iter().map(|f| Value::from(*f)).collect());
    match build_line_records(&bbox_value, segments) {
        Value::Array(items) => items,
        _ => vec![],
    }
}
