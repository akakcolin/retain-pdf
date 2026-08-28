//! The `PdfDocument` PDF IO abstraction (Phase B1).
//!
//! Other crates depend ONLY on this trait — signatures use
//! `rendering_core` DTOs (`Rect`, `PageSnapshot`, `ImageInfo`) and
//! `RenderedPixels`/`RenderedRgbPixels`, never mupdf types. `mupdf::Document`
//! is the reference implementation. New primitives (span-dict / bboxlog /
//! xobject access) are Phase B2 additions to this trait.
//!
//! Contract vs PyMuPDF/fitz (both wrap the same MuPDF C engine):
//! - `page_rect` == fitz `page.rect` (rotation-applied); `page_rotation` ==
//!   fitz `page.rotation`. `page_cropbox` == fitz `page.cropbox` on unrotated
//!   pages only: mupdf-rs `PdfPage::crop_box()` returns the rotation-applied
//!   bounds re-positioned from the crop-box origin (dimensions equal to
//!   `page_rect`), so on rotated pages its origin diverges from fitz's
//!   unrotated cropbox (verified by `tests/page_sizes_diff.rs`).
//! - `page_drawing_count` == fitz `len(page.get_cdrawings())`.
//! - `page_drawings` == fitz `page.get_cdrawings()` per-drawing `type`
//!   ("f"/"s"/"fs") and `width` (`line_width * path_factor`, `None` for fills)
//!   — verified by `tests/drawings_diff.rs`. The per-drawing `rect` matches
//!   fitz EXCEPT on some stroked zigzag paths where PyMuPDF's lineart device
//!   drops the last path item's first point: fitz reports a smaller rect while
//!   this returns the full display-list path bound. That divergence is
//!   documented-but-unused — `page_drawing_count` (the only production
//!   consumer of `page_drawings`) reads only the length, and
//!   `collect_page_drawing_rects` stays on the Python reference.
//! - `page_image_infos` xref list == fitz `page.get_images(full=True)` xref
//!   list, with a zero bbox (the mupdf-rs image API exposes no xref-associated
//!   bbox query; `primary_background_image` skips empty bboxes).
//! - `page_image_placement_rects` == fitz `page.get_image_info(hashes=False)`
//!   bboxes: placement bounds (content / rotation-stripped page space) in paint
//!   order, `fill_image` + `fill_image_mask` (PyMuPDF's image-info device
//!   reports image masks as their own entries) — verified by
//!   `tests/image_rects_diff.rs`. Unlike `page_image_infos`, this exposes the
//!   *placement* rects the background-image detector consumes.
//! - `page_word_count` is CLOSE to fitz `len(page.get_text("words"))` but not
//!   identical: fitz extracts via `fz_stext_words` (PRESERVE_LIGATURES + its
//!   glyph-to-Unicode mapping), mupdf-rs via `fz_page_words` (maps ligature
//!   glyphs ﬁ/ﬂ to U+FFFD, merges neighbors). Measured 25/42 golden pages
//!   differ, max 5.4%; the replay asserts a 10% relative tolerance.
//! - `page_text_spans`/`page_text_blocks`/`page_math_rects`/`page_span_heights`
//!   == fitz `get_text("dict"/"blocks")` consumers (`source/cleanup`
//!   `text_extract`/`math_spans`) in content / rotation-stripped page space:
//!   spans grouped by (font name, size, RGB color, flags) with
//!   PRESERVE_LIGATURES | PRESERVE_IMAGES | PRESERVE_WHITESPACE; text blocks
//!   with lines joined by "\n"; deduped math-font span rects; non-math span
//!   heights > 0.5 — verified by `tests/text_read_diff.rs`. Word segmentation
//!   (`extract_item_word_entries`) is NOT exposed here (fitz's clip truncates
//!   words by glyph-ink bbox, which mupdf-rs cannot reproduce).
//! - `text_traces` stays empty (no mupdf-rs text-trace type/opacity API).
//!   `image_rects` is the page's full image-placement set (==
//!   `page_image_placement_rects`) attributed to EVERY real xref (aggregate):
//!   mupdf-rs exposes no placement→xref association, so each xref carries the
//!   whole list. The union is exactly fitz `get_image_info`; the per-xref tie
//!   is a documented divergence (`profile_build`'s primary-image picker consumes
//!   only the placement union).

