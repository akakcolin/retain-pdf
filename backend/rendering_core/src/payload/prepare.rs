// Port of services/rendering/layout/payload/prepare.py —
// `prepare_render_payloads_by_page`, the C3-N7 boundary. Deep-copies the
// translated pages, seeds render fields, attaches first-line indents and
// effective inner bboxes, splits continuation group render text across member
// capacities, then drops suspicious OCR-glued blocks. All writes go through a
// flat item list so group-unit mutations share references exactly like Python's
// `flat_items` list of dict references.

use crate::inline_content::is_direct_typst_math_mode;
use crate::item::{formula_map, Item};
use crate::layout::payload_dict::py_str;
use crate::layout::render_item::{
    clear_render_fields, group_render_unit_items, group_unit_formula_map,
    group_unit_protected_text, group_unit_source_text, item_has_group_render_text,
    seed_render_fields,
};
use crate::layout::suspicious_ocr::detect_and_drop_suspicious_ocr_glued_blocks;
use crate::payload::block_seed_metrics::{block_metrics, is_annotation_like};
use crate::payload::capacity::{box_capacity_units, text_demand_units};
use crate::payload::continuation_split::split_protected_text_for_boxes;
use crate::payload::text_common::same_meaningful_render_text;
use crate::semantics::block_kind;
use crate::typography::baseline::page_baseline_font_size;
use crate::typography::geometry::inner_bbox;
use crate::typography::line_metrics::bbox_width;
use crate::util::median_f64;
use serde_json::Value;
use std::collections::BTreeMap;

pub const CONTINUATION_NARROW_BOX_MIN_NEIGHBOR_RATIO: f64 = 0.78;
pub const CONTINUATION_NARROW_BOX_CAPACITY_RELAX_RATIO: f64 = 0.72;

/// `(page_font_size, page_line_pitch, page_line_height, density_baseline,
/// page_text_width_med)` — the five-tuple `page_metrics` value.
pub type PageMetrics = [f64; 5];

/// Intermediate state holding the shared-reference mutation semantics: a flat
/// list of all deep-copied items (page-sorted) plus the page index ranges and
/// per-page metrics needed to route mutations back to pages.
pub struct PrepareState {
    pub flat_items: Vec<Value>,
    pub page_ranges: BTreeMap<i64, (usize, usize)>,
    pub page_metrics: BTreeMap<i64, PageMetrics>,
}

/// Python `str(item.get("item_id", "") or "")` — falsy scalars (missing, "",
/// 0, false) become "".
fn py_item_id(item: &Value) -> String {
    match item.get("item_id") {
        Some(v) if value_falsy(v) => String::new(),
        Some(v) => py_str(v),
        None => String::new(),
    }
}

/// Python `bool(x)` over scalar dict values (used for the `or ""` gate).
fn value_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f == 0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// `_is_annotation_like`: caption-like or footnote-like block.
fn is_annotation_like_item(item: &Item) -> bool {
    is_annotation_like(item)
}

/// `_inner_bbox_width`: `max(0.0, inner_bbox[2] - inner_bbox[0])`, 0.0 when the
/// inner bbox is not 4 numbers.
fn inner_bbox_width(item: &Item) -> f64 {
    let bbox = inner_bbox(item);
    if bbox.len() != 4 {
        return 0.0;
    }
    (bbox[2] - bbox[0]).max(0.0)
}

/// `_continuation_adjusted_capacities`: relax interior boxes that are
/// substantially narrower than their neighbors, so a narrow continuation member
/// does not starve its split chunk.
fn continuation_adjusted_capacities(items: &[&Item], capacities: &[f64]) -> Vec<f64> {
    if items.len() != capacities.len() || items.len() < 3 {
        return capacities.to_vec();
    }
    let widths: Vec<f64> = items.iter().map(|item| inner_bbox_width(item)).collect();
    let mut adjusted = capacities.to_vec();
    for index in 1..(items.len() - 1) {
        let current_width = widths[index];
        if current_width <= 0.0 {
            continue;
        }
        let prev_width = widths[index - 1];
        let next_width = widths[index + 1];
        if prev_width <= 0.0 || next_width <= 0.0 {
            continue;
        }
        let neighbor_avg = (prev_width + next_width) / 2.0;
        if neighbor_avg <= 0.0 {
            continue;
        }
        if current_width >= neighbor_avg * CONTINUATION_NARROW_BOX_MIN_NEIGHBOR_RATIO {
            continue;
        }
        let width_ratio = current_width / neighbor_avg;
        let relax_ratio = width_ratio.max(CONTINUATION_NARROW_BOX_CAPACITY_RELAX_RATIO);
        adjusted[index] = capacities[index] * relax_ratio;
    }
    adjusted
}

