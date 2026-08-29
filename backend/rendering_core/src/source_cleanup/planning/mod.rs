//! Ports of `backend/scripts/services/rendering/source_cleanup/planning/`
//! pure logic (the candidates assembly). Page geometry comes from
//! `PlanningPageContext` (built by the bridge with mupdf-rs); translated /
//! protected item dicts are consumed as `serde_json::Value` mirrors of the
//! Python dicts. The top-level entry is `planner::plan_source_cleanup`.

pub mod accumulator;
pub mod coordinate_resolver;
pub mod drawing_classifier;
pub mod evidence;
pub mod formula_classifier;
pub mod geometry;
pub mod intent_classifier;
pub mod item_classifier;
pub mod mixed_content;
pub mod page_features;
pub mod page_gate;
pub mod planner;
pub mod policy;
pub mod rect_filter;
pub mod rect_ops;
pub mod segments;
pub mod spatial_index;

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::rect::{Matrix, Rect};

pub const BBOX_TEXT_STRIP_PAGE_SKIP_NONE: &str = "none";
pub const BBOX_TEXT_STRIP_PAGE_SKIP_COMPLEX: &str = "complex";
pub const BBOX_TEXT_STRIP_PAGE_SKIP_NO_TEXT_OVERLAP: &str = "no_text_overlap";
pub const BBOX_TEXT_STRIP_PAGE_SKIP_VISUAL_BACKGROUND: &str = "visual_background";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    None,
    Complex,
    NoTextOverlap,
    VisualBackground,
}

impl Default for SkipReason {
    fn default() -> Self {
        SkipReason::None
    }
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::None => BBOX_TEXT_STRIP_PAGE_SKIP_NONE,
            SkipReason::Complex => BBOX_TEXT_STRIP_PAGE_SKIP_COMPLEX,
            SkipReason::NoTextOverlap => BBOX_TEXT_STRIP_PAGE_SKIP_NO_TEXT_OVERLAP,
            SkipReason::VisualBackground => BBOX_TEXT_STRIP_PAGE_SKIP_VISUAL_BACKGROUND,
        }
    }
}

/// Per-page context mirroring `planning/page_context.py::PlanningPageContext`.
#[derive(Debug, Clone)]
pub struct PlanningPageContext {
    pub page_index: i64,
    pub page_rect: Rect,
    pub bboxlog_entries: Vec<(String, Rect)>,
    pub content_stream_size: u64,
    pub has_form_xobjects: bool,
    pub inverse_ctm: Matrix,
}

/// Per-page cleanup features (content-stream size + form-xobject flag),
/// mirroring `planning/page_features.py::PageCleanupFeatures`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageCleanupFeatures {
    pub content_stream_size: u64,
    pub has_form_xobjects: bool,
}

impl PageCleanupFeatures {
    pub fn to_manifest(&self) -> Value {
        serde_json::json!({
            "content_stream_size": self.content_stream_size,
            "has_form_xobjects": self.has_form_xobjects,
        })
    }
}

/// Per-page plan mirroring `types.py::BBoxTextStripPagePlan`.
#[derive(Debug, Clone, Default)]
pub struct BBoxTextStripPagePlan {
    pub strip_rects: Vec<Rect>,
    pub protected_rects: Vec<Rect>,
    pub skip_reason: SkipReason,
    pub uncovered_unsafe_vector_item_ids: Vec<String>,
}

// ---- item JSON helpers (mirror the Python dict accessors) ----

/// `str(item.get(key) or "").strip()`.
pub fn item_str(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// First non-empty `str(item.get(key) or "").strip()` across `keys`, in order.
pub fn item_first_str(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        let value = item_str(item, key);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

/// `item.get("bbox")` as a `Vec<Value>`; empty when absent / not an array.
pub fn item_bbox(item: &Value) -> Vec<Value> {
    match item.get("bbox") {
        Some(Value::Array(values)) => values.clone(),
        _ => Vec::new(),
    }
}

/// `item.get("bbox")` coerced to `Vec<f64>` (non-numeric → 0.0), mirroring the
/// `float(value)` coercion in `raw_bbox_rect`.
pub fn item_bbox_f64(item: &Value) -> Vec<f64> {
    item_bbox(item).iter().map(|value| value.as_f64().unwrap_or(0.0)).collect()
}

/// `item.get("lines")` as a `Vec<Value>`; empty when absent / not an array.
pub fn item_lines(item: &Value) -> Vec<Value> {
    match item.get("lines") {
        Some(Value::Array(values)) => values.clone(),
        _ => Vec::new(),
    }
}

/// `item.get("tags")` as a `Vec<String>` (lowercased, trimmed).
pub fn item_tags(item: &Value) -> Vec<String> {
    match item.get("tags") {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| item_str_from(value))
            .filter(|value| !value.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn item_str_from(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_lowercase(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// `item.get("lines")` spans: flatten `line["spans"]` plus `line` itself for
/// role probing (mirrors `mixed_content.lines_have_formula_spans`).
pub fn item_role_value(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        let s = item_str(value, key);
        if !s.is_empty() {
            return s.to_lowercase();
        }
    }
    String::new()
}

/// `block_kind(item)` from `services/document_schema/semantics.py`.
pub fn item_block_kind(item: &Value) -> String {
    let explicit = item_str(item, "block_kind");
    if !explicit.is_empty() {
        return explicit.to_lowercase();
    }
    let block_type = item_str(item, "block_type");
    if !block_type.is_empty() {
        return block_type.to_lowercase();
    }
    "unknown".to_string()
}

/// `item_rect(item)` from `services/rendering/policy/geometry.py` — bbox of
/// len 4 → non-empty `Rect`, else `None`.
pub fn item_rect(item: &Value) -> Option<Rect> {
    let bbox = item_bbox(item);
    if bbox.len() != 4 {
        return None;
    }
    let mut coords = [0.0f64; 4];
    for (index, value) in bbox.iter().enumerate() {
        coords[index] = value.as_f64().unwrap_or(0.0);
    }
    let rect = Rect::new(coords[0], coords[1], coords[2], coords[3]);
    if rect.is_empty() {
        None
    } else {
        Some(rect)
    }
}

pub type PageContexts = BTreeMap<i64, PlanningPageContext>;
pub type TranslatedPages = BTreeMap<i64, Vec<Value>>;
