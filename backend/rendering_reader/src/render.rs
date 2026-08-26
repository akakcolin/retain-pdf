//! Clipped grayscale rendering for `rendering_core::first_line_indent`.
//!
//! Replicates PyMuPDF/fitz's clipped pixmap render byte-for-byte so the golden
//! differential can compare mupdf-rs pixels against fitz pixels. Both wrap the
//! same MuPDF C engine; the contract below mirrors the C calls fitz makes:
//!
//! `page.get_pixmap(matrix=M, colorspace=csGRAY, alpha=False, clip=clip)`:
//!   1. `rrect = (clip & page.bounds()).transform(M)`  (fitz `fz_intersect_rect`
//!      then `fz_transform_rect`, all in f32)
//!   2. `irect = rrect.round()`                        (`fz_round_rect`)
//!   3. `pix = fz_new_pixmap_with_bbox(gray, irect, alpha=false)`
//!   4. `fz_clear_pixmap_with_value(pix, 0xff)`        (white background)
//!   5. `dev = fz_new_draw_device_with_bbox(pix, irect)`
//!   6. `fz_run_page(page, dev, M)`
//!
//! `DisplayList.get_pixmap(...)` differs only in step 6: the area (in page
//! space) is passed to `fz_run_display_list(dl, dev, M, area)` and the draw
//! device is created without a bbox clip.

use mupdf::{Colorspace, Device, DisplayList, Document, Error, Matrix, Pixmap, Rect};

/// Rendered grayscale pixels, 1 byte per pixel, row-major, white (255) base.
pub struct RenderedPixels {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<u8>,
}

fn empty() -> RenderedPixels {
    RenderedPixels {
        width: 0,
        height: 0,
        samples: Vec::new(),
    }
}

/// Render `page_idx` clipped to `clip` (page space; intersected with page
/// bounds), scaled by `scale`, into device-gray pixels. `clip == None` renders
/// the full page.
pub fn render_page_clip_gray(
    doc: &Document,
    page_idx: i32,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedPixels, Error> {
    let page = doc.load_page(page_idx)?;
    let page_bounds = page.bounds()?;
    let rclip = match clip {
        Some(c) => c.intersect(&page_bounds),
        None => page_bounds,
    };
    let ctm = Matrix::new_scale(scale, scale);
    let irect = rclip.transform(&ctm).round();
    if irect.is_empty() {
        return Ok(empty());
    }
    let mut pix = Pixmap::new_with_rect(&Colorspace::device_gray(), irect, false)?;
    pix.clear_with(0xff)?;
    let dev = Device::from_pixmap_with_clip(&pix, irect)?;
    page.run(&dev, &ctm)?;
    Ok(RenderedPixels {
        width: irect.width() as u32,
        height: irect.height() as u32,
        samples: pix.samples().to_vec(),
    })
}

/// Rendered RGB pixels (3 bytes per pixel, row-major, white base). Mirrors
/// `fitz.Page.get_pixmap(..., colorspace=csRGB, alpha=False, clip=clip)`.
pub struct RenderedRgbPixels {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub samples: Vec<u8>,
}

/// Render `page_idx` clipped to `clip` (page space; intersected with page
/// bounds), scaled by `scale`, into device-RGB pixels. `clip == None` renders
/// the full page. Backs the background-fill sampler (`source/background/fill.py`
/// `_clip_pixmap` with `colorspace=csRGB`).
pub fn render_page_clip_rgb(
    doc: &Document,
    page_idx: i32,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedRgbPixels, Error> {
    let page = doc.load_page(page_idx)?;
    let page_bounds = page.bounds()?;
    let rclip = match clip {
        Some(c) => c.intersect(&page_bounds),
        None => page_bounds,
    };
    let ctm = Matrix::new_scale(scale, scale);
    let irect = rclip.transform(&ctm).round();
    if irect.is_empty() {
        return Ok(RenderedRgbPixels {
            width: 0,
            height: 0,
            stride: 3,
            samples: Vec::new(),
        });
    }
    let mut pix = Pixmap::new_with_rect(&Colorspace::device_rgb(), irect, false)?;
    pix.clear_with(0xff)?;
    let dev = Device::from_pixmap_with_clip(&pix, irect)?;
    page.run(&dev, &ctm)?;
    Ok(RenderedRgbPixels {
        width: irect.width() as u32,
        height: irect.height() as u32,
        stride: pix.n() as u32,
        samples: pix.samples().to_vec(),
    })
}

/// Render `dl` clipped to `clip` (page space; intersected with the display
/// list bounds), scaled by `scale`, into device-gray pixels.
pub fn render_display_list_clip_gray(
    dl: &DisplayList,
    clip: Option<&Rect>,
    scale: f32,
) -> Result<RenderedPixels, Error> {
    let bounds = dl.bounds();
    let area = match clip {
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
