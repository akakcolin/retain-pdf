//! Port of the pure pixel-analysis half of
//! `services/rendering/layout/payload/first_line_indent.py`: given a rendered
//! grayscale pixmap of a text block, detect the first line's indent in points,
//! plus the candidate gate `is_first_line_indent_candidate` (rendering the clip
//! lives in `rendering_reader::render`).

use crate::font_roles::is_body_text_candidate;
use crate::item::Item;
use crate::semantics::{
    is_caption_like_block, is_footnote_like_block, is_title_like_block, layout_role,
    semantic_role,
};
use crate::typography::line_metrics::{bbox_height, bbox_width};

/// `MIN_BLOCK_WIDTH_PT` — min block width before indent candidates are considered.
pub const MIN_BLOCK_WIDTH_PT: f64 = 80.0;
/// `MIN_BLOCK_HEIGHT_PT` — min block height before indent candidates are considered.
pub const MIN_BLOCK_HEIGHT_PT: f64 = 20.0;

/// `INK_THRESHOLD` — min absolute pixel-to-background delta considered ink.
pub const INK_THRESHOLD: i64 = 26;
/// `ROW_INK_RATIO` — min fraction of a row that must be ink to count as a line.
pub const ROW_INK_RATIO: f64 = 0.012;
/// `COL_INK_RATIO` — min fraction of a column band that must be ink to count.
pub const COL_INK_RATIO: f64 = 0.035;
/// `MIN_LINE_HEIGHT_PX` — min band height (px) to count as a line.
pub const MIN_LINE_HEIGHT_PX: usize = 3;
/// `MIN_DETECTED_LINES` — min lines required before reporting an indent.
pub const MIN_DETECTED_LINES: usize = 2;
/// `MAX_INDENT_EM` — indent clamp as a multiple of font size.
pub const MAX_INDENT_EM: f64 = 2.2;

/// `_border_background` — median of all four border edges, truncated.
fn border_background(samples: &[u8], width: usize, height: usize) -> i64 {
    if width == 0 || height == 0 {
        return 255;
    }
    let mut values: Vec<i64> = Vec::with_capacity(2 * width + 2 * height);
    for x in 0..width {
        values.push(samples[x] as i64);
        values.push(samples[(height - 1) * width + x] as i64);
    }
    for y in 0..height {
        values.push(samples[y * width] as i64);
        values.push(samples[y * width + width - 1] as i64);
    }
    median_truncate(values)
}

fn median_truncate(mut values: Vec<i64>) -> i64 {
    values.sort_unstable();
    let n = values.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        return values[n / 2];
    }
    // Python `int(statistics.median(...))` truncates the two-middle average.
    let avg = (values[n / 2 - 1] + values[n / 2]) as f64 / 2.0;
    avg.trunc() as i64
}

/// `_ink_rows` — per-row ink flag. When `background >= INK_THRESHOLD` uses the
/// dark-threshold lookup path (pixel <= background - INK_THRESHOLD); otherwise
/// the strict dark-relative path (never true, preserved for parity).
fn ink_rows(samples: &[u8], width: usize, height: usize, background: i64) -> Vec<bool> {
    let min_ink = (ROW_INK_RATIO * width as f64).trunc() as usize;
    let min_ink = 2.max(min_ink);
    let mut table = [0u8; 256];
    if background - INK_THRESHOLD >= 0 {
        let threshold = (background - INK_THRESHOLD).clamp(0, 255) as usize;
        for (i, slot) in table.iter_mut().enumerate() {
            *slot = if i <= threshold { 1 } else { 0 };
        }
        return (0..height)
            .map(|y| {
                let row = &samples[y * width..(y + 1) * width];
                row.iter().map(|&p| table[p as usize] as usize).sum::<usize>() >= min_ink
            })
            .collect();
    }
    (0..height)
        .map(|y| {
            let ink = samples[y * width..(y + 1) * width]
                .iter()
                .filter(|&&p| {
                    let p = p as i64;
                    (p - background).abs() >= INK_THRESHOLD && p < background
                })
                .count();
            ink >= min_ink
        })
        .collect()
}

