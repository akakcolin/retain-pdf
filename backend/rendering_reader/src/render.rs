//! Clipped rendering entry points over `PdfDocument`, plus the display-list
//! leaf renderer.
//!
//! The page-clip render logic lives in `PdfDocument::render_page_clip_gray` /
//! `render_page_clip_rgb` (see `pdf_document.rs`); these generic wrappers keep
//! the pre-trait free-function API working for existing callers and tests
//! without naming a mupdf type. Phase B2 deletes the wrappers once production
//! fitz call sites migrate onto the trait.
//!
//! `render_display_list_clip_gray` stays a free function (leaf op needing the
//! concrete `mupdf::DisplayList`) and is deliberately outside the trait.

use mupdf::{Colorspace, Device, Matrix, Pixmap};
use rendering_core::rect::Rect;

use crate::error::PdfError;
use crate::pdf_document::{to_mupdf_rect, PdfDocument};

/// Rendered grayscale pixels, 1 byte per pixel, row-major, white (255) base.
pub struct RenderedPixels {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<u8>,
}

/// Rendered RGB pixels (3 bytes per pixel, row-major, white base).
pub struct RenderedRgbPixels {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub samples: Vec<u8>,
}

fn empty() -> RenderedPixels {
    RenderedPixels {
        width: 0,
        height: 0,
        samples: Vec::new(),
    }
}

/// Clipped device-gray page render via `PdfDocument`. `clip` is page space;
/// `None` renders the full page.
pub fn render_page_clip_gray<T: PdfDocument>(
    doc: &T,
    page_idx: i32,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedPixels, PdfError> {
    doc.render_page_clip_gray(page_idx as i64, clip, scale)
}

/// Clipped device-RGB page render via `PdfDocument`. `clip` is page space;
/// `None` renders the full page.
pub fn render_page_clip_rgb<T: PdfDocument>(
    doc: &T,
    page_idx: i32,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedRgbPixels, PdfError> {
    doc.render_page_clip_rgb(page_idx as i64, clip, scale)
}

/// Render a display list clipped to `clip` (page space; intersected with the
/// display list bounds), scaled by `scale`, into device-gray pixels. Mirrors
/// `fitz.DisplayList.get_pixmap(..., clip=clip)`.
pub fn render_display_list_clip_gray(
    dl: &mupdf::DisplayList,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedPixels, PdfError> {
    let bounds = dl.bounds();
    let area = match clip.map(to_mupdf_rect).as_ref() {
        Some(c) => c.intersect(&bounds),
        None => bounds,
    };
    let ctm = Matrix::new_scale(scale, scale);
    let irect = area.transform(&ctm).round();
    if irect.is_empty() {
        return Ok(empty());
    }
    let mut pix = Pixmap::new_with_rect(&Colorspace::device_gray(), irect, false)?;
    pix.clear_with(0xff)?;
    let dev = Device::from_pixmap(&pix)?;
    // MuPDF's fz_run_display_list intersects content against the scissor after
    // transforming by top_ctm, so the scissor must be in device space.
    dl.run(&dev, &ctm, area.transform(&ctm))?;
    Ok(RenderedPixels {
        width: irect.width() as u32,
        height: irect.height() as u32,
        samples: pix.samples().to_vec(),
    })
}