use std::collections::HashMap;
use std::path::Path;

use mupdf::pdf::PdfObject;
use mupdf::pdf::PdfPage;
use mupdf::{Colorspace, Device, Document, Matrix, Pixmap, Rect as MuPdfRect};
use mupdf::{TextExtractOptions, TextPageFlags};
use rendering_core::page::{
    BboxlogEntry, FormXObjectInfo, ImageInfo, PageDrawing, PageDrawingType, PageSnapshot,
};
use rendering_core::rect::Rect;

use crate::bboxlog;
use crate::error::PdfError;
use crate::image_placements;
use crate::render::{RenderedPixels, RenderedRgbPixels};
use crate::text_spans;

/// PDF IO abstraction. The only PDF-reading API other crates may use.
pub trait PdfDocument {
    /// Open a PDF file. `Self` must be Sized to be returned by value.
    fn open(path: &Path) -> Result<Self, PdfError>
    where
        Self: Sized;

    /// Total page count.
    fn page_count(&self) -> Result<i64, PdfError>;

    /// Page geometry == fitz `page.rect` (rotation-applied bounds).
    fn page_rect(&self, idx: i64) -> Result<Rect, PdfError>;

    /// Page crop box. Equal to fitz `page.cropbox` on unrotated pages; on
    /// rotated pages mupdf-rs returns the rotation-applied bounds re-positioned
    /// from the crop-box origin (dimensions match `page_rect`), not fitz's
    /// unrotated cropbox — see the module contract.
    fn page_cropbox(&self, idx: i64) -> Result<Rect, PdfError>;

    /// Page rotation in degrees.
    fn page_rotation(&self, idx: i64) -> Result<i64, PdfError>;

    /// Word count (fitz-compatible PRESERVE_LIGATURES extraction).
    fn page_word_count(&self, idx: i64) -> Result<i64, PdfError>;

    /// Vector drawing count.
    fn page_drawing_count(&self, idx: i64) -> Result<i64, PdfError>;

    /// Per-drawing facts (bounds / paint type / stroke width). `type` and
    /// `width` == fitz `page.get_cdrawings()`; `rect` matches fitz except on
    /// some stroked zigzag paths where fitz drops the last path item's first
    /// point (see module contract).
    fn page_drawings(&self, idx: i64) -> Result<Vec<PageDrawing>, PdfError>;

    /// Image resource facts (xref > 0, zero bbox) — consumed by background
    /// detection.
    fn page_image_infos(&self, idx: i64) -> Result<Vec<ImageInfo>, PdfError>;

    /// Full snapshot consumed by `rendering_core::profile_build`.
    fn page_snapshot(&self, idx: i64) -> Result<PageSnapshot, PdfError>;

    /// Clipped device-gray render (fitz `page.get_pixmap(csGRAY, alpha=False,
    /// clip=...)`). `clip` is page space; intersected with page bounds.
    fn render_page_clip_gray(
        &self,
        idx: i64,
        clip: Option<&Rect>,
        scale: f32,
    ) -> Result<RenderedPixels, PdfError>;

    /// Clipped device-RGB render (fitz `page.get_pixmap(csRGB, alpha=False,
    /// clip=...)`).
    fn render_page_clip_rgb(
        &self,
        idx: i64,
        clip: Option<&Rect>,
        scale: f32,
    ) -> Result<RenderedRgbPixels, PdfError>;

    /// `page.get_bboxlog()` — drawing-op bounds in rotation-stripped fitz page
    /// space, in op order. A load/run failure yields an empty list (mirrors fitz
    /// `get_bboxlog`'s exception → empty grouping).
    fn page_bboxlog(&self, idx: i64) -> Vec<BboxlogEntry> {
        let _ = idx;
        Vec::new()
    }

    /// Image placement rects == fitz `page.get_image_info(hashes=False)` bboxes
    /// (content / rotation-stripped page space, f32-widened), in paint order.
    /// Both `fill_image` and `fill_image_mask` placements are captured —
    /// PyMuPDF's image-info device reports image masks as their own entries
    /// (verified on golden 1.pdf p6). A load/run failure yields an empty list
    /// (the bridge then falls back to the Python reference).
    fn page_image_placement_rects(&self, idx: i64) -> Vec<Rect> {
        let _ = idx;
        Vec::new()
    }

