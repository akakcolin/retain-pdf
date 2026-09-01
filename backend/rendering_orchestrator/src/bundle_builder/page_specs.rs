//! N11f: native `build_render_page_specs` — the bundle's `page_specs` key
//! (retired `entrypoints/run_render_delegate.py::build_bundle`): per selected
//! page, the page geometry from the RENDER-SOURCE PDF plus the full emitter dicts
//! (`_page_spec_to_dict` / `_block_to_dict`, 29-key blocks).
//!
//! Mirrors `page_specs.build_render_page_specs(source_pdf_path=render_source.path,
//! translated_pages=prepared_pages, prepared=True)`. `prepared=True` re-applies
//! the C3-N8 policy boundary; the bundle's pages are already policy-applied
//! (N11d), so the pass-through is idempotent and skipped here.
//!
//! Per page the blocks run the page-spec layout path (NOT `build_render_blocks`):
//! `build_block_payloads` with `page_height=None` (blocks.build_render_block_payloads
//! `del page_height`), order by `(inner_bbox[1], inner_bbox[0])`,
//! `apply_body_payload_pipeline` (font-unify target from the whole book, `None`
//! when font_unify_mode == "off") + `unify_annotation_fonts` +
//! `recover_underfilled_annotation_density` -> `mark_adjacent_collision_risk` ->
//! `emit_render_blocks` (re-orders by payload `index`). The emitted `RenderBlock`
//! dicts convert to layout blocks (`block_id = "item-{source_item_id}"`), then
//! `apply_title_fit_budget_to_render_blocks` rounds the fit budget with
//! `round_to_digits` (CPython `round(x, 2)` half-even, NOT `f64::round`).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use mupdf::Document;
use rendering_core::item::Item;
use rendering_core::layout::body_font_unify::resolve_book_body_font_target;
use rendering_core::layout::collision::mark_adjacent_collision_risk;
use rendering_core::payload::block_seed::build_block_payloads;
use rendering_core::payload::body_pipeline::apply_body_payload_pipeline;
use rendering_core::payload::body_policy_facade::{
    recover_underfilled_annotation_density, unify_annotation_fonts,
};
use rendering_core::payload::emit::emit_render_blocks;
use rendering_core::rect::round_to_digits;
use rendering_reader::PdfDocument as _;
use serde_json::{json, Map, Value};

const TITLE_FIT_GAP_PT: f64 = 2.0;
const TITLE_FIT_HORIZONTAL_OVERLAP_RATIO: f64 = 0.35;
const TITLE_FIT_VERTICAL_OVERLAP_RATIO: f64 = 0.35;
const TITLE_FIT_WIDTH_EXPAND_RATIO: f64 = 0.42;
const TITLE_FIT_MAX_WIDTH_EXPAND_RATIO: f64 = 0.05;
const TITLE_FIT_DOWNWARD_EXPAND_RATIO: f64 = 0.05;
const TITLE_FIT_MAX_TOTAL_HEIGHT_EXPAND_RATIO: f64 = 0.05;
const TITLE_FIT_UPWARD_HEIGHT_RATIO_MIN: f64 = 0.02;
const TITLE_FIT_UPWARD_HEIGHT_RATIO_MAX: f64 = 0.05;

