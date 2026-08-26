//! Auto redaction executor, port of `source/cleanup/auto.py`. Math collection
//! is a Phase 7R-2 shim (always empty / no intrusive math); the corpus pages
//! contain no special-math-font spans, so production returns the same empty
//! rects and `False`.

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Error, Page};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::dto::{build_cleanup_item_plan, RedactionItem, ValidRedactionItem};
use super::diagnostics::{new_redaction_diagnostics, RedactionDiagnostics};
use super::primitives::{cover_rects_from_valid_items, merge_rects, remove_text_under_rects};
use super::text_matching::item_removable_text_rects;
use super::super::fill::{draw_flat_white_covers, draw_white_covers, RgbPixmap};

/// `auto.py::item_is_safe_for_auto_text_cleanup`.
pub fn item_is_safe_for_auto_text_cleanup(item: &RedactionItem) -> bool {
    if build_cleanup_item_plan(item).visual_cover_only {
        return false;
    }
    let block_kind = if !item.block_kind.is_empty() {
        &item.block_kind
    } else {
        &item.block_type
    };
    if block_kind.trim().to_lowercase() == "render_block" {
        return false;
    }
    if item.continuation_group.is_some() || item.continuation_group_id.is_some() {
        return false;
    }
    true
}

/// `auto.py::apply_auto_redaction` — per-item text-removal detection, covers
/// for the rest, then text redaction over the merged removable rects.
#[allow(clippy::too_many_arguments)]
pub fn apply_auto_redaction(
    render_page: &Page,
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    flat_cover: bool,
) -> Result<RedactionDiagnostics, Error> {
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    diagnostics.route = "auto".to_string();
    diagnostics.strategy = "auto".to_string();

    // collect_page_math_protection_rects / collect_page_non_math_span_heights /
    // page_has_intrusive_math_protection: no special-math-font spans on the
    // corpus pages, so these are empty rects and `False` (documented divergence
    // for math-bearing pages, covered in 7R-4/7R-6).
    let protected_math_rects: Vec<RectTuple> = Vec::new();
    let _non_math_span_heights: Vec<f64> = Vec::new();
    let has_intrusive_math = false;
    if has_intrusive_math {
        diagnostics.auto_text_cleanup_math_protected = true;
    }

    let mut cover_items: Vec<ValidRedactionItem> = Vec::new();
    let mut removable_rects: Vec<RectTuple> = Vec::new();
    let mut skipped_risky_items = 0usize;
    for entry in valid_items {
        if !item_is_safe_for_auto_text_cleanup(&entry.item) {
            skipped_risky_items += 1;
            cover_items.push(entry.clone());
            continue;
        }
        let special_math = if has_intrusive_math { Some(&protected_math_rects[..]) } else { None };
        let item_rects = item_removable_text_rects(
            render_page,
            &entry.item,
            &entry.rect,
            special_math,
            None,
        )?;
        diagnostics.raw_removable_rects += item_rects.len();
        if !item_rects.is_empty() {
            removable_rects.extend(item_rects);
        } else {
            cover_items.push(entry.clone());
        }
    }

    let cover_rects = cover_rects_from_valid_items(&cover_items);
    if flat_cover {
        draw_flat_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    } else {
        draw_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    }
    diagnostics.cover_rects = cover_rects.len();
    diagnostics.fast_page_cover_only =
        !cover_rects.is_empty() && cover_items.len() == valid_items.len();

    let merged_removable_rects = merge_rects(&removable_rects);
    diagnostics.merged_removable_rects = merged_removable_rects.len();
    diagnostics.auto_text_cleanup_items_skipped = skipped_risky_items;
    if !merged_removable_rects.is_empty() {
        remove_text_under_rects(edit_page, &merged_removable_rects)?;
        diagnostics.uses_pymupdf_redaction = true;
        diagnostics.legacy_pdf_write_reason = "auto_text_cleanup".to_string();
    }
    Ok(diagnostics)
}
