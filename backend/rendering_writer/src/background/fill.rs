//! Port of `source/background/fill.py` (pure pixel subset). Implements the
//! 8-bin dominant-fill histogram, trimmed robust-median fill, p90-p10
//! brightness-spread gate, text-contamination rejection, and the batched
//! `LocalBackgroundSampler` — all over raw RGB sample buffers. The fitz
//! rendering glue (`_clip_pixmap` and the page-level `sample_local_background_fill`)
//! lives in Phase 5D-7 alongside `detect`/`extract`/`image_route`.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::*;
use super::patch::{
    patch_candidate_rects, rect_area, rect_height, rect_intersection, rect_is_empty, rect_is_finite,
    rect_width, rect_union,
};
use super::sampling::quantile;

pub type Rgb = [u8; 3];

/// `int((r + g + b) / 3)` — truncating mean (non-negative samples).
fn brightness(pixel: &Rgb) -> u8 {
    ((pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3) as u8
}

/// `fill._brightness_spread` (identical to `patch.brightness_spread`): p90-p10
/// over per-pixel brightness. Empty input → 255.
pub fn brightness_spread(pixels: &[Rgb]) -> u8 {
    if pixels.is_empty() {
        return 255;
    }
    let mut values: Vec<u8> = pixels.iter().map(brightness).collect();
    values.sort_unstable();
    quantile(&values, 9, 10) - quantile(&values, 1, 10)
}

/// `fill._looks_like_text_contaminated_light_patch` (identical to
/// `patch.looks_like_text_contaminated_light_patch`).
pub fn looks_like_text_contaminated_light_patch(pixels: &[Rgb]) -> bool {
    if pixels.is_empty() {
        return false;
    }
    let mut values: Vec<u8> = pixels.iter().map(brightness).collect();
    values.sort_unstable();
    let median = quantile(&values, 1, 2);
    let p90 = quantile(&values, 9, 10);
    if median < BACKGROUND_PATCH_LIGHT_BG_MEDIAN_MIN || p90 < BACKGROUND_PATCH_LIGHT_BG_P90_MIN {
        return false;
    }
    let dark = values
        .iter()
        .filter(|&&v| v < BACKGROUND_PATCH_TEXT_CONTAMINATION_DARK_VALUE)
        .count();
    let dark_ratio = dark as f64 / values.len().max(1) as f64;
    dark_ratio >= BACKGROUND_PATCH_TEXT_CONTAMINATION_DARK_RATIO
}

/// Per-channel median (`quantile(., 1, 2)`), normalized to 0..1.
fn channel_medians(pixels: &[Rgb]) -> [f64; 3] {
    let mut rs: Vec<u8> = pixels.iter().map(|p| p[0]).collect();
    let mut gs: Vec<u8> = pixels.iter().map(|p| p[1]).collect();
    let mut bs: Vec<u8> = pixels.iter().map(|p| p[2]).collect();
    rs.sort_unstable();
    gs.sort_unstable();
    bs.sort_unstable();
    [
        quantile(&rs, 1, 2) as f64 / 255.0,
        quantile(&gs, 1, 2) as f64 / 255.0,
        quantile(&bs, 1, 2) as f64 / 255.0,
    ]
}

/// `fill._robust_fill_from_pixels` — drop the darkest 1/5 of pixels (by
/// brightness), then take the per-channel median. `None` below the minimum
/// sample count.
pub fn robust_fill_from_pixels(pixels: &[Rgb]) -> Option<[f64; 3]> {
    if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
        return None;
    }
    let mut ordered: Vec<(u8, Rgb)> = pixels.iter().map(|p| (brightness(p), *p)).collect();
    // Python `sorted(...)` is stable; `sort_by_key` is stable too.
    ordered.sort_by_key(|(b, _)| *b);
    let keep_from = ordered.len().saturating_sub(1).min(ordered.len() / 5);
    let trimmed: Vec<Rgb> = ordered[keep_from..].iter().map(|(_, p)| *p).collect();
    if trimmed.is_empty() {
        return None;
    }
    Some(channel_medians(&trimmed))
}

/// `fill._dominant_nonwhite_fill_from_pixels` — 8-bin histogram; accept the
/// most-populated bin if it holds ≥ 35% of pixels and its median fill is
/// non-white (no channel ≥ 0.98). Python `max(..., key=len)` keeps the FIRST
/// bin on size ties, so iteration order (insertion) is preserved.
pub fn dominant_nonwhite_fill_from_pixels(pixels: &[Rgb]) -> Option<[f64; 3]> {
    if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
        return None;
    }
    let bin_size = BACKGROUND_FILL_DOMINANT_BIN_SIZE.max(1);
    let mut bins: Vec<((u8, u8, u8), Vec<Rgb>)> = Vec::new();
    for pixel in pixels {
        let key = (pixel[0] / bin_size, pixel[1] / bin_size, pixel[2] / bin_size);
        match bins.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(*pixel),
            None => bins.push((key, vec![*pixel])),
        }
    }
    let mut dominant_idx = 0;
    for (i, (_, group)) in bins.iter().enumerate().skip(1) {
        if group.len() > bins[dominant_idx].1.len() {
            dominant_idx = i;
        }
    }
    let dominant = &bins[dominant_idx].1;
    if dominant.len() as f64 / (pixels.len().max(1) as f64) < BACKGROUND_FILL_DOMINANT_MIN_RATIO {
        return None;
    }
    let fill = channel_medians(dominant);
    if fill.iter().copied().fold(f64::NEG_INFINITY, f64::max) >= BACKGROUND_FILL_NONWHITE_MAX_CHANNEL {
        return None;
    }
    Some(fill)
}