/// `build_render_page_specs(..., prepared=True)`: the `page_specs` array as
/// emitter dicts. Pages whose index is outside the render-source PDF are skipped
/// (the retired Python `build_bundle`'s `read_source_page_sizes` gate).
pub fn build_render_page_specs(
    source_pdf_path: &Path,
    translated_pages: &BTreeMap<i64, Vec<Value>>,
    font_unify_mode: &str,
) -> Result<Value> {
    let doc = Document::open(source_pdf_path).map_err(|e| {
        anyhow!(
            "open render source pdf for page specs {}: {e}",
            source_pdf_path.display()
        )
    })?;

    // `build_render_page_specs_from_page_sizes`: per page (sorted key order),
    // `build_render_block_payloads` (page_height dropped), then the whole-book
    // body font target from the payload tuples.
    let mut page_payloads: Vec<(i64, f64, f64, Vec<Value>, f64)> = Vec::new();
    for (&page_idx, items) in translated_pages {
        let Ok(page_rect) = doc.page_rect(page_idx) else { continue };
        let width = page_rect.x1 - page_rect.x0;
        let height = page_rect.y1 - page_rect.y0;
        let (block_payloads, page_text_width_med) = build_page_block_payloads(items, width);
        page_payloads.push((page_idx, width, height, block_payloads, page_text_width_med));
    }
    let book_body_font_target = if font_unify_mode == "off" {
        None
    } else {
        let tuples: Vec<(Vec<Value>, f64)> = page_payloads
            .iter()
            .map(|(_, _, _, payloads, med)| (payloads.clone(), *med))
            .collect();
        resolve_book_body_font_target(&tuples)
    };

    let mut specs = Vec::new();
    for (page_idx, width, height, block_payloads, page_text_width_med) in page_payloads {
        let blocks = layout_page_spec_blocks(
            &block_payloads,
            page_idx,
            width,
            height,
            page_text_width_med,
            book_body_font_target,
            font_unify_mode,
        );
        specs.push(json!({
            "page_index": page_idx,
            "page_width_pt": width,
            "page_height_pt": height,
            "background_pdf_path": Value::Null,
            "blocks": blocks,
        }));
    }
    Ok(Value::Array(specs))
}

/// `blocks.build_render_block_payloads`: the C3-N2 seed boundary with
/// `page_height=None` (the Python wrapper `del page_height`s it). Items are the
/// already-seeded prepared pages; the page-spec path does NOT re-seed.
fn build_page_block_payloads(items: &[Value], page_width: f64) -> (Vec<Value>, f64) {
    let typed: Vec<Item> = items.iter().map(Item::from_json_value).collect();
    build_block_payloads(&typed, items, Some(page_width), None)
}

/// `_layout_page_spec`: the body-pipeline stages over the y-sorted payloads, then
/// emit -> layout-block conversion -> title-fit budget -> 29-key emitter dicts.
fn layout_page_spec_blocks(
    block_payloads: &[Value],
    page_index: i64,
    page_width: f64,
    page_height: f64,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
    font_unify_mode: &str,
) -> Vec<Value> {
    let keys: Vec<(f64, f64)> = block_payloads
        .iter()
        .map(|p| {
            let arr = p.get("inner_bbox").and_then(Value::as_array);
            let get = |k: usize| -> f64 {
                arr.and_then(|a| a.get(k))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            };
            (get(1), get(0))
        })
        .collect();
    let mut order: Vec<usize> = (0..block_payloads.len()).collect();
    order.sort_by(|&a, &b| keys[a].partial_cmp(&keys[b]).unwrap_or(Ordering::Equal));

    let mut ordered: Vec<Value> = order.iter().map(|&i| block_payloads[i].clone()).collect();
    apply_body_payload_pipeline(
        &mut ordered,
        page_text_width_med,
        book_body_font_target,
        font_unify_mode,
    );
    if font_unify_mode != "off" {
        unify_annotation_fonts(&mut ordered);
    }
    recover_underfilled_annotation_density(&mut ordered);
    mark_adjacent_collision_risk(&mut ordered);
    let mut mutated: Vec<Value> = block_payloads.to_vec();
    for (k, &i) in order.iter().enumerate() {
        mutated[i] = ordered[k].clone();
    }

    let emitted = emit_render_blocks(&mutated);
    let mut layout_blocks: Vec<Value> = emitted
        .iter()
        .map(|b| layout_block_from_emitted(b, page_index))
        .collect();
    apply_title_fit_budget_to_render_blocks(&mut layout_blocks, page_width, page_height);
    layout_blocks
}