    /// `text_extract.extract_page_text_spans` — fitz `get_text("dict")` spans
    /// as `(bbox, stripped-text)` pairs (content / rotation-stripped page
    /// space). A load/grouping failure yields an empty list (the bridge then
    /// falls back to the Python reference).
    fn page_text_spans(&self, idx: i64) -> Vec<(Rect, String)> {
        let _ = idx;
        Vec::new()
    }

    /// `text_extract.extract_page_text_blocks` — fitz `get_text("blocks")`
    /// text blocks as `(bbox, stripped-text)` pairs.
    fn page_text_blocks(&self, idx: i64) -> Vec<(Rect, String)> {
        let _ = idx;
        Vec::new()
    }

    /// `math_spans.collect_page_math_protection_rects` — deduped rects of
    /// math-font spans (fitz `get_text("dict")` spans whose font name is a
    /// special-math-font).
    fn page_math_rects(&self, idx: i64) -> Vec<Rect> {
        let _ = idx;
        Vec::new()
    }

    /// `math_spans.collect_page_non_math_span_heights` — non-math span heights
    /// > 0.5.
    fn page_span_heights(&self, idx: i64) -> Vec<f64> {
        let _ = idx;
        Vec::new()
    }

    /// Accumulated decoded `/Contents` stream length, early-returning the
    /// running total once it reaches `threshold` (mirrors
    /// `page_probe.page_content_stream_size`: per-xref add, `>= threshold` stops
    /// summing; a stream read error skips that xref; no contents → 0).
    fn page_content_stream_size(&self, idx: i64, threshold: u64) -> u64 {
        let _ = (idx, threshold);
        0
    }

    /// True when the page has at least one form XObject in its (inherited)
    /// resources (`page.get_xobjects()` non-empty). A missing resource/XObject
    /// dict is false; a structural error mirrors the Python `except → True`.
    fn page_has_form_xobjects(&self, idx: i64) -> bool {
        let _ = idx;
        false
    }

    /// The page's transformation matrix `[a,b,c,d,e,f]` with rotation cleared
    /// (== fitz `page.transformation_matrix`). None when the page cannot be
    /// loaded / the matrix is not available.
    fn page_ctm(&self, idx: i64) -> Option<[f64; 6]> {
        let _ = idx;
        None
    }

    /// `pdf_structure_profile` form-xobject read — the (inherited)
    /// `/Resources/XObject` entries that carry a `/BBox`, in resource-dict
    /// order, as `{name, xref, bbox}`. Mirrors fitz `page.get_xobjects()` for
    /// single-level forms (a nested form invoking another form is a documented
    /// divergence: fitz reports each `Do` instance, this returns one entry per
    /// named resource). A load/read failure yields an empty list (the bridge
    /// then falls back to the Python reference).
    fn page_form_xobjects(&self, idx: i64) -> Vec<FormXObjectInfo> {
        let _ = idx;
        Vec::new()
    }
}

impl PdfDocument for mupdf::Document {
    fn open(path: &Path) -> Result<Self, PdfError> {
        Document::open(path).map_err(PdfError::from)
    }

    fn page_count(&self) -> Result<i64, PdfError> {
        // UFCS: mupdf::Document has an inherent `page_count`; be explicit to
        // avoid the trait method shadowing (inherent methods take precedence).
        Document::page_count(self).map(|n| n as i64).map_err(PdfError::from)
    }

    fn page_rect(&self, idx: i64) -> Result<Rect, PdfError> {
        let page = load_pdf_page(self, idx)?;
        Ok(to_core_rect(&page.bounds()?))
    }

    fn page_cropbox(&self, idx: i64) -> Result<Rect, PdfError> {
        let page = load_pdf_page(self, idx)?;
        Ok(to_core_rect(&page.crop_box()?))
    }

    fn page_rotation(&self, idx: i64) -> Result<i64, PdfError> {
        let page = load_pdf_page(self, idx)?;
        Ok(page.rotation()? as i64)
    }

