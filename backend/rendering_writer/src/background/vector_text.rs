//! Port of `source/vector_text.py::collect_vector_text_rects` — the vector-text
//! glyph detector that flips `redact_source_text_areas` into cover-only.
//!
//! Reproduces fitz `get_cdrawings` semantics with a `NativeDevice` over the
//! page display list. Each `fill_path`/`stroke_path` is walked into an item
//! count and a bounds rect:
//!   * item count: `move=0, line=1, curve=1, rect=1, close=(1 only when not
//!     line/rect-paired)`, matching fitz's closed-polyline probe (the `h`
//!     LINETOCLOSE emits `lineto(start)` as a counted `l` item and the trailing
//!     `closepath` contributes nothing);
//!   * bounds: an exact port of MuPDF `fz_bound_path(path, NULL, ctm)`
//!     (`trailing_move` flushed by the first line/curve, curves bounded by
//!     their three control points, `re` expanded as moveto+3 lineto).
//! A same-path fill+stroke pair (identical walked signature + ctm) merges into
//! the rejected type "fs"; everything else emits as `f`/`s`. The classifier
//! then runs the small-glyph / large-black-cluster rules over the emitted
//! drawings with the `RectOverlapIndex`.
//!
//! The display list is created from page contents, so its stored per-path ctm
//! already includes the page transform (`pdf_page_obj_transform_box`: y-flip +
//! rotation + cropbox origin, top-left origin, y down). Running the list with
//! the identity ctm therefore hands the device a ctm equal to that page
//! transform (composed with any content `cm`), and the walked bounds land in
//! fitz's top-left (y-down) page coordinate space.
//!
//! Divergences (documented): gray (`n==1`) fills are rejected here (`len != 3`)
//! while fitz reports a converted 3-tuple and may qualify; stroke-then-fill
//! same-path pairs are not merged (only fill-then-stroke). The corpus uses only
//! RGB fills and fill-then-stroke content, so neither triggers a diff.

use std::cell::RefCell;
use std::rc::Rc;

use mupdf::{
    ColorParams, Colorspace, Device, Document, Error, Matrix, NativeDevice, Path, Rect,
    StrokeState,
};

use rendering_core::source_cleanup::hit_test::RectTuple;

/// `vector_text.py::MAX_GLYPH_HEIGHT_PT` — small glyphs taller than this are
/// rejected (large black clusters have no height limit).
pub const MAX_GLYPH_HEIGHT_PT: f32 = 20.0;
/// `vector_text.py::MIN_GLYPH_ITEM_COUNT` — min path items for a small glyph.
pub const MIN_GLYPH_ITEM_COUNT: usize = 8;
/// `vector_text.py::MIN_LARGE_TEXT_CLUSTER_ITEM_COUNT` — min items for a large
/// black-text cluster (no height limit).
pub const MIN_LARGE_TEXT_CLUSTER_ITEM_COUNT: usize = 400;
/// `vector_text.py::MIN_BLACK_FILL` — max fill component for "black".
pub const MIN_BLACK_FILL: f32 = 0.2;

/// One walked path op, kept for fill+stroke content-equality pairing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SigOp {
    Move(f32, f32),
    Line(f32, f32),
    Curve(f32, f32, f32, f32, f32, f32),
    Close,
    Rect(f32, f32, f32, f32),
}

/// fitz `get_cdrawings` item-count + bounds walker. `rect` expands exactly like
/// MuPDF's `bound_path_walker` (moveto + 3 lineto), since mupdf-rs routes `re`
/// to the `rect` callback rather than the C moveto/lineto fallback.
struct PathStats {
    ctm: Matrix,
    items: usize,
    sig: Vec<SigOp>,
    bounds: Rect,
    has_bounds: bool,
    trailing_move: bool,
    move_point: (f32, f32),
    current: (f32, f32),
}

