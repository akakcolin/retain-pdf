//! `page.get_image_info()` placement replication: a custom MuPDF device that
//! records the bounds of every painted image in **rotation-stripped fitz page
//! space** — the same device-run mirror as `bboxlog::collect_bboxlog`
//! (rotation cleared to 0, then contents + annotations + widgets run through the
//! device with an identity ctm).
//!
//! Semantics vs fitz `get_image_info(hashes=False)`:
//! - Both `fill-image` and `fill-image-mask` placements are captured: PyMuPDF's
//!   image-info device reports image-mask placements too (verified on golden
//!   page 1.pdf p6, where the mask is emitted as a distinct info entry), so
//!   `fill_image_mask` is implemented alongside `fill_image` — contrast
//!   `bboxlog`, which keeps them as separate kinds.
//! - Each placement bound is `fz_bound_image` = unit rect × ctm (the image's
//!   intrinsic matrix is already baked into the device ctm by pdf-run.c) — the
//!   same value PyMuPDF reports, so a page's placement rect set matches
//!   `get_image_info` bboxes.
//! - Non-finite bounds are dropped (mirrors the `JM_emit_bbox` filter; also keeps
//!   JSON serialization downstream honest). Empty rects are kept — PyMuPDF
//!   records every placement, including zero-area ones; the Python consumer
//!   intersects with the page rect and drops empty results.
//!
//! `rotation-stripped` matters: on a rotated page `get_image_info` reports
//! CONTENT-space bboxes (the image's placement before /Rotate is applied), so
//! the rotation must be cleared before the run and restored afterwards.

use std::cell::RefCell;
use std::rc::Rc;

use mupdf::pdf::PdfPage;
use mupdf::{ColorParams, Colorspace, Device, Image, Matrix, NativeDevice, Rect};
use rendering_core::rect::Rect as CoreRect;

use crate::error::PdfError;

/// Run the image-placement device over `page` (rotation cleared to 0) and
/// return the placement rects in paint order. `page` is taken by value so the
/// rotation mutation never leaks to the caller's wrapper.
pub fn collect_page_image_rects(mut page: PdfPage) -> Result<Vec<CoreRect>, PdfError> {
    let rotation = page.rotation()?;
    page.set_rotation(0)?;
    let recorder = Rc::new(RefCell::new(ImagePlacementDevice::default()));
    let device = Device::from_native(recorder.clone())?;
    page.run(&device, &Matrix::IDENTITY)?;
    drop(device);
    let rects = recorder.borrow_mut().finish();
    if rotation != 0 {
        let _ = page.set_rotation(rotation);
    }
    Ok(rects)
}

#[derive(Default)]
struct ImagePlacementDevice {
    rects: Vec<CoreRect>,
}

impl ImagePlacementDevice {
    fn finish(&mut self) -> Vec<CoreRect> {
        std::mem::take(&mut self.rects)
    }

    fn push(&mut self, rect: Rect) {
        // PyMuPDF's `get_image_info` records every placement bound it computes;
        // only non-finite coords are dropped (they would break downstream
        // intersection / JSON serialization).
        if !rect.x0.is_finite() || !rect.y0.is_finite() || !rect.x1.is_finite() || !rect.y1.is_finite() {
            return;
        }
        self.rects.push(CoreRect::new(
            rect.x0 as f64,
            rect.y0 as f64,
            rect.x1 as f64,
            rect.y1 as f64,
        ));
    }
}

impl NativeDevice for ImagePlacementDevice {
    fn fill_image(&mut self, _img: &Image, ctm: Matrix, _alpha: f32, _cp: ColorParams) {
        self.push(Rect::new(0.0, 0.0, 1.0, 1.0).transform(&ctm));
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
        // PyMuPDF's image-info device reports image-mask placements too (the
        // mask's own `fz_bound_image`); same unit-rect × ctm bound.
        self.push(Rect::new(0.0, 0.0, 1.0, 1.0).transform(&ctm));
    }
}
