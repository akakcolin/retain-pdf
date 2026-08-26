// Data-shape layer mirroring the fitz.Page surface used by
// services/rendering/analysis/profile/*. Collectors consume a PageSnapshot
// instead of a live fitz.Page; a real reader (mupdf-rs) populates it in Phase 4/5.

use crate::rect::Rect;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextTrace {
    pub trace_type: i64,
    pub opacity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub xref: i64,
    pub bbox: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageSnapshot {
    pub number: i64,
    pub rotation: i64,
    pub rect: Rect,
    pub cropbox: Rect,
    pub text_traces: Vec<TextTrace>,
    pub word_count: i64,
    pub drawing_count: i64,
    pub image_infos: Vec<ImageInfo>,
    pub image_entries: Vec<i64>,
    pub image_rects: HashMap<i64, Vec<Rect>>,
}
