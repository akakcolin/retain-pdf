//! Port of the `source/cleanup` text-read family — `text_extract.py` +
//! `math_spans.py`, the `page.get_text("dict"/"blocks")` consumers.
//!
//! Four collectors mirror fitz `get_text` with the SAME extraction flags
//! (`TEXTFLAGS_DICT`/`TEXTFLAGS_BLOCKS` = PRESERVE_LIGATURES | PRESERVE_IMAGES |
//! PRESERVE_WHITESPACE — see PyMuPDF `Page.get_textpage`) and in the SAME
//! rotation-stripped content space (fitz `get_text` clears `/Rotate` before
//! extracting — mirror of `color_adapt::build_text_page_unrotated`).
//!
//! - `collect_page_text_spans` — `text_extract.extract_page_text_spans`: fitz
//!   `get_text("dict")` spans. A span is a run of consecutive `TextChar`s
//!   within a line sharing the same (stripped font name, size, argb,
//!   masked char flags, font-style flags) — the fitz span grouping key from
//!   PyMuPDF `JM_make_spanlist`. The span bbox is the union of the *corrected*
//!   per-char bboxes (`JM_char_quad`, see `char_bbox` below); text = char
//!   concatenation (untrimmed; the Python consumer `.strip()`s and drops empty
//!   spans).
//! - `collect_page_text_blocks` — `text_extract.extract_page_text_blocks`:
//!   fitz `get_text("blocks")` text blocks. bbox = block bounds; text = the
//!   block's lines joined by "\n" (fitz appends a trailing "\n"; the consumer
//!   strips it).
//! - `collect_page_math_rects` —
//!   `math_spans.collect_page_math_protection_rects`: the rect of every span
//!   whose font name (trimmed, lowercased) contains any
//!   `SPECIAL_MATH_FONT_MARKERS` marker, deduped by `rect_key`
//!   (`int(round(x * 10))` per coordinate, banker's rounding).
//! - `collect_page_span_heights` —
//!   `math_spans.collect_page_non_math_span_heights`: `max(0, y1 - y0)` for
//!   every span with trimmed non-empty, non-math text, kept when > 0.5.
//!
//! Span geometry fidelity (PyMuPDF `src/extra.i`):
//! - **bbox**: `JM_char_quad` corrects the raw glyph quad when the font's
//!   `asc - dsc + EPSILON < 1` (sub-1 em-box fonts — e.g. AdvOT) by
//!   re-deriving the vertical extent from the font ascender/descender scaled to
//!   `fsize` (so `y1 - y0 == fsize`), placed at `origin.y -/+ asc`/`dsc` with
//!   the up-down flip decided by `quad.ul.y > origin.y`, and the horizontal
//!   extent `[max(quad.ll.x, origin.x), quad.lr.x]` (zero-width glyphs fall
//!   back to `advance * fsize`). Base-14 em-box fonts (`asc_dsc >= 1`) and all
//!   vertical (`wmode == 1`) lines keep the raw quad. `g_skip_quad_corrections`
//!   / `g_small_glyph_heights` are both 0 in production. A line is treated as
//!   horizontal when its establishing glyph's quad width-axis advances rightward
//!   (`line_is_horizontal`) — mupdf sets `line->dir` from the first char's text
//!   matrix, which mupdf-rs cannot read directly; rotated sidebars and
//!   right-to-left lines fall back to the raw quad.
//! - **font style flags**: `JM_char_font_flags` = superscript*1 + italic*2 +
//!   serif*4 + monospaced*8 + bold*16, where superscript is
//!   `origin.y < first_char.origin.y - size * 0.1` on horizontal lines.
//! - **char flags**: fitz groups on `ch->flags & ~FZ_STEXT_SYNTHETIC`.
//!   mupdf-rs's bundled mupdf additionally sets `SYNTHETIC_LARGE` on
//!   gap-inserted spaces (a flag PyMuPDF 1.26.x's mupdf does not emit for the
//!   same PDFs), so both are masked here to keep the space inside the run
//!   exactly as fitz records it.
//! - **font name**: `JM_font_name` strips the 6-char subset prefix ("XXXXXX+")
//!   from the raw font name (with `g_subset_fontnames == 0`).
//! - **page clip**: `JM_make_textpage_dict` drops chars whose corrected bbox
//!   does not overlap the page mediabox (`tp->mediabox`).
//!
//! Word segmentation is deliberately NOT ported: fitz `get_text("words",
//! clip=...)` truncates words to chars whose glyph-ink bbox is not entirely
//! outside the clip (`fz_glyph_entirely_outside_box`), which mupdf-rs cannot
//! reproduce (no clip-capable text page, no glyph-ink access), so
//! `extract_item_word_entries` stays on the Python reference.