/// `fill._sample_step_for_bounds` — subsampling stride so large regions stay
/// under `BACKGROUND_COVER_MAX_SAMPLE_PIXELS`.
pub fn sample_step_for_bounds(x0: usize, y0: usize, x1: usize, y1: usize) -> usize {
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    let total = width.saturating_mul(height);
    if total <= BACKGROUND_COVER_MAX_SAMPLE_PIXELS {
        return 1;
    }
    ((total as f64 / BACKGROUND_COVER_MAX_SAMPLE_PIXELS as f64).sqrt() as usize).max(1)
}

/// `fill._pixels_in_bounds` — RGB triples sampled with `sample_step_for_bounds`.
pub fn pixels_in_bounds(
    samples: &[u8],
    pixmap_width: usize,
    stride: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> Vec<Rgb> {
    let step = sample_step_for_bounds(x0, y0, x1, y1);
    let mut pixels = Vec::new();
    for y in (y0..y1).step_by(step) {
        let row_offset = y * pixmap_width * stride;
        for x in (x0..x1).step_by(step) {
            let offset = row_offset + x * stride;
            pixels.push([samples[offset], samples[offset + 1], samples[offset + 2]]);
        }
    }
    pixels
}

/// `fill._pixels_in_rect_excluding` — sample a bounds rectangle while skipping
/// an inner excluded rectangle (both already in pixel coordinates).
#[allow(clippy::too_many_arguments)]
pub fn pixels_in_rect_excluding_bounds(
    samples: &[u8],
    pixmap_width: usize,
    stride: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    ex0: usize,
    ey0: usize,
    ex1: usize,
    ey1: usize,
) -> Vec<Rgb> {
    let step = sample_step_for_bounds(x0, y0, x1, y1);
    let mut pixels = Vec::new();
    for y in (y0..y1).step_by(step) {
        let inside_y = (ey0..ey1).contains(&y);
        let row_offset = y * pixmap_width * stride;
        for x in (x0..x1).step_by(step) {
            if inside_y && (ex0..ex1).contains(&x) {
                continue;
            }
            let offset = row_offset + x * stride;
            pixels.push([samples[offset], samples[offset + 1], samples[offset + 2]]);
        }
    }
    pixels
}

/// `fill._coord_to_pixel` — page coordinate → pixel index with truncation
/// (`ceil=false`) or `int(raw + 0.999)` (`ceil=true`), clamped to `[0, pixels]`.
pub fn coord_to_pixel(value: f64, origin: f64, span: f64, pixels: usize, ceil: bool) -> usize {
    let raw = (value - origin) / span.max(1e-6) * pixels as f64;
    let i = if ceil { (raw + 0.999) as i64 } else { raw as i64 };
    i.clamp(0, pixels as i64) as usize
}

/// `fill._rect_contains` — `rect` is fully inside `container` (within 0.01pt²).
pub fn rect_contains(container: &RectTuple, rect: &RectTuple) -> bool {
    let clipped = rect_intersection(rect, container);
    if rect_is_empty(&clipped) {
        return false;
    }
    (rect_area(&clipped) - rect_area(rect)).abs() <= 0.01
}

/// `fill._background_sample_outer_rect_from_page_rect` — `rect` inflated by the
/// sample margin and clipped to the page; `None` if the result collapses to a
/// sliver (≤ 1pt in either axis).
pub fn background_sample_outer_rect_from_page_rect(
    page_rect: &RectTuple,
    rect: &RectTuple,
) -> Option<RectTuple> {
    let margin = BACKGROUND_COVER_SAMPLE_MARGIN_PT;
    let expanded = [rect[0] - margin, rect[1] - margin, rect[2] + margin, rect[3] + margin];
    let outer = rect_intersection(&expanded, page_rect);
    if rect_is_empty(&outer) || rect_width(&outer) <= 1.0 || rect_height(&outer) <= 1.0 {
        return None;
    }
    Some(outer)
}

/// `fill._batch_sampler_clip_rect` — union of the sampling rects (inflated by
/// `EXTRA_MARGIN_PT` and clipped to the page). Falls back to the full page when
/// the union covers > 35% of the page and the caller allows it; `None` when
/// there are too few valid rects or the fallback is disallowed.
pub fn batch_sampler_clip_rect(
    page_rect: &RectTuple,
    rects: &[RectTuple],
    allow_full_page: bool,
) -> Option<RectTuple> {
    let valid: Vec<RectTuple> = rects
        .iter()
        .copied()
        .filter(|r| !rect_is_empty(r) && rect_is_finite(r))
        .collect();
    if valid.len() < BACKGROUND_CLIP_SAMPLER_MIN_RECTS {
        return None;
    }
    let mut clip: Option<RectTuple> = None;
    for rect in &valid {
        let outer = background_sample_outer_rect_from_page_rect(page_rect, rect).unwrap_or(*rect);
        let expanded = rect_intersection(
            &[
                outer[0] - BACKGROUND_CLIP_SAMPLER_EXTRA_MARGIN_PT,
                outer[1] - BACKGROUND_CLIP_SAMPLER_EXTRA_MARGIN_PT,
                outer[2] + BACKGROUND_CLIP_SAMPLER_EXTRA_MARGIN_PT,
                outer[3] + BACKGROUND_CLIP_SAMPLER_EXTRA_MARGIN_PT,
            ],
            page_rect,
        );
        clip = Some(match clip {
            None => expanded,
            Some(c) => rect_union(&c, &expanded),
        });
    }
    let clip = match clip {
        Some(c) if !rect_is_empty(&c) => c,
        _ => return None,
    };
    if rect_area(&clip) / rect_area(page_rect).max(1.0) > BACKGROUND_CLIP_SAMPLER_MAX_PAGE_AREA_RATIO {
        if allow_full_page && valid.len() >= BACKGROUND_FULL_PAGE_SAMPLER_MIN_RECTS {
            return Some(*page_rect);
        }
        return None;
    }
    Some(clip)
}

/// `fill.LocalBackgroundSampler` — samples a rect's clean surrounding region
/// from one pre-rendered RGB pixmap. `samples` is the raw pixel buffer
/// (row-major, `stride` bytes per pixel); coordinates map page space → pixels
/// through `clip_rect` (page space) and the pixmap dimensions.
pub struct LocalBackgroundSampler {
    page_rect: RectTuple,
    clip_rect: RectTuple,
    pixmap_width: usize,
    pixmap_height: usize,
    stride: usize,
    samples: Vec<u8>,
}

impl LocalBackgroundSampler {
    pub fn new(
        page_rect: RectTuple,
        clip_rect: RectTuple,
        pixmap_width: usize,
        pixmap_height: usize,
        stride: usize,
        samples: Vec<u8>,
    ) -> Self {
        Self {
            page_rect,
            clip_rect,
            pixmap_width,
            pixmap_height,
            stride,
            samples,
        }
    }

    fn pixel_bounds(&self, rect: &RectTuple) -> Option<(usize, usize, usize, usize)> {
        let clipped = rect_intersection(rect, &self.clip_rect);
        if rect_is_empty(&clipped) || rect_width(&clipped) <= 0.0 || rect_height(&clipped) <= 0.0 {
            return None;
        }
        let x0 = coord_to_pixel(clipped[0], self.clip_rect[0], rect_width(&self.clip_rect), self.pixmap_width, false);
        let y0 = coord_to_pixel(clipped[1], self.clip_rect[1], rect_height(&self.clip_rect), self.pixmap_height, false);
        let x1 = coord_to_pixel(clipped[2], self.clip_rect[0], rect_width(&self.clip_rect), self.pixmap_width, true);
        let y1 = coord_to_pixel(clipped[3], self.clip_rect[1], rect_height(&self.clip_rect), self.pixmap_height, true);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some((x0, y0, x1, y1))
    }

    fn pixels_in_rect(&self, rect: &RectTuple) -> Vec<Rgb> {
        match self.pixel_bounds(rect) {
            Some((x0, y0, x1, y1)) => {
                pixels_in_bounds(&self.samples, self.pixmap_width, self.stride, x0, y0, x1, y1)
            }
            None => Vec::new(),
        }
    }

    fn pixels_in_rect_excluding(&self, rect: &RectTuple, excluded: &RectTuple) -> Vec<Rgb> {
        let Some((x0, y0, x1, y1)) = self.pixel_bounds(rect) else {
            return Vec::new();
        };
        match self.pixel_bounds(excluded) {
            None => pixels_in_bounds(&self.samples, self.pixmap_width, self.stride, x0, y0, x1, y1),
            Some((ex0, ey0, ex1, ey1)) => pixels_in_rect_excluding_bounds(
                &self.samples, self.pixmap_width, self.stride, x0, y0, x1, y1, ex0, ey0, ex1, ey1,
            ),
        }
    }

    fn can_sample(&self, rect: &RectTuple) -> bool {
        let outer = background_sample_outer_rect_from_page_rect(&self.page_rect, rect);
        let target = outer.as_ref().unwrap_or(rect);
        rect_contains(&self.clip_rect, target)
    }

    /// `fill._sample_clean_neighbor_fill` — best of the four bordering strips
    /// that is fully inside the clip, has enough pixels, and is not
    /// text-contaminated; returns its robust fill only when the best strip is
    /// low-complexity.
    fn sample_clean_neighbor_fill(&self, rect: &RectTuple) -> Option<[f64; 3]> {
        let mut best_fill: Option<[f64; 3]> = None;
        let mut best_score: Option<(u8, u8, f64)> = None;
        for candidate in patch_candidate_rects(&self.page_rect, rect) {
            if !rect_contains(&self.clip_rect, &candidate) {
                continue;
            }
            let pixels = self.pixels_in_rect(&candidate);
            if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
                continue;
            }
            if looks_like_text_contaminated_light_patch(&pixels) {
                continue;
            }
            let Some(fill) = robust_fill_from_pixels(&pixels) else {
                continue;
            };
            let spread = brightness_spread(&pixels);
            let complexity_bucket = if spread <= BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD { 0 } else { 1 };
            let score = (complexity_bucket, spread, -rect_area(&candidate));
            let better = match best_score {
                None => true,
                Some(bs) => {
                    score.0 < bs.0
                        || (score.0 == bs.0
                            && (score.1 < bs.1 || (score.1 == bs.1 && score.2 < bs.2)))
                }
            };
            if better {
                best_score = Some(score);
                best_fill = Some(fill);
            }
        }
        if let Some((bucket, _, _)) = best_score {
            if bucket == 0 {
                return best_fill;
            }
        }
        None
    }

    /// `fill.LocalBackgroundSampler.sample_local_background_fill` — dominant
    /// non-white inner fill first; otherwise the clean outer border (excluding
    /// the rect itself), falling back to a clean neighbor fill, then white.
    pub fn sample_local_background_fill(&self, rect: &RectTuple) -> Option<[f64; 3]> {
        if !self.can_sample(rect) {
            return None;
        }
        let inner_fill = dominant_nonwhite_fill_from_pixels(&self.pixels_in_rect(rect));
        if inner_fill.is_some() {
            return inner_fill;
        }
        let outer = match background_sample_outer_rect_from_page_rect(&self.page_rect, rect) {
            Some(o) => o,
            None => return Some(self.sample_clean_neighbor_fill(rect).unwrap_or([1.0, 1.0, 1.0])),
        };
        let pixels = self.pixels_in_rect_excluding(&outer, rect);
        let robust_fill = robust_fill_from_pixels(&pixels);
        if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
            return Some(
                robust_fill
                    .or_else(|| self.sample_clean_neighbor_fill(rect))
                    .unwrap_or([1.0, 1.0, 1.0]),
            );
        }
        let spread = brightness_spread(&pixels);
        if spread > BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD {
            return Some(
                robust_fill
                    .or_else(|| self.sample_clean_neighbor_fill(rect))
                    .unwrap_or([1.0, 1.0, 1.0]),
            );
        }
        Some(channel_medians(&pixels))
    }
}