impl PathStats {
    fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            items: 0,
            sig: Vec::new(),
            bounds: Rect::new(0.0, 0.0, -1.0, -1.0),
            has_bounds: false,
            trailing_move: false,
            move_point: (0.0, 0.0),
            current: (0.0, 0.0),
        }
    }

    #[inline]
    fn tx(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.ctm;
        (m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f)
    }

    fn bound_moveto(&mut self, x: f32, y: f32) {
        self.move_point = self.tx(x, y);
        self.trailing_move = true;
    }

    fn bound_lineto(&mut self, x: f32, y: f32) {
        let (px, py) = self.tx(x, y);
        if self.has_bounds {
            self.expand(px, py);
        } else {
            self.bounds = Rect::new(px, py, px, py);
            self.has_bounds = true;
        }
        if self.trailing_move {
            self.trailing_move = false;
            self.expand(self.move_point.0, self.move_point.1);
        }
    }

    fn bound_curveto(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, ex: f32, ey: f32) {
        let (px, py) = self.tx(c1x, c1y);
        if self.has_bounds {
            self.expand(px, py);
        } else {
            self.bounds = Rect::new(px, py, px, py);
            self.has_bounds = true;
        }
        let (q1x, q1y) = self.tx(c2x, c2y);
        self.expand(q1x, q1y);
        let (q2x, q2y) = self.tx(ex, ey);
        self.expand(q2x, q2y);
        if self.trailing_move {
            self.trailing_move = false;
            self.expand(self.move_point.0, self.move_point.1);
        }
    }

    fn expand(&mut self, px: f32, py: f32) {
        if px < self.bounds.x0 {
            self.bounds.x0 = px;
        }
        if py < self.bounds.y0 {
            self.bounds.y0 = py;
        }
        if px > self.bounds.x1 {
            self.bounds.x1 = px;
        }
        if py > self.bounds.y1 {
            self.bounds.y1 = py;
        }
    }
}

impl mupdf::PathWalker for PathStats {
    fn move_to(&mut self, x: f32, y: f32) {
        self.sig.push(SigOp::Move(x, y));
        self.bound_moveto(x, y);
        self.current = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.sig.push(SigOp::Line(x, y));
        self.items += 1;
        self.bound_lineto(x, y);
        self.current = (x, y);
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, ex: f32, ey: f32) {
        self.sig.push(SigOp::Curve(c1x, c1y, c2x, c2y, ex, ey));
        self.items += 1;
        self.bound_curveto(c1x, c1y, c2x, c2y, ex, ey);
        self.current = (ex, ey);
    }

    fn close(&mut self) {
        // fitz adds a synthetic closing line item only when the close does not
        // follow a line/rect: LINETOCLOSE already emitted the closing lineto
        // (counted above) and RECTTO implies close.
        let covered = matches!(self.sig.last(), Some(SigOp::Line(..)) | Some(SigOp::Rect(..)));
        self.sig.push(SigOp::Close);
        if !covered {
            self.items += 1;
        }
    }

    fn curve_to_y(&mut self, cx: f32, cy: f32, ex: f32, ey: f32) {
        // MuPDF's no-curvetoy fallback emits curveto(current, cx, cy, ex, ey).
        let (sx, sy) = self.current;
        self.sig.push(SigOp::Curve(sx, sy, cx, cy, ex, ey));
        self.items += 1;
        self.bound_curveto(sx, sy, cx, cy, ex, ey);
        self.current = (ex, ey);
    }

    fn rect(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.sig.push(SigOp::Rect(x1, y1, x2, y2));
        self.items += 1;
        // bound_path_walker has rectto=NULL, so `re` falls back to
        // moveto(x1,y1) + lineto(x2,y1) + lineto(x2,y2) + lineto(x1,y2).
        self.bound_moveto(x1, y1);
        self.bound_lineto(x2, y1);
        self.bound_lineto(x2, y2);
        self.bound_lineto(x1, y2);
        self.current = (x1, y1);
    }
}

/// Only fill (`F`) qualifies; stroke-only and merged fill+stroke pairs are both
/// rejected by the classifier, so they collapse into `S`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DrawType {
    F,
    S,
}

struct Drawing {
    typ: DrawType,
    fill: Option<[f32; 3]>,
    rect: Rect,
    items: usize,
    sig: Vec<SigOp>,
    ctm: Matrix,
}

#[derive(Default)]
struct Collector {
    pending_fill: Option<Drawing>,
    drawings: Vec<Drawing>,
}

impl Collector {
    fn from_path(
        &self,
        path: &Path,
        ctm: Matrix,
        color: &[f32],
        cs: &Colorspace,
        stroke: bool,
    ) -> Drawing {
        let mut stats = PathStats::new(ctm.clone());
        let _ = path.walk(&mut stats);
        let fill = if cs.n() == 3 {
            Some([color[0], color[1], color[2]])
        } else {
            None
        };
        Drawing {
            typ: if stroke { DrawType::S } else { DrawType::F },
            fill,
            rect: stats.bounds,
            items: stats.items,
            sig: stats.sig,
            ctm,
        }
    }

