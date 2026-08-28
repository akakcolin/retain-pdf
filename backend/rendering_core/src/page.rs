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

/// One `/Resources/XObject` entry that carries a `/BBox` (Form xobjects always
/// do; plain images usually do not): the resource name, the referenced xref,
/// and the Form bounds. Mirrors what `sampler._form_xobject_objects` extracts
/// from fitz `page.get_xobjects()` for single-level forms (nested
/// form-invokes-form instances are a documented divergence — the resource dict
/// carries one entry per named form while fitz reports each `Do` instance).
#[derive(Debug, Clone, PartialEq)]
pub struct FormXObjectInfo {
    pub name: String,
    pub xref: i64,
    pub bbox: Rect,
}

/// One `page.get_bboxlog()` entry: the drawing kind ("fill-text",
/// "fill-path", "stroke-path", "fill-image", "fill-image-mask") and its
/// rotation-stripped fitz-space bounds.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BboxlogEntry {
    pub kind: String,
    pub rect: Rect,
}

/// A vector drawing's paint operation, mirroring fitz `get_cdrawings` `type`
/// ("f"/"s"/"fs").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageDrawingType {
    Fill,
    Stroke,
    FillStroke,
}

impl PageDrawingType {
    /// fitz `get_cdrawings` type strings.
    pub fn as_fitz_str(self) -> &'static str {
        match self {
            PageDrawingType::Fill => "f",
            PageDrawingType::Stroke => "s",
            PageDrawingType::FillStroke => "fs",
        }
    }
}

/// One fitz `get_cdrawings` drawing: path bounds, paint type, and stroke width
/// (`line_width * path_factor`, None for fills). Fill/color/items are
/// deliberately NOT carried: mupdf-rs converts fills to DeviceRGB while fitz
/// preserves the drawing's colorspace, and no B2-7 consumer reads them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDrawing {
    pub rect: Rect,
    pub drawing_type: PageDrawingType,
    pub width: Option<f32>,
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