use mupdf::pdf::PdfPage;
use mupdf::text_page::{TextBlockType, TextChar, TextCharFlags, TextPage};
use mupdf::{Error, Font, Rect, TextPageFlags, WriteMode};
use rendering_core::rect::Rect as CoreRect;

use crate::error::PdfError;

/// fitz `get_text("dict"/"blocks")` flags (`TEXTFLAGS_DICT`/`TEXTFLAGS_BLOCKS`).
/// Both are `PRESERVE_LIGATURES | PRESERVE_WHITESPACE | CLIP |
/// USE_CID_FOR_UNKNOWN_UNICODE` (plus PRESERVE_IMAGES for dict). `CLIP` drops
/// glyphs whose ink is entirely outside the page mediabox at extraction time;
/// `USE_CID_FOR_UNKNOWN_UNICODE` resolves special-font glyphs (no ToUnicode —
/// e.g. the golden PDFs' "Pi" fonts) to their CID instead of U+FFFD.
const TEXT_READ_FLAGS: TextPageFlags = TextPageFlags::PRESERVE_LIGATURES
    .union(TextPageFlags::PRESERVE_IMAGES)
    .union(TextPageFlags::PRESERVE_WHITESPACE)
    .union(TextPageFlags::CLIP)
    .union(TextPageFlags::USE_CID_FOR_UNKNOWN_UNICODE);

/// `FLT_EPSILON`, used by `JM_char_quad`'s `asc - dsc + FLT_EPSILON`.
const FLT_EPSILON: f32 = 1.192_092_9e-7;

/// `JM_make_spanlist`'s per-char grouping thresholds.
const SUP_SCRIPT_THRESHOLD: f32 = 0.1;
/// `JM_char_quad`'s glyphless-font sentinel (`asc < 1e-3`).
const GLYPHLESS_ASC: f32 = 1e-3;

/// Mirror of `math_fonts.py`'s `SPECIAL_MATH_FONT_MARKERS` (8 items). Drift is
/// guarded by the math_font corpus case and the three-way smoke.
const SPECIAL_MATH_FONT_MARKERS: [&str; 8] = [
    "pearsonmath",
    "mathematicalpi",
    "newcmmath",
    "stixmath",
    "cambria math",
    "latinmodernmath",
    "xitsmath",
    "math",
];

