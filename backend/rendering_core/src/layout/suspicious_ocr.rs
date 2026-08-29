// Port of services/rendering/layout/payload/suspicious_ocr.py — the
// detect-and-drop pass that flags text blocks whose estimated render height
// overflows the vertical budget before the next block, clearing their render
// text and recording a diagnostic.

use crate::inline_content::is_direct_typst_math_mode;
use crate::item::{formula_map, Item};
use crate::layout::payload_dict::py_str;
use crate::payload::block_seed_metrics::block_metrics;
use crate::payload::capacity::estimated_render_height_pt;
use crate::payload::fit_common::VERTICAL_COLLISION_GAP_PT;
use crate::semantics::block_kind;
use crate::typography::geometry::inner_bbox;
use crate::util::py_round;
use serde_json::{json, Value};

pub const SUSPICIOUS_OCR_GLUE_MIN_CHARS: usize = 1000;
pub const SUSPICIOUS_OCR_GLUE_MIN_CHAR_HEIGHT_RATIO: f64 = 15.0;
pub const SUSPICIOUS_OCR_GLUE_MAX_SOURCE_GAP_PT: f64 = 28.0;
pub const SUSPICIOUS_OCR_GLUE_MIN_WIDTH_OVERLAP_RATIO: f64 = 0.6;
pub const SUSPICIOUS_OCR_GLUE_OVERFLOW_RATIO: f64 = 1.25;
pub const SUSPICIOUS_OCR_GLUE_REASON: &str = "suspicious_ocr_glued_block";
pub const SUSPICIOUS_OCR_GLUE_DIAGNOSTIC_KIND: &str = "render_skip_detector";
pub const SUSPICIOUS_OCR_GLUE_DIAGNOSTIC_NAME: &str = "suspicious_ocr_glued_block";

/// Python `bool(v)` over the payload dict values.
fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `_compact_text_len`: `len(re.sub(r"\s+", "", text or ""))` — non-whitespace
/// Unicode char count.
fn compact_text_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

/// `_width_overlap_ratio`: overlap width / min width (floored at 1.0).
fn width_overlap_ratio(current_inner: &[f64], next_inner: &[f64]) -> f64 {
    let overlap_width =
        (current_inner[2].min(next_inner[2]) - current_inner[0].max(next_inner[0])).max(0.0);
    let min_width =
        ((current_inner[2] - current_inner[0]).min(next_inner[2] - next_inner[0])).max(1.0);
    overlap_width / min_width
}

/// `clear_render_text`: blank every render/translation text slot and tag the
/// skip reason.
fn clear_render_text(item: &mut Value, reason: &str) {
    item["render_protected_text"] = Value::String(String::new());
    item["render_formula_map"] = Value::Array(Vec::new());
    item["translation_unit_protected_translated_text"] = Value::String(String::new());
    item["translation_unit_translated_text"] = Value::String(String::new());
    item["protected_translated_text"] = Value::String(String::new());
    item["translated_text"] = Value::String(String::new());
    item["group_protected_translated_text"] = Value::String(String::new());
    item["group_translated_text"] = Value::String(String::new());
    item["render_skip_reason"] = Value::String(reason.to_string());
}

/// `_append_item_diagnostic`: append onto an existing `render_diagnostics` list,
/// else create it.
fn append_item_diagnostic(item: &mut Value, diagnostic: Value) {
    match item.get_mut("render_diagnostics") {
        Some(Value::Array(arr)) => arr.push(diagnostic),
        _ => {
            item["render_diagnostics"] = Value::Array(vec![diagnostic]);
        }
    }
}

/// Python `current.get("item_id", "")` — the raw value (string or number).
fn item_id_value(item: &Value) -> Value {
    item.get("item_id")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
}

/// First truthy string among the chained keys (Python `a or b or ... or ""`).
fn first_truthy_string(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = item.get(*key) {
            if value_truthy(v) {
                return v.as_str().unwrap_or("").to_string();
            }
        }
    }
    String::new()
}

/// Sort key: `(inner_bbox(item)[1], inner_bbox(item)[0])`, zero when the inner
/// bbox is not 4 numbers.
fn inner_sort_key(item: &Value) -> (f64, f64) {
    let inner = inner_bbox(&Item::from_json_value(item));
    if inner.len() == 4 {
        (inner[1], inner[0])
    } else {
        (0.0, 0.0)
    }
}

