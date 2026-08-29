//! Port of `planning/planner.py` — the source-cleanup candidates assembly.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use serde_json::Value;

use crate::rect::Rect;
use crate::source_cleanup::constants::BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD;
use crate::source_cleanup::planning::accumulator::BBoxTextStripCandidateAccumulator;
use crate::source_cleanup::planning::accumulator::BBoxTextStripCandidates;
use crate::source_cleanup::planning::coordinate_resolver::PageBBoxResolver;
use crate::source_cleanup::planning::geometry::{formula_guard_rects, ocr_bbox_to_pdf_rect_with_ctm};
use crate::source_cleanup::planning::intent_classifier::classify_source_cleanup_intent;
use crate::source_cleanup::planning::item_classifier::item_allows_item_cover_fallback;
use crate::source_cleanup::planning::page_gate::bbox_text_strip_items_skip_reason;
use crate::source_cleanup::planning::rect_filter::rect_overlaps_any_unsafe_vector;
use crate::source_cleanup::planning::rect_ops::merge_rects;
use crate::source_cleanup::planning::segments::strip_segments_for_text_rect;
use crate::source_cleanup::planning::spatial_index::RectOverlapIndex;
use crate::source_cleanup::planning::BBoxTextStripPagePlan;
use crate::source_cleanup::planning::PageCleanupFeatures;
use crate::source_cleanup::planning::PageContexts;
use crate::source_cleanup::planning::PlanningPageContext;
use crate::source_cleanup::planning::SkipReason;
use crate::source_cleanup::planning::TranslatedPages;

#[derive(Debug, Clone)]
pub struct SourceCleanupItemRects {
    pub item: Value,
    pub pdf_rect: Rect,
    pub view_rect: Rect,
    pub probe_rects: Vec<Rect>,
}

/// `plan_source_cleanup` — the candidates assembly entry. `allows_pikepdf_strip`
/// mirrors `document_analysis.page(idx).allows_pikepdf_text_strip` for pages
/// that have an analysis route; pages absent from the map have no route (never
/// skipped by the visual-background route gate).
pub fn plan_source_cleanup(
    contexts: &PageContexts,
    translated_pages: &TranslatedPages,
    protected_pages: &TranslatedPages,
    skip_formula_pages: bool,
    skip_form_xobject_pages: bool,
    allows_pikepdf_strip: Option<&HashMap<i64, bool>>,
) -> BBoxTextStripCandidates {
    let mut accumulator = BBoxTextStripCandidateAccumulator::new();
    for (page_idx, items) in translated_pages {
        let Some(ctx) = contexts.get(page_idx) else {
            continue;
        };
        let features = PageCleanupFeatures {
            content_stream_size: ctx.content_stream_size,
            has_form_xobjects: ctx.has_form_xobjects,
        };
        accumulator.add_page_features(*page_idx, features);
        let page_plan = plan_source_cleanup_page_ctx(
            ctx,
            items,
            protected_pages.get(page_idx).map(Vec::as_slice).unwrap_or(&[]),
            skip_formula_pages,
            skip_form_xobject_pages,
            allows_pikepdf_strip
                .and_then(|map| map.get(page_idx).copied()),
        );
        accumulator.add_page_plan(*page_idx, &page_plan);
    }
    accumulator.build()
}

