//! Redaction engine, port of `source/cleanup/redaction_flow.py` and its
//! executors. Phase 7R-2 scope: routing/decision for the auto / visual_cover /
//! visual_cover_and_remove_text routes, the cover + text-removal primitives,
//! and a safe-direct-only text matcher (full span/word matching in 7R-3).
//! Math protection and vector-text routes are shimmed empty/deferred.

pub mod auto;
pub mod config;
pub mod diagnostics;
pub mod dto;
pub mod page_specs;
pub mod primitives;
pub mod redaction_padding;
pub mod routes;
pub mod text_matching;
pub mod text_ownership;
pub mod visual_cover;

pub use diagnostics::RedactionDiagnostics;
pub use dto::{iter_valid_redaction_items, item_translated_text, RedactionItem, ValidRedactionItem};
pub use routes::apply_redaction_route;

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Error, Page};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::fill::RgbPixmap;
use diagnostics::new_empty_redaction_result;

/// `redaction_flow.py::execute_redaction_flow` — build the valid-item plan; an
/// empty plan yields the empty result; otherwise dispatch to the route.
/// `render_page` is the pristine render-side page (text extraction and clip
/// sampling read it before redaction mutates the edit page). `profile_fill`
/// reads the shim-injected `_visual_profile_fill` field (production resolves
/// it per item from the visual profile's `background_fill_for_item`).
#[allow(clippy::too_many_arguments)]
pub fn execute_redaction_flow(
    render_page: &Page,
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    translated_items: &[RedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    cover_only: bool,
    strategy: Option<&str>,
) -> Result<RedactionDiagnostics, Error> {
    let valid_items = iter_valid_redaction_items(translated_items);
    if valid_items.is_empty() {
        return Ok(new_empty_redaction_result(strategy));
    }
    let profile_fill = |item: &RedactionItem| -> Option<[f64; 3]> { item.visual_profile_fill };
    apply_redaction_route(
        render_page,
        edit_page,
        doc,
        page_rect,
        &valid_items,
        render_clip,
        &profile_fill,
        cover_only,
        strategy,
    )
}
