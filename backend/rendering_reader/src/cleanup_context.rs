//! Planning-page-context construction, shared by the bridge
//! (`plan_source_cleanup_native` / `uncovered_unsafe_vector_item_ids_native`)
//! and the native orchestrator's render-source prep (C3-N11c).
//!
//! Mirrors `page_context._build_context_from_fitz` and the native batch skip:
//! bboxlog entries with empty rects are dropped (the fitz consumers never see
//! them), the inverse ctm is the pure `inverse_affine` of the raw page ctm, and
//! pages whose rect or ctm cannot be read are omitted.

use std::collections::BTreeMap;

use mupdf::Document;
use rendering_core::rect::{Matrix, Rect};
use rendering_core::source_cleanup::constants::BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD;
use rendering_core::source_cleanup::planning::{PageContexts, PlanningPageContext};

use crate::pdf_document::PdfDocument;

/// Build the `PlanningPageContext`s the source-cleanup planner consumes from a
/// loaded document. Extraction from the bridge is behavior-neutral: same trait
/// readers, same skip semantics, same field order.
pub fn build_planning_contexts(doc: &Document, indices: &[i64]) -> PageContexts {
    let mut contexts: PageContexts = BTreeMap::new();
    for &idx in indices {
        let Ok(page_rect) = doc.page_rect(idx) else {
            continue;
        };
        let Some(ctm) = doc.page_ctm(idx) else {
            continue;
        };
        let inverse_ctm = Matrix::new(ctm[0], ctm[1], ctm[2], ctm[3], ctm[4], ctm[5]).inverse();
        let bboxlog_entries: Vec<(String, Rect)> = doc
            .page_bboxlog(idx)
            .iter()
            .filter(|entry| !entry.rect.is_empty())
            .map(|entry| (entry.kind.clone(), entry.rect))
            .collect();
        contexts.insert(
            idx,
            PlanningPageContext {
                page_index: idx,
                page_rect,
                bboxlog_entries,
                content_stream_size: doc.page_content_stream_size(
                    idx,
                    BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD as u64,
                ),
                has_form_xobjects: doc.page_has_form_xobjects(idx),
                inverse_ctm,
            },
        );
    }
    contexts
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const GOLDEN_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/samples/golden-pdfs"
    );

    fn golden_doc(name: &str) -> mupdf::Document {
        PdfDocument::open(&Path::new(GOLDEN_ROOT).join(name))
            .unwrap_or_else(|e| panic!("open {name}: {e}"))
    }

    fn page_count(doc: &mupdf::Document) -> i64 {
        <mupdf::Document as PdfDocument>::page_count(doc).expect("page_count")
    }

    #[test]
    fn builds_planning_contexts_for_present_pages() {
        for name in ["1.pdf", "2.pdf"] {
            let doc = golden_doc(name);
            let indices: Vec<i64> = (0..page_count(&doc)).collect();
            let contexts = build_planning_contexts(&doc, &indices);
            assert_eq!(contexts.len() as i64, page_count(&doc));
            for (idx, ctx) in &contexts {
                assert_eq!(*idx, ctx.page_index);
                assert!(!ctx.page_rect.is_empty(), "{name} p{idx}: empty page rect");
                let m = ctx.inverse_ctm;
                assert!(
                    m.a.is_finite() && m.b.is_finite() && m.c.is_finite()
                        && m.d.is_finite() && m.e.is_finite() && m.f.is_finite(),
                    "{name} p{idx}: non-finite inverse ctm"
                );
                assert!(
                    ctx.bboxlog_entries.iter().all(|(_, rect)| !rect.is_empty()),
                    "{name} p{idx}: empty bboxlog rect survived the filter"
                );
            }
        }
    }

    #[test]
    fn absent_pages_are_omitted() {
        let doc = golden_doc("1.pdf");
        let contexts = build_planning_contexts(&doc, &[999_999]);
        assert!(contexts.is_empty());
    }
}