/// A rendered RGB clip: `samples` is row-major with 3 bytes per pixel.
pub struct RgbPixmap {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<u8>,
}

/// `fill._pixmap_rgb_pixels` — all RGB triples of a rendered clip, subsampled
/// with `sample_step_for_bounds` over the whole pixmap.
pub fn pixmap_rgb_pixels(pix: &RgbPixmap) -> Vec<Rgb> {
    if pix.width == 0 || pix.height == 0 {
        return Vec::new();
    }
    pixels_in_bounds(&pix.samples, pix.width, 3, 0, 0, pix.width, pix.height)
}

fn neighbor_score_less(score: (u8, u8, f64), best: (u8, u8, f64)) -> bool {
    score.0 < best.0 || (score.0 == best.0 && (score.1 < best.1 || (score.1 == best.1 && score.2 < best.2)))
}

/// `fill._sample_clean_neighbor_fill(page, rect)` — module-level variant that
/// renders each candidate strip via `render_clip` instead of a live page.
pub fn sample_clean_neighbor_fill(
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    rect: &RectTuple,
) -> Option<[f64; 3]> {
    let mut best_fill: Option<[f64; 3]> = None;
    let mut best_score: Option<(u8, u8, f64)> = None;
    for candidate in patch_candidate_rects(page_rect, rect) {
        let Some(pix) = render_clip(&candidate) else {
            continue;
        };
        let pixels = pixmap_rgb_pixels(&pix);
        if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
            continue;
        }
        if looks_like_text_contaminated_light_patch(&pixels) {
            continue;
        }
        let Some(fill) = robust_fill_from_pixels(&pixels) else {
            continue;
        };
        let spread = brightness_spread(&pixels);
        let complexity_bucket = if spread <= BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD {
            0
        } else {
            1
        };
        let score = (complexity_bucket, spread, -rect_area(&candidate));
        if match best_score {
            None => true,
            Some(bs) => neighbor_score_less(score, bs),
        } {
            best_score = Some(score);
            best_fill = Some(fill);
        }
    }
    if let Some((bucket, _, _)) = best_score {
        if bucket == 0 {
            return best_fill;
        }
    }
    None
}