    fn flush_pending(&mut self) {
        if let Some(d) = self.pending_fill.take() {
            self.drawings.push(d);
        }
    }
}

impl NativeDevice for Collector {
    fn fill_path(
        &mut self,
        path: &Path,
        _even_odd: bool,
        cmt: Matrix,
        color_space: &Colorspace,
        color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        let d = self.from_path(path, cmt.clone(), color, color_space, false);
        self.flush_pending();
        self.pending_fill = Some(d);
    }

    fn stroke_path(
        &mut self,
        path: &Path,
        _stroke_state: &StrokeState,
        cmt: Matrix,
        color_space: &Colorspace,
        color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        let d = self.from_path(path, cmt, color, color_space, true);
        let paired = self
            .pending_fill
            .as_ref()
            .map_or(false, |p| p.sig == d.sig && p.ctm == d.ctm);
        if paired {
            // Same-path fill+stroke → type "fs", always rejected: discard both.
            self.pending_fill = None;
        } else {
            self.flush_pending();
            self.drawings.push(d);
        }
    }

    fn close_device(&mut self) {
        self.flush_pending();
    }
}

/// `spatial_index.py::RectOverlapIndex` over f32 rects (targets are f64 tuples).
struct RectOverlapIndex {
    rects: Vec<Rect>,
    y0_sorted: Vec<f32>,
}

impl RectOverlapIndex {
    fn build(targets: &[RectTuple]) -> Self {
        let mut rects: Vec<Rect> = targets
            .iter()
            .filter(|r| r[0] < r[2] && r[1] < r[3])
            .map(|r| Rect::new(r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32))
            .collect();
        rects.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        let y0_sorted = rects.iter().map(|r| r.y0).collect();
        Self { rects, y0_sorted }
    }

    fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    fn overlaps_any(&self, target: &Rect) -> bool {
        if target.is_empty() || self.rects.is_empty() {
            return false;
        }
        // bisect_right(y0_sorted, target.y1)
        let limit = self.y0_sorted.partition_point(|&y| y <= target.y1);
        for rect in self.rects.iter().take(limit) {
            if rect.y1 < target.y0 {
                continue;
            }
            if rect.x1 < target.x0 || rect.x0 > target.x1 {
                continue;
            }
            let inter = rect.intersect(target);
            if inter.width() > 0.0 && inter.height() > 0.0 {
                return true;
            }
        }
        false
    }
}

fn looks_like_black_filled_glyph(d: &Drawing) -> bool {
    if d.typ != DrawType::F {
        return false;
    }
    let fill = match d.fill {
        Some(f) => f,
        None => return false,
    };
    if fill.iter().copied().fold(f32::NEG_INFINITY, f32::max) > MIN_BLACK_FILL {
        return false;
    }
    if d.rect.is_empty() || d.rect.height() > MAX_GLYPH_HEIGHT_PT {
        return false;
    }
    d.items >= MIN_GLYPH_ITEM_COUNT
}

fn looks_like_large_black_text_cluster(d: &Drawing) -> bool {
    if d.typ != DrawType::F {
        return false;
    }
    let fill = match d.fill {
        Some(f) => f,
        None => return false,
    };
    if fill.iter().copied().fold(f32::NEG_INFINITY, f32::max) > MIN_BLACK_FILL {
        return false;
    }
    if d.rect.is_empty() {
        return false;
    }
    d.items >= MIN_LARGE_TEXT_CLUSTER_ITEM_COUNT
}

fn first_target_intersection(draw_rect: &Rect, target_rects: &[RectTuple]) -> RectTuple {
    for t in target_rects {
        let tr = Rect::new(t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32);
        let inter = draw_rect.intersect(&tr);
        if !inter.is_empty() {
            return [inter.x0 as f64, inter.y0 as f64, inter.x1 as f64, inter.y1 as f64];
        }
    }
    [draw_rect.x0 as f64, draw_rect.y0 as f64, draw_rect.x1 as f64, draw_rect.y1 as f64]
}

fn classify(drawings: &[Drawing], index: &RectOverlapIndex, target_rects: &[RectTuple]) -> Vec<RectTuple> {
    let mut out = Vec::new();
    for d in drawings {
        let small = looks_like_black_filled_glyph(d);
        let large = looks_like_large_black_text_cluster(d);
        if !small && !large {
            continue;
        }
        if !index.overlaps_any(&d.rect) {
            continue;
        }
        if small {
            out.push([
                d.rect.x0 as f64,
                d.rect.y0 as f64,
                d.rect.x1 as f64,
                d.rect.y1 as f64,
            ]);
        } else {
            out.push(first_target_intersection(&d.rect, target_rects));
        }
    }
    out
}