    fn page_word_count(&self, idx: i64) -> Result<i64, PdfError> {
        let page = load_pdf_page(self, idx)?;
        // PyMuPDF's get_text enables PRESERVE_LIGATURES by default; mupdf-rs's
        // Default does not. Without it, ligature glyphs (ﬁ/ﬂ) become U+FFFD and
        // neighboring tokens merge, shifting word_count.
        let options = TextExtractOptions {
            flags: TextPageFlags::PRESERVE_LIGATURES,
        };
        Ok(page.words(options)?.len() as i64)
    }

    fn page_drawing_count(&self, idx: i64) -> Result<i64, PdfError> {
        let page = load_pdf_page(self, idx)?;
        Ok(page.drawings()?.len() as i64)
    }

    fn page_drawings(&self, idx: i64) -> Result<Vec<PageDrawing>, PdfError> {
        let page = load_pdf_page(self, idx)?;
        let drawings = page.drawings()?;
        Ok(drawings
            .into_iter()
            .map(|d| PageDrawing {
                rect: to_core_rect(&d.rect),
                drawing_type: match d.drawing_type {
                    mupdf::drawing::DrawingType::Fill => PageDrawingType::Fill,
                    mupdf::drawing::DrawingType::Stroke => PageDrawingType::Stroke,
                    mupdf::drawing::DrawingType::FillStroke => PageDrawingType::FillStroke,
                },
                width: d.width,
            })
            .collect())
    }

    fn page_image_infos(&self, idx: i64) -> Result<Vec<ImageInfo>, PdfError> {
        let (_, infos) = image_facts(self, idx)?;
        Ok(infos)
    }

    fn page_snapshot(&self, idx: i64) -> Result<PageSnapshot, PdfError> {
        let page = load_pdf_page(self, idx)?;
        let rect = to_core_rect(&page.bounds()?);
        let cropbox = to_core_rect(&page.crop_box()?);
        let options = TextExtractOptions {
            flags: TextPageFlags::PRESERVE_LIGATURES,
        };
        let word_count = page.words(options)?.len() as i64;
        let drawing_count = page.drawings()?.len() as i64;
        let rotation = page.rotation()? as i64;
        let image_entries: Vec<i64> = page
            .images()?
            .into_iter()
            .map(|info| info.xref as i64)
            .collect();
        let image_infos: Vec<ImageInfo> = image_entries
            .iter()
            .filter(|&&xref| xref > 0)
            .map(|&xref| ImageInfo {
                xref,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            })
            .collect();
        // Inc 3: image_rects carries the page's full placement set attributed to
        // every real xref (aggregate). mupdf-rs exposes no placement→xref
        // association, so each xref gets the same list; the union is exactly
        // fitz `get_image_info`, and `profile_build`'s primary-image picker
        // consumes only that union — the per-xref tie is a documented divergence.
        let placements = image_placements::collect_page_image_rects(page)?;
        let image_rects: HashMap<i64, Vec<Rect>> = image_entries
            .iter()
            .filter(|&&xref| xref > 0)
            .map(|&xref| (xref, placements.clone()))
            .collect();
        Ok(PageSnapshot {
            number: idx,
            rotation,
            rect,
            cropbox,
            text_traces: Vec::new(),
            word_count,
            drawing_count,
            image_infos,
            image_entries,
            image_rects,
        })
    }

    fn render_page_clip_gray(
        &self,
        idx: i64,
        clip: Option<&Rect>,
        scale: f32,
    ) -> Result<RenderedPixels, PdfError> {
        let page = self.load_page(idx as i32)?;
        let page_bounds = page.bounds()?;
        let rclip = match clip.map(to_mupdf_rect).as_ref() {
            Some(c) => c.intersect(&page_bounds),
            None => page_bounds,
        };
        let ctm = Matrix::new_scale(scale, scale);
        let irect = rclip.transform(&ctm).round();
        if irect.is_empty() {
            return Ok(RenderedPixels {
                width: 0,
                height: 0,
                samples: Vec::new(),
            });
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

    fn render_page_clip_rgb(
        &self,
        idx: i64,
        clip: Option<&Rect>,
        scale: f32,
    ) -> Result<RenderedRgbPixels, PdfError> {
        let page = self.load_page(idx as i32)?;
        let page_bounds = page.bounds()?;
        let rclip = match clip.map(to_mupdf_rect).as_ref() {
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

    fn page_bboxlog(&self, idx: i64) -> Vec<BboxlogEntry> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        bboxlog::collect_bboxlog(page).unwrap_or_default()
    }

    fn page_image_placement_rects(&self, idx: i64) -> Vec<Rect> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        image_placements::collect_page_image_rects(page).unwrap_or_default()
    }

    fn page_text_spans(&self, idx: i64) -> Vec<(Rect, String)> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        text_spans::collect_page_text_spans(page).unwrap_or_default()
    }

    fn page_text_blocks(&self, idx: i64) -> Vec<(Rect, String)> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        text_spans::collect_page_text_blocks(page).unwrap_or_default()
    }