/// `fill.LocalBackgroundSampler.build` — derive the batched clip rect from
/// `rects`, render it via `render_clip`, and wrap it in a sampler. `None` when
/// there are too few rects or the rendered clip is unusable.
pub fn build_local_background_sampler(
    page_rect: &RectTuple,
    rects: &[RectTuple],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Option<LocalBackgroundSampler> {
    let clip_rect = batch_sampler_clip_rect(page_rect, rects, true)?;
    let pix = render_clip(&clip_rect)?;
    if pix.width == 0 || pix.height == 0 {
        return None;
    }
    Some(LocalBackgroundSampler::new(
        *page_rect,
        clip_rect,
        pix.width,
        pix.height,
        3,
        pix.samples,
    ))
}

/// `fill.sample_local_background_fill(page, rect, sampler=...)` — module-level
/// variant that renders clips via `render_clip` (an RGB clip renderer in page
/// space). Falls back to a clean neighbor fill, then white.
pub fn sample_local_background_fill(
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    rect: &RectTuple,
    sampler: Option<&LocalBackgroundSampler>,
) -> [f64; 3] {
    if let Some(sampler) = sampler {
        if let Some(sampled) = sampler.sample_local_background_fill(rect) {
            return sampled;
        }
    }
    let clean_fallback = |page_rect: &RectTuple| -> [f64; 3] {
        sample_clean_neighbor_fill(page_rect, render_clip, rect).unwrap_or([1.0, 1.0, 1.0])
    };

    if let Some(inner_pix) = render_clip(rect) {
        if inner_pix.width > 0 && inner_pix.height > 0 {
            let pixels = pixmap_rgb_pixels(&inner_pix);
            if let Some(fill) = dominant_nonwhite_fill_from_pixels(&pixels) {
                return fill;
            }
        }
    }

    let outer = match background_sample_outer_rect_from_page_rect(page_rect, rect) {
        Some(o) => o,
        None => return clean_fallback(page_rect),
    };
    let Some(pix) = render_clip(&outer) else {
        return clean_fallback(page_rect);
    };
    if pix.width == 0 || pix.height == 0 {
        return clean_fallback(page_rect);
    }

    let inner_x0 = (rect[0] - outer[0]) / (outer[2] - outer[0]).max(1e-6) * pix.width as f64;
    let inner_y0 = (rect[1] - outer[1]) / (outer[3] - outer[1]).max(1e-6) * pix.height as f64;
    let inner_x1 = (rect[2] - outer[0]) / (outer[2] - outer[0]).max(1e-6) * pix.width as f64;
    let inner_y1 = (rect[3] - outer[1]) / (outer[3] - outer[1]).max(1e-6) * pix.height as f64;
    let mut pixels: Vec<Rgb> = Vec::new();
    for y in 0..pix.height {
        let inside_y = inner_y0 <= y as f64 && (y as f64) < inner_y1;
        let row_offset = y * pix.width * 3;
        for x in 0..pix.width {
            if inside_y && inner_x0 <= x as f64 && (x as f64) < inner_x1 {
                continue;
            }
            let offset = row_offset + x * 3;
            pixels.push([pix.samples[offset], pix.samples[offset + 1], pix.samples[offset + 2]]);
        }
    }

    let robust_fill = robust_fill_from_pixels(&pixels);
    if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
        return robust_fill
            .or_else(|| sample_clean_neighbor_fill(page_rect, render_clip, rect))
            .unwrap_or([1.0, 1.0, 1.0]);
    }
    let spread = brightness_spread(&pixels);
    if spread > BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD {
        return robust_fill
            .or_else(|| sample_clean_neighbor_fill(page_rect, render_clip, rect))
            .unwrap_or([1.0, 1.0, 1.0]);
    }
    channel_medians(&pixels)
}

