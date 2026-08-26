//! Visual-cover redaction executor, port of
//! `source/cleanup/visual_cover_execution.py`. The visual-profile branch draws
//! a per-item profile fill (never triggered in 7R-2: the corpus passes
//! `visual_profile=None`, so `profile_fill` always returns `None`).

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Error, Page};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::dto::{RedactionItem, ValidRedactionItem};
use super::diagnostics::{new_redaction_diagnostics, RedactionDiagnostics};
use super::primitives::{cover_rects_from_valid_items, remove_text_under_rects};
use super::super::fill::{draw_flat_white_covers, draw_solid_cover, draw_white_covers, RgbPixmap};

/// `visual_cover_execution.py::apply_visual_cover_redaction`.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub fn apply_visual_cover_redaction(
    _render_page: &Page,
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    profile_fill: &dyn Fn(&RedactionItem) -> Option<[f64; 3]>,
    remove_text_layer: bool,
    flat_cover: bool,
    route: &str,
) -> Result<RedactionDiagnostics, Error> {
    let mut diagnostics = new_redaction_diagnostics(valid_items.len());
    let mut profile_cover_count = 0usize;
    let mut profile_miss_items: Vec<ValidRedactionItem> = Vec::new();
    if !flat_cover {
        for entry in valid_items {
            match profile_fill(&entry.item) {
                Some(fill) => {
                    draw_solid_cover(edit_page, doc, &entry.rect, &fill)?;
                    profile_cover_count += 1;
                }
                None => profile_miss_items.push(entry.clone()),
            }
        }
    }
    let cover_rects = if !flat_cover {
        cover_rects_from_valid_items(&profile_miss_items)
    } else {
        cover_rects_from_valid_items(valid_items)
    };
    if flat_cover {
        draw_flat_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    } else {
        draw_white_covers(edit_page, doc, page_rect, &cover_rects, render_clip)?;
    }
    if remove_text_layer {
        remove_text_under_rects(edit_page, &cover_rects)?;
        diagnostics.uses_pymupdf_redaction = !cover_rects.is_empty();
        diagnostics.legacy_pdf_write_reason = if cover_rects.is_empty() {
            String::new()
        } else {
            "visual_cover_remove_text_layer".to_string()
        };
    }
    diagnostics.cover_rects = cover_rects.len() + profile_cover_count;
    diagnostics.visual_profile_cover_rects = profile_cover_count;
    diagnostics.fast_page_cover_only = true;
    diagnostics.route = route.to_string();
    diagnostics.strategy = if remove_text_layer {
        "visual_cover_and_remove_text".to_string()
    } else {
        "visual_cover".to_string()
    };
    Ok(diagnostics)
}