    fn page_math_rects(&self, idx: i64) -> Vec<Rect> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        text_spans::collect_page_math_rects(page).unwrap_or_default()
    }

    fn page_span_heights(&self, idx: i64) -> Vec<f64> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        text_spans::collect_page_span_heights(page).unwrap_or_default()
    }

    fn page_content_stream_size(&self, idx: i64, threshold: u64) -> u64 {
        let Ok(page) = load_pdf_page(self, idx) else {
            return 0;
        };
        content_stream_size(&page, threshold).unwrap_or(0)
    }

    fn page_has_form_xobjects(&self, idx: i64) -> bool {
        let Ok(page) = load_pdf_page(self, idx) else {
            return true; // unknown page — mirror the Python `except → True`
        };
        has_form_xobjects(&page).unwrap_or(true)
    }

    fn page_ctm(&self, idx: i64) -> Option<[f64; 6]> {
        let mut page = load_pdf_page(self, idx).ok()?;
        // Clear rotation so the reported matrix equals fitz
        // `page.transformation_matrix` (which is rotation-stripped); restore
        // afterwards so the shared page object keeps its /Rotate.
        let rotation = page.rotation().ok()?;
        page.set_rotation(0).ok()?;
        let m = page.ctm().ok()?;
        let _ = page.set_rotation(rotation);
        Some([m.a as f64, m.b as f64, m.c as f64, m.d as f64, m.e as f64, m.f as f64])
    }

    fn page_form_xobjects(&self, idx: i64) -> Vec<FormXObjectInfo> {
        let Ok(page) = load_pdf_page(self, idx) else {
            return Vec::new();
        };
        form_xobjects(&page).unwrap_or_default()
    }
}

/// Mirrors `sampler._form_xobject_objects`: scan the page's (inherited)
/// `/Resources/XObject` dict for entries that carry a `/BBox` (Form xobjects
/// always do; plain images usually do not). Returns `{name, xref, bbox}` in
/// resource-dict order. A missing resource / XObject dict is empty; an
/// unexpected structural error propagates so the caller can mirror the
/// reference's `except → fallback`.
fn form_xobjects(page: &PdfPage) -> Result<Vec<FormXObjectInfo>, PdfError> {
    let resources = page.object().get_dict_inheritable("Resources")?;
    let Some(resources) = resources else {
        return Ok(Vec::new());
    };
    if !resources.is_dict()? {
        return Ok(Vec::new());
    }
    let xobjects = resources.get_dict("XObject")?;
    let Some(xobjects) = xobjects else {
        return Ok(Vec::new());
    };
    if !xobjects.is_dict()? {
        return Ok(Vec::new());
    }
    let len = xobjects.dict_len()?;
    let mut out: Vec<FormXObjectInfo> = Vec::with_capacity(len);
    for i in 0..len as i32 {
        let Some(key) = xobjects.get_dict_key(i)? else {
            continue;
        };
        let Some(value) = xobjects.get_dict_val(i)? else {
            continue;
        };
        let Some(bbox) = value.get_dict("BBox")? else {
            continue;
        };
        let Some(rect) = obj_rect(&bbox)? else {
            continue;
        };
        if rect.is_empty() {
            continue;
        }
        out.push(FormXObjectInfo {
            name: key
                .as_name()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default(),
            xref: value.as_indirect().unwrap_or(0) as i64,
            bbox: rect,
        });
    }
    Ok(out)
}