/// `fill.draw_white_covers` — for each rect, sample the surrounding background
/// fill and draw a solid cover rectangle on top of the page content. Replicates
/// the PyMuPDF `Shape.draw_rect + finish(fill) + commit(overlay=True)` sequence
/// as one appended content stream (`q <fill> rg x y w h re f Q` per rect).
pub fn draw_white_covers(
    page: &mut mupdf::pdf::PdfPage,
    doc: &mut mupdf::pdf::PdfDocument,
    page_rect: &RectTuple,
    rects: &[RectTuple],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<(), mupdf::Error> {
    if rects.is_empty() {
        return Ok(());
    }
    let mut ops = String::new();
    for rect in rects {
        let fill = sample_local_background_fill(page_rect, render_clip, rect, None);
        ops.push_str("q\n");
        ops.push_str(&format!("{} {} {} rg\n", fill[0], fill[1], fill[2]));
        ops.push_str(&format!(
            "{} {} {} {} re\nf\nQ\n",
            rect[0],
            rect[1],
            rect[2] - rect[0],
            rect[3] - rect[1],
        ));
    }
    page.insert_contents(doc, ops.as_bytes(), true)?;
    Ok(())
}

/// `fill.py::draw_flat_white_covers` — a pure alias of `draw_white_covers`
/// (production's "flat" drawer still samples the local background fill).
pub fn draw_flat_white_covers(
    page: &mut mupdf::pdf::PdfPage,
    doc: &mut mupdf::pdf::PdfDocument,
    page_rect: &RectTuple,
    rects: &[RectTuple],
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
) -> Result<(), mupdf::Error> {
    draw_white_covers(page, doc, page_rect, rects, render_clip)
}

/// `fill.py::draw_solid_cover` — one opaque cover rectangle with an explicit
/// fill, the `Shape.draw_rect + finish(fill) + commit(overlay=True)` sequence.
pub fn draw_solid_cover(
    page: &mut mupdf::pdf::PdfPage,
    doc: &mut mupdf::pdf::PdfDocument,
    rect: &RectTuple,
    fill: &[f64; 3],
) -> Result<(), mupdf::Error> {
    let ops = format!(
        "q\n{} {} {} rg\n{} {} {} {} re\nf\nQ\n",
        fill[0],
        fill[1],
        fill[2],
        rect[0],
        rect[1],
        rect[2] - rect[0],
        rect[3] - rect[1],
    );
    page.insert_contents(doc, ops.as_bytes(), true)?;
    Ok(())
}

/// `fill.py::PreparedBackgroundCover` — a text-layer rect with either a sampled
/// background pixmap to paint over it or a resolved solid fill.
pub struct PreparedBackgroundCover {
    pub rect: RectTuple,
    pub pixmap: Option<RgbPixmap>,
    pub fill: Option<[f64; 3]>,
}

/// `fill.py::prepare_background_cover` — best patch strip whose render is
/// low-complexity (bucket 0) and not text-contaminated becomes the pixmap;
/// otherwise the local background fill.
pub fn prepare_background_cover(
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    rect: &RectTuple,
) -> PreparedBackgroundCover {
    let mut best_pixmap: Option<RgbPixmap> = None;
    let mut best_score: Option<(u8, u8, f64)> = None;
    for candidate in patch_candidate_rects(page_rect, rect) {
        let Some(pix) = render_clip(&candidate) else {
            continue;
        };
        let pixels = pixmap_rgb_pixels(&pix);
        if pixels.len() < BACKGROUND_COVER_MIN_SAMPLE_PIXELS {
            continue;
        }
        if looks_like_text_contaminated_light_patch(&pixels) {
            continue;
        }
        let spread = brightness_spread(&pixels);
        let complexity_bucket = if spread <= BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD {
            0
        } else {
            1
        };
        let score = (complexity_bucket, spread, -rect_area(&candidate));
        if match best_score {
            None => true,
            Some(bs) => neighbor_score_less(score, bs),
        } {
            best_score = Some(score);
            best_pixmap = Some(pix);
        }
    }
    if let (Some(pix), Some(score)) = (best_pixmap, best_score) {
        if score.0 == 0 {
            return PreparedBackgroundCover {
                rect: *rect,
                pixmap: Some(pix),
                fill: None,
            };
        }
    }
    let fill = sample_local_background_fill(page_rect, render_clip, rect, None);
    PreparedBackgroundCover {
        rect: *rect,
        pixmap: None,
        fill: Some(fill),
    }
}

/// `fill.py::prepare_background_covers` — per-rect prepare; never `None`
/// (the production fallback inside the loop is unreachable).
pub fn prepare_background_covers(
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    rects: &[RectTuple],
) -> Vec<PreparedBackgroundCover> {
    rects.iter()
        .map(|rect| prepare_background_cover(page_rect, render_clip, rect))
        .collect()
}

/// `fill.py::apply_prepared_background_cover` — paint the sampled pixmap into
/// the rect (`insert_image`, stretch to fill, overlay), falling back to a solid
/// fill cover when the pixmap path errors (Python swallows the exception).
pub fn apply_prepared_background_cover(
    page: &mut mupdf::pdf::PdfPage,
    doc: &mut mupdf::pdf::PdfDocument,
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    cover: &PreparedBackgroundCover,
) -> Result<(), mupdf::Error> {
    if let Some(pixmap) = &cover.pixmap {
        let inserted = (|| -> Result<(), mupdf::Error> {
            let mut pix = mupdf::Pixmap::new_with_w_h(
                &mupdf::Colorspace::device_rgb(),
                pixmap.width as i32,
                pixmap.height as i32,
                false,
            )?;
            let stride = pix.stride() as usize;
            if stride == pixmap.width * 3 {
                pix.samples_mut().copy_from_slice(&pixmap.samples);
            } else {
                for y in 0..pixmap.height {
                    let src = &pixmap.samples[y * pixmap.width * 3..(y + 1) * pixmap.width * 3];
                    let dst =
                        &mut pix.samples_mut()[y * stride..y * stride + pixmap.width * 3];
                    dst.copy_from_slice(src);
                }
            }
            page.insert_image(
                doc,
                mupdf::Rect::new(
                    cover.rect[0] as f32,
                    cover.rect[1] as f32,
                    cover.rect[2] as f32,
                    cover.rect[3] as f32,
                ),
                mupdf::pdf::PageImageSource::Pixmap(&pix),
                mupdf::pdf::InsertImageOptions {
                    overlay: true,
                    ..Default::default()
                },
            )?;
            Ok(())
        })();
        if inserted.is_ok() {
            return Ok(());
        }
    }
    let fill = cover
        .fill
        .unwrap_or_else(|| sample_local_background_fill(page_rect, render_clip, &cover.rect, None));
    draw_solid_cover(page, doc, &cover.rect, &fill)
}

/// `fill.py::apply_prepared_background_covers`.
pub fn apply_prepared_background_covers(
    page: &mut mupdf::pdf::PdfPage,
    doc: &mut mupdf::pdf::PdfDocument,
    page_rect: &RectTuple,
    render_clip: &dyn Fn(&RectTuple) -> Option<RgbPixmap>,
    covers: &[PreparedBackgroundCover],
) -> Result<(), mupdf::Error> {
    for cover in covers {
        apply_prepared_background_cover(page, doc, page_rect, render_clip, cover)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    fn assert_fill_close(actual: [f64; 3], expected: [f64; 3], label: &str) {
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                (a - e).abs() < 1e-12,
                "{label}[{i}]: actual={a}, expected={e}"
            );
        }
    }

    #[test]
    fn brightness_spread_matches_python() {
        assert_eq!(brightness_spread(&[]), 255);
        assert_eq!(brightness_spread(&[[200, 200, 200]; 32]), 0);
        let ramp: Vec<Rgb> = (0..32).map(|i| { let v = (i * 8) as u8; [v, v, v] }).collect();
        assert_eq!(brightness_spread(&ramp), 200);
    }

    #[test]
    fn text_contamination_matches_python() {
        let mut clean = vec![[240u8, 240, 240]; 40];
        clean.extend(vec![[250, 250, 250]; 40]);
        assert!(!looks_like_text_contaminated_light_patch(&clean));

        let mut spots = vec![[240u8, 240, 240]; 40];
        spots.extend(vec![[80, 80, 80]; 2]);
        spots.extend(vec![[250, 250, 250]; 40]);
        assert!(!looks_like_text_contaminated_light_patch(&spots));

        assert!(!looks_like_text_contaminated_light_patch(&vec![[200, 200, 200]; 40]));
        assert!(!looks_like_text_contaminated_light_patch(&[]));
    }

    #[test]
    fn robust_fill_matches_python() {
        let mut grayish = vec![[230u8, 230, 230]; 40];
        grayish.extend(vec![[80, 80, 80]; 6]);
        grayish.extend(vec![[240, 240, 240]; 20]);
        assert_fill_close(
            robust_fill_from_pixels(&grayish).unwrap(),
            [230.0 / 255.0; 3],
            "grayish",
        );
        assert_fill_close(
            robust_fill_from_pixels(&vec![[80, 80, 80]; 40]).unwrap(),
            [80.0 / 255.0; 3],
            "dark",
        );
        assert_eq!(robust_fill_from_pixels(&vec![[200, 200, 200]; 4]), None);
    }

    #[test]
    fn dominant_fill_matches_python() {
        let mut white_dom = vec![[250u8, 250, 250]; 60];
        white_dom.extend(vec![[80, 80, 80]; 10]);
        assert_eq!(dominant_nonwhite_fill_from_pixels(&white_dom), None);

        let mut half = vec![[250u8, 250, 250]; 30];
        half.extend(vec![[80, 80, 80]; 30]);
        assert_eq!(dominant_nonwhite_fill_from_pixels(&half), None);

        let mut blue = vec![[200u8, 100, 50]; 60];
        blue.extend(vec![[80, 80, 80]; 10]);
        assert_fill_close(
            dominant_nonwhite_fill_from_pixels(&blue).unwrap(),
            [200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0],
            "blue",
        );
    }

    #[test]
    fn sample_step_matches_python() {
        assert_eq!(sample_step_for_bounds(0, 0, 32, 32), 1);
        assert_eq!(sample_step_for_bounds(0, 0, 1024, 1024), 16);
        assert_eq!(sample_step_for_bounds(5, 5, 5, 5), 1);
        assert_eq!(sample_step_for_bounds(0, 0, 4096, 16), 4);
    }

    #[test]
    fn coord_to_pixel_matches_python() {
        assert_eq!(coord_to_pixel(10.0, 0.0, 100.0, 100, false), 10);
        assert_eq!(coord_to_pixel(10.5, 0.0, 100.0, 100, false), 10);
        assert_eq!(coord_to_pixel(10.0, 0.0, 100.0, 100, true), 10);
        assert_eq!(coord_to_pixel(10.1, 0.0, 100.0, 100, true), 11);
        assert_eq!(coord_to_pixel(-5.0, 0.0, 100.0, 100, true), 0);
        assert_eq!(coord_to_pixel(200.0, 0.0, 100.0, 100, true), 100);
    }

    #[test]
    fn outer_rect_matches_python() {
        let page = rect(0.0, 0.0, 612.0, 792.0);
        assert_eq!(
            background_sample_outer_rect_from_page_rect(&page, &rect(300.0, 300.0, 400.0, 400.0)),
            Some(rect(294.0, 294.0, 406.0, 406.0))
        );
        assert_eq!(
            background_sample_outer_rect_from_page_rect(&page, &rect(0.0, 0.0, 50.0, 50.0)),
            Some(rect(0.0, 0.0, 56.0, 56.0))
        );
        assert_eq!(
            background_sample_outer_rect_from_page_rect(&page, &rect(0.0, 0.0, 20.0, 20.0)),
            Some(rect(0.0, 0.0, 26.0, 26.0))
        );
        // collapsed to a >1pt sliver at the corner is still a valid outer rect
        assert_eq!(
            background_sample_outer_rect_from_page_rect(&page, &rect(0.0, 0.0, 1.0, 1.0)),
            Some(rect(0.0, 0.0, 7.0, 7.0))
        );
    }

    #[test]
    fn batch_clip_matches_python() {
        let page = rect(0.0, 0.0, 612.0, 792.0);
        let few: Vec<RectTuple> = (0..3).map(|i| rect(i as f64 * 60.0, 100.0, i as f64 * 60.0 + 40.0, 140.0)).collect();
        assert_eq!(batch_sampler_clip_rect(&page, &few, true), None);

        let ten: Vec<RectTuple> = (0..10).map(|i| rect(i as f64 * 60.0, 100.0, i as f64 * 60.0 + 40.0, 140.0)).collect();
        let expected = rect(0.0, 76.0, 604.0, 164.0);
        assert_eq!(batch_sampler_clip_rect(&page, &ten, true), Some(expected));
        assert_eq!(batch_sampler_clip_rect(&page, &ten, false), Some(expected));

        let scattered: Vec<RectTuple> = [
            rect(0.0, 0.0, 10.0, 10.0),
            rect(500.0, 700.0, 610.0, 792.0),
            rect(20.0, 20.0, 80.0, 80.0),
        ]
        .iter()
        .flat_map(|r| std::iter::repeat(*r).take(4))
        .collect();
        assert_eq!(batch_sampler_clip_rect(&page, &scattered, true), None);
    }

    /// 200x200 RGB buffer covering page pt (0..100, 0..100) at scale 2.
    /// Base value 240; `block` may paint an inner region.
    fn make_buffer(kind: &str) -> Vec<u8> {
        let w = 200usize;
        let h = 200usize;
        let stride = 3usize;
        let mut buf = vec![0u8; w * h * stride];
        for y in 0..h {
            for x in 0..w {
                let in_block = (70..90).contains(&x) && (70..90).contains(&y);
                let v: u8 = match kind {
                    "white_all" => 255,
                    "white_block" => if in_block { 255 } else { 240 },
                    "dark_block" => if in_block { 60 } else { 240 },
                    "gradient" => 130 + (x * 120 / 200) as u8,
                    _ => 240,
                };
                let o = (y * w + x) * stride;
                buf[o] = v;
                buf[o + 1] = v;
                buf[o + 2] = v;
            }
        }
        buf
    }

    fn sampler_for(kind: &str) -> LocalBackgroundSampler {
        LocalBackgroundSampler::new(
            rect(0.0, 0.0, 100.0, 100.0),
            rect(0.0, 0.0, 100.0, 100.0),
            200,
            200,
            3,
            make_buffer(kind),
        )
    }

    #[test]
    fn sampler_matches_python() {
        let cases: [(&str, RectTuple, [f64; 3]); 5] = [
            ("flat", rect(30.0, 30.0, 50.0, 50.0), [0.9411764705882353; 3]),
            ("dark_block", rect(30.0, 30.0, 50.0, 50.0), [0.9411764705882353; 3]),
            ("white_block", rect(35.0, 35.0, 45.0, 45.0), [0.9411764705882353; 3]),
            ("white_all", rect(30.0, 30.0, 50.0, 50.0), [1.0; 3]),
            ("gradient", rect(30.0, 30.0, 50.0, 50.0), [0.6980392156862745; 3]),
        ];
        for (kind, sample_rect, expected) in cases {
            let sampler = sampler_for(kind);
            let fill = sampler.sample_local_background_fill(&sample_rect);
            let fill = fill.unwrap_or_else(|| panic!("{kind}: expected Some"));
            assert_fill_close(fill, expected, kind);
        }
    }
}
