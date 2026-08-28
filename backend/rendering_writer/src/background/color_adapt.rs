//! Port of `output/typst/color_adapt.py` + `visual_profile/foreground.py` — the
//! color-adaptation primitives over raw pixels and structured text.
//!
//! Two primitives:
//!   * `extract_span_dicts_from_page` / `extract_span_dicts_from_text_page` —
//!     fitz `get_text("dict")`-equivalent span extraction. Each span carries the
//!     bounding rect (page space), the raw text (not trimmed; the Python
//!     bucketers `.strip()` when weighting), and the 0xRRGGBB color. Spans are
//!     grouped per line by (font name, size, color), and `clip` filters
//!     characters whose quad does not intersect it (fitz clip semantics: the
//!     span bbox is the union of the surviving glyph quads). PRESERVE_LIGATURES
//!     matches fitz `get_text`'s default.
//!   * `foreground_color_impl` — the parameterised connected-component
//!     foreground-color estimator that both
//!     `color_adapt._title_foreground_color_from_pixmap` and
//!     `visual_profile/foreground.py::foreground_color_from_pixmap` reduce to;
//!     they differ only in the thresholds/quantum
//!     (`TITLE_FOREGROUND_COLOR_PARAMS` / `FOREGROUND_COLOR_PARAMS`).

use mupdf::pdf::PdfPage;
use mupdf::text_page::{TextBlockType, TextPage};
use mupdf::{Error, Page, Rect, TextPageFlags};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::fill::RgbPixmap;

/// A structured-text span: bounding rect (page space), raw text (untrimmed),
/// and the 0xRRGGBB color (`argb() & 0xFFFFFF`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpanEntry {
    pub rect: RectTuple,
    pub text: String,
    pub color: u32,
}

/// Span-dict extraction for a whole page (builds the text page once).
pub fn extract_span_dicts_from_page(page: &Page, clip: Option<&RectTuple>) -> Result<Vec<SpanEntry>, Error> {
    let text_page = build_text_page_for_extraction(page)?;
    extract_span_dicts_from_text_page(&text_page, clip)
}

/// Builds a text page in the unrotated content space, mirroring fitz
/// `Page.get_textpage`: PyMuPDF temporarily clears the page rotation before
/// extracting so `get_text("dict")` spans carry content coordinates. mupdf's
/// `Page::to_text_page` otherwise applies the page rotation and yields
/// display-space spans that never intersect content-space clips on rotated
/// pages. Non-PDF pages (no `/Rotate`) fall back to the default extraction.
pub fn build_text_page_for_extraction(page: &Page) -> Result<TextPage, Error> {
    match PdfPage::try_from(page.clone()) {
        Ok(pdf_page) => build_text_page_unrotated(pdf_page),
        Err(_) => page.to_text_page(TextPageFlags::PRESERVE_LIGATURES),
    }
}

