//! mupdf-rs-backed reader producing `rendering_core::page::PageSnapshot`.
//!
//! Contract vs PyMuPDF/fitz (both wrap the same MuPDF C engine):
//! - `bounds()` == fitz `page.rect` (rotation-applied); `crop_box()` == `page.cropbox`.
//! - `drawings().len()` == fitz `len(page.get_cdrawings())`.
//! - `images()` xref list == fitz `page.get_images(full=True)` xref list.
//! - `words()` count is CLOSE to fitz `len(page.get_text("words"))` but not
//!   identical: fitz extracts via `fz_stext_words` (PRESERVE_LIGATURES + its
//!   glyph-to-Unicode mapping), mupdf-rs via `fz_page_words` (maps ligature
//!   glyphs ﬁ/ﬂ to U+FFFD, merges neighbors). Measured 25/42 golden pages
//!   differ, max 5.4%; the replay asserts a 10% relative tolerance
//!   (WORD_COUNT_TOLERANCE in tests/common/mod.rs).
//!
//! Unavailable fields (mupdf-rs exposes no text-trace type/opacity and no
//! xref-associated image bbox query): `text_traces` stays empty, `image_rects`
//! stays empty, and `image_infos` carries xref-only with a zero bbox. A zero
//! bbox is deliberate: `primary_background_image` skips empty bboxes, so the
//! image-background profile deterministically reports no large background.
//! Full text-trace / image-bbox fidelity is Phase 5 work.

use std::collections::HashMap;
use std::path::Path;

use mupdf::pdf::PdfPage;
use mupdf::{Document, TextExtractOptions, TextPageFlags};
use rendering_core::page::{ImageInfo, PageSnapshot, TextTrace};
use rendering_core::rect::Rect;

pub fn open(path: &Path) -> Result<Document, mupdf::Error> {
    Document::open(path)
}

pub fn page_count(doc: &Document) -> Result<i64, mupdf::Error> {
    doc.page_count().map(|n| n as i64)
}

pub fn read_page_snapshot(doc: &Document, idx: i64) -> Result<PageSnapshot, mupdf::Error> {
    let page = doc.load_page(idx as i32)?;
    let pdf = PdfPage::try_from(page)?;
    let rect = to_core_rect(&pdf.bounds()?);
    let cropbox = to_core_rect(&pdf.crop_box()?);
    // PyMuPDF's get_text enables PRESERVE_LIGATURES by default; mupdf-rs's
    // Default does not. Without it, ligature glyphs (ﬁ/ﬂ) become U+FFFD and
    // neighboring tokens merge, shifting word_count.
    let options = TextExtractOptions {
        flags: TextPageFlags::PRESERVE_LIGATURES,
    };
    let word_count = pdf.words(options)?.len() as i64;
    let drawing_count = pdf.drawings()?.len() as i64;
    let image_entries: Vec<i64> = pdf
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
    Ok(PageSnapshot {
        number: idx,
        rotation: pdf.rotation()? as i64,
        rect,
        cropbox,
        text_traces: Vec::<TextTrace>::new(),
        word_count,
        drawing_count,
        image_infos,
        image_entries,
        image_rects: HashMap::new(),
    })
}

/// MuPDF stores coordinates as f32; widen to rendering_core's f64 Rect.
fn to_core_rect(r: &mupdf::Rect) -> Rect {
    Rect::new(r.x0 as f64, r.y0 as f64, r.x1 as f64, r.y1 as f64)
}
