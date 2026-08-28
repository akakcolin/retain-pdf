//! Batched first-line-indent detection over display-list renders.
//!
//! The pixel stage mirrors fitz's
//! `detect_first_line_indent_pt_with_displaylist` (`layout/payload/
//! first_line_indent.py`): each candidate item bbox is clipped against the
//! page, rendered to device gray via the display list, and analyzed by
//! `rendering_core::first_line_indent::detect_first_line_indent_pt_from_samples`
//! (which applies INDENT_RENDER_SCALE, thresholds and clamps). The candidate
//! gate itself is semantic and stays on the caller side.
//!
//! Unreadable pages / failed clip renders yield `None` entries — the caller
//! (bridge shim) maps them to `0.0`, matching the Python reference (empty or
//! out-of-page clip -> empty pixmap -> 0.0).

use std::collections::BTreeMap;

use mupdf::Document;
use rendering_core::first_line_indent::detect_first_line_indent_pt_from_samples;
use rendering_core::rect::Rect;

use crate::render::{render_display_list_clip_gray, RenderedPixels};

/// Render scale for the first-line-indent pixmap (`first_line_indent.py:18`).
/// The analysis halves the pixel offset back to points by this factor.
pub const INDENT_RENDER_SCALE: f32 = 2.0;

/// Detect per-candidate first-line indents. Candidates are grouped by page so
/// each page's display list is built once (mirrors prepare.py's lazy reuse).
/// `candidates[page_idx] = [(bbox [x0,y0,x1,y1] page-space, font_size_pt), ...]`;
/// results keep input order, with `None` when the page could not be read or a
/// clip render failed.
#[allow(clippy::type_complexity)]
pub fn detect_first_line_indents(
    doc: &Document,
    page_indices: &[i64],
    candidates: &BTreeMap<i64, Vec<([f64; 4], f64)>>,
) -> BTreeMap<i64, Vec<Option<f64>>> {
    let mut out = BTreeMap::new();
    for idx in page_indices {
        let Some(page_candidates) = candidates.get(idx) else {
            continue;
        };
        let page = match doc.load_page(*idx as i32) {
            Ok(page) => page,
            Err(_) => {
                out.insert(*idx, vec![None; page_candidates.len()]);
                continue;
            }
        };
        let dl = match page.to_display_list(false) {
            Ok(dl) => dl,
            Err(_) => {
                out.insert(*idx, vec![None; page_candidates.len()]);
                continue;
            }
        };
        let rendered = page_candidates
            .iter()
            .map(|&([x0, y0, x1, y1], font_size_pt)| {
                let clip = Rect::new(x0, y0, x1, y1);
                let pixels = render_display_list_clip_gray(&dl, Some(&clip), INDENT_RENDER_SCALE);
                Some(indent_pt_from_pixels(pixels.ok()?, font_size_pt))
            })
            .collect();
        out.insert(*idx, rendered);
    }
    out
}

fn indent_pt_from_pixels(pixels: RenderedPixels, font_size_pt: f64) -> f64 {
    detect_first_line_indent_pt_from_samples(
        &pixels.samples,
        pixels.width as usize,
        pixels.height as usize,
        font_size_pt,
    )
}
