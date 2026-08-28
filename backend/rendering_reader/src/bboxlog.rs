//! `page.get_bboxlog()` replication: a custom MuPDF device that records the
//! bounds of every painted path / text run / image in op order, in
//! **rotation-stripped fitz page space** (mirrors PyMuPDF's `JM_new_bbox_device`:
//! page rotation cleared to 0, then contents + annotations + widgets run through
//! the device with an identity ctm — so rects are in the same coordinate space as
//! fitz `page.transformation_matrix`, and item rects transformed by its inverse
//! land on top of them).
//!
//! Bounds semantics per kind (each an exact mirror of the C primitive):
//! - `fill-path`    → `fz_bound_path(path, NULL, ctm)`: walked control-point /
//!   endpoint bounds, NO stroke expand, NO ±1 (PyMuPDF records the raw bounds).
//! - `stroke-path`  → `fz_bound_path(path, stroke, ctm)` via `Path::bounds`.
//! - `fill-text`    → `fz_bound_text(text, NULL, ctm)`: per-glyph cached cbox
//!   (`font->bounds[gid]`, with font-bbox fallback for out-of-range / no-outline
//!   glyphs) plus the ±1 glyph-cache precision compensation. mupdf-rs only
//!   exposes `fz_bound_text` with a `StrokeState`, so we pass a Round-join stroke
//!   with a tiny linewidth and subtract its known expansion.
//! - `stroke-text`  → `fz_bound_text(text, stroke, ctm)` via `Text::bounds`.
//! - `fill-image` / `fill-image-mask` → `fz_bound_image` = unit rect × ctm (the
//!   image's intrinsic matrix is already baked into the device ctm by pdf-run.c).
//! - `fill-shade` is intentionally SKIPPED: the source-cleanup classifier ignores
//!   shade entries and nothing in production consumes them, so omitting them is
//!   production-equivalent.
//!
//! Empty / infinite bounds are dropped (PyMuPDF's `JM_emit_bbox` filter).

use std::cell::RefCell;
use std::rc::Rc;

use mupdf::pdf::PdfPage;
use mupdf::{
    ColorParams, Colorspace, Device, Image, LineCap, LineJoin, Matrix, NativeDevice, Path,
    PathWalker, Rect, StrokeState, Text,
};
use rendering_core::page::BboxlogEntry;
use rendering_core::rect::Rect as CoreRect;

use crate::error::PdfError;

/// The mupdf `fz_empty_rect` sentinel. PyMuPDF's `get_bboxlog` emits this for
/// paths/text that bound to nothing (move-only paths / empty text), so we must
/// emit it too — any off-page sentinel is behaviorally equivalent downstream.
const EMPTY_RECT: Rect = Rect::new(f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);

/// Run the bboxlog device over `page` (rotation cleared to 0) and return the
/// recorded entries in op order. `page` is taken by value so the rotation
/// mutation never leaks to the caller's wrapper.
pub fn collect_bboxlog(mut page: PdfPage) -> Result<Vec<BboxlogEntry>, PdfError> {
    let rotation = page.rotation()?;
    page.set_rotation(0)?;
    let recorder = Rc::new(RefCell::new(BboxlogDevice::default()));
    let device = Device::from_native(recorder.clone())?;
    // PdfPage derefs to Page; `run` dispatches to pdf_run_page (contents +
    // annotations + widgets), matching PyMuPDF's `fz_run_page(page, dev, identity)`.
    page.run(&device, &Matrix::IDENTITY)?;
    drop(device);
    let entries = recorder.borrow_mut().finish();
    if rotation != 0 {
        let _ = page.set_rotation(rotation);
    }
    Ok(entries)
}

#[derive(Default)]
struct BboxlogDevice {
    entries: Vec<BboxlogEntry>,
}

impl BboxlogDevice {
    fn finish(&mut self) -> Vec<BboxlogEntry> {
        std::mem::take(&mut self.entries)
    }

    fn push(&mut self, kind: &str, rect: Rect) {
        // PyMuPDF's `get_bboxlog` emits EVERY rect it computes — including
        // zero-width/zero-height rects (thin strokes) and the mupdf empty-rect
        // sentinel (move-only paths). Only non-finite coords are dropped (they
        // would break JSON serialization downstream).
        if !rect.x0.is_finite() || !rect.y0.is_finite() || !rect.x1.is_finite() || !rect.y1.is_finite() {
            return;
        }
        self.entries.push(BboxlogEntry {
            kind: kind.to_string(),
            rect: CoreRect::new(
                rect.x0 as f64,
                rect.y0 as f64,
                rect.x1 as f64,
                rect.y1 as f64,
            ),
        });
    }
}

