//! Redaction diagnostics, port of `source/cleanup/diagnostics.py` and
//! `empty_result.py`. Serialized with every field present (defaults for the
//! absent ones) so the corpus and the Rust side compare the same shape.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionDiagnostics {
    pub items: usize,
    pub raw_removable_rects: usize,
    pub merged_removable_rects: usize,
    pub cover_rects: usize,
    pub fast_page_cover_only: bool,
    pub item_fast_cover_count: usize,
    pub route: String,
    pub strategy: String,
    pub uses_pymupdf_redaction: bool,
    pub legacy_pdf_write_reason: String,
    pub visual_profile_cover_rects: usize,
    pub auto_text_cleanup_math_protected: bool,
    pub auto_text_cleanup_items_skipped: usize,
}

impl RedactionDiagnostics {
    pub fn is_empty_route(&self) -> bool {
        self.route == "empty"
    }

    pub fn is_deferred(&self) -> bool {
        self.route == "deferred_text_layer_only"
    }
}

/// `new_redaction_diagnostics(valid_items)` — the initial dict shape every
/// executor starts from.
pub fn new_redaction_diagnostics(items: usize) -> RedactionDiagnostics {
    RedactionDiagnostics {
        items,
        raw_removable_rects: 0,
        merged_removable_rects: 0,
        cover_rects: 0,
        fast_page_cover_only: false,
        item_fast_cover_count: 0,
        route: String::new(),
        strategy: String::new(),
        uses_pymupdf_redaction: false,
        legacy_pdf_write_reason: String::new(),
        visual_profile_cover_rects: 0,
        auto_text_cleanup_math_protected: false,
        auto_text_cleanup_items_skipped: 0,
    }
}

/// `new_empty_redaction_result(strategy)`.
pub fn new_empty_redaction_result(strategy: Option<&str>) -> RedactionDiagnostics {
    RedactionDiagnostics {
        items: 0,
        raw_removable_rects: 0,
        merged_removable_rects: 0,
        cover_rects: 0,
        fast_page_cover_only: false,
        item_fast_cover_count: 0,
        route: "empty".to_string(),
        strategy: strategy.unwrap_or("auto").to_string(),
        uses_pymupdf_redaction: false,
        legacy_pdf_write_reason: String::new(),
        visual_profile_cover_rects: 0,
        auto_text_cleanup_math_protected: false,
        auto_text_cleanup_items_skipped: 0,
    }
}