/// `_bands` — contiguous runs of `true` flags with length >= `min_size`.
fn bands(flags: &[bool], min_size: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &flag) in flags.iter().enumerate() {
        match (flag, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min_size {
                    out.push((s, i));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if flags.len() - s >= min_size {
            out.push((s, flags.len()));
        }
    }
    out
}

/// `_line_left_px` — leftmost column of the band whose ink fraction clears
/// `COL_INK_RATIO`, or `None`.
fn line_left_px(samples: &[u8], width: usize, background: i64, y0: usize, y1: usize) -> Option<usize> {
    let band_height = (y1 - y0).max(1);
    let min_ink = 1.max((COL_INK_RATIO * band_height as f64).trunc() as usize);
    for x in 0..width {
        let mut ink = 0usize;
        for y in y0..y1 {
            let p = samples[y * width + x] as i64;
            if (p - background).abs() >= INK_THRESHOLD && p < background {
                ink += 1;
            }
        }
        if ink >= min_ink {
            return Some(x);
        }
    }
    None
}

/// Python `round(x, 2)` semantics (ties to even) for the indent clamp.
fn py_round(value: f64) -> f64 {
    let scaled = value * 100.0;
    let floored = scaled.floor();
    let diff = scaled - floored;
    let rounded = if diff > 0.5 {
        floored + 1.0
    } else if diff < 0.5 {
        floored
    } else if (floored as i64) % 2 == 0 {
        floored
    } else {
        floored + 1.0
    };
    rounded / 100.0
}

/// `_is_body_paragraph`: not caption/footnote/title-like, and either a
/// paragraph/list_item layout role or a body-text candidate, with a
/// body-compatible semantic role.
fn is_body_paragraph(item: &Item, page_text_width_med: f64) -> bool {
    if is_caption_like_block(item) || is_footnote_like_block(item) || is_title_like_block(item) {
        return false;
    }
    let item_layout_role = layout_role(item);
    let item_semantic_role = semantic_role(item);
    if item_layout_role != "paragraph"
        && item_layout_role != "list_item"
        && !is_body_text_candidate(item, page_text_width_med)
    {
        return false;
    }
    matches!(item_semantic_role.as_str(), "" | "body" | "abstract" | "unknown")
}

/// `is_first_line_indent_candidate`: a body paragraph whose block is wide and
/// tall enough, with a 4-number bbox.
pub fn is_first_line_indent_candidate(item: &Item, page_text_width_med: f64) -> bool {
    if !is_body_paragraph(item, page_text_width_med) {
        return false;
    }
    if bbox_width(item) < MIN_BLOCK_WIDTH_PT || bbox_height(item) < MIN_BLOCK_HEIGHT_PT {
        return false;
    }
    item.bbox.is_some()
}

/// Detect the first line's indent (pt) from a grayscale pixmap of a text block.
///
/// Mirrors `detect_first_line_indent_pt`'s pixel stage (the caller has already
/// gated the candidate and rendered `samples`). Returns `0.0` when fewer than
/// `MIN_DETECTED_LINES` lines are detected or the indent is below threshold.
pub fn detect_first_line_indent_pt_from_samples(
    samples: &[u8],
    width: usize,
    height: usize,
    font_size_pt: f64,
) -> f64 {
    if width == 0 || height == 0 || samples.len() < width * height {
        return 0.0;
    }
    let background = border_background(samples, width, height);
    let line_bands = bands(&ink_rows(samples, width, height, background), MIN_LINE_HEIGHT_PX);
    if line_bands.len() < MIN_DETECTED_LINES {
        return 0.0;
    }
    let mut lefts: Vec<f64> = Vec::new();
    for &(y0, y1) in &line_bands {
        if let Some(left) = line_left_px(samples, width, background, y0, y1) {
            lefts.push(left as f64);
        }
    }
    if lefts.len() < MIN_DETECTED_LINES {
        return 0.0;
    }
    let first_left = lefts[0];
    let rest = &lefts[1..];
    let mut rest_sorted = rest.to_vec();
    rest_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rest_median = if rest_sorted.len() % 2 == 1 {
        rest_sorted[rest_sorted.len() / 2]
    } else {
        (rest_sorted[rest_sorted.len() / 2 - 1] + rest_sorted[rest_sorted.len() / 2]) / 2.0
    };
    let indent_pt = (first_left - rest_median) / 2.0;
    let threshold = 4.0_f64.max(8.0_f64.min(font_size_pt * 0.75));
    if indent_pt < threshold {
        return 0.0;
    }
    let max_indent = 8.0_f64.max(font_size_pt * MAX_INDENT_EM);
    py_round(indent_pt.clamp(0.0, max_indent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All-white pixmap with two ink lines: first starts at x=20, second at x=2.
    fn two_line_image(width: usize, height: usize) -> Vec<u8> {
        let mut samples = vec![255u8; width * height];
        for x in 20..width {
            for y in 5..10 {
                samples[y * width + x] = 0;
            }
        }
        for x in 2..width {
            for y in 12..17 {
                samples[y * width + x] = 0;
            }
        }
        samples
    }

    #[test]
    fn detects_first_line_indent_pt() {
        let samples = two_line_image(40, 20);
        assert_eq!(detect_first_line_indent_pt_from_samples(&samples, 40, 20, 12.0), 9.0);
    }

    #[test]
    fn sub_threshold_indent_is_zero() {
        // First line indents only 4px (2pt) past the second line: below the
        // 8pt threshold for a 12pt font.
        let mut samples = vec![255u8; 40 * 20];
        for x in 6..40 {
            for y in 5..10 {
                samples[y * 40 + x] = 0;
            }
        }
        for x in 2..40 {
            for y in 12..17 {
                samples[y * 40 + x] = 0;
            }
        }
        assert_eq!(detect_first_line_indent_pt_from_samples(&samples, 40, 20, 12.0), 0.0);
    }

    #[test]
    fn single_line_is_zero() {
        let mut samples = vec![255u8; 40 * 20];
        for x in 20..40 {
            for y in 5..10 {
                samples[y * 40 + x] = 0;
            }
        }
        assert_eq!(detect_first_line_indent_pt_from_samples(&samples, 40, 20, 12.0), 0.0);
    }

    #[test]
    fn border_background_matches_median() {
        // 2x2: every pixel is a border pixel, collected twice -> median of
        // [200,220,240,250] doubled = 220 and 240 average = 230.
        let samples = [200, 220, 240, 250];
        assert_eq!(border_background(&samples, 2, 2), 230);
    }

    #[test]
    fn bands_filters_short_runs() {
        let flags = [false, true, true, true, false, true, false, true, true, true, true];
        assert_eq!(bands(&flags, 3), vec![(1, 4), (7, 11)]);
    }

    #[test]
    fn ink_rows_uses_dark_threshold_path() {
        // background 255 >= 26 -> threshold 229; row of black (0) is ink.
        let samples = [0u8; 8];
        assert_eq!(ink_rows(&samples, 8, 1, 255), vec![true]);
        let white = [255u8; 8];
        assert_eq!(ink_rows(&white, 8, 1, 255), vec![false]);
    }

    #[test]
    fn line_left_px_finds_leftmost_ink_column() {
        // width 10, band rows 0..3, background 255: ink only in columns 3..=5.
        let mut samples = [255u8; 50];
        for y in 0..3 {
            for x in 3..6 {
                samples[y * 10 + x] = 0;
            }
        }
        assert_eq!(line_left_px(&samples, 10, 255, 0, 3), Some(3));
        assert_eq!(line_left_px(&samples, 10, 255, 3, 4), None);
    }

    #[test]
    fn py_round_uses_ties_to_even() {
        assert_eq!(py_round(0.125), 0.12);
        assert_eq!(py_round(0.135), 0.14);
        assert_eq!(py_round(9.0), 9.0);
    }

    use crate::item::{Item, Line, Span};

    fn paragraph_item() -> Item {
        Item {
            block_type: Some("text".into()),
            layout_role: Some("paragraph".into()),
            semantic_role: Some("body".into()),
            source_text: "text".into(),
            bbox: Some([40.0, 100.0, 400.0, 160.0]),
            lines: vec![Line { bbox: None, spans: vec![Span { span_type: "text".into(), content: "x".into() }] }],
            ..Default::default()
        }
    }

    #[test]
    fn body_paragraph_is_candidate() {
        assert!(is_first_line_indent_candidate(&paragraph_item(), 300.0));
    }

    #[test]
    fn list_item_is_candidate() {
        let mut item = paragraph_item();
        item.layout_role = Some("list_item".into());
        assert!(is_first_line_indent_candidate(&item, 300.0));
    }

    #[test]
    fn caption_not_candidate() {
        let mut item = paragraph_item();
        item.layout_role = Some("caption".into());
        assert!(!is_first_line_indent_candidate(&item, 300.0));
    }

    #[test]
    fn narrow_block_not_candidate() {
        let mut item = paragraph_item();
        item.bbox = Some([40.0, 100.0, 100.0, 160.0]);
        assert!(!is_first_line_indent_candidate(&item, 300.0));
    }

    #[test]
    fn non_body_semantic_not_candidate() {
        let mut item = paragraph_item();
        item.semantic_role = Some("header".into());
        assert!(!is_first_line_indent_candidate(&item, 300.0));
    }

    #[test]
    fn missing_bbox_not_candidate() {
        let mut item = paragraph_item();
        item.bbox = None;
        assert!(!is_first_line_indent_candidate(&item, 300.0));
    }
}
