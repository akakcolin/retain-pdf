//! Port of backend/scripts/services/rendering/source_cleanup/pdf/path_removal.py.
//!
//! `rect_is_text_like_fill_path` (from planning/drawing_classifier.py) is
//! inlined here; the `fitz.Rect` wrapper is replaced with
//! `rendering_core::rect::Rect`.

use super::hit_test::{RectIndex, RectTuple};
use super::pdf_math::{to_float, transform_point, Operand, PdfMatrix};
use crate::rect::Rect;

pub const PATH_CONSTRUCTION_OPERATORS: [&str; 7] = ["m", "l", "c", "v", "y", "h", "re"];
pub const PATH_PAINT_OPERATORS: [&str; 10] = ["f", "F", "f*", "B", "B*", "b", "b*", "S", "s", "n"];
pub const TEXT_LIKE_PATH_PAINT_OPERATORS: [&str; 3] = ["f", "F", "f*"];

const MAX_TEXT_LIKE_FILL_PATH_HEIGHT_PT: f64 = 32.0;
const MAX_TEXT_LIKE_FILL_PATH_AREA_PT2: f64 = 3500.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PathPaintRewriteDecision {
    pub remove: bool,
    pub rect: Option<RectTuple>,
}

/// Accumulates transformed path points to derive the path's bbox.
#[derive(Debug, Clone, Default)]
pub struct PathTracker {
    pub points: Vec<(f64, f64)>,
}

impl PathTracker {
    pub fn empty() -> Self {
        PathTracker { points: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// `record(op, operands, ctm)` — append transformed points for construction
    /// operators; `h` closes the subpath, `re` expands to its four corners.
    pub fn record(&mut self, op: &str, operands: &[Operand], ctm: &PdfMatrix) {
        if op == "h" {
            return;
        }
        if op == "re" {
            self.record_rect(operands, ctm);
            return;
        }
        for (x, y) in operator_points(op, operands) {
            self.points.push(transform_point(ctm, x, y));
        }
    }

    /// Bbox of all recorded points, or None when empty.
    pub fn rect(&self) -> Option<RectTuple> {
        if self.points.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &(x, y) in &self.points {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        Some([min_x, min_y, max_x, max_y])
    }

    /// `_record_rect(operands, ctm)` — `re` x y w h → four corners.
    fn record_rect(&mut self, operands: &[Operand], ctm: &PdfMatrix) {
        if operands.len() < 4 {
            return;
        }
        let x = to_float(&operands[0], 0.0);
        let y = to_float(&operands[1], 0.0);
        let width = to_float(&operands[2], 0.0);
        let height = to_float(&operands[3], 0.0);
        for (px, py) in [
            (x, y),
            (x + width, y),
            (x + width, y + height),
            (x, y + height),
        ] {
            self.points.push(transform_point(ctm, px, py));
        }
    }
}

/// `decide_path_paint_rewrite(op, path_rect, strip_index, protected_index)`.
pub fn decide_path_paint_rewrite(
    op: &str,
    path_rect: Option<RectTuple>,
    strip_index: &RectIndex,
    protected_index: &RectIndex,
) -> PathPaintRewriteDecision {
    if !TEXT_LIKE_PATH_PAINT_OPERATORS.contains(&op) || path_rect.is_none() {
        return PathPaintRewriteDecision {
            remove: false,
            rect: path_rect,
        };
    }
    let rect = path_rect.unwrap();
    let core_rect = Rect::new(rect[0], rect[1], rect[2], rect[3]);
    if !rect_is_text_like_fill_path(&core_rect) {
        return PathPaintRewriteDecision {
            remove: false,
            rect: path_rect,
        };
    }
    let remove = strip_index.intersects(&rect) && !protected_index.intersects(&rect);
    PathPaintRewriteDecision {
        remove,
        rect: path_rect,
    }
}

/// `rect_is_text_like_fill_path` — small, non-empty fill rects are plausible
/// glyph/path text and are candidates for removal.
pub fn rect_is_text_like_fill_path(rect: &Rect) -> bool {
    if rect.is_empty() {
        return false;
    }
    if rect.height() <= 0.0 || rect.height() > MAX_TEXT_LIKE_FILL_PATH_HEIGHT_PT {
        return false;
    }
    rect.area() <= MAX_TEXT_LIKE_FILL_PATH_AREA_PT2
}

/// `_operator_points(op, operands)` — control points for construction ops.
fn operator_points(op: &str, operands: &[Operand]) -> Vec<(f64, f64)> {
    match op {
        "m" | "l" if operands.len() >= 2 => {
            vec![(to_float(&operands[0], 0.0), to_float(&operands[1], 0.0))]
        }
        "c" if operands.len() >= 6 => vec![
            (to_float(&operands[0], 0.0), to_float(&operands[1], 0.0)),
            (to_float(&operands[2], 0.0), to_float(&operands[3], 0.0)),
            (to_float(&operands[4], 0.0), to_float(&operands[5], 0.0)),
        ],
        "v" | "y" if operands.len() >= 4 => vec![
            (to_float(&operands[0], 0.0), to_float(&operands[1], 0.0)),
            (to_float(&operands[2], 0.0), to_float(&operands[3], 0.0)),
        ],
        _ => Vec::new(),
    }
}