fn build_text_page_unrotated(mut pdf_page: PdfPage) -> Result<TextPage, Error> {
    let rotation = pdf_page.rotation()?;
    if rotation % 90 != 0 {
        return Err(Error::InvalidArgument(format!(
            "page rotation must be a multiple of 90, got {rotation}"
        )));
    }
    if rotation == 0 {
        return pdf_page.to_text_page(TextPageFlags::PRESERVE_LIGATURES);
    }
    let obj = pdf_page.object();
    let had_direct_rotate = obj.get_dict("Rotate")?.is_some();
    pdf_page.set_rotation(0)?;
    let extracted = pdf_page.to_text_page(TextPageFlags::PRESERVE_LIGATURES);
    let restored = if had_direct_rotate {
        pdf_page.set_rotation(rotation)
    } else {
        let mut obj = pdf_page.object();
        obj.dict_delete("Rotate")
    };
    match (extracted, restored) {
        (Ok(text_page), Ok(())) => Ok(text_page),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

/// Span-dict extraction against an already-built text page, so the caller can
/// extract several clips from one page without rebuilding it.
pub fn extract_span_dicts_from_text_page(
    text_page: &TextPage,
    clip: Option<&RectTuple>,
) -> Result<Vec<SpanEntry>, Error> {
    let mut spans: Vec<SpanEntry> = Vec::new();
    for block in text_page.blocks() {
        if block.r#type() != TextBlockType::Text {
            continue;
        }
        for line in block.lines() {
            let mut current: Option<(String, f32, u32, Vec<Rect>)> = None;
            let mut current_text = String::new();
            for ch in line.chars() {
                let quad_rect = Rect::from(ch.quad());
                if let Some(clip) = clip {
                    if !quad_intersects_rect(&quad_rect, clip) {
                        continue;
                    }
                }
                let font_name = ch.font().map(|f| f.name().to_string()).unwrap_or_default();
                let size = ch.size();
                let color = ch.argb() & 0x00FF_FFFF;
                match &mut current {
                    Some((name, sz, col, rects))
                        if *name == font_name && (*sz - size).abs() < 1e-6 && *col == color =>
                    {
                        rects.push(quad_rect);
                        if let Some(c) = ch.char() {
                            current_text.push(c);
                        }
                    }
                    _ => {
                        flush_span(&mut spans, &mut current, &mut current_text);
                        current = Some((font_name, size, color, vec![quad_rect]));
                        if let Some(c) = ch.char() {
                            current_text.push(c);
                        }
                    }
                }
            }
            flush_span(&mut spans, &mut current, &mut current_text);
        }
    }
    Ok(spans)
}

fn flush_span(
    spans: &mut Vec<SpanEntry>,
    current: &mut Option<(String, f32, u32, Vec<Rect>)>,
    current_text: &mut String,
) {
    if let Some((_, _, color, rects)) = current.take() {
        if !rects.is_empty() {
            let mut rect = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            for r in &rects {
                rect.x0 = rect.x0.min(r.x0);
                rect.y0 = rect.y0.min(r.y0);
                rect.x1 = rect.x1.max(r.x1);
                rect.y1 = rect.y1.max(r.y1);
            }
            if !rect.is_empty() {
                spans.push(SpanEntry {
                    rect: [rect.x0 as f64, rect.y0 as f64, rect.x1 as f64, rect.y1 as f64],
                    text: current_text.clone(),
                    color,
                });
            }
        }
        current_text.clear();
    }
}

/// Non-empty-overlap test between a mupdf quad-bounds rect and a page-space
/// `RectTuple` (matches fitz `Rect.intersects`).
fn quad_intersects_rect(rect: &Rect, clip: &RectTuple) -> bool {
    rect.x0 < clip[2] as f32 && rect.x1 > clip[0] as f32 && rect.y0 < clip[3] as f32 && rect.y1 > clip[1] as f32
}

// --- foreground-color estimator ------------------------------------------------

/// Thresholds for the connected-component foreground estimator. Both Python
/// callers (`color_adapt._title_foreground_color_from_pixmap`,
/// `visual_profile/foreground.py::foreground_color_from_pixmap`) are the same
/// algorithm with different constants.
#[derive(Debug, Clone, Copy)]
pub struct ForegroundColorParams {
    /// Minimum per-pixel color distance from the background to count as foreground.
    pub min_distance: f64,
    /// Channel bucket size for the color histogram (`// quantum`).
    pub quantum: i32,
    /// `max(floor, int(total * denom))` for the minimum component pixel count.
    pub min_component_floor: i64,
    pub min_component_denom: f64,
    /// `max(floor, int(total * denom))` for the maximum component pixel count.
    pub max_component_floor: i64,
    pub max_component_denom: f64,
    /// Thin-strip rejection: width >= page_width * ratio and height <= 3.max(page_height * ratio).
    pub thin_width_ratio: f64,
    pub thin_height_ratio: f64,
    /// Full-width rejection: width >= page_width * ratio and height >= page_height * ratio.
    pub flat_width_ratio: f64,
    pub flat_height_ratio: f64,
}

pub const TITLE_FOREGROUND_COLOR_PARAMS: ForegroundColorParams = ForegroundColorParams {
    min_distance: 42.0,
    quantum: 16,
    min_component_floor: 3,
    min_component_denom: 0.0004,
    max_component_floor: 24,
    max_component_denom: 0.35,
    thin_width_ratio: 0.82,
    thin_height_ratio: 0.08,
    flat_width_ratio: 0.92,
    flat_height_ratio: 0.55,
};

pub const FOREGROUND_COLOR_PARAMS: ForegroundColorParams = ForegroundColorParams {
    min_distance: 36.0,
    quantum: 16,
    min_component_floor: 2,
    min_component_denom: 0.00025,
    max_component_floor: 32,
    max_component_denom: 0.45,
    thin_width_ratio: 0.86,
    thin_height_ratio: 0.07,
    flat_width_ratio: 0.92,
    flat_height_ratio: 0.55,
};

/// Python `round(x)` (round-half-even on the exact binary value), matching
/// `background/sampling.rs::py_round`.
fn round_ties_even(x: f64) -> i64 {
    x.round_ties_even() as i64
}

fn color_distance_sq(a: &[u8; 3], b: &[u8; 3]) -> i64 {
    let dr = a[0] as i64 - b[0] as i64;
    let dg = a[1] as i64 - b[1] as i64;
    let db = a[2] as i64 - b[2] as i64;
    dr * dr + dg * dg + db * db
}

/// The shared connected-component foreground estimator. Returns `(color, confidence)`;
/// `color` is `None` when no text-like foreground component survives
/// (`foreground_count == 0` or empty buckets), with `confidence == 0.0`.
pub fn foreground_color_impl(
    pix: &RgbPixmap,
    background: [f64; 3],
    params: &ForegroundColorParams,
) -> (Option<[f64; 3]>, f64) {
    let width = pix.width;
    let height = pix.height;
    if width == 0 || height == 0 {
        return (None, 0.0);
    }
    let stride = 3usize;
    let total_pixels = width * height;
    let bg = [
        round_ties_even(background[0] * 255.0).clamp(0, 255) as u8,
        round_ties_even(background[1] * 255.0).clamp(0, 255) as u8,
        round_ties_even(background[2] * 255.0).clamp(0, 255) as u8,
    ];
    let threshold_sq = (params.min_distance * params.min_distance) as i64;

    let mut foreground = vec![false; total_pixels];
    let mut foreground_count = 0usize;
    for idx in 0..total_pixels {
        let offset = idx * stride;
        let rgb = [pix.samples[offset], pix.samples[offset + 1], pix.samples[offset + 2]];
        if color_distance_sq(&rgb, &bg) >= threshold_sq {
            foreground[idx] = true;
            foreground_count += 1;
        }
    }
    if foreground_count == 0 {
        return (None, 0.0);
    }

    let min_component_pixels = params
        .min_component_floor
        .max((total_pixels as f64 * params.min_component_denom) as i64);
    let max_component_pixels = params
        .max_component_floor
        .max((total_pixels as f64 * params.max_component_denom) as i64);

    let mut visited = vec![false; total_pixels];
    // Insertion-ordered buckets so the max-count pick is first-wins on ties,
    // matching Python dict ordering.
    let mut buckets: Vec<((u8, u8, u8), [i64; 4])> = Vec::new();
    let mut kept_pixels = 0usize;

    for start in 0..total_pixels {
        if !foreground[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        let mut queue: Vec<usize> = vec![start];
        let mut head = 0usize;
        let mut component: Vec<usize> = Vec::new();
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        while head < queue.len() {
            let current = queue[head];
            head += 1;
            component.push(current);
            let y = current / width;
            let x = current % width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            if x > 0 {
                let nb = current - 1;
                if foreground[nb] && !visited[nb] {
                    visited[nb] = true;
                    queue.push(nb);
                }
            }
            if x + 1 < width {
                let nb = current + 1;
                if foreground[nb] && !visited[nb] {
                    visited[nb] = true;
                    queue.push(nb);
                }
            }
            if y > 0 {
                let nb = current - width;
                if foreground[nb] && !visited[nb] {
                    visited[nb] = true;
                    queue.push(nb);
                }
            }
            if y + 1 < height {
                let nb = current + width;
                if foreground[nb] && !visited[nb] {
                    visited[nb] = true;
                    queue.push(nb);
                }
            }
        }

        let component_pixels = component.len() as i64;
        if component_pixels < min_component_pixels || component_pixels > max_component_pixels {
            continue;
        }
        let component_width = (max_x - min_x + 1) as f64;
        let component_height = (max_y - min_y + 1) as f64;
        let thin_height_floor = (3.0f64).max(height as f64 * params.thin_height_ratio);
        if component_width >= width as f64 * params.thin_width_ratio && component_height <= thin_height_floor {
            continue;
        }
        if component_width >= width as f64 * params.flat_width_ratio
            && component_height >= height as f64 * params.flat_height_ratio
        {
            continue;
        }

        kept_pixels += component_pixels as usize;
        for &idx in &component {
            let offset = idx * stride;
            let rgb = [pix.samples[offset], pix.samples[offset + 1], pix.samples[offset + 2]];
            let key = (
                (rgb[0] as i32 / params.quantum) as u8,
                (rgb[1] as i32 / params.quantum) as u8,
                (rgb[2] as i32 / params.quantum) as u8,
            );
            match buckets.iter_mut().find(|(k, _)| *k == key) {
                Some((_, sums)) => {
                    sums[0] += rgb[0] as i64;
                    sums[1] += rgb[1] as i64;
                    sums[2] += rgb[2] as i64;
                    sums[3] += 1;
                }
                None => buckets.push((key, [rgb[0] as i64, rgb[1] as i64, rgb[2] as i64, 1])),
            }
        }
    }

    if buckets.is_empty() {
        return (None, 0.0);
    }
    let mut best = 0usize;
    for (i, (_, sums)) in buckets.iter().enumerate().skip(1) {
        if sums[3] > buckets[best].1[3] {
            best = i;
        }
    }
    let bucket = &buckets[best].1;
    let count = bucket[3];
    if count <= 0 {
        return (None, 0.0);
    }
    let confidence = (kept_pixels as f64 / foreground_count.max(1) as f64).clamp(0.35, 0.9);
    (
        Some([
            bucket[0] as f64 / count as f64 / 255.0,
            bucket[1] as f64 / count as f64 / 255.0,
            bucket[2] as f64 / count as f64 / 255.0,
        ]),
        confidence,
    )
}

/// `color_adapt._title_foreground_color_from_pixmap` — title-rule foreground.
pub fn title_foreground_color_from_pixmap(pix: &RgbPixmap, background: [f64; 3]) -> Option<[f64; 3]> {
    foreground_color_impl(pix, background, &TITLE_FOREGROUND_COLOR_PARAMS).0
}

/// `visual_profile/foreground.py::foreground_color_from_pixmap` — visual-profile
/// foreground plus confidence.
pub fn foreground_color_from_pixmap(pix: &RgbPixmap, background: [f64; 3]) -> (Option<[f64; 3]>, f64) {
    foreground_color_impl(pix, background, &FOREGROUND_COLOR_PARAMS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_color_close(actual: Option<[f64; 3]>, expected: [f64; 3], label: &str) {
        let actual = actual.unwrap_or_else(|| panic!("{label}: expected Some"));
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!((a - e).abs() < 1e-9, "{label}[{i}]: actual={a}, expected={e}");
        }
    }

    fn flat_buffer(width: usize, height: usize, value: u8) -> Vec<u8> {
        vec![value; width * height * 3]
    }

    fn paint_block(buf: &mut [u8], width: usize, x0: usize, y0: usize, x1: usize, y1: usize, rgb: [u8; 3]) {
        for y in y0..y1 {
            for x in x0..x1 {
                let o = (y * width + x) * 3;
                buf[o] = rgb[0];
                buf[o + 1] = rgb[1];
                buf[o + 2] = rgb[2];
            }
        }
    }

    #[test]
    fn title_foreground_detects_dark_block_on_light_bg() {
        let w = 200usize;
        let h = 200usize;
        let mut buf = flat_buffer(w, h, 240);
        paint_block(&mut buf, w, 60, 80, 80, 120, [80, 80, 80]);
        let pix = RgbPixmap {
            width: w,
            height: h,
            samples: buf,
        };
        let bg = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0];
        assert_color_close(title_foreground_color_from_pixmap(&pix, bg), [80.0 / 255.0; 3], "title");
    }

    #[test]
    fn foreground_returns_color_and_confidence() {
        let w = 200usize;
        let h = 200usize;
        let mut buf = flat_buffer(w, h, 240);
        paint_block(&mut buf, w, 60, 80, 80, 120, [80, 80, 80]);
        let pix = RgbPixmap {
            width: w,
            height: h,
            samples: buf,
        };
        let bg = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0];
        let (color, confidence) = foreground_color_from_pixmap(&pix, bg);
        assert_color_close(color, [80.0 / 255.0; 3], "foreground");
        // kept == foreground (single clean component) → clamped to the 0.9 cap.
        assert!((confidence - 0.9).abs() < 1e-12, "confidence={confidence}");
    }

    #[test]
    fn flat_background_yields_none() {
        let w = 100usize;
        let h = 100usize;
        let pix = RgbPixmap {
            width: w,
            height: h,
            samples: flat_buffer(w, h, 255),
        };
        let (title, _) = foreground_color_impl(&pix, [1.0, 1.0, 1.0], &TITLE_FOREGROUND_COLOR_PARAMS);
        assert_eq!(title, None);
        let (fg, conf) = foreground_color_from_pixmap(&pix, [1.0, 1.0, 1.0]);
        assert_eq!(fg, None);
        assert_eq!(conf, 0.0);
    }

    #[test]
    fn full_width_thin_line_is_rejected() {
        // A full-width 3px-high line fails the thin-strip rule → no foreground.
        let w = 200usize;
        let h = 200usize;
        let mut buf = flat_buffer(w, h, 240);
        paint_block(&mut buf, w, 0, 100, w, 103, [80, 80, 80]);
        let pix = RgbPixmap {
            width: w,
            height: h,
            samples: buf,
        };
        let bg = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0];
        assert_eq!(title_foreground_color_from_pixmap(&pix, bg), None);
        // foreground.py floor is max(2, int(40000*0.00025))=10 pixels; the line
        // is 200*3=600 pixels, so it is rejected by the thin rule, not the min.
        assert_eq!(foreground_color_from_pixmap(&pix, bg).0, None);
    }

    #[test]
    fn bg_uses_round_ties_even() {
        // background 0.5 → round(127.5) = 128 (ties-to-even), so 128 is treated
        // as background; a 127 pixel is still foreground (distance 1 < threshold,
        // so it is NOT foreground either — the threshold gate dominates).
        let w = 64usize;
        let h = 64usize;
        let buf = flat_buffer(w, h, 128);
        let pix = RgbPixmap {
            width: w,
            height: h,
            samples: buf,
        };
        let (color, _) = foreground_color_impl(&pix, [0.5, 0.5, 0.5], &TITLE_FOREGROUND_COLOR_PARAMS);
        assert_eq!(color, None);
    }

    fn build_text_pdf() -> (mupdf::Document, std::path::PathBuf) {
        use mupdf::shape::Shape;
        use mupdf::{Point, Size};
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        // Unique temp path per call: cargo runs tests in parallel threads, and two
        // callers sharing one file raced on rewrite/open.
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut doc = mupdf::pdf::PdfDocument::new();
        let mut page = doc.new_page(Size::new(612.0, 792.0)).expect("new_page");
        let mut shape = Shape::new(&mut page).expect("shape");
        let opts = mupdf::shape::TextOptions::default();
        shape
            .insert_text(Point::new(72.0, 720.0), "Alpha beta", &opts)
            .expect("insert 1")
            .insert_text(Point::new(72.0, 700.0), "Second line", &opts)
            .expect("insert 2");
        shape.commit(&mut doc, true).expect("commit");
        let dir = std::env::temp_dir().join(format!("rpcoloradapt-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("spans.pdf");
        doc.save(path.to_str().unwrap()).expect("save");
        let render = mupdf::Document::open(path.as_path()).expect("open");
        (render, path)
    }

    #[test]
    fn span_dicts_group_by_line() {
        let (doc, path) = build_text_pdf();
        let page = doc.load_page(0).expect("load");
        let spans = extract_span_dicts_from_page(&page, None).expect("spans");
        assert_eq!(spans.len(), 2, "expected 2 line spans: {spans:?}");
        assert_eq!(spans[0].text, "Alpha beta");
        assert_eq!(spans[1].text, "Second line");
        // single font/color → spans are one run each
        assert_eq!(spans[0].color, spans[1].color);
        assert!(spans[0].rect[2] > spans[0].rect[0]);
        assert!(spans[0].rect[3] > spans[0].rect[1]);
        drop(page);
        drop(doc);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn span_dicts_clip_filters_other_line() {
        let (doc, path) = build_text_pdf();
        let page = doc.load_page(0).expect("load");
        // A clip around the first line (y ~ 700-720) keeps only that span.
        let top_clip = [70.0, 705.0, 300.0, 725.0];
        let spans = extract_span_dicts_from_page(&page, Some(&top_clip)).expect("spans");
        assert_eq!(spans.len(), 1, "clip should keep one line: {spans:?}");
        assert_eq!(spans[0].text, "Alpha beta");
        drop(page);
        drop(doc);
        let _ = std::fs::remove_file(&path);
    }

    /// Hand-built 1-page PDF: 500x700 MediaBox, `/Rotate 90` (→ 700x500 display
    /// space), Helvetica 12pt "RED" at content-space (60, 42.8). Raw content
    /// operators give exact content coordinates (mupdf `Shape` instead transforms
    /// insertion points through the page matrix, so it cannot produce this input).
    fn rotated_red_pdf() -> Vec<u8> {
        // Raw PDF content is in user space (y up, origin bottom-left), so baseline
        // 42.8 in content space (y down, origin top-left) is user-space 700 - 42.8.
        let stream = b"BT /F1 12 Tf 60 657.2 Td (RED) Tj ET\n";
        let content_obj = format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            String::from_utf8_lossy(stream)
        );
        let objects = [
            String::from("<< /Type /Catalog /Pages 2 0 R >>"),
            String::from("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            String::from(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 700] /Rotate 90 \
                 /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
            ),
            String::from("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
            content_obj,
        ];
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(out.len() as u64);
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(obj.as_bytes());
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_pos = out.len() as u64;
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n", objects.len() + 1).as_bytes(),
        );
        out
    }

    #[test]
    fn rotated_page_spans_use_unrotated_content_space() {
        // fitz `get_text("dict")` returns the span in content space (x0≈60, baseline
        // 42.8 inside the y-range); an extraction that applies the page rotation
        // returns display-space coords (~[652, 60, 674, 214]).
        let dir = std::env::temp_dir().join(format!("rpcolorrot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("rot.pdf");
        std::fs::write(&path, rotated_red_pdf()).expect("write");
        let render = mupdf::Document::open(path.as_path()).expect("open");
        let page = render.load_page(0).expect("load");
        let spans = extract_span_dicts_from_page(&page, None).expect("spans");
        assert_eq!(spans.len(), 1, "one RED span: {spans:?}");
        let span = &spans[0];
        assert_eq!(span.text, "RED");
        assert_eq!(span.color, 0);
        let [x0, y0, x1, y1] = span.rect;
        assert!((x0 - 60.0).abs() < 2.0, "content x0={x0} (was display ~652)");
        assert!(x1 < 200.0, "content x1={x1} (was display ~674)");
        assert!(y0 < 42.8 && y1 > 42.8, "baseline 42.8 in y-range [{y0}, {y1}]");
        drop(page);
        drop(render);
        let _ = std::fs::remove_file(&path);
    }
}