/// `_attach_first_line_indents`: write `_render_first_line_indent_pt` from the
/// precomputed item-id lookup. The pixel-detection resolution of `source_pdf_path`
/// lives on the Python shim side (rendering_core is a leaf crate); `None` skips
/// the attach, matching Python's early return when no lookup is present.
fn attach_first_line_indents(
    flat_items: &mut [Value],
    page_ranges: &BTreeMap<i64, (usize, usize)>,
    page_metrics: &BTreeMap<i64, PageMetrics>,
    lookup: Option<&BTreeMap<String, f64>>,
) {
    let Some(lookup) = lookup else { return };
    for (&page_idx, &(start, end)) in page_ranges {
        if !page_metrics.contains_key(&page_idx) {
            continue;
        }
        for index in start..end {
            let item_id = py_item_id(&flat_items[index]);
            let indent_pt = lookup.get(&item_id).copied().unwrap_or(0.0);
            if indent_pt > 0.0 {
                flat_items[index]["_render_first_line_indent_pt"] = Value::from(indent_pt);
            }
        }
    }
}

/// `_build_page_metrics`: per-page `(page_font_size, page_line_pitch,
/// page_line_height, density_baseline, page_text_width_med)`. Reads only raw
/// item fields, so values are identical before and after `seed_render_fields`.
pub fn build_page_metrics(
    translated_pages: &BTreeMap<i64, Vec<Value>>,
) -> BTreeMap<i64, PageMetrics> {
    let mut page_metrics: BTreeMap<i64, PageMetrics> = BTreeMap::new();
    for (&page_idx, items) in translated_pages {
        let typed: Vec<Item> = items.iter().map(Item::from_json_value).collect();
        let typed_refs: Vec<&Item> = typed.iter().collect();
        let (page_font_size, page_line_pitch, page_line_height, density_baseline) =
            page_baseline_font_size(&typed_refs);
        let text_widths: Vec<f64> = items
            .iter()
            .map(Item::from_json_value)
            .filter(|item| block_kind(item) == "text" && !is_annotation_like_item(item))
            .map(|item| bbox_width(&item))
            .collect();
        let page_text_width_med = if text_widths.is_empty() {
            0.0
        } else {
            median_f64(&text_widths)
        };
        page_metrics.insert(
            page_idx,
            [
                page_font_size,
                page_line_pitch,
                page_line_height,
                density_baseline,
                page_text_width_med,
            ],
        );
    }
    page_metrics
}

