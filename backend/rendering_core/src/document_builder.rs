//! Port of services/rendering/analysis/document/builder.py — the per-page
//! composition of `RenderPageProfile` + `RenderPageRoute` into
//! `RenderPageAnalysis`. The document-level `build_render_document_analysis`
//! opens a PDF with fitz and is deferred to the mupdf-rs reader (5B).

use std::collections::HashMap;

use crate::profile::{RenderPageKind, RenderPageProfile};
use crate::route::build_render_page_route;

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPageAnalysis {
    pub page_index: i64,
    pub kind: RenderPageKind,
    pub redaction: &'static str,
    pub background: &'static str,
    pub compose: &'static str,
    pub layout: &'static str,
    pub reason: String,
    pub has_large_background: bool,
    pub background_coverage_ratio: f64,
    pub visible_text: bool,
    pub hidden_text: bool,
    pub editable_text: bool,
    pub drawing_count: i64,
    pub vector_heavy: bool,
}

/// `RenderDocumentAnalysis` — pages keyed by page index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderDocumentAnalysis {
    pub pages: HashMap<i64, RenderPageAnalysis>,
}

/// `build_render_page_analysis(profile)` — route + profile facts.
pub fn build_render_page_analysis(profile: &RenderPageProfile) -> RenderPageAnalysis {
    let route = build_render_page_route(profile);
    RenderPageAnalysis {
        page_index: profile.geometry.page_index,
        kind: profile.kind,
        redaction: route.redaction,
        background: route.background,
        compose: route.compose,
        layout: route.layout,
        reason: route.reason,
        has_large_background: profile.image_background.has_large_background,
        background_coverage_ratio: profile.image_background.coverage_ratio,
        visible_text: profile.text_layer.has_visible_text,
        hidden_text: profile.text_layer.has_hidden_text,
        editable_text: profile.text_layer.editable,
        drawing_count: profile.vector_layer.drawing_count,
        vector_heavy: profile.vector_layer.vector_heavy,
    }
}
