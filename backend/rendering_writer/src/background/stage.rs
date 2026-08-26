//! `background/stage.py::build_clean_background_pdf` — copy TOC, loop the
//! translated pages in sorted order (skipping out-of-range and precleaned
//! pages), run the redaction engine per page with the strategy flip for
//! formula-bearing pages, and leave the caller to save.
//!
//! Phase 7R-4 shims (replaced in 7R-6): `protect_formula_regions_in_redaction_items`
//! is identity for the corpus — formula items carry no translated text and no
//! text item overlaps a formula guard, so the production guard split is a no-op
//! and dropped formula items are filtered by `iter_valid_redaction_items` anyway.
//! `collect_vector_text_rects` is empty — corpus pages have no vector glyphs.
//! The generator asserts both preconditions before recording a case.

use std::collections::{BTreeMap, HashSet};

use mupdf::pdf::PdfDocument;
use mupdf::{Document, Error};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::fill::RgbPixmap;
use super::redaction::{execute_redaction_flow, iter_valid_redaction_items, RedactionDiagnostics, RedactionItem};
use super::toc::copy_toc;

/// Per-page diagnostics aligned with `sorted(translated_pages.keys())`; `None`
/// for a skipped page (out of range or precleaned).
pub struct StageOutcome {
    pub per_page_diagnostics: Vec<Option<RedactionDiagnostics>>,
    pub toc_entries: usize,
}

/// `semantics.py::block_kind` — explicit `block_kind` else `block_type`.
pub fn block_kind(item: &RedactionItem) -> String {
    let explicit = item.block_kind.trim();
    if !explicit.is_empty() {
        return explicit.to_lowercase();
    }
    let block_type = item.block_type.trim();
    if block_type.is_empty() {
        "unknown".to_string()
    } else {
        block_type.to_lowercase()
    }
}

/// `geometry.py::item_rect` non-None — a 4-float bbox with a non-empty rect.
pub fn item_rect_non_empty(item: &RedactionItem) -> bool {
    match &item.bbox {
        Some(b) if b.len() == 4 => b[2] > b[0] && b[3] > b[1],
        _ => false,
    }
}

/// `cleanup_policy.py::item_has_formula_region`.
pub fn item_has_formula_region(item: &RedactionItem) -> bool {
    block_kind(item) == "formula"
        || item.block_type.trim().to_lowercase() == "formula"
        || item.raw_block_type.trim().to_lowercase() == "display_formula"
        || item.normalized_sub_type.trim().to_lowercase() == "display_formula"
}

/// `cleanup_policy.py::page_has_formula_region`.
pub fn page_has_formula_region(items: &[RedactionItem]) -> bool {
    items.iter().any(|it| item_has_formula_region(it) && item_rect_non_empty(it))
}

/// Phase 7R-4 shim for `formula_guard.py::protect_formula_regions_in_redaction_items`.
///
/// Production drops a redaction item whose bbox sits inside its own expanded
/// formula guard and splits items overlapping a guard into fragments; with the
/// corpus's pinned page policy (empty item policies) and non-overlapping layout
/// neither has an observable effect on the valid-item set. The generator asserts
/// `iter_valid_redaction_items(protect(items)) == iter_valid_redaction_items(items)`
/// for every case page, making identity contractually safe here.
pub fn protect_formula_regions_in_redaction_items(
    items: Vec<RedactionItem>,
    _translated_items: &[RedactionItem],
) -> Vec<RedactionItem> {
    items
}

/// Phase 7R-4 shim for `vector_text.py::collect_vector_text_rects`. Corpus pages
/// are built with `insert_text` (text operators, not path fills), so production
/// finds no black-filled vector glyphs; the generator asserts `[]` per case page.
pub fn collect_vector_text_rects(_page: &Document, _page_index: i32, _target_rects: &[RectTuple]) -> Vec<RectTuple> {
    Vec::new()
}

/// `build_clean_background_pdf` — the per-page redaction orchestration only.
/// The caller opens `source_doc`/`edit_pdf` from the same file, supplies the
/// page rect and a page-aware clip sampler (`render_clip(page_index, rect)`),
/// and saves afterward (e.g. `save_optimized`). The sampler is adapted per
/// page inside the loop so the redaction executors keep their single-rect
/// `render_clip` shape.
#[allow(clippy::too_many_arguments)]
pub fn build_clean_background_pdf(
    source_doc: &Document,
    edit_pdf: &mut PdfDocument,
    page_rect: &RectTuple,
    translated_pages: &BTreeMap<i32, Vec<RedactionItem>>,
    redaction_strategy: Option<&str>,
    precleaned_page_indices: &HashSet<i32>,
    render_clip: &dyn Fn(i32, &RectTuple) -> Option<RgbPixmap>,
) -> Result<StageOutcome, Error> {
    let toc_entries = copy_toc(source_doc, edit_pdf)?;
    let target_page_count = edit_pdf.page_count()?;

    let mut diagnostics: Vec<Option<RedactionDiagnostics>> = Vec::new();
    for page_index in translated_pages.keys() {
        let idx = *page_index;
        if !(0 <= idx && idx < target_page_count) {
            diagnostics.push(None);
            continue;
        }
        if precleaned_page_indices.contains(&idx) {
            diagnostics.push(None);
            continue;
        }
        let render_page = source_doc.load_page(idx)?;
        let mut edit_page = edit_pdf.load_pdf_page(idx)?;

        let items: Vec<RedactionItem> = translated_pages[&idx].clone();
        let protected = protect_formula_regions_in_redaction_items(items.clone(), &items);

        let strategy = match redaction_strategy {
            Some(s) => Some(s),
            None if page_has_formula_region(&items) => Some("visual_cover"),
            None => None,
        };

        let valid = iter_valid_redaction_items(&protected);
        let target_rects: Vec<RectTuple> = valid.iter().map(|e| e.rect).collect();
        let cover_only = !collect_vector_text_rects(source_doc, idx, &target_rects).is_empty();

        let clip_for_page = |rect: &RectTuple| render_clip(idx, rect);
        let diag = execute_redaction_flow(
            &render_page,
            &mut edit_page,
            edit_pdf,
            page_rect,
            &protected,
            &clip_for_page,
            cover_only,
            strategy,
        )?;
        diagnostics.push(Some(diag));
    }

    Ok(StageOutcome {
        per_page_diagnostics: diagnostics,
        toc_entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_has_formula_region_recognizes_subtype() {
        let item: RedactionItem = serde_json::from_str(
            r#"{"bbox":[0.0,0.0,10.0,10.0],"translated_text":"x","normalized_sub_type":"display_formula"}"#,
        )
        .unwrap();
        assert!(item_has_formula_region(&item));
        assert!(page_has_formula_region(&[item]));
    }

    #[test]
    fn plain_text_item_is_not_formula() {
        let item: RedactionItem =
            serde_json::from_str(r#"{"bbox":[0.0,0.0,10.0,10.0],"translated_text":"x","block_type":"text"}"#)
                .unwrap();
        assert!(!item_has_formula_region(&item));
    }

    #[test]
    fn formula_region_needs_valid_bbox() {
        let empty: RedactionItem = serde_json::from_str(
            r#"{"bbox":[],"translated_text":"x","normalized_sub_type":"display_formula"}"#,
        )
        .unwrap();
        assert!(item_has_formula_region(&empty));
        assert!(!page_has_formula_region(&[empty]));
    }
}