/// `vector_text.py::collect_vector_text_rects` — run the page display list with
/// a collector device under the identity ctm (the stored per-path ctm already
/// carries the page transform) and classify the drawings.
///
/// Errors mirror Python's `except Exception: return []` (any failure → empty).
pub fn collect_vector_text_rects(
    source_doc: &Document,
    page_index: i32,
    target_rects: &[RectTuple],
) -> Vec<RectTuple> {
    let run = (|| -> Result<Vec<RectTuple>, Error> {
        let index = RectOverlapIndex::build(target_rects);
        if index.is_empty() {
            return Ok(Vec::new());
        }
        let page = source_doc.load_page(page_index)?;
        let list = page.to_display_list(false)?;
        let collector = Rc::new(RefCell::new(Collector::default()));
        let dev = Device::from_native(collector.clone())?;
        let area = page.bounds()?;
        list.run(&dev, &Matrix::IDENTITY, area)?;
        drop(dev);
        let collector = collector.borrow();
        Ok(classify(&collector.drawings, &index, target_rects))
    })();
    run.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupdf::PathWalker;

    fn rgb_fill_drawing(items: usize, height: f32, fill: [f32; 3]) -> Drawing {
        Drawing {
            typ: DrawType::F,
            fill: Some(fill),
            rect: Rect::new(10.0, 10.0, 20.0, 10.0 + height),
            items,
            sig: Vec::new(),
            ctm: Matrix::IDENTITY,
        }
    }

    #[test]
    fn small_glyph_pass_thresholds() {
        assert!(looks_like_black_filled_glyph(&rgb_fill_drawing(8, 10.0, [0.1, 0.1, 0.1])));
        assert!(!looks_like_black_filled_glyph(&rgb_fill_drawing(7, 10.0, [0.1, 0.1, 0.1])));
        assert!(!looks_like_black_filled_glyph(&rgb_fill_drawing(8, 21.0, [0.1, 0.1, 0.1])));
        assert!(!looks_like_black_filled_glyph(&rgb_fill_drawing(8, 10.0, [0.5, 0.1, 0.1])));
    }

    #[test]
    fn stroke_and_fill_stroke_rejected() {
        let stroke = Drawing {
            typ: DrawType::S,
            ..rgb_fill_drawing(8, 10.0, [0.1, 0.1, 0.1])
        };
        assert!(!looks_like_black_filled_glyph(&stroke));
        assert!(!looks_like_large_black_text_cluster(&stroke));
    }

    #[test]
    fn large_cluster_ignores_height_limit() {
        let cluster = rgb_fill_drawing(400, 60.0, [0.05, 0.05, 0.05]);
        assert!(looks_like_large_black_text_cluster(&cluster));
        assert!(!looks_like_large_black_text_cluster(&rgb_fill_drawing(399, 5.0, [0.05, 0.05, 0.05])));
        assert!(!looks_like_large_black_text_cluster(&rgb_fill_drawing(400, 5.0, [0.5, 0.05, 0.05])));
    }

    #[test]
    fn close_counts_only_after_non_line() {
        let mut stats = PathStats::new(Matrix::IDENTITY);
        stats.move_to(0.0, 0.0);
        stats.line_to(10.0, 0.0);
        stats.close();
        assert_eq!(stats.items, 1); // line 1 + (covered close 0)

        let mut stats = PathStats::new(Matrix::IDENTITY);
        stats.move_to(0.0, 0.0);
        stats.curve_to(1.0, 1.0, 2.0, 2.0, 3.0, 3.0);
        stats.close();
        assert_eq!(stats.items, 2); // curve 1 + uncovered close 1
    }

    #[test]
    fn overlap_index_matches_bisect_semantics() {
        let targets: Vec<RectTuple> = vec![[205.0, 97.0, 250.0, 103.0], [300.0, 90.0, 400.0, 200.0]];
        let index = RectOverlapIndex::build(&targets);
        assert!(!index.is_empty());
        assert!(index.overlaps_any(&Rect::new(200.0, 95.0, 260.0, 105.0)));
        assert!(!index.overlaps_any(&Rect::new(400.0, 500.0, 500.0, 550.0)));
        // y0-sorted order is preserved (target 1 has y0 90 < 97).
        assert_eq!(index.rects[0].y0, 90.0);
    }
}