/// `math_fonts.is_special_math_font`: trimmed-lowercased name contains a marker.
fn is_special_math_font(font_name: &str) -> bool {
    let normalized = font_name.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    SPECIAL_MATH_FONT_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

/// The fitz span-grouping char flags: `ch->flags & ~FZ_STEXT_SYNTHETIC`
/// (`JM_make_spanlist`). SYNTHETIC marks a gap-inserted space, which fitz
/// merges into the surrounding run regardless of the gap size (in mupdf-rs's
/// bundled mupdf large gaps additionally set SYNTHETIC_LARGE, a flag PyMuPDF
/// 1.26.x's mupdf does not emit for the same PDFs — masked here to keep the
/// space inside the run exactly as fitz records it).
fn char_flags(flags: TextCharFlags) -> u16 {
    (flags.bits() & !(TextCharFlags::SYNTHETIC.bits() | TextCharFlags::SYNTHETIC_LARGE.bits())) as u16
}

/// `JM_font_name` (with `g_subset_fontnames == 0`): strip a 6-char subset
/// prefix ("XXXXXX+") from the raw font name.
fn font_name(font: &Font) -> String {
    match font.name().find('+') {
        Some(pos) if pos == 6 => font.name()[pos + 1..].to_string(),
        _ => font.name().to_string(),
    }
}

/// `JM_char_font_flags` (with `g_skip_quad_corrections == 0`): font-style
/// flags = superscript*1 + italic*2 + serif*4 + monospaced*8 + bold*16.
/// `detect_super_script` only fires on horizontal lines (`line->wmode == 0`
/// and exact `line->dir == (1, 0)`); vertical lines and rotated-direction
/// horizontal lines (e.g. 90-degree sidebars) always yield 0 for the
/// superscript bit.
fn font_style_flags(
    font: &Font,
    wmode: WriteMode,
    is_horizontal: bool,
    first_char_y: f32,
    ch: &TextChar,
) -> u32 {
    let mut flags: u32 = 0;
    if wmode == WriteMode::Horizontal && is_horizontal {
        let origin = ch.origin();
        if origin.y < first_char_y - ch.size() * SUP_SCRIPT_THRESHOLD {
            flags += 1;
        }
    }
    if font.is_italic() {
        flags += 2;
    }
    if font.is_serif() {
        flags += 4;
    }
    if font.is_monospaced() {
        flags += 8;
    }
    if font.is_bold() {
        flags += 16;
    }
    flags
}

/// `JM_char_bbox` — rect-from-quad of `JM_char_quad`, specialized for
/// horizontal lines (`dir = (1, 0)`, `wmode = 0`). With
/// `g_skip_quad_corrections == 0` and `g_small_glyph_heights == 0`:
/// - vertical lines (`wmode != 0`) and rotated-direction horizontal lines
///   (sidebars) return the raw quad (fitz's full rotation math for
///   `dir != (1, 0)` cannot be reproduced without `line.dir`);
/// - fonts with `asc - dsc + FLT_EPSILON >= 1` (full em-box, e.g. base-14)
///   return the raw quad;
/// - otherwise the vertical extent is re-derived from the font ascender/
///   descender normalized so `asc - dsc == fsize`, with the top/bottom at
///   `origin.y -/+ asc`/`dsc` and the up-down flip decided by whether the raw
///   glyph top sits below the baseline (`quad.ul.y > origin.y`); the
///   horizontal extent is `[max(quad.ll.x, origin.x), quad.lr.x]`, with a
///   zero-width glyph falling back to `advance * fsize`.
fn char_bbox(ch: &TextChar, font: Option<&Font>, wmode: WriteMode, is_horizontal: bool) -> Rect {
    if wmode != WriteMode::Horizontal || !is_horizontal {
        return Rect::from(ch.quad());
    }
    let quad = ch.quad();
    let origin = ch.origin();
    let fsize = ch.size();

    let (mut asc, mut dsc) = match font {
        Some(f) => (f.ascender(), f.descender()),
        None => (0.0, 0.0),
    };
    let mut asc_dsc = asc - dsc + FLT_EPSILON;
    if asc_dsc >= 1.0 {
        return Rect::from(quad);
    }
    if asc < GLYPHLESS_ASC {
        dsc = -0.1;
        asc = 0.9;
        asc_dsc = 1.0;
    }
    if asc_dsc < 1.0 {
        dsc /= asc_dsc;
        asc /= asc_dsc;
    }
    asc_dsc = asc - dsc;
    asc = asc * fsize / asc_dsc;
    dsc = dsc * fsize / asc_dsc;

    // Translated-to-origin frame: the up-down flip fires when the glyph's top
    // is below the baseline (`quad.ul.y > origin.y` in the original frame).
    let (y0, y1) = if quad.ul.y > origin.y {
        (origin.y + dsc, origin.y + asc)
    } else {
        (origin.y - asc, origin.y - dsc)
    };

    let x0 = quad.ll.x.max(origin.x);
    let mut x1 = quad.lr.x;
    if x1 - x0 < FLT_EPSILON {
        if let (Some(f), Some(c)) = (font, ch.char()) {
            if let Ok(glyph) = f.encode_character(c as i32) {
                if glyph != 0 {
                    if let Ok(fwidth) = f.advance_glyph(glyph) {
                        x1 = x0 + fwidth * fsize;
                    }
                }
            }
        }
    }
    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

/// `JM_rects_overlap`: `false` iff either rect is entirely outside the other.
fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    !(a.x0 >= b.x1 || a.y0 >= b.y1 || a.x1 <= b.x0 || a.y1 <= b.y0)
}

/// Derive the line direction (mupdf-rs exposes no `line.dir`) from the first
/// glyph's quad width-axis — the `ll -> lr` edge of the glyph's (unrotated,
/// font-metric) bbox. mupdf sets `line->dir` at line creation from the
/// establishing run's text matrix: `dir = transform_vector((1,0), trm)`, so a
/// horizontal run's quad `ll->lr` edge advances rightward (`dx > 0`, `dy == 0`)
/// while a 90-degree-rotated run (a sidebar) advances vertically (`dx == 0`).
/// Origin deltas are NOT a reliable proxy (they carry cross-run baseline shifts
/// between different font sizes, e.g. a large overhanging pi-font glyph), but
/// the quad width-axis is exact to float precision. Degenerate (zero-area)
/// leading quads are skipped; if every quad is degenerate the first origin
/// delta decides; single-char lines default to horizontal (the superscript
/// test is vacuous for them — `origin.y < origin.y - size*0.1` is never true).
fn line_is_horizontal(chars: &[TextChar<'_>]) -> bool {
    for ch in chars {
        let q = ch.quad();
        let dx = q.lr.x - q.ll.x;
        let dy = q.lr.y - q.ll.y;
        if dx == 0.0 && dy == 0.0 {
            continue;
        }
        return dx > 0.0 && dy.abs() <= dx * 0.01;
    }
    if chars.len() > 1 {
        let (a, b) = (chars[0].origin(), chars[1].origin());
        let dx = b.x - a.x;
        let dy = (b.y - a.y).abs();
        return dx > 0.0 && dy < dx;
    }
    true
}

/// `rects.rect_key`: per-coordinate `int(round(x * 10))` (banker's rounding).
fn rect_key(rect: Rect) -> (i64, i64, i64, i64) {
    (
        (rect.x0 as f64 * 10.0).round_ties_even() as i64,
        (rect.y0 as f64 * 10.0).round_ties_even() as i64,
        (rect.x1 as f64 * 10.0).round_ties_even() as i64,
        (rect.y1 as f64 * 10.0).round_ties_even() as i64,
    )
}

/// A grouped span run — the fitz `get_text("dict")` span unit. The grouping
/// key mirrors `JM_make_spanlist`'s `char_style`: `font` (subset-stripped),
/// `size`, `argb` (full, alpha included), `char_flags` (SYNTHETIC-masked) and
/// `style_flags` (font-style bits).
#[derive(Debug, Clone, PartialEq)]
struct SpanRun {
    rect: Rect,
    text: String,
    font_name: String,
    size: f32,
    argb: u32,
    char_flags: u16,
    style_flags: u32,
}

/// fitz `Page.get_textpage` mirror: clear `/Rotate` so extraction yields
/// content-space spans, then restore. Returns the page mediabox (content space)
/// alongside the text page — the `tp->mediabox` clip for `JM_make_textpage_dict`.
fn build_text_page_unrotated(mut pdf_page: PdfPage) -> Result<(TextPage, Rect), Error> {
    let rotation = pdf_page.rotation()?;
    if rotation % 90 != 0 {
        return Err(Error::InvalidArgument(format!(
            "page rotation must be a multiple of 90, got {rotation}"
        )));
    }
    if rotation == 0 {
        let media = pdf_page.bounds()?;
        let text_page = pdf_page.to_text_page(TEXT_READ_FLAGS)?;
        return Ok((text_page, media));
    }
    let obj = pdf_page.object();
    let had_direct_rotate = obj.get_dict("Rotate")?.is_some();
    pdf_page.set_rotation(0)?;
    let media = pdf_page.bounds()?;
    let extracted = pdf_page.to_text_page(TEXT_READ_FLAGS);
    let restored = if had_direct_rotate {
        pdf_page.set_rotation(rotation)
    } else {
        let mut obj = pdf_page.object();
        obj.dict_delete("Rotate")
    };
    match (extracted, restored) {
        (Ok(text_page), Ok(())) => Ok((text_page, media)),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

/// One kept text line's raw kept-char concatenation (the "\n"-joined block
/// text source).
struct LineData {
    text: String,
}

/// A kept text block: the dict/"blocks" bbox (union of the kept lines'
/// corrected rects — fitz's block bbox is NOT `fz_stext_block.bbox`), the kept
/// lines' corrected geometry, and the span runs.
struct BlockData {
    bbox: Rect,
    lines: Vec<LineData>,
    runs: Vec<SpanRun>,
}

/// Traverse every kept text block's chars, mirroring `JM_make_textpage_dict` +
/// `JM_make_text_block` + `JM_make_spanlist`:
/// - block kept iff its raw `fz_stext_block.bbox` overlaps the page mediabox;
/// - line kept iff its raw `fz_stext_line.bbox` overlaps the page mediabox;
/// - char kept iff its *corrected* bbox overlaps the page mediabox;
/// - span runs group kept chars by the `char_style` key; each span rect is the
///   union of its kept chars' corrected bboxes;
/// - the block bbox and each line rect are the union of the kept chars'
///   corrected bboxes.
fn collect_block_data(text_page: &TextPage, media: &Rect) -> Vec<BlockData> {
    let mut blocks: Vec<BlockData> = Vec::new();
    for block in text_page.blocks() {
        if block.r#type() != TextBlockType::Text {
            continue;
        }
        let block_rect = block.bounds();
        if !rects_overlap(media, &block_rect) {
            continue;
        }
        let mut block_bbox = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut lines: Vec<LineData> = Vec::new();
        let mut runs: Vec<SpanRun> = Vec::new();
        for line in block.lines() {
            if !rects_overlap(media, &line.bounds()) {
                continue;
            }
            let wmode = line.wmode();
            let chars: Vec<TextChar> = line.chars().collect();
            let is_horizontal = line_is_horizontal(&chars);
            let first_char_y = chars.first().map(|c| c.origin().y).unwrap_or(0.0);
            let mut current: Option<SpanRun> = None;
            let mut current_rects: Vec<Rect> = Vec::new();
            let mut current_text = String::new();
            let mut line_bbox = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            let mut line_text = String::new();
            for ch in &chars {
                let font = ch.font();
                let bbox = char_bbox(ch, font.as_ref(), wmode, is_horizontal);
                if !rects_overlap(media, &bbox) {
                    continue;
                }
                line_bbox.x0 = line_bbox.x0.min(bbox.x0);
                line_bbox.y0 = line_bbox.y0.min(bbox.y0);
                line_bbox.x1 = line_bbox.x1.max(bbox.x1);
                line_bbox.y1 = line_bbox.y1.max(bbox.y1);
                if let Some(c) = ch.char() {
                    line_text.push(c);
                }
                let font_name = font.as_ref().map(font_name).unwrap_or_default();
                let size = ch.size();
                let argb = ch.argb();
                let flags = char_flags(ch.flags());
                let style_flags = font
                    .as_ref()
                    .map(|f| font_style_flags(f, wmode, is_horizontal, first_char_y, ch))
                    .unwrap_or(0);
                let same_run = match &current {
                    Some(run) => {
                        run.font_name == font_name
                            && run.size == size
                            && run.argb == argb
                            && run.char_flags == flags
                            && run.style_flags == style_flags
                    }
                    None => false,
                };
                if same_run {
                    current_rects.push(bbox);
                    if let Some(c) = ch.char() {
                        current_text.push(c);
                    }
                } else {
                    flush_run(&mut runs, &mut current, &mut current_rects, &mut current_text);
                    current = Some(SpanRun {
                        rect: bbox,
                        text: String::new(),
                        font_name,
                        size,
                        argb,
                        char_flags: flags,
                        style_flags,
                    });
                    current_rects.push(bbox);
                    if let Some(c) = ch.char() {
                        current_text.push(c);
                    }
                }
            }
            flush_run(&mut runs, &mut current, &mut current_rects, &mut current_text);
            if !line_bbox.x0.is_infinite() {
                block_bbox.x0 = block_bbox.x0.min(line_bbox.x0);
                block_bbox.y0 = block_bbox.y0.min(line_bbox.y0);
                block_bbox.x1 = block_bbox.x1.max(line_bbox.x1);
                block_bbox.y1 = block_bbox.y1.max(line_bbox.y1);
                lines.push(LineData { text: line_text });
            }
        }
        blocks.push(BlockData {
            bbox: block_bbox,
            lines,
            runs,
        });
    }
    blocks
}

/// All span runs across every kept text block.
fn collect_span_runs(text_page: &TextPage, media: &Rect) -> Vec<SpanRun> {
    collect_block_data(text_page, media)
        .into_iter()
        .flat_map(|block| block.runs)
        .collect()
}

fn flush_run(
    runs: &mut Vec<SpanRun>,
    current: &mut Option<SpanRun>,
    current_rects: &mut Vec<Rect>,
    current_text: &mut String,
) {
    if let Some(mut run) = current.take() {
        run.rect = bounds_of(current_rects);
        if !run.rect.is_empty() {
            run.text = std::mem::take(current_text);
            runs.push(run);
        }
        current_text.clear();
        current_rects.clear();
    }
}

fn bounds_of(rects: &[Rect]) -> Rect {
    let mut rect = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for r in rects {
        rect.x0 = rect.x0.min(r.x0);
        rect.y0 = rect.y0.min(r.y0);
        rect.x1 = rect.x1.max(r.x1);
        rect.y1 = rect.y1.max(r.y1);
    }
    rect
}

fn to_core_rect(r: &Rect) -> CoreRect {
    CoreRect::new(r.x0 as f64, r.y0 as f64, r.x1 as f64, r.y1 as f64)
}

/// `text_extract.extract_page_text_spans` — fitz `get_text("dict")` spans:
/// `(bbox, stripped-text)` pairs, non-empty text blocks only.
pub fn collect_page_text_spans(page: PdfPage) -> Result<Vec<(CoreRect, String)>, PdfError> {
    let (text_page, media) = build_text_page_unrotated(page)?;
    Ok(collect_span_runs(&text_page, &media)
        .into_iter()
        .filter_map(|run| {
            let text = run.text.trim().to_string();
            if text.is_empty() || run.rect.is_empty() {
                return None;
            }
            Some((to_core_rect(&run.rect), text))
        })
        .collect())
}

/// `text_extract.extract_page_text_blocks` — fitz `get_text("blocks")` text
/// blocks: `(bbox, stripped-text)` pairs. The block bbox is the union of the
/// kept lines' corrected rects (fitz's "blocks" bbox, not the raw
/// `fz_stext_block.bbox`).
pub fn collect_page_text_blocks(page: PdfPage) -> Result<Vec<(CoreRect, String)>, PdfError> {
    let (text_page, media) = build_text_page_unrotated(page)?;
    Ok(collect_block_data(&text_page, &media)
        .into_iter()
        .filter_map(|block| {
            let text = block
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let text = text.trim().to_string();
            if text.is_empty() || block.bbox.is_empty() {
                return None;
            }
            Some((to_core_rect(&block.bbox), text))
        })
        .collect())
}

/// `math_spans.collect_page_math_protection_rects` — deduped rects of
/// math-font spans.
pub fn collect_page_math_rects(page: PdfPage) -> Result<Vec<CoreRect>, PdfError> {
    let (text_page, media) = build_text_page_unrotated(page)?;
    let mut rects: Vec<CoreRect> = Vec::new();
    let mut seen: std::collections::HashSet<(i64, i64, i64, i64)> = std::collections::HashSet::new();
    for run in collect_span_runs(&text_page, &media) {
        if !is_special_math_font(&run.font_name) {
            continue;
        }
        if run.rect.is_empty() {
            continue;
        }
        let key = rect_key(run.rect);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        rects.push(to_core_rect(&run.rect));
    }
    Ok(rects)
}

/// `math_spans.collect_page_non_math_span_heights` — non-math span heights
/// > 0.5.
pub fn collect_page_span_heights(page: PdfPage) -> Result<Vec<f64>, PdfError> {
    let (text_page, media) = build_text_page_unrotated(page)?;
    let mut heights = Vec::new();
    for run in collect_span_runs(&text_page, &media) {
        if run.text.trim().is_empty() || is_special_math_font(&run.font_name) {
            continue;
        }
        if run.rect.is_empty() {
            continue;
        }
        let height = (run.rect.y1 as f64 - run.rect.y0 as f64).max(0.0);
        if height > 0.5 {
            heights.push(height);
        }
    }
    Ok(heights)
}