/// `plan_source_cleanup_page_ctx` — per-page plan from context + items.
pub fn plan_source_cleanup_page_ctx(
    ctx: &PlanningPageContext,
    translated_items: &[Value],
    protected_items: &[Value],
    skip_formula_pages: bool,
    skip_form_xobject_pages: bool,
    allows_pikepdf_strip: Option<bool>,
) -> BBoxTextStripPagePlan {
    if let Some(allows) = allows_pikepdf_strip {
        if !allows {
            return BBoxTextStripPagePlan {
                skip_reason: SkipReason::VisualBackground,
                ..Default::default()
            };
        }
    }
    let items_skip_reason = bbox_text_strip_items_skip_reason(translated_items, skip_formula_pages);
    if items_skip_reason != SkipReason::None {
        return BBoxTextStripPagePlan {
            skip_reason: items_skip_reason,
            ..Default::default()
        };
    }
    let strip_items: Vec<&Value> = translated_items
        .iter()
        .filter(|item| item_should_emit_strip_rect(item))
        .collect();
    if strip_items.is_empty() {
        return BBoxTextStripPagePlan::default();
    }
    if ctx.content_stream_size >= BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD as u64 {
        return BBoxTextStripPagePlan {
            skip_reason: SkipReason::Complex,
            ..Default::default()
        };
    }
    if skip_form_xobject_pages && ctx.has_form_xobjects {
        return plan_form_xobject_page_ctx(ctx, translated_items, &strip_items, protected_items);
    }

    let bboxes = bboxes_of(&strip_items);
    let resolver = PageBBoxResolver::build(ctx, &bboxes);
    let strip_pairs = iter_strip_item_rect_pairs(ctx, &strip_items, Some(&resolver), true);
    let item_view_rects = merge_rects(
        strip_pairs
            .iter()
            .filter(|pair| !pair.view_rect.is_empty())
            .map(|pair| pair.view_rect),
    );
    if item_view_rects.is_empty() {
        return BBoxTextStripPagePlan::default();
    }
    if !resolver.text_index.overlaps_any(&item_view_rects) {
        return BBoxTextStripPagePlan {
            skip_reason: SkipReason::NoTextOverlap,
            ..Default::default()
        };
    }
    if resolver.has_large_background_image(0.75) {
        return BBoxTextStripPagePlan {
            skip_reason: SkipReason::VisualBackground,
            ..Default::default()
        };
    }

    let formula_rects = iter_formula_item_rects(ctx, translated_items);
    let source_protected_rects = iter_protected_item_rects(ctx, protected_items);
    let source_strip_rects = merge_rects(strip_pairs.iter().map(|pair| pair.pdf_rect));
    let strip_rects = build_page_strip_rects_from_pairs(&strip_pairs, &formula_rects);
    let protected_rects = merge_rects(
        formula_guard_rects(&formula_rects, Some(&source_strip_rects))
            .into_iter()
            .chain(source_protected_rects),
    );
    let uncovered = uncovered_unsafe_vector_item_ids(&strip_pairs, &resolver.unsafe_vector_index);
    BBoxTextStripPagePlan {
        strip_rects,
        protected_rects,
        skip_reason: SkipReason::None,
        uncovered_unsafe_vector_item_ids: uncovered,
    }
}

/// `item_ids_with_uncovered_unsafe_vector_overlap` — the global uncovered-id
/// set across all pages.
pub fn item_ids_with_uncovered_unsafe_vector_overlap(
    contexts: &PageContexts,
    translated_pages: &TranslatedPages,
) -> HashSet<String> {
    let mut item_ids: HashSet<String> = HashSet::new();
    for (page_idx, items) in translated_pages {
        let Some(ctx) = contexts.get(page_idx) else {
            continue;
        };
        item_ids.extend(page_uncovered_unsafe_vector_item_ids_ctx(ctx, items));
    }
    item_ids
}

/// `page_uncovered_unsafe_vector_item_ids_ctx`.
pub fn page_uncovered_unsafe_vector_item_ids_ctx(
    ctx: &PlanningPageContext,
    translated_items: &[Value],
) -> HashSet<String> {
    let strip_items: Vec<&Value> = translated_items
        .iter()
        .filter(|item| item_should_emit_strip_rect(item))
        .collect();
    if strip_items.is_empty() {
        return HashSet::new();
    }
    let resolver = PageBBoxResolver::build(ctx, &bboxes_of(&strip_items));
    let pairs = iter_strip_item_rect_pairs(ctx, &strip_items, Some(&resolver), true);
    uncovered_unsafe_vector_item_ids(&pairs, &resolver.unsafe_vector_index)
        .into_iter()
        .collect()
}

/// `iter_strip_item_rect_pairs_for_page_ctx`.
pub fn iter_strip_item_rect_pairs(
    ctx: &PlanningPageContext,
    translated_items: &[&Value],
    resolver: Option<&PageBBoxResolver>,
    prefiltered: bool,
) -> Vec<SourceCleanupItemRects> {
    let owned_resolver;
    let active_resolver = match resolver {
        Some(resolver) => resolver,
        None => {
            owned_resolver = PageBBoxResolver::build(ctx, &bboxes_of(translated_items));
            &owned_resolver
        }
    };
    let mut pairs: Vec<SourceCleanupItemRects> = Vec::new();
    for item in translated_items {
        if !prefiltered && !item_should_emit_strip_rect(item) {
            continue;
        }
        let bbox = crate::source_cleanup::planning::item_bbox_f64(item);
        let pdf_rect = active_resolver.ocr_bbox_to_pdf_rect(&bbox);
        let view_rect = active_resolver.resolve_bbox_rect(&bbox);
        let probe_rects = active_resolver.resolve_bbox_probe_rects(&bbox);
        if let (Some(pdf_rect), Some(view_rect)) = (pdf_rect, view_rect) {
            let probe = if probe_rects.is_empty() { vec![view_rect] } else { probe_rects };
            pairs.push(SourceCleanupItemRects {
                item: (*item).clone(),
                pdf_rect,
                view_rect,
                probe_rects: probe,
            });
        }
    }
    pairs
}