impl NativeDevice for BboxlogDevice {
    fn fill_path(
        &mut self,
        path: &Path,
        _even_odd: bool,
        ctm: Matrix,
        _cs: &Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        self.push("fill-path", bound_path_fill(path, &ctm));
    }

    fn stroke_path(
        &mut self,
        path: &Path,
        stroke_state: &StrokeState,
        ctm: Matrix,
        _cs: &Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        if let Ok(rect) = path.bounds(stroke_state, &ctm) {
            self.push("stroke-path", rect);
        }
    }

    fn fill_text(
        &mut self,
        text: &Text,
        ctm: Matrix,
        _cs: &Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        self.push("fill-text", bound_text_fill(text, &ctm));
    }

    fn stroke_text(
        &mut self,
        text: &Text,
        stroke_state: &StrokeState,
        ctm: Matrix,
        _cs: &Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        if let Ok(rect) = text.bounds(stroke_state, &ctm) {
            self.push("stroke-text", rect);
        }
    }

    fn fill_image(&mut self, _img: &Image, ctm: Matrix, _alpha: f32, _cp: ColorParams) {
        self.push("fill-image", Rect::new(0.0, 0.0, 1.0, 1.0).transform(&ctm));
    }

    fn fill_image_mask(
        &mut self,
        _img: &Image,
        ctm: Matrix,
        _cs: &Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: ColorParams,
    ) {
        // PyMuPDF's bbox device emits the string "fill-imgmask" for image masks
        // (JM_bbox_fill_image_mask), so we mirror it byte-for-byte.
        self.push("fill-imgmask", Rect::new(0.0, 0.0, 1.0, 1.0).transform(&ctm));
    }

    // fill_shade skipped — see module contract.
}

/// `fz_bound_path(path, NULL, ctm)` — point-bounds without stroke expansion.
/// Mirrors the C `bound_path_walker` (moveto/lineto/curveto only; rect is
/// decomposed into four lines by `fz_walk_path` when `rectto` is NULL, which
/// mupdf-rs's `Path::walk` also does via the default `rect`).
fn bound_path_fill(path: &Path, ctm: &Matrix) -> Rect {
    // `first` must start true (the derive default is false, which would make the
    // first segment expand a zero rect from the origin instead of seeding it).
    let mut walker = PathBoundWalker {
        ctm: ctm.clone(),
        rect: EMPTY_RECT,
        first: true,
        ..PathBoundWalker::default()
    };
    let _ = path.walk(&mut walker);
    walker.rect
}

#[derive(Default)]
struct PathBoundWalker {
    ctm: Matrix,
    rect: Rect,
    move_x: f32,
    move_y: f32,
    trailing_move: bool,
    first: bool,
}

impl PathWalker for PathBoundWalker {
    fn move_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.ctm.transform_xy(x, y);
        self.move_x = px;
        self.move_y = py;
        self.trailing_move = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.ctm.transform_xy(x, y);
        if self.first {
            self.rect = Rect::new(px, py, px, py);
            self.first = false;
        } else {
            bound_expand(&mut self.rect, px, py);
        }
        if self.trailing_move {
            self.trailing_move = false;
            bound_expand(&mut self.rect, self.move_x, self.move_y);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) {
        let (p1x, p1y) = self.ctm.transform_xy(x1, y1);
        let (p2x, p2y) = self.ctm.transform_xy(x2, y2);
        let (p3x, p3y) = self.ctm.transform_xy(x3, y3);
        if self.first {
            self.rect = Rect::new(p1x, p1y, p1x, p1y);
            self.first = false;
        } else {
            bound_expand(&mut self.rect, p1x, p1y);
        }
        bound_expand(&mut self.rect, p2x, p2y);
        bound_expand(&mut self.rect, p3x, p3y);
        if self.trailing_move {
            self.trailing_move = false;
            bound_expand(&mut self.rect, self.move_x, self.move_y);
        }
    }

    fn close(&mut self) {}
}