/// `_layout_block_from_render_block` over an emitted `RenderBlock` DTO dict.
fn layout_block_from_emitted(block: &Value, page_index: i64) -> Value {
    let source_item_id = block
        .get("source_item_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let block_id = if !source_item_id.is_empty() {
        format!("item-{source_item_id}")
    } else {
        block
            .get("block_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let render_kind = block
        .get("render_kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let content_text = if render_kind == "plain" {
        block
            .get("plain_text")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()))
    } else {
        block
            .get("markdown_text")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()))
    };

    let mut m = Map::new();
    m.insert("block_id".to_string(), json!(block_id));
    m.insert("page_index".to_string(), json!(page_index));
    m.insert(
        "background_rect".to_string(),
        block.get("cover_bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    m.insert(
        "content_rect".to_string(),
        block.get("inner_bbox").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    m.insert("content_kind".to_string(), json!(render_kind));
    m.insert("content_text".to_string(), content_text);
    m.insert(
        "plain_text".to_string(),
        block.get("plain_text").cloned().unwrap_or_else(|| Value::String(String::new())),
    );
    m.insert(
        "math_map".to_string(),
        block.get("math_map").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    for key in [
        "font_size_pt",
        "leading_em",
        "fit_min_font_size_pt",
        "fit_max_font_size_pt",
        "fit_min_leading_em",
        "fit_max_height_pt",
        "fit_target_width_pt",
        "fit_target_height_pt",
        "fit_shift_up_pt",
        "first_line_indent_pt",
    ] {
        m.insert(
            key.to_string(),
            block.get(key).cloned().unwrap_or_else(|| Value::from(0.0)),
        );
    }
    m.insert(
        "font_weight".to_string(),
        block
            .get("font_weight")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    for key in [
        "fit_to_box",
        "fit_single_line",
        "justify_text",
        "use_cover_fill",
        "preserve_line_breaks",
    ] {
        m.insert(
            key.to_string(),
            block.get(key).cloned().unwrap_or(Value::Bool(false)),
        );
    }
    m.insert(
        "text_color".to_string(),
        block.get("text_color").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    m.insert(
        "cover_fill".to_string(),
        block.get("cover_fill").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    m.insert(
        "skip_reason".to_string(),
        block
            .get("skip_reason")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    m.insert(
        "preserved_line_boxes".to_string(),
        block
            .get("preserved_line_boxes")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
    );
    m.insert(
        "toc_entries".to_string(),
        block.get("toc_entries").cloned().unwrap_or_else(|| Value::Array(vec![])),
    );
    Value::Object(m)
}

/// `_overlap_ratio(start_a, end_a, start_b, end_b)`.
fn overlap_ratio(start_a: f64, end_a: f64, start_b: f64, end_b: f64) -> f64 {
    let overlap = (end_a.min(end_b) - start_a.max(start_b)).max(0.0);
    let min_span = (end_a - start_a).min(end_b - start_b).max(1.0);
    overlap / min_span
}

/// `title_fit._resolve_fit_budget`.
fn resolve_fit_budget(
    rect: &[f64],
    sibling_rects: &[Vec<f64>],
    page_width: f64,
    page_height: f64,
) -> (f64, f64, f64) {
    if rect.len() != 4 {
        return (0.0, 0.0, 0.0);
    }
    let x0 = rect[0];
    let y0 = rect[1];
    let x1 = rect[2];
    let y1 = rect[3];
    let width = (x1 - x0).max(8.0);
    let height = (y1 - y0).max(8.0);
    let mut top_bound: f64 = 0.0;
    let mut right_bound: f64 = x1.max(page_width);
    let mut bottom_bound: f64 = y1.max(page_height);

    for sibling in sibling_rects {
        if sibling.len() != 4 {
            continue;
        }
        let sx0 = sibling[0];
        let sy0 = sibling[1];
        let sx1 = sibling[2];
        let sy1 = sibling[3];
        if overlap_ratio(x0, x1, sx0, sx1) >= TITLE_FIT_HORIZONTAL_OVERLAP_RATIO
            && sy1 <= y0
        {
            top_bound = top_bound.max(y0.min(sy1 + TITLE_FIT_GAP_PT));
        }
        if overlap_ratio(y0, y1, sy0, sy1) >= TITLE_FIT_VERTICAL_OVERLAP_RATIO
            && sx0 >= x1
        {
            right_bound = right_bound.min(x1.max(sx0 - TITLE_FIT_GAP_PT));
        }
        if overlap_ratio(x0, x1, sx0, sx1) >= TITLE_FIT_HORIZONTAL_OVERLAP_RATIO
            && sy0 >= y1
        {
            bottom_bound = bottom_bound.min(y1.max(sy0 - TITLE_FIT_GAP_PT));
        }
    }

    let max_width = width.max(right_bound - x0);
    let upward_room = (y0 - top_bound).max(0.0);
    let downward_room = (bottom_bound - y1).max(0.0);
    let upward_target = upward_room.min(height * TITLE_FIT_UPWARD_HEIGHT_RATIO_MAX);
    let upward_floor = upward_room.min(height * TITLE_FIT_UPWARD_HEIGHT_RATIO_MIN);
    let mut upward_shift = if upward_room > 0.0 {
        upward_floor.max(upward_target)
    } else {
        0.0
    };
    let downward_expand = downward_room * TITLE_FIT_DOWNWARD_EXPAND_RATIO;
    let max_width_expand = width * TITLE_FIT_MAX_WIDTH_EXPAND_RATIO;
    // Python `title_fit.py`: `min(max_width_expand, room * RATIO)` — scale the
    // room FIRST, then clamp (clamping the raw room first diverges when the
    // horizontal room exceeds ~11.9% of the title width).
    let width_expand = ((max_width - width) * TITLE_FIT_WIDTH_EXPAND_RATIO).min(max_width_expand);
    let max_total_height_expand = height * TITLE_FIT_MAX_TOTAL_HEIGHT_EXPAND_RATIO;
    upward_shift = upward_shift.min(max_total_height_expand);
    let downward_expand = (max_total_height_expand - upward_shift)
        .max(0.0)
        .min(downward_expand);
    let target_width = width + width_expand;
    let target_height = height + upward_shift + downward_expand;
    (
        width.max(target_width),
        height.max(target_height),
        upward_shift.max(0.0),
    )
}

/// `title_fit.apply_title_fit_budget_to_render_blocks` over the layout-block
/// dicts; the fit budget rounds with CPython half-even `round(x, 2)`.
fn apply_title_fit_budget_to_render_blocks(
    blocks: &mut [Value],
    page_width: f64,
    page_height: f64,
) {
    let sibling_rects: Vec<Vec<f64>> = blocks
        .iter()
        .map(|b| {
            b.get("content_rect")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default()
        })
        .collect();
    for index in 0..blocks.len() {
        if !bool_field(&blocks[index], "fit_single_line")
            || str_field(&blocks[index], "content_kind") != "markdown"
        {
            continue;
        }
        let rect: Vec<f64> = blocks[index]
            .get("content_rect")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let mut siblings: Vec<Vec<f64>> = Vec::with_capacity(sibling_rects.len() - 1);
        siblings.extend_from_slice(&sibling_rects[..index]);
        siblings.extend_from_slice(&sibling_rects[index + 1..]);
        let (width_pt, height_pt, shift_up_pt) =
            resolve_fit_budget(&rect, &siblings, page_width, page_height);
        blocks[index]["fit_target_width_pt"] = json!(round_to_digits(width_pt, 2));
        blocks[index]["fit_target_height_pt"] = json!(round_to_digits(height_pt, 2));
        blocks[index]["fit_shift_up_pt"] = json!(round_to_digits(shift_up_pt, 2));
    }
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use serde_json::json;

    const GOLDEN_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/samples/golden-pdfs"
    );

    fn paragraph_item(item_id: &str, page_idx: i64, bbox: [f64; 4]) -> Value {
        json!({
            "item_id": item_id,
            "page_idx": page_idx,
            "block_type": "text",
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "bbox": bbox,
            "source_text": "source text",
            "translated_text": "译文",
            "protected_source_text": "source text",
            "protected_translated_text": "译文",
            "should_translate": true,
            "final_status": "translated",
            "translation_unit_id": item_id,
            "translation_unit_kind": "single",
            "translation_unit_protected_source_text": "source text",
            "translation_unit_protected_translated_text": "译文",
            "translation_unit_formula_map": [],
            "formula_map": [],
            "protected_map": [],
            "continuation_group": "",
            "group_protected_source_text": "",
            "group_formula_map": [],
            "group_protected_translated_text": "",
            "group_translated_text": "",
            "_render_policy": {"overlay_fill": "sampled", "cleanup_mode": "delete_text"},
            "_render_cover_fill": [1.0, 1.0, 1.0],
            "_render_text_color": [0.0, 0.0, 0.0],
            "lines": [{"bbox": bbox, "spans": [{"type": "text", "content": "source text", "bbox": bbox}]}],
        })
    }

    #[test]
    fn skips_out_of_range_pages() {
        let source = Path::new(GOLDEN_ROOT).join("2.pdf");
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        pages.insert(-1, vec![paragraph_item("n001", -1, [0.0, 0.0, 10.0, 10.0])]);
        pages.insert(100, vec![paragraph_item("p100", 100, [0.0, 0.0, 10.0, 10.0])]);
        let specs = build_render_page_specs(&source, &pages, "role_min").unwrap();
        assert_eq!(specs, Value::Array(vec![]));
    }

    #[test]
    fn emits_29_key_blocks() {
        let source = Path::new(GOLDEN_ROOT).join("2.pdf");
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        pages.insert(0, vec![
            paragraph_item("p001-b001", 0, [40.0, 40.0, 360.0, 80.0]),
            paragraph_item("p001-b002", 0, [40.0, 140.0, 360.0, 180.0]),
        ]);
        let specs = build_render_page_specs(&source, &pages, "role_min").unwrap();
        let arr = specs.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let spec = &arr[0];
        assert_eq!(spec["page_index"], 0);
        assert!(spec["page_width_pt"].as_f64().unwrap() > 0.0);
        assert!(spec["page_height_pt"].as_f64().unwrap() > 0.0);
        assert_eq!(spec["background_pdf_path"], Value::Null);
        let blocks = spec["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        // `_layout_block_from_render_block` re-keys by `item-{source_item_id}`.
        assert_eq!(blocks[0]["block_id"], "item-p001-b001");
        assert_eq!(blocks[1]["block_id"], "item-p001-b002");
        // Every block carries the full 29-key emitter shape.
        for block in blocks {
            let obj = block.as_object().unwrap();
            assert_eq!(obj.len(), 29, "block keys: {obj:?}");
            assert!(obj.contains_key("background_rect"));
            assert!(obj.contains_key("content_rect"));
            assert!(obj.contains_key("toc_entries"));
            assert!(obj.contains_key("preserved_line_boxes"));
        }
    }

    #[test]
    fn title_fit_budget_rounds_half_even() {
        // `round_to_digits` matches CPython `round(x, 2)` (correctly-rounded
        // ties-to-even). 2.675's binary double is 2.6749999..., so it rounds to
        // 2.67, while `f64::round(2.675 * 100.0) / 100.0` lands on 2.68 (a
        // spurious .5 tie). 1.125 is exactly representable and ties to 1.12.
        assert_eq!(round_to_digits(2.675, 2), 2.67);
        assert_eq!(round_to_digits(2.665, 2), 2.67);
        assert_eq!(round_to_digits(1.125, 2), 1.12);
    }

    #[test]
    fn overlap_ratio_min_span_floor() {
        assert_eq!(overlap_ratio(0.0, 10.0, 5.0, 15.0), 0.5);
        // Zero-span intervals clamp the denominator to 1.0.
        assert_eq!(overlap_ratio(5.0, 5.0, 5.0, 5.0), 0.0);
    }

    #[test]
    fn fit_budget_expands_toward_siblings() {
        // A title with a sibling above: upward room is capped by the sibling +
        // gap, and the budget returns the expanded target (rounded later).
        let rect = vec![50.0, 100.0, 350.0, 130.0];
        let sibling = vec![50.0, 40.0, 350.0, 90.0];
        let (w, h, up) = resolve_fit_budget(&rect, &[sibling], 595.0, 842.0);
        assert!(w >= 300.0);
        assert!(h >= 30.0);
        assert!(up >= 0.0);
    }
}