/// `prepare_render_payloads_by_page` — the C3-N7 boundary (port of prepare.py).
/// Deep-copies the input pages, so the caller's dicts are untouched, and returns
/// the prepared page map.
pub fn prepare_render_payloads_by_page(
    translated_pages: &BTreeMap<i64, Vec<Value>>,
    first_line_indent_lookup: Option<&BTreeMap<String, f64>>,
    effective_inner_bbox_lookup: Option<&BTreeMap<String, Vec<f64>>>,
) -> BTreeMap<i64, Vec<Value>> {
    let mut prepared: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for (page_idx, items) in translated_pages {
        prepared.insert(*page_idx, items.clone());
    }
    if prepared.is_empty() {
        return prepared;
    }

    let page_metrics = build_page_metrics(&prepared);
    let mut flat_items: Vec<Value> = Vec::new();
    let mut page_ranges: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
    for (&page_idx, items) in &prepared {
        let start = flat_items.len();
        for mut item in items.clone() {
            seed_render_fields(&mut item);
            if let Some(lookup) = effective_inner_bbox_lookup {
                let item_id = py_item_id(&item);
                if !item_id.is_empty() {
                    if let Some(effective_bbox) = lookup.get(&item_id) {
                        if effective_bbox.len() == 4 {
                            item["_render_inner_bbox"] =
                                Value::Array(effective_bbox.iter().map(|v| Value::from(*v)).collect());
                        }
                    }
                }
            }
            flat_items.push(item);
        }
        page_ranges.insert(page_idx, (start, flat_items.len()));
    }

    attach_first_line_indents(
        &mut flat_items,
        &page_ranges,
        &page_metrics,
        first_line_indent_lookup,
    );

    let units = group_render_unit_items(&flat_items);
    for unit_items in units.values() {
        let members: Vec<usize> = unit_items
            .iter()
            .copied()
            .filter(|&index| item_has_group_render_text(&flat_items[index]))
            .collect();
        if members.is_empty() {
            continue;
        }
        let member_refs: Vec<&Value> = members.iter().map(|&index| &flat_items[index]).collect();
        let direct_math_mode = member_refs.iter().any(|item| is_direct_typst_math_mode(item));
        let unit_formula_map = group_unit_formula_map(&member_refs);
        let protected_unit_text = group_unit_protected_text(&member_refs);
        let protected_unit_source_text = group_unit_source_text(&member_refs);
        if same_meaningful_render_text(&protected_unit_source_text, &protected_unit_text) {
            for &index in &members {
                clear_render_fields(&mut flat_items[index]);
            }
            continue;
        }

        let mut capacities: Vec<f64> = Vec::with_capacity(members.len());
        let mut source_weights: Vec<f64> = Vec::with_capacity(members.len());
        for &index in &members {
            let item = &flat_items[index];
            let page_idx = item.get("page_idx").and_then(|v| v.as_i64()).unwrap_or(0);
            let metrics = page_metrics.get(&page_idx).copied().unwrap_or([0.0; 5]);
            let item_typed = Item::from_json_value(item);
            let (font_size_pt, leading_em) = block_metrics(
                &item_typed,
                metrics[0],
                metrics[1],
                metrics[2],
                metrics[3],
                metrics[4],
            );
            capacities.push(box_capacity_units(
                &inner_bbox(&item_typed),
                font_size_pt,
                leading_em,
                None,
            ));
            let source_text = item
                .get("protected_source_text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let source_fm = formula_map(item.get("formula_map"));
            source_weights.push(text_demand_units(&source_text, &source_fm));
        }

        let member_typed: Vec<Item> = members
            .iter()
            .map(|&index| Item::from_json_value(&flat_items[index]))
            .collect();
        let member_typed_refs: Vec<&Item> = member_typed.iter().collect();
        let capacities = continuation_adjusted_capacities(&member_typed_refs, &capacities);

        let has_positive_weight = source_weights.iter().any(|&weight| weight > 0.0);
        let preferred_weights = if has_positive_weight {
            Some(source_weights.as_slice())
        } else {
            None
        };
        let unit_formula_entries = formula_map(Some(&unit_formula_map));
        let unit_formula_pairs: Vec<(String, String)> = unit_formula_entries
            .iter()
            .map(|entry| (entry.placeholder.clone(), entry.formula_text.clone()))
            .collect();
        let chunks = split_protected_text_for_boxes(
            &protected_unit_text,
            &unit_formula_pairs,
            &capacities,
            preferred_weights,
            direct_math_mode,
        );
        let source_chunks = split_protected_text_for_boxes(
            &protected_unit_source_text,
            &unit_formula_pairs,
            &capacities,
            preferred_weights,
            false,
        );
        for (k, &index) in members.iter().enumerate() {
            flat_items[index]["render_protected_text"] = Value::String(chunks[k].clone());
            flat_items[index]["render_source_text"] = Value::String(source_chunks[k].clone());
            flat_items[index]["render_formula_map"] = unit_formula_map.clone();
        }
    }

    for (&page_idx, &(start, end)) in &page_ranges {
        let metrics = match page_metrics.get(&page_idx) {
            Some(m) => *m,
            None => continue,
        };
        detect_and_drop_suspicious_ocr_glued_blocks(
            &mut flat_items[start..end],
            page_idx,
            metrics[0],
            metrics[1],
            metrics[2],
            metrics[3],
            metrics[4],
        );
    }

    let mut result: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for (&page_idx, &(start, end)) in &page_ranges {
        result.insert(page_idx, flat_items[start..end].to_vec());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_pages_return_empty() {
        let pages = BTreeMap::new();
        let result = prepare_render_payloads_by_page(&pages, None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn seeds_single_page_items() {
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![json!({
                "item_id": "a",
                "page_idx": 0,
                "bbox": [40.0, 100.0, 300.0, 200.0],
                "block_type": "text",
                "source_text": "Hello world",
                "translated_text": "你好世界",
                "should_translate": true,
            })],
        );
        let result = prepare_render_payloads_by_page(&pages, None, None);
        let item = &result[&0][0];
        assert_eq!(item["render_protected_text"], json!("你好世界"));
        assert_eq!(item["render_source_text"], json!("Hello world"));
        assert_eq!(item["render_formula_map"], json!([]));
        // Caller input untouched.
        assert!(!pages[&0][0].as_object().unwrap().contains_key("render_protected_text"));
    }

    #[test]
    fn continuation_member_text_survives_without_resplit() {
        // Mirrors test_typst_continuation_rendering: every continuation member
        // carries member text, so the whole unit is skipped and the per-member
        // seeds survive.
        let mut pages = BTreeMap::new();
        pages.insert(
            11,
            vec![
                json!({
                    "item_id": "p012-b009",
                    "page_idx": 11,
                    "bbox": [56.994, 740.875, 302.469, 764.371],
                    "block_type": "text",
                    "math_mode": "direct_typst",
                    "translation_unit_id": "__cg__:cg-012-016",
                    "translation_unit_kind": "single",
                    "continuation_group": "cg-012-016",
                    "protected_source_text": "Having demonstrated the good performance of GFN2-xTB for small systems including",
                    "translation_unit_protected_source_text": "Having demonstrated the good performance of GFN2-xTB for small systems including different elements and interaction types, we next turn our attention to larger systems. This behavior partially results from nonadditivity dispersion effects.",
                    "translated_text": "我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的",
                    "protected_translated_text": "我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的",
                    "translation_unit_protected_translated_text": "我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的非共价相互作用具有良好的性能，接下来我们将关注更大的体系。这种行为部分源于非加和色散效应。",
                    "translation_unit_formula_map": [],
                }),
                json!({
                    "item_id": "p012-b012",
                    "page_idx": 11,
                    "bbox": [319.967, 499.916, 567.442, 765.371],
                    "block_type": "text",
                    "math_mode": "direct_typst",
                    "translation_unit_id": "__cg__:cg-012-016",
                    "translation_unit_kind": "single",
                    "continuation_group": "cg-012-016",
                    "protected_source_text": "different elements and interaction types, we next turn our attention to larger systems.",
                    "translation_unit_protected_source_text": "Having demonstrated the good performance of GFN2-xTB for small systems including different elements and interaction types, we next turn our attention to larger systems. This behavior partially results from nonadditivity dispersion effects.",
                    "translated_text": "非共价相互作用具有良好的性能，接下来我们将关注更大的体系。",
                    "protected_translated_text": "非共价相互作用具有良好的性能，接下来我们将关注更大的体系。",
                    "translation_unit_protected_translated_text": "我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的非共价相互作用具有良好的性能，接下来我们将关注更大的体系。这种行为部分源于非加和色散效应。",
                    "translation_unit_formula_map": [],
                }),
            ],
        );
        pages.insert(
            12,
            vec![json!({
                "item_id": "p013-b004",
                "page_idx": 12,
                "bbox": [56.994, 290.451, 302.969, 378.436],
                "block_type": "text",
                "math_mode": "direct_typst",
                "translation_unit_id": "__cg__:cg-012-016",
                "translation_unit_kind": "single",
                "continuation_group": "cg-012-016",
                "protected_source_text": "This behavior partially results from nonadditivity dispersion effects.",
                "translation_unit_protected_source_text": "Having demonstrated the good performance of GFN2-xTB for small systems including different elements and interaction types, we next turn our attention to larger systems. This behavior partially results from nonadditivity dispersion effects.",
                "translated_text": "这种行为部分源于非加和色散效应。",
                "protected_translated_text": "这种行为部分源于非加和色散效应。",
                "translation_unit_protected_translated_text": "我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的非共价相互作用具有良好的性能，接下来我们将关注更大的体系。这种行为部分源于非加和色散效应。",
                "translation_unit_formula_map": [],
            })],
        );
        let result = prepare_render_payloads_by_page(&pages, None, None);
        assert_eq!(
            result[&11][0]["render_protected_text"],
            json!("我们已经证明了GFN2-xTB对于包含不同元素和相互作用类型的小体系的")
        );
        assert_eq!(
            result[&11][1]["render_protected_text"],
            json!("非共价相互作用具有良好的性能，接下来我们将关注更大的体系。")
        );
        assert_eq!(
            result[&12][0]["render_protected_text"],
            json!("这种行为部分源于非加和色散效应。")
        );
    }

    #[test]
    fn group_unit_text_split_across_capacities() {
        // A "group" unit with no member text: the unit text is split across the
        // two member boxes by capacity. Expectations hardcoded from the Python
        // reference (byte-exact).
        let unit_translated = "这是第一段比较长的中文渲染文本，用来测试续行分片逻辑是否正确地把长文本切分成多个块。\
第二句话补充更多内容确保文本长度超过单个盒子的容量。第三句话继续填充。";
        let unit_source = "This is the first sentence of a longer English source text used to test the continuation \
split logic. A second sentence adds more content so the total length exceeds any single box \
capacity. A third sentence keeps filling.";
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![
                json!({
                    "item_id": "a",
                    "page_idx": 0,
                    "bbox": [40.0, 100.0, 300.0, 300.0],
                    "block_type": "text",
                    "translation_unit_id": "u1",
                    "translation_unit_kind": "group",
                    "translation_unit_protected_translated_text": unit_translated,
                    "translation_unit_protected_source_text": unit_source,
                    "protected_translated_text": "",
                    "translated_text": "",
                    "translation_unit_formula_map": [],
                }),
                json!({
                    "item_id": "b",
                    "page_idx": 0,
                    "bbox": [40.0, 320.0, 300.0, 340.0],
                    "block_type": "text",
                    "translation_unit_id": "u1",
                    "translation_unit_kind": "group",
                    "translation_unit_protected_translated_text": unit_translated,
                    "translation_unit_protected_source_text": unit_source,
                    "protected_translated_text": "",
                    "translated_text": "",
                    "translation_unit_formula_map": [],
                }),
            ],
        );
        let result = prepare_render_payloads_by_page(&pages, None, None);
        assert_eq!(
            result[&0][0]["render_protected_text"],
            json!("这是第一段比较长的中文渲染文本，用来测试续行分片逻辑是否正确地把长文本切分成多个块。第二句话补充更多内容确保文本长度超过单个盒子的容量。第三")
        );
        assert_eq!(result[&0][1]["render_protected_text"], json!("句话继续填充。"));
        assert_eq!(
            result[&0][0]["render_source_text"],
            json!("This is the first sentence of a longer English source text used to test the continuation split logic. A second sentence adds more content so the total length exceeds any single box capacity. A third sentence")
        );
        assert_eq!(result[&0][1]["render_source_text"], json!("keeps filling."));
        assert_eq!(result[&0][0]["render_formula_map"], json!([]));
    }

    #[test]
    fn same_meaningful_unit_text_clears_render_fields() {
        // Group unit whose source and translated unit texts are the same string:
        // render fields are cleared instead of split.
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![
                json!({
                    "item_id": "a",
                    "page_idx": 0,
                    "bbox": [40.0, 100.0, 300.0, 300.0],
                    "block_type": "text",
                    "translation_unit_id": "u1",
                    "translation_unit_kind": "group",
                    "translation_unit_protected_source_text": "Same text here",
                    "translation_unit_protected_translated_text": "Same text here",
                    "translation_unit_formula_map": [],
                }),
                json!({
                    "item_id": "b",
                    "page_idx": 0,
                    "bbox": [40.0, 320.0, 300.0, 340.0],
                    "block_type": "text",
                    "translation_unit_id": "u1",
                    "translation_unit_kind": "group",
                    "translation_unit_protected_source_text": "Same text here",
                    "translation_unit_protected_translated_text": "Same text here",
                    "translation_unit_formula_map": [],
                }),
            ],
        );
        let result = prepare_render_payloads_by_page(&pages, None, None);
        for item in &result[&0] {
            assert_eq!(item["render_protected_text"], json!(""));
            assert_eq!(item["render_formula_map"], json!([]));
        }
    }

    #[test]
    fn effective_inner_bbox_lookup_writes_field() {
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![json!({
                "item_id": "a",
                "page_idx": 0,
                "bbox": [40.0, 100.0, 300.0, 200.0],
                "block_type": "text",
                "source_text": "x",
                "translated_text": "y",
                "should_translate": true,
            })],
        );
        let mut lookup = BTreeMap::new();
        lookup.insert("a".to_string(), vec![42.0, 110.0, 250.0, 190.0]);
        let result = prepare_render_payloads_by_page(&pages, None, Some(&lookup));
        assert_eq!(
            result[&0][0]["_render_inner_bbox"],
            json!([42.0, 110.0, 250.0, 190.0])
        );
    }

    #[test]
    fn first_line_indent_lookup_writes_field_only_when_positive() {
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![
                json!({
                    "item_id": "a",
                    "page_idx": 0,
                    "bbox": [40.0, 100.0, 300.0, 200.0],
                    "block_type": "text",
                    "source_text": "x",
                    "translated_text": "y",
                    "should_translate": true,
                }),
                json!({
                    "item_id": "b",
                    "page_idx": 0,
                    "bbox": [40.0, 300.0, 300.0, 400.0],
                    "block_type": "text",
                    "source_text": "z",
                    "translated_text": "w",
                    "should_translate": true,
                }),
            ],
        );
        let mut lookup = BTreeMap::new();
        lookup.insert("a".to_string(), 24.0);
        lookup.insert("b".to_string(), 0.0);
        let result = prepare_render_payloads_by_page(&pages, Some(&lookup), None);
        assert_eq!(result[&0][0]["_render_first_line_indent_pt"], json!(24.0));
        assert!(!result[&0][1].as_object().unwrap().contains_key("_render_first_line_indent_pt"));
    }

    #[test]
    fn continuation_adjusted_capacities_relaxes_narrow_interior() {
        let items: Vec<Item> = vec![
            item_with_bbox(100.0),
            item_with_bbox(50.0),
            item_with_bbox(100.0),
        ];
        let refs: Vec<&Item> = items.iter().collect();
        let capacities = vec![100.0, 100.0, 100.0];
        let adjusted = continuation_adjusted_capacities(&refs, &capacities);
        // Middle box (50pt wide vs 100pt neighbors, ratio 0.5 < 0.78) is relaxed
        // to 0.72 (the floor), ends unchanged.
        assert_eq!(adjusted[0], 100.0);
        assert!((adjusted[1] - 72.0).abs() < 1e-9);
        assert_eq!(adjusted[2], 100.0);
    }

    #[test]
    fn continuation_adjusted_capacities_passthrough_when_few_items() {
        let items: Vec<Item> = vec![item_with_bbox(100.0), item_with_bbox(50.0)];
        let refs: Vec<&Item> = items.iter().collect();
        assert_eq!(continuation_adjusted_capacities(&refs, &[100.0, 100.0]), vec![100.0, 100.0]);
    }

    fn item_with_bbox(width: f64) -> Item {
        let mut item = Item::default();
        item.bbox = Some([40.0, 100.0, 40.0 + width, 200.0]);
        item.block_type = Some("text".into());
        item
    }

    #[test]
    fn suspicious_ocr_glued_block_dropped() {
        // First block has huge source density and a short vertical budget before
        // the next block -> render text cleared and skip reason tagged.
        let mut pages = BTreeMap::new();
        pages.insert(
            0,
            vec![
                json!({
                    "item_id": "a",
                    "page_idx": 0,
                    "block_type": "text",
                    "bbox": [50.0, 20.0, 300.0, 40.0],
                    "source_text": "字".repeat(3000),
                    "translated_text": "译".repeat(3000),
                    "should_translate": true,
                }),
                json!({
                    "item_id": "b",
                    "page_idx": 0,
                    "block_type": "text",
                    "bbox": [50.0, 60.0, 300.0, 100.0],
                    "source_text": "x",
                    "translated_text": "y",
                    "should_translate": true,
                }),
            ],
        );
        let result = prepare_render_payloads_by_page(&pages, None, None);
        assert_eq!(result[&0][0]["render_protected_text"], json!(""));
        assert_eq!(result[&0][0]["render_skip_reason"], json!("suspicious_ocr_glued_block"));
        assert_eq!(result[&0][0]["render_diagnostics"][0]["kind"], json!("render_skip_detector"));
    }
}
