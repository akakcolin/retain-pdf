//! Port of the `text_layer_only` subroute executors — `image_page.py`,
//! `cover_only.py`, `vector_heavy.py`, `standard.py` + `standard_execution.py` +
//! `standard_policy.py` + `layer_items.py` — plus the decision that routes
//! between them (`route_decider.py`). `fill_background` is always `None` in
//! production (stage.py:80 passes None, redaction_flow.py defaults it), so the
//! `fill_background is None` guards in the Python decider/executors hold and are
//! not represented here.

use mupdf::pdf::{
    PdfDocument, PdfPage, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::{Error, Page, Rect};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::complex_math::item_has_complex_inline_math;
use super::diagnostics::{new_redaction_diagnostics, RedactionDiagnostics};
use super::dto::{build_cleanup_item_plan, RedactionItem, ValidRedactionItem};
use super::primitives::{
    cover_rects_from_valid_items, merge_rects, remove_text_under_rects,
    text_removal_rects_from_valid_items,
};
use super::standard_thresholds::{
    HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD, ITEM_REMOVABLE_RECTS_FAST_COVER_THRESHOLD,
    PAGE_AVG_REMOVABLE_RECTS_FAST_COVER_THRESHOLD, PAGE_ITEM_REMOVABLE_RECTS_FAST_COVER_COUNT,
    PAGE_REMOVABLE_RECTS_FAST_COVER_THRESHOLD, VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD,
};
use super::text_matching::item_removable_text_rects;
use super::super::fill::{
    apply_prepared_background_covers, draw_flat_white_covers, draw_white_covers,
    prepare_background_covers, RgbPixmap,
};

/// `route_decider.py` text_layer_only branch with `fill_background` always None.
pub enum TextLayerOnlySubroute {
    ImagePage,
    CoverOnlyCount,
    VectorHeavy,
    Standard,
}

pub fn decide_text_layer_only(image_page: bool, drawing_count: usize) -> TextLayerOnlySubroute {
    if image_page {
        TextLayerOnlySubroute::ImagePage
    } else if drawing_count >= HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD {
        TextLayerOnlySubroute::CoverOnlyCount
    } else if drawing_count >= VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD {
        TextLayerOnlySubroute::VectorHeavy
    } else {
        TextLayerOnlySubroute::Standard
    }
}

/// `layer_items.py::visual_cover_rects`.
pub fn visual_cover_rects(valid_items: &[ValidRedactionItem]) -> Vec<RectTuple> {
    valid_items
        .iter()
        .filter(|entry| build_cleanup_item_plan(&entry.item).visual_cover_only)
        .map(|entry| entry.rect)
        .collect()
}

/// `layer_items.py::bbox_text_strip_rects`.
pub fn bbox_text_strip_rects(valid_items: &[ValidRedactionItem]) -> Vec<RectTuple> {
    valid_items
        .iter()
        .filter(|entry| build_cleanup_item_plan(&entry.item).bbox_text_strip_allowed)
        .map(|entry| entry.rect)
        .collect()
}

/// `standard_policy.py::should_force_bbox_redaction` — `bool(item.get("continuation_group"))`.
pub fn should_force_bbox_redaction(item: &RedactionItem) -> bool {
    !item.continuation_group.as_deref().unwrap_or("").is_empty()
}

/// `standard_policy.py::should_force_visual_cover`.
pub fn should_force_visual_cover(item: &RedactionItem) -> bool {
    build_cleanup_item_plan(item).visual_cover_only || item_has_complex_inline_math(item)
}

/// `standard_policy.py::should_use_fast_page_cover_for_removable_counts`.
pub fn should_use_fast_page_cover_for_removable_counts(removable_counts: &[usize]) -> bool {
    if removable_counts.is_empty() {
        return false;
    }
    let total_raw_rects: usize = removable_counts.iter().sum();
    let avg_raw_rects = total_raw_rects as f64 / removable_counts.len().max(1) as f64;
    let large_item_count = removable_counts
        .iter()
        .filter(|&&count| count >= ITEM_REMOVABLE_RECTS_FAST_COVER_THRESHOLD)
        .count();
    total_raw_rects >= PAGE_REMOVABLE_RECTS_FAST_COVER_THRESHOLD
        || avg_raw_rects >= PAGE_AVG_REMOVABLE_RECTS_FAST_COVER_THRESHOLD
        || large_item_count >= PAGE_ITEM_REMOVABLE_RECTS_FAST_COVER_COUNT
}

/// `standard_execution.py::apply_page_cover_text_cleanup` — cover every valid
/// item and strip the text layer of every strip-allowed item.
pub fn apply_page_cover_text_cleanup(
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    mut diagnostics: RedactionDiagnostics,
    route: &str,
    reason: &str,
) -> Result<RedactionDiagnostics, Error> {
    let cover_rects = cover_rects_from_valid_items(valid_items);
    let text_removal_rects = text_removal_rects_from_valid_items(valid_items);
    draw_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    remove_text_under_rects(edit_page, &text_removal_rects)?;
    diagnostics.cover_rects = cover_rects.len();
    diagnostics.fast_page_cover_only = true;
    diagnostics.route = route.to_string();
    diagnostics.uses_pymupdf_redaction = !text_removal_rects.is_empty();
    diagnostics.legacy_pdf_write_reason = if text_removal_rects.is_empty() {
        String::new()
    } else {
        reason.to_string()
    };
    Ok(diagnostics)
}

/// `standard_execution.py::apply_redaction_annotations` — redact every rect and
/// apply with images/graphics=NONE, text=REMOVE. Production passes
/// `fill=resolve_fill(...)`; with `fill_background` always None every fill
/// resolves to None, so the annotation carries no fill color (equivalent).
pub fn apply_redaction_annotations(
    edit_page: &mut PdfPage,
    redactions: &[(RectTuple, Option<[f64; 3]>)],
    diagnostics: &mut RedactionDiagnostics,
) -> Result<(), Error> {
    for (rect, _fill) in redactions {
        edit_page.add_redact_annotation(Rect::new(
            rect[0] as f32,
            rect[1] as f32,
            rect[2] as f32,
            rect[3] as f32,
        ))?;
    }
    if redactions.is_empty() {
        return Ok(());
    }
    edit_page.apply_redactions_with_options(PdfRedactOptions {
        black_boxes: false,
        image_method: PdfRedactImageMethod::None,
        line_art: PdfRedactLineArtMethod::None,
        text: PdfRedactTextMethod::Remove,
    })?;
    diagnostics.uses_pymupdf_redaction = true;
    diagnostics.legacy_pdf_write_reason = "standard_redaction".to_string();
    Ok(())
}

/// `image_page.py::apply_image_page_redaction` — remove the text layer under
/// the strip-allowed rects, repaint their background from a sampled pixmap,
/// then cover the visual-cover rects.
pub fn apply_image_page_redaction(
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<RedactionDiagnostics, Error> {
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    let cover_rects = visual_cover_rects(valid_items);
    let rects = bbox_text_strip_rects(valid_items);
    let prepared_covers = prepare_background_covers(page_rect, render_clip, &rects);
    remove_text_under_rects(edit_page, &rects)?;
    apply_prepared_background_covers(edit_page, doc, page_rect, render_clip, &prepared_covers)?;
    draw_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    diagnostics.route = "image_page_redaction".to_string();
    diagnostics.strategy = "text_layer_only".to_string();
    diagnostics.cover_rects = rects.len() + cover_rects.len();
    diagnostics.uses_pymupdf_redaction = !rects.is_empty();
    diagnostics.legacy_pdf_write_reason = if rects.is_empty() {
        String::new()
    } else {
        "image_page_text_layer_cleanup".to_string()
    };
    Ok(diagnostics)
}

/// `cover_only.py::apply_cover_only_count_redaction`.
pub fn apply_cover_only_count_redaction(
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<RedactionDiagnostics, Error> {
    let cover_rects = cover_rects_from_valid_items(valid_items);
    let text_removal_rects = text_removal_rects_from_valid_items(valid_items);
    draw_flat_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    remove_text_under_rects(edit_page, &text_removal_rects)?;
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    diagnostics.cover_rects = cover_rects.len();
    diagnostics.fast_page_cover_only = true;
    diagnostics.route = "cover_only_count".to_string();
    diagnostics.strategy = "text_layer_only".to_string();
    diagnostics.uses_pymupdf_redaction = !text_removal_rects.is_empty();
    diagnostics.legacy_pdf_write_reason = if text_removal_rects.is_empty() {
        String::new()
    } else {
        "cover_only_count_text_cleanup".to_string()
    };
    Ok(diagnostics)
}

/// `vector_heavy.py::apply_vector_heavy_redaction`.
pub fn apply_vector_heavy_redaction(
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<RedactionDiagnostics, Error> {
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    let cover_rects = visual_cover_rects(valid_items);
    let rects = bbox_text_strip_rects(valid_items);
    diagnostics.cover_rects = rects.len() + cover_rects.len();
    diagnostics.route = "vector_heavy_redaction".to_string();
    diagnostics.strategy = "text_layer_only".to_string();
    let mut combined = rects.clone();
    combined.extend(cover_rects);
    draw_white_covers(edit_page, doc, page_rect, &combined, render_clip)?;
    remove_text_under_rects(edit_page, &rects)?;
    diagnostics.uses_pymupdf_redaction = !rects.is_empty();
    diagnostics.legacy_pdf_write_reason = if rects.is_empty() {
        String::new()
    } else {
        "vector_heavy_text_layer_cleanup".to_string()
    };
    Ok(diagnostics)
}

/// `standard.py::apply_standard_redaction` — per-item force-cover / force-bbox
/// / removable-text-rect decision, then the fast-page-cover shortcut or the
/// merged cover + redaction-annotation path. The production
/// `page_should_use_cover_only(drawing_rects)` (>=5000) branch is skipped: the
/// route decision pre-routes drawing_count >= 5000 to cover_only_count, and for
/// the corpus pages the raw drawing count equals `len(collect_page_drawing_rects)`
/// (every drawing carries a non-empty rect after thin-expansion).
pub fn apply_standard_redaction(
    render_page: &Page,
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<RedactionDiagnostics, Error> {
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    diagnostics.strategy = "text_layer_only".to_string();

    let mut redactions: Vec<(RectTuple, Option<[f64; 3]>)> = Vec::new();
    let mut cover_rects: Vec<RectTuple> = Vec::new();
    let mut removable_counts: Vec<usize> = Vec::new();
    for entry in valid_items {
        if should_force_visual_cover(&entry.item) {
            cover_rects.push(entry.rect);
            diagnostics.item_fast_cover_count += 1;
            continue;
        }
        if should_force_bbox_redaction(&entry.item) {
            redactions.push((entry.rect, None));
            continue;
        }
        let removable_rects =
            item_removable_text_rects(render_page, &entry.item, &entry.rect, None, None)?;
        let raw_count = removable_rects.len();
        diagnostics.raw_removable_rects += raw_count;
        if raw_count > 0 {
            removable_counts.push(raw_count);
        }
        let merged_removable_rects = merge_rects(&removable_rects);
        let merged_count = merged_removable_rects.len();
        diagnostics.merged_removable_rects += merged_count;
        if raw_count >= ITEM_REMOVABLE_RECTS_FAST_COVER_THRESHOLD {
            cover_rects.push(entry.rect);
            diagnostics.item_fast_cover_count += 1;
            continue;
        }
        if !merged_removable_rects.is_empty() {
            redactions.extend(merged_removable_rects.into_iter().map(|r| (r, None)));
            continue;
        }
        cover_rects.push(entry.rect);
    }

    if should_use_fast_page_cover_for_removable_counts(&removable_counts) {
        return apply_page_cover_text_cleanup(
            edit_page,
            doc,
            page_rect,
            valid_items,
            render_clip,
            diagnostics,
            "fast_page_cover_only",
            "fast_page_cover_only_text_cleanup",
        );
    }

    let merged_cover_rects = merge_rects(&cover_rects);
    diagnostics.cover_rects = merged_cover_rects.len();
    draw_white_covers(edit_page, doc, page_rect, &merged_cover_rects, render_clip)?;

    apply_redaction_annotations(edit_page, &redactions, &mut diagnostics)?;
    diagnostics.route = "standard_redaction".to_string();
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> RedactionItem {
        serde_json::from_str(json).expect("parse")
    }

    #[test]
    fn decide_routes_by_count_and_image() {
        assert!(matches!(
            decide_text_layer_only(true, 0),
            TextLayerOnlySubroute::ImagePage
        ));
        assert!(matches!(
            decide_text_layer_only(false, 5000),
            TextLayerOnlySubroute::CoverOnlyCount
        ));
        assert!(matches!(
            decide_text_layer_only(false, 2000),
            TextLayerOnlySubroute::VectorHeavy
        ));
        assert!(matches!(
            decide_text_layer_only(false, 1999),
            TextLayerOnlySubroute::Standard
        ));
    }

    #[test]
    fn bbox_redaction_needs_nonempty_group() {
        assert!(should_force_bbox_redaction(&item(r#"{"continuation_group":"g"}"#)));
        assert!(!should_force_bbox_redaction(&item(r#"{"continuation_group":""}"#)));
        assert!(!should_force_bbox_redaction(&item(r#"{}"#)));
    }

    #[test]
    fn force_visual_cover_from_plan_or_math() {
        let plan_cover = item(r#"{"_render_cleanup_mode":"visual_cover"}"#);
        assert!(should_force_visual_cover(&plan_cover));
        let math_cover = item(r#"{"translated_text":"$\\frac{a}{b}$"}"#);
        assert!(should_force_visual_cover(&math_cover));
        let plain = item(r#"{"translated_text":"hello"}"#);
        assert!(!should_force_visual_cover(&plain));
    }

    #[test]
    fn fast_page_cover_counts() {
        assert!(!should_use_fast_page_cover_for_removable_counts(&[]));
        // avg >= 24.0 with a single 24-removable item.
        assert!(should_use_fast_page_cover_for_removable_counts(&[24]));
        // total >= 180.
        assert!(should_use_fast_page_cover_for_removable_counts(&[90, 90]));
        // eight >=24 items.
        assert!(should_use_fast_page_cover_for_removable_counts(&[24; 8]));
        // below all thresholds.
        assert!(!should_use_fast_page_cover_for_removable_counts(&[5, 10]));
    }
}