/// `iter_formula_item_rects_for_page_ctx`.
pub fn iter_formula_item_rects(ctx: &PlanningPageContext, translated_items: &[Value]) -> Vec<Rect> {
    let mut rects: Vec<Rect> = Vec::new();
    for item in translated_items {
        if !classify_source_cleanup_intent(item).should_protect_source {
            continue;
        }
        let bbox = crate::source_cleanup::planning::item_bbox_f64(item);
        if let Some(rect) = ocr_bbox_to_pdf_rect_with_ctm(&ctx.inverse_ctm, &bbox) {
            rects.push(rect);
        }
    }
    rects
}

/// `iter_protected_item_rects_for_page_ctx`.
pub fn iter_protected_item_rects(ctx: &PlanningPageContext, protected_items: &[Value]) -> Vec<Rect> {
    let protected_refs: Vec<&Value> = protected_items.iter().collect();
    let bboxes = bboxes_of(&protected_refs);
    let resolver = PageBBoxResolver::build(ctx, &bboxes);
    let mut rects: Vec<Rect> = Vec::new();
    for item in protected_items {
        let bbox = crate::source_cleanup::planning::item_bbox_f64(item);
        if let Some(rect) = resolver.ocr_bbox_to_pdf_rect(&bbox) {
            rects.push(rect);
        }
    }
    rects
}

/// `_build_page_strip_rects_from_pairs`.
pub fn build_page_strip_rects_from_pairs(
    strip_pairs: &[SourceCleanupItemRects],
    formula_rects: &[Rect],
) -> Vec<Rect> {
    let rects: Vec<Rect> = strip_pairs
        .iter()
        .flat_map(|pair| strip_segments_for_text_rect(&pair.pdf_rect, formula_rects))
        .collect();
    merge_rects(rects)
}

/// `_plan_form_xobject_page_ctx`.
fn plan_form_xobject_page_ctx(
    ctx: &PlanningPageContext,
    translated_items: &[Value],
    strip_items: &[&Value],
    protected_items: &[Value],
) -> BBoxTextStripPagePlan {
    let formula_rects = iter_formula_item_rects(ctx, translated_items);
    let mut source_strip_rects: Vec<Rect> = Vec::new();
    for item in strip_items {
        let bbox = crate::source_cleanup::planning::item_bbox_f64(item);
        if let Some(rect) = ocr_bbox_to_pdf_rect_with_ctm(&ctx.inverse_ctm, &bbox) {
            source_strip_rects.push(rect);
        }
    }
    let strip_rects: Vec<Rect> = source_strip_rects
        .iter()
        .flat_map(|rect| strip_segments_for_text_rect(rect, &formula_rects))
        .collect();
    let strip_rects = merge_rects(strip_rects);
    let source_strip_merged = merge_rects(source_strip_rects.clone());
    let protected_rects = merge_rects(
        formula_guard_rects(&formula_rects, Some(&source_strip_merged))
            .into_iter()
            .chain(iter_protected_item_rects(ctx, protected_items)),
    );
    BBoxTextStripPagePlan {
        strip_rects,
        protected_rects,
        skip_reason: SkipReason::None,
        uncovered_unsafe_vector_item_ids: Vec::new(),
    }
}