/// Read a PDF array object as a rect (`/BBox` is `[x0 y0 x1 y1]`), resolving an
/// indirect reference to the array first. Non-array / too-short values are
/// skipped, mirroring fitz's rect coercion.
fn obj_rect(obj: &PdfObject) -> Result<Option<Rect>, PdfError> {
    if !obj.is_array()? {
        return Ok(None);
    }
    let len = obj.len()?;
    if len < 4 {
        return Ok(None);
    }
    let mut vals = [0.0f32; 4];
    for (index, slot) in vals.iter_mut().enumerate() {
        let Some(item) = obj.get_array(index as i32)? else {
            return Ok(None);
        };
        *slot = item.as_float()?;
    }
    Ok(Some(Rect::new(
        vals[0] as f64,
        vals[1] as f64,
        vals[2] as f64,
        vals[3] as f64,
    )))
}

/// Load `idx` as an editing-capable PDF page (PdfPage derefs to Page, so the
/// generic page ops resolve through it).
pub(crate) fn load_pdf_page(doc: &Document, idx: i64) -> Result<PdfPage, PdfError> {
    let page = doc.load_page(idx as i32)?;
    Ok(PdfPage::try_from(page)?)
}

/// Mirrors `page_probe.page_content_stream_size`: the `/Contents` value is a
/// single stream or an array of streams; accumulate each decoded stream's length,
/// early-returning the running total once it reaches `threshold`. A stream read
/// error skips that xref (Python `except: continue`); no contents → 0.
fn content_stream_size(page: &PdfPage, threshold: u64) -> Result<u64, PdfError> {
    let contents = page.contents()?;
    let Some(contents) = contents else {
        return Ok(0);
    };
    let mut total: u64 = 0;
    if contents.is_array()? {
        let len = contents.len()?;
        for i in 0..len as i32 {
            let Some(item) = contents.get_array(i)? else {
                continue;
            };
            if let Ok(bytes) = item.read_stream() {
                total += bytes.len() as u64;
                if total >= threshold {
                    return Ok(total);
                }
            }
        }
    } else if let Ok(bytes) = contents.read_stream() {
        total += bytes.len() as u64;
        if total >= threshold {
            return Ok(total);
        }
    }
    Ok(total)
}

/// Mirrors `page_probe.page_has_form_xobjects`: scan the page's (inherited)
/// `/Resources/XObject` dict for any entry whose `/Subtype` is `/Form`. A missing
/// resource / XObject dict is false; an unexpected structural error propagates so
/// the caller can mirror the Python `except → True`.
fn has_form_xobjects(page: &PdfPage) -> Result<bool, PdfError> {
    let resources = page.object().get_dict_inheritable("Resources")?;
    let Some(resources) = resources else {
        return Ok(false);
    };
    if !resources.is_dict()? {
        return Ok(false);
    }
    let xobjects = resources.get_dict("XObject")?;
    let Some(xobjects) = xobjects else {
        return Ok(false);
    };
    if !xobjects.is_dict()? {
        return Ok(false);
    }
    let len = xobjects.dict_len()?;
    for i in 0..len as i32 {
        let Some(value) = xobjects.get_dict_val(i as i32)? else {
            continue;
        };
        let Some(subtype) = value.get_dict("Subtype")? else {
            continue;
        };
        if subtype.as_name()? == b"Form" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Image resource facts: raw xref list plus the xref > 0 `ImageInfo` list with
/// a zero bbox.
fn image_facts(doc: &Document, idx: i64) -> Result<(Vec<i64>, Vec<ImageInfo>), PdfError> {
    let page = load_pdf_page(doc, idx)?;
    let entries: Vec<i64> = page
        .images()?
        .into_iter()
        .map(|info| info.xref as i64)
        .collect();
    let infos: Vec<ImageInfo> = entries
        .iter()
        .filter(|&&xref| xref > 0)
        .map(|&xref| ImageInfo {
            xref,
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
        })
        .collect();
    Ok((entries, infos))
}

/// MuPDF stores coordinates as f32; widen to rendering_core's f64 Rect.
fn to_core_rect(r: &MuPdfRect) -> Rect {
    Rect::new(r.x0 as f64, r.y0 as f64, r.x1 as f64, r.y1 as f64)
}

/// Narrow a core f64 Rect to mupdf's f32 Rect (used for clip/render paths).
pub(crate) fn to_mupdf_rect(r: &Rect) -> MuPdfRect {
    MuPdfRect::new(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)
}
