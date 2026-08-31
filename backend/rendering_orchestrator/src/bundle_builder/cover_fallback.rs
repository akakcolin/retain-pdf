//! `render_diagnostics` cover-fallback plan (native mirror of
//! `workflow/cover_fallback.py::TypstCoverFallbackPlan`). The plan's
//! `diagnostics()` shape (`{count, head, tail}` per key) is what the rust_api
//! `/diagnostics` endpoint surfaces, so render_rs must emit it in the summary.
//!
//! Native mirror limits: `page_indices` is `document_analysis.visual_cover_page_indices`
//! (the Python union with `source_cleanup_cover_fallback_page_indices` is populated
//! only on the pikepdf_text_strip bbox-strip path, which the native render-source
//! prep does not record). `item_ids` is always empty — the Python probe
//! `item_ids_with_uncovered_unsafe_vector_overlap` returns empty for non-pikepdf
//! strategies and the native prep tracks no uncovered-vector item ids.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::analysis::RenderDocumentAnalysis;

/// `TypstCoverFallbackPlan.diagnostics()` over the available native data.
#[allow(clippy::ptr_arg)]
pub fn typst_cover_fallback_diagnostics(
    analysis: &RenderDocumentAnalysis,
    _translated_pages: &BTreeMap<i64, Vec<Value>>,
    _precleaned_page_indices: &[i32],
    _cleanup_strategy: &str,
) -> Value {
    let page_indices: Vec<i64> = analysis.visual_cover_page_indices().into_iter().collect();
    let item_ids: Vec<String> = Vec::new();
    json!({
        "typst_cover_fallback_pages": summary_plan(&page_indices),
        "typst_cover_fallback_items": summary_plan(&item_ids),
    })
}

/// `cover_fallback._summary`: `{count, head: first 20, tail: last 20}`.
fn summary_plan<T: Ord + serde::Serialize>(values: &[T]) -> Value {
    let count = values.len();
    let head: Vec<&T> = values.iter().take(20).collect();
    let tail: Vec<&T> = if count > 20 {
        values[count - 20..].iter().collect()
    } else {
        Vec::new()
    };
    json!({
        "count": count,
        "head": head,
        "tail": tail,
    })
}

#[cfg(test)]
mod tests {
    use rendering_core::profile::RenderPageKind;

    use super::*;

    fn page(kind: RenderPageKind, redaction: &'static str) -> rendering_core::document_builder::RenderPageAnalysis {
        rendering_core::document_builder::RenderPageAnalysis {
            page_index: 0,
            kind,
            redaction,
            background: "source_pdf_page",
            compose: "typst_overlay",
            layout: "ocr_bbox_overlay",
            reason: String::new(),
            has_large_background: false,
            background_coverage_ratio: 0.0,
            visible_text: false,
            hidden_text: false,
            editable_text: false,
            drawing_count: 0,
            vector_heavy: false,
        }
    }

    fn analysis(pages: Vec<(i64, rendering_core::document_builder::RenderPageAnalysis)>) -> RenderDocumentAnalysis {
        RenderDocumentAnalysis {
            pages: pages.into_iter().collect(),
        }
    }

    fn translated(items: Vec<i64>) -> BTreeMap<i64, Vec<Value>> {
        items.into_iter().map(|idx| (idx, vec![json!({"item_id": format!("p{idx}")})])).collect()
    }

    #[test]
    fn pages_from_visual_cover_indices() {
        let doc = analysis(vec![
            (0, page(RenderPageKind::ScanImage, "visual_cover")),
            (1, page(RenderPageKind::EditableText, "text_layer_only")),
            (2, page(RenderPageKind::ScanImage, "visual_cover_and_remove_text")),
        ]);
        let diagnostics =
            typst_cover_fallback_diagnostics(&doc, &translated(vec![0, 1, 2]), &[0], "typst_fill");
        let pages = &diagnostics["typst_cover_fallback_pages"];
        assert_eq!(pages["count"], 2);
        assert_eq!(pages["head"], json!([0, 2]));
        assert_eq!(pages["tail"], json!([]));
        assert_eq!(diagnostics["typst_cover_fallback_items"]["count"], 0);
    }

    #[test]
    fn no_cover_pages_is_empty_plan() {
        let doc = analysis(vec![
            (0, page(RenderPageKind::EditableText, "text_layer_only")),
            (1, page(RenderPageKind::EditableText, "text_layer_only")),
        ]);
        let diagnostics =
            typst_cover_fallback_diagnostics(&doc, &translated(vec![0, 1]), &[0, 1], "pikepdf_text_strip");
        assert_eq!(diagnostics["typst_cover_fallback_pages"]["count"], 0);
        assert_eq!(diagnostics["typst_cover_fallback_items"]["count"], 0);
    }

    #[test]
    fn summary_head_tail_split_at_20() {
        let values: Vec<i64> = (0..25).collect();
        let summary = summary_plan(&values);
        assert_eq!(summary["count"], 25);
        assert_eq!(summary["head"].as_array().unwrap().len(), 20);
        assert_eq!(summary["head"], json!((0..20).collect::<Vec<i64>>()));
        assert_eq!(summary["tail"].as_array().unwrap().len(), 20);
        assert_eq!(summary["tail"], json!((5..25).collect::<Vec<i64>>()));
    }

    #[test]
    fn summary_no_tail_below_cap() {
        let summary = summary_plan(&[1, 2, 3]);
        assert_eq!(summary["count"], 3);
        assert_eq!(summary["head"], json!([1, 2, 3]));
        assert_eq!(summary["tail"], json!([]));
    }
}