/// `detect_and_drop_suspicious_ocr_glued_blocks`: order the page's text items
/// with render text top-to-bottom, flag adjacent pairs where the current block's
/// estimated render height exceeds the budget before the next block, clear its
/// render text, and return the summary dict.
pub fn detect_and_drop_suspicious_ocr_glued_blocks(
    items: &mut [Value],
    page_idx: i64,
    page_font_size: f64,
    page_line_pitch: f64,
    page_line_height: f64,
    density_baseline: f64,
    page_text_width_med: f64,
) -> Value {
    let mut ordered: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            block_kind(&Item::from_json_value(item)) == "text"
                && !item
                    .get("render_protected_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .is_empty()
        })
        .map(|(index, _)| index)
        .collect();
    ordered.sort_by(|&a, &b| {
        inner_sort_key(&items[a])
            .partial_cmp(&inner_sort_key(&items[b]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut hits: Vec<Value> = Vec::new();
    for pair in ordered.windows(2) {
        let current_idx = pair[0];
        let next_idx = pair[1];
        let current_inner = inner_bbox(&Item::from_json_value(&items[current_idx]));
        let next_inner = inner_bbox(&Item::from_json_value(&items[next_idx]));
        if current_inner.len() != 4 || next_inner.len() != 4 {
            continue;
        }
        let current = &items[current_idx];
        let has_continuation = current.get("continuation_group").map(value_truthy).unwrap_or(false)
            || current.get("continuation_group_id").map(value_truthy).unwrap_or(false);
        if has_continuation {
            continue;
        }
        // suspicious_ocr compares the raw string, NOT render_item's stripped/lowered
        // render_unit_kind.
        let unit_kind = match current.get("translation_unit_kind") {
            Some(v) => py_str(v),
            None => String::new(),
        };
        if unit_kind == "group" {
            continue;
        }
        if is_direct_typst_math_mode(current) {
            continue;
        }
        let has_render_formula_map = current
            .get("render_formula_map")
            .map(value_truthy)
            .unwrap_or(false);
        if has_render_formula_map {
            continue;
        }
        let source_text = first_truthy_string(
            current,
            &[
                "translation_unit_protected_source_text",
                "protected_source_text",
                "source_text",
            ],
        );
        let source_chars = compact_text_len(&source_text);
        let bbox = current
            .get("bbox")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_f64()).collect::<Vec<f64>>())
            .unwrap_or_default();
        let bbox_height = if bbox.len() == 4 { (bbox[3] - bbox[1]).max(1.0) } else { 1.0 };
        let char_height_ratio = source_chars as f64 / bbox_height;
        if source_chars < SUSPICIOUS_OCR_GLUE_MIN_CHARS {
            continue;
        }
        if char_height_ratio < SUSPICIOUS_OCR_GLUE_MIN_CHAR_HEIGHT_RATIO {
            continue;
        }
        let width_overlap_ratio = width_overlap_ratio(&current_inner, &next_inner);
        if width_overlap_ratio < SUSPICIOUS_OCR_GLUE_MIN_WIDTH_OVERLAP_RATIO {
            continue;
        }
        let source_gap = next_inner[1] - current_inner[3];
        if source_gap < 0.0 || source_gap > SUSPICIOUS_OCR_GLUE_MAX_SOURCE_GAP_PT {
            continue;
        }
        let item_typed = Item::from_json_value(&items[current_idx]);
        let (font_size_pt, leading_em) = block_metrics(
            &item_typed,
            page_font_size,
            page_line_pitch,
            page_line_height,
            density_baseline,
            page_text_width_med,
        );
        let protected_text = current
            .get("render_protected_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let render_fm = formula_map(current.get("render_formula_map"));
        let estimated_height =
            estimated_render_height_pt(&current_inner, &protected_text, &render_fm, font_size_pt, leading_em);
        let max_height_pt = next_inner[1] - current_inner[1] - VERTICAL_COLLISION_GAP_PT;
        if max_height_pt <= 0.0 {
            continue;
        }
        let overflow_ratio = estimated_height / max_height_pt;
        if estimated_height <= max_height_pt * SUSPICIOUS_OCR_GLUE_OVERFLOW_RATIO {
            continue;
        }
        let diagnostic = json!({
            "kind": SUSPICIOUS_OCR_GLUE_DIAGNOSTIC_KIND,
            "name": SUSPICIOUS_OCR_GLUE_DIAGNOSTIC_NAME,
            "reason": SUSPICIOUS_OCR_GLUE_REASON,
            "page_idx": page_idx,
            "item_id": item_id_value(current),
            "next_item_id": item_id_value(&items[next_idx]),
            "source_chars": source_chars,
            "bbox_height_pt": py_round(bbox_height, 2),
            "char_height_ratio": py_round(char_height_ratio, 2),
            "width_overlap_ratio": py_round(width_overlap_ratio, 3),
            "source_gap_pt": py_round(source_gap, 2),
            "font_size_pt": py_round(font_size_pt, 2),
            "leading_em": py_round(leading_em, 2),
            "estimated_height_pt": py_round(estimated_height, 2),
            "allowed_height_pt": py_round(max_height_pt, 2),
            "overflow_ratio": py_round(overflow_ratio, 3),
            "thresholds": {
                "min_chars": SUSPICIOUS_OCR_GLUE_MIN_CHARS,
                "min_char_height_ratio": SUSPICIOUS_OCR_GLUE_MIN_CHAR_HEIGHT_RATIO,
                "max_source_gap_pt": SUSPICIOUS_OCR_GLUE_MAX_SOURCE_GAP_PT,
                "min_width_overlap_ratio": SUSPICIOUS_OCR_GLUE_MIN_WIDTH_OVERLAP_RATIO,
                "overflow_ratio": SUSPICIOUS_OCR_GLUE_OVERFLOW_RATIO,
            },
        });
        clear_render_text(&mut items[current_idx], SUSPICIOUS_OCR_GLUE_REASON);
        append_item_diagnostic(&mut items[current_idx], diagnostic.clone());
        hits.push(diagnostic);
    }

    json!({
        "name": SUSPICIOUS_OCR_GLUE_DIAGNOSTIC_NAME,
        "reason": SUSPICIOUS_OCR_GLUE_REASON,
        "count": hits.len(),
        "page_idx": page_idx,
        "hits": hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_item(item_id: &str, y0: f64, y1: f64, chars: usize) -> Value {
        json!({
            "item_id": item_id,
            "block_kind": "text",
            "bbox": [50.0, y0, 300.0, y1],
            "source_text": "字".repeat(chars),
            "render_protected_text": "译".repeat(chars),
        })
    }

    #[test]
    fn skip_when_source_chars_below_min() {
        // 5 chars < 1000 -> no hit, items untouched.
        let mut items = vec![text_item("a", 20.0, 60.0, 5), text_item("b", 80.0, 120.0, 5)];
        let summary = detect_and_drop_suspicious_ocr_glued_blocks(&mut items, 0, 12.0, 14.0, 14.0, 1.0, 260.0);
        assert_eq!(summary["count"], 0);
        assert_eq!(items[0]["render_protected_text"], json!("译译译译译"));
    }

    #[test]
    fn drop_when_height_overflows() {
        // First block: huge source density and short vertical budget -> drop.
        // The 20pt gap to the next block is under MAX_SOURCE_GAP_PT (28).
        let mut items = vec![
            text_item("a", 20.0, 40.0, 3000),
            text_item("b", 60.0, 100.0, 5),
        ];
        let summary = detect_and_drop_suspicious_ocr_glued_blocks(&mut items, 0, 12.0, 14.0, 14.0, 1.0, 260.0);
        assert_eq!(summary["count"], 1);
        let hit = &summary["hits"][0];
        assert_eq!(hit["item_id"], "a");
        assert_eq!(hit["next_item_id"], "b");
        assert_eq!(hit["page_idx"], 0);
        // render text cleared + skip reason tagged.
        assert_eq!(items[0]["render_protected_text"], json!(""));
        assert_eq!(items[0]["render_formula_map"], json!([]));
        assert_eq!(items[0]["render_skip_reason"], json!("suspicious_ocr_glued_block"));
        assert_eq!(items[0]["render_diagnostics"][0]["kind"], json!("render_skip_detector"));
        assert_eq!(summary["name"], json!("suspicious_ocr_glued_block"));
    }

    #[test]
    fn skips_group_units_and_direct_math() {
        let mut items = vec![
            json!({
                "item_id": "g",
                "block_kind": "text",
                "translation_unit_kind": "group",
                "bbox": [50.0, 20.0, 300.0, 60.0],
                "source_text": "字".repeat(2000),
                "render_protected_text": "译".repeat(2000),
            }),
            json!({
                "item_id": "m",
                "block_kind": "text",
                "math_mode": "direct_typst",
                "bbox": [50.0, 80.0, 300.0, 120.0],
                "source_text": "字".repeat(2000),
                "render_protected_text": "译".repeat(2000),
            }),
            text_item("n", 140.0, 180.0, 5),
        ];
        let summary = detect_and_drop_suspicious_ocr_glued_blocks(&mut items, 0, 12.0, 14.0, 14.0, 1.0, 260.0);
        assert_eq!(summary["count"], 0);
    }

    #[test]
    fn orders_by_inner_y_before_x() {
        let mut items = vec![
            text_item("lower", 200.0, 240.0, 5),
            text_item("upper", 40.0, 80.0, 5),
            text_item("mid", 120.0, 160.0, 5),
        ];
        let summary = detect_and_drop_suspicious_ocr_glued_blocks(&mut items, 0, 12.0, 14.0, 14.0, 1.0, 260.0);
        // No overflow with tiny 5-char blocks; ordering only matters via pairs.
        assert_eq!(summary["count"], 0);
    }
}