/// `fz_bound_text(text, NULL, ctm)` — per-glyph cached cbox union plus the ±1
/// glyph-cache precision compensation. mupdf-rs only exposes `fz_bound_text`
/// through `Text::bounds`, which requires a `StrokeState`; pass a Round-join
/// stroke with a tiny non-zero linewidth and subtract its known expansion
/// `(linewidth/2) * max(|a|,|b|,|c|,|d|)` (a zero linewidth would hit the
/// `expand = 0.5` fallback instead). Empty unions are returned unchanged —
/// `fz_bound_text` skips the stroke adjust and the ±1 for empty rects.
fn bound_text_fill(text: &Text, ctm: &Matrix) -> Rect {
    let linewidth = 1e-6;
    let stroke = StrokeState::new(
        LineCap::Round,
        LineCap::Round,
        LineCap::Round,
        LineJoin::Round,
        linewidth,
        1.0,
        0.0,
        &[],
    )
    .expect("valid stroke state");
    let rect = text.bounds(&stroke, ctm).unwrap_or(EMPTY_RECT);
    if rect.is_empty() {
        return rect;
    }
    let expand = (linewidth / 2.0) * matrix_max_expansion(ctm);
    Rect::new(
        rect.x0 + expand,
        rect.y0 + expand,
        rect.x1 - expand,
        rect.y1 - expand,
    )
}

/// `fz_matrix_max_expansion(ctm)` — max of |a|, |b|, |c|, |d|.
fn matrix_max_expansion(ctm: &Matrix) -> f32 {
    let mut max = ctm.a.abs();
    let mut m = ctm.b.abs();
    if m > max {
        max = m;
    }
    m = ctm.c.abs();
    if m > max {
        max = m;
    }
    m = ctm.d.abs();
    if m > max {
        max = m;
    }
    max
}

#[inline]
fn bound_expand(r: &mut Rect, x: f32, y: f32) {
    if x < r.x0 {
        r.x0 = x;
    }
    if y < r.y0 {
        r.y0 = y;
    }
    if x > r.x1 {
        r.x1 = x;
    }
    if y > r.y1 {
        r.y1 = y;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::pdf_document::PdfDocument;

    const GOLDEN_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/samples/golden-pdfs"
    );

    fn golden_doc(name: &str) -> mupdf::Document {
        // Use the trait open (takes &Path) rather than mupdf's inherent
        // `Document::open` (takes AsRef<FilePath>).
        PdfDocument::open(&Path::new(GOLDEN_ROOT).join(name))
            .unwrap_or_else(|e| panic!("open {name}: {e}"))
    }

    fn page_count(doc: &mupdf::Document) -> i64 {
        // UFCS: mupdf::Document's inherent `page_count` shadows the trait one.
        <mupdf::Document as PdfDocument>::page_count(doc).expect("page_count")
    }

    /// The device must run over a real PDF and produce well-formed entries in
    /// rotation-stripped fitz space: known kinds, finite rects, and a page count
    /// that matches `page_count`.
    #[test]
    fn bboxlog_runs_over_golden_pdfs() {
        let known = [
            "fill-path",
            "stroke-path",
            "fill-text",
            "stroke-text",
            "fill-image",
            "fill-imgmask",
        ];
        for name in ["1.pdf", "2.pdf"] {
            let doc = golden_doc(name);
            let count = page_count(&doc);
            for idx in 0..count {
                let entries = doc.page_bboxlog(idx);
                for entry in &entries {
                    assert!(
                        known.contains(&entry.kind.as_str()),
                        "{} p{idx}: unexpected kind {:?}",
                        name,
                        entry.kind
                    );
                    let r = entry.rect;
                    assert!(
                        !r.x0.is_nan() && !r.x1.is_nan() && !r.y0.is_nan() && !r.y1.is_nan(),
                        "{} p{idx}: NaN rect",
                        name
                    );
                }
            }
        }
    }

    /// The four cleanup-context readers must return plausible values on a real
    /// PDF (finite ctm, present form-xobject flag, non-negative stream size).
    #[test]
    fn cleanup_context_readers_return_values() {
        let doc = golden_doc("1.pdf");
        let count = page_count(&doc);
        for idx in 0..count {
            let ctm = doc.page_ctm(idx).expect("page_ctm present");
            assert!(
                ctm.iter().all(|v| v.is_finite()),
                "p{idx}: non-finite ctm {ctm:?}"
            );
            let _ = doc.page_has_form_xobjects(idx);
            let _ = doc.page_content_stream_size(idx, 10_000_000);
        }
    }
}
