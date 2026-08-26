//! Redaction route resolution, decision, and dispatch, port of
//! `source/cleanup/{strategy,route_decider,routes}.py`. The `text_layer_only`
//! subroutes (image_page / cover_only_count / vector_heavy / standard) are
//! deferred to 7R-4+ and surface as a `deferred_text_layer_only` diagnostic
//! route so the bridge shim can fall back to Python.

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Error, Page};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::auto::apply_auto_redaction;
use super::diagnostics::{new_redaction_diagnostics, RedactionDiagnostics};
use super::dto::{RedactionItem, ValidRedactionItem};
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

/// `route_decider.py::decide_redaction_execution` — for the three routes the
/// 7R-2 corpus reaches, the decision is the route itself. `text_layer_only` is
/// deferred (its subroute predicates land in 7R-4).
pub enum RedactionExecution {
    Auto,
    VisualCover,
    VisualCoverAndRemoveText,
    DeferredTextLayerOnly,
}

pub fn decide_redaction_execution(route: &str) -> RedactionExecution {
    match route {
        ROUTE_AUTO => RedactionExecution::Auto,
        ROUTE_VISUAL_COVER => RedactionExecution::VisualCover,
        ROUTE_VISUAL_COVER_AND_REMOVE_TEXT => RedactionExecution::VisualCoverAndRemoveText,
        _ => RedactionExecution::DeferredTextLayerOnly,
    }
}

fn deferred_diagnostics(items: usize) -> RedactionDiagnostics {
    let mut d = new_redaction_diagnostics(items);
    d.route = "deferred_text_layer_only".to_string();
    d
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
        RedactionExecution::DeferredTextLayerOnly => Ok(deferred_diagnostics(valid_items.len())),
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
            RedactionExecution::DeferredTextLayerOnly
        ));
    }
}