/// `uncovered_unsafe_vector_item_ids`.
pub fn uncovered_unsafe_vector_item_ids(
    strip_pairs: &[SourceCleanupItemRects],
    unsafe_rects: &RectOverlapIndex,
) -> Vec<String> {
    if unsafe_rects.rects.is_empty() {
        return Vec::new();
    }
    let mut item_ids: HashSet<String> = HashSet::new();
    for pair in strip_pairs {
        let item_id = crate::source_cleanup::planning::item_str(&pair.item, "item_id");
        if item_id.is_empty() {
            continue;
        }
        if !item_allows_item_cover_fallback(&pair.item) {
            continue;
        }
        if pair_overlaps_unsafe_vector(pair, unsafe_rects) {
            item_ids.insert(item_id);
        }
    }
    let mut sorted: Vec<String> = item_ids.into_iter().collect();
    sorted.sort_unstable();
    sorted
}

fn pair_overlaps_unsafe_vector(pair: &SourceCleanupItemRects, unsafe_rects: &RectOverlapIndex) -> bool {
    pair.probe_rects
        .iter()
        .any(|rect| rect_overlaps_any_unsafe_vector(rect, unsafe_rects))
}

/// `item_should_emit_strip_rect`.
pub fn item_should_emit_strip_rect(item: &Value) -> bool {
    classify_source_cleanup_intent(item).should_strip_text
}

fn bboxes_of(items: &[&Value]) -> Vec<Vec<f64>> {
    items
        .iter()
        .map(|item| crate::source_cleanup::planning::item_bbox_f64(item))
        .collect()
}

/// `build_formula_guard_rects`.
pub fn build_formula_guard_rects(formula_rects: &[Rect], strip_rects: Option<&[Rect]>) -> Vec<Rect> {
    formula_guard_rects(formula_rects, strip_rects)
}

/// Serialize a `BTreeMap<i64, Vec<Value>>` (translated / protected pages) from
/// raw JSON object keys.
pub fn pages_from_json(value: &Value) -> TranslatedPages {
    let mut pages: TranslatedPages = BTreeMap::new();
    if let Value::Object(map) = value {
        for (key, items) in map {
            let Ok(page_idx) = key.parse::<i64>() else {
                continue;
            };
            let items = match items {
                Value::Array(values) => values.clone(),
                _ => Vec::new(),
            };
            pages.insert(page_idx, items);
        }
    }
    pages
}

/// Parse the `{page_idx: allows_pikepdf_text_strip}` map (absent → `None`).
pub fn allows_pikepdf_from_json(value: &Value) -> Option<HashMap<i64, bool>> {
    match value {
        Value::Null => None,
        Value::Object(map) => {
            let mut out: HashMap<i64, bool> = HashMap::new();
            for (key, flag) in map {
                let Ok(page_idx) = key.parse::<i64>() else {
                    continue;
                };
                out.insert(page_idx, flag.as_bool().unwrap_or(true));
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::Matrix;

    fn item(item_id: &str, role: &str, bbox: [f64; 4]) -> Value {
        serde_json::json!({
            "item_id": item_id,
            "block_type": "text",
            "block_kind": "text",
            "layout_role": role,
            "translation_overlay_text": "译文",
            "bbox": bbox,
        })
    }

    fn ctx() -> PlanningPageContext {
        PlanningPageContext {
            page_index: 0,
            page_rect: Rect::new(0.0, 0.0, 612.0, 792.0),
            bboxlog_entries: vec![("text".to_string(), Rect::new(48.0, 96.0, 200.0, 116.0))],
            content_stream_size: 100,
            has_form_xobjects: false,
            inverse_ctm: Matrix::identity(),
        }
    }

    #[test]
    fn plans_strip_rects_for_caption() {
        let items = vec![item("c0", "caption", [48.0, 96.0, 200.0, 116.0])];
        let plan = plan_source_cleanup_page_ctx(&ctx(), &items, &[], false, true, None);
        assert_eq!(plan.skip_reason, SkipReason::None);
        assert!(!plan.strip_rects.is_empty(), "caption must produce strip rects");
    }

    #[test]
    fn skips_no_text_overlap() {
        let items = vec![item("c0", "caption", [300.0, 500.0, 400.0, 520.0])];
        let plan = plan_source_cleanup_page_ctx(&ctx(), &items, &[], false, true, None);
        assert_eq!(plan.skip_reason, SkipReason::NoTextOverlap);
    }

    #[test]
    fn skips_visual_background_via_route() {
        let items = vec![item("c0", "caption", [48.0, 96.0, 200.0, 116.0])];
        let plan = plan_source_cleanup_page_ctx(&ctx(), &items, &[], false, true, Some(false));
        assert_eq!(plan.skip_reason, SkipReason::VisualBackground);
    }
}
