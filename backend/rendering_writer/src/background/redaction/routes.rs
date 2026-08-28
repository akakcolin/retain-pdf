//! Redaction route resolution, decision, and dispatch, port of
//! `source/cleanup/{strategy,route_decider,routes}.py`.

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Error, Page};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::auto::apply_auto_redaction;
use super::diagnostics::RedactionDiagnostics;
use super::dto::{RedactionItem, ValidRedactionItem};
use super::text_layer_only::{
    apply_cover_only_count_redaction, apply_image_page_redaction, apply_standard_redaction,
    apply_vector_heavy_redaction, decide_text_layer_only, TextLayerOnlySubroute,
};
use super::visual_cover::apply_visual_cover_redaction;
use super::super::fill::RgbPixmap;

pub const DEFAULT_REDACTION_ROUTE: &str = "auto";
pub const ROUTE_AUTO: &str = "auto";
pub const ROUTE_VISUAL_COVER: &str = "visual_cover";
pub const ROUTE_VISUAL_COVER_AND_REMOVE_TEXT: &str = "visual_cover_and_remove_text";
pub const ROUTE_TEXT_LAYER_ONLY: &str = "text_layer_only";

/// `strategy.py::resolve_redaction_route` — alias resolution, ValueError on
/// unknown strategies.
pub fn resolve_redaction_route(strategy: Option<&str>, cover_only: bool) -> Result<String, Error> {
    if cover_only && strategy.is_none() {
        return Ok(ROUTE_VISUAL_COVER.to_string());
    }
    let Some(strategy) = strategy else {
        return Ok(DEFAULT_REDACTION_ROUTE.to_string());
    };
    let normalized = strategy.trim().to_lowercase();
    match normalized.as_str() {
        "auto" => Ok(ROUTE_AUTO.to_string()),
        "text_layer_only" | "text_redaction" => Ok(ROUTE_TEXT_LAYER_ONLY.to_string()),
        "visual_cover" | "visual_only" => Ok(ROUTE_VISUAL_COVER.to_string()),
        "visual_cover_and_remove_text" | "visual_and_text" => {
            Ok(ROUTE_VISUAL_COVER_AND_REMOVE_TEXT.to_string())
        }
        _ => Err(Error::InvalidArgument(format!(
            "unsupported redaction strategy: {strategy:?}; expected auto, text_layer_only, visual_cover, or visual_cover_and_remove_text"
        ))),
    }
}

/// `route_decider.py::decide_redaction_execution` — for the auto / visual_cover
/// routes the decision is the route itself; `text_layer_only` resolves its
/// subroute from the page context at dispatch time.
pub enum RedactionExecution {
    Auto,
    VisualCover,
    VisualCoverAndRemoveText,
    TextLayerOnly,
}

pub fn decide_redaction_execution(route: &str) -> RedactionExecution {
    match route {
        ROUTE_AUTO => RedactionExecution::Auto,
        ROUTE_VISUAL_COVER => RedactionExecution::VisualCover,
        ROUTE_VISUAL_COVER_AND_REMOVE_TEXT => RedactionExecution::VisualCoverAndRemoveText,
        _ => RedactionExecution::TextLayerOnly,
    }
}

/// `routes.py::apply_redaction_route` — resolve, decide, dispatch.
#[allow(clippy::too_many_arguments)]
pub fn apply_redaction_route(
    render_page: &Page,
    edit_page: &mut PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    valid_items: &[ValidRedactionItem],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    profile_fill: &dyn Fn(&RedactionItem) -> Option<[f64; 3]>,
    cover_only: bool,
    strategy: Option<&str>,
) -> Result<RedactionDiagnostics, Error> {
    let route = resolve_redaction_route(strategy, cover_only)?;
    match decide_redaction_execution(&route) {
        RedactionExecution::Auto => apply_auto_redaction(
            render_page,
            edit_page,
            doc,
            page_rect,
            valid_items,
            render_clip,
            cover_only,
        ),
        RedactionExecution::VisualCover => apply_visual_cover_redaction(
            render_page,
            edit_page,
            doc,
            page_rect,
            valid_items,
            render_clip,
            profile_fill,
            false,
            cover_only,
            ROUTE_VISUAL_COVER,
        ),
        RedactionExecution::VisualCoverAndRemoveText => apply_visual_cover_redaction(
            render_page,
            edit_page,
            doc,
            page_rect,
            valid_items,
            render_clip,
            profile_fill,
            true,
            cover_only,
            ROUTE_VISUAL_COVER_AND_REMOVE_TEXT,
        ),
        RedactionExecution::TextLayerOnly => {
            // `route_context.py::build_redaction_route_context` with no plan:
            // image_page from the edit page, drawing_count from the raw drawing
            // scan of the pristine render page (== `page_drawing_count` for the
            // corpus pages, whose drawings all carry non-empty rects).
            let image_page = super::super::detect::page_has_large_background_image(edit_page, page_rect)?;
            let drawing_count = render_page.drawings()?.len();
            match decide_text_layer_only(image_page, drawing_count) {
                TextLayerOnlySubroute::ImagePage => {
                    apply_image_page_redaction(edit_page, doc, page_rect, valid_items, render_clip)
                }
                TextLayerOnlySubroute::CoverOnlyCount => apply_cover_only_count_redaction(
                    edit_page,
                    doc,
                    page_rect,
                    valid_items,
                    render_clip,
                ),
                TextLayerOnlySubroute::VectorHeavy => apply_vector_heavy_redaction(
                    edit_page,
                    doc,
                    page_rect,
                    valid_items,
                    render_clip,
                ),
                TextLayerOnlySubroute::Standard => apply_standard_redaction(
                    render_page,
                    edit_page,
                    doc,
                    page_rect,
                    valid_items,
                    render_clip,
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_route_matches_python() {
        assert_eq!(resolve_redaction_route(None, false).unwrap(), "auto");
        assert_eq!(resolve_redaction_route(None, true).unwrap(), "visual_cover");
        assert_eq!(resolve_redaction_route(Some("visual_only"), false).unwrap(), "visual_cover");
        assert_eq!(resolve_redaction_route(Some("visual_and_text"), false).unwrap(), "visual_cover_and_remove_text");
        assert_eq!(resolve_redaction_route(Some("text_redaction"), false).unwrap(), "text_layer_only");
        assert_eq!(resolve_redaction_route(Some("  Auto "), false).unwrap(), "auto");
        assert!(resolve_redaction_route(Some("bogus"), false).is_err());
    }

    #[test]
    fn decide_execution_for_corpus_routes() {
        assert!(matches!(decide_redaction_execution("auto"), RedactionExecution::Auto));
        assert!(matches!(decide_redaction_execution("visual_cover"), RedactionExecution::VisualCover));
        assert!(matches!(
            decide_redaction_execution("visual_cover_and_remove_text"),
            RedactionExecution::VisualCoverAndRemoveText
        ));
        assert!(matches!(
            decide_redaction_execution("text_layer_only"),
            RedactionExecution::TextLayerOnly
        ));
    }
}
