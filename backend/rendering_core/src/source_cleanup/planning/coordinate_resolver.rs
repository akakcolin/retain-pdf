//! Port of `planning/coordinate_resolver.py` — bboxlog grouping, the
//! text-overlap text index, the coordinate-candidate resolver, and tiled
//! background detection.

use crate::rect::{Matrix, Rect, round_ties_even};
use crate::source_cleanup::planning::drawing_classifier;
use crate::source_cleanup::planning::spatial_index::{RectOverlapIndex, bisect_right};
use crate::source_cleanup::planning::PlanningPageContext;

/// `TextRectIndex` from `coordinate_resolver.py` — y0-sorted text rects probed
/// by a bisect-then-sweep score.
#[derive(Debug, Clone)]
pub struct TextRectIndex {
    pub rects: Vec<Rect>,
    pub y0_sorted: Vec<f64>,
}

impl TextRectIndex {
    pub fn build(rects: impl IntoIterator<Item = Rect>) -> Self {
        let mut ordered: Vec<Rect> = rects.into_iter().collect();
        ordered.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        let y0_sorted = ordered.iter().map(|r| r.y0).collect();
        TextRectIndex { rects: ordered, y0_sorted }
    }

    /// `score(target) -> (count, area)` — overlap count + area over candidate
    /// text rects intersecting `target`.
    pub fn score(&self, target_rect: &Rect) -> (usize, f64) {
        if target_rect.is_empty() || self.rects.is_empty() {
            return (0, 0.0);
        }
        let mut count = 0usize;
        let mut area = 0.0f64;
        let limit = bisect_right(&self.y0_sorted, target_rect.y1);
        for index in 0..limit {
            let text_rect = &self.rects[index];
            if text_rect.y1 < target_rect.y0 {
                continue;
            }
            let overlap = text_rect.intersect(target_rect).area();
            if overlap <= 0.0 {
                continue;
            }
            count += 1;
            area += overlap;
        }
        (count, area)
    }

    pub fn overlaps_any(&self, target_rects: &[Rect]) -> bool {
        target_rects.iter().any(|target| self.score(target).0 > 0)
    }
}

/// Affine bbox transform, mirroring the Python coordinate-candidate transforms.
pub type BboxTransform = fn(&Matrix, &Rect) -> Rect;

#[derive(Debug, Clone)]
pub struct BBoxCoordinateCandidate {
    pub name: &'static str,
    pub transform: BboxTransform,
}

pub const BBOX_COORDINATE_CANDIDATES: [BBoxCoordinateCandidate; 2] = [
    BBoxCoordinateCandidate {
        name: "pdf_matrix",
        transform: |inverse_ctm, rect| rect.transformed(inverse_ctm),
    },
    BBoxCoordinateCandidate {
        name: "raw_top_left",
        transform: |_inverse_ctm, rect| Rect::new(rect.x0, rect.y0, rect.x1, rect.y1),
    },
];

#[derive(Debug, Clone)]
pub struct PageBBoxResolver {
    pub inverse_ctm: Matrix,
    pub page_rect: Rect,
    pub text_rects: Vec<Rect>,
    pub text_index: TextRectIndex,
    pub image_rects: Vec<Rect>,
    pub unsafe_vector_rects: Vec<Rect>,
    pub unsafe_vector_index: RectOverlapIndex,
    pub preferred_candidate: BBoxCoordinateCandidate,
}

impl PageBBoxResolver {
    pub fn build(ctx: &PlanningPageContext, bboxes: &[Vec<f64>]) -> Self {
        let (text_rects, image_rects, unsafe_vector_rects) = page_bboxlog_rect_groups(&ctx.bboxlog_entries);
        let text_index = TextRectIndex::build(text_rects.clone());
        let preferred_candidate = choose_page_coordinate_candidate_with_inverse_ctm(
            &ctx.inverse_ctm,
            bboxes,
            &text_index,
        );
        PageBBoxResolver {
            inverse_ctm: ctx.inverse_ctm,
            page_rect: ctx.page_rect,
            text_rects: text_rects.clone(),
            text_index,
            image_rects: image_rects.clone(),
            unsafe_vector_rects: unsafe_vector_rects.clone(),
            unsafe_vector_index: RectOverlapIndex::build(unsafe_vector_rects),
            preferred_candidate,
        }
    }

    pub fn resolve_bbox_rect(&self, bbox: &[f64]) -> Option<Rect> {
        let raw_rect = raw_bbox_rect(bbox)?;
        let rect = (self.preferred_candidate.transform)(&self.inverse_ctm, &raw_rect);
        if rect.is_empty() {
            None
        } else {
            Some(rect)
        }
    }

    pub fn resolve_bbox_probe_rects(&self, bbox: &[f64]) -> Vec<Rect> {
        let Some(raw_rect) = raw_bbox_rect(bbox) else {
            return Vec::new();
        };
        let mut rects: Vec<Rect> = Vec::new();
        let mut seen: Vec<(i64, i64, i64, i64)> = Vec::new();
        for candidate in BBOX_COORDINATE_CANDIDATES {
            let rect = (candidate.transform)(&self.inverse_ctm, &raw_rect);
            if rect.is_empty() {
                continue;
            }
            let key = probe_key(&rect);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            rects.push(rect);
        }
        rects
    }

    pub fn ocr_bbox_to_pdf_rect(&self, bbox: &[f64]) -> Option<Rect> {
        let raw_rect = raw_bbox_rect(bbox)?;
        let pdf_rect = raw_rect.transformed(&self.inverse_ctm);
        if pdf_rect.is_empty() {
            None
        } else {
            Some(pdf_rect)
        }
    }

    pub fn has_large_background_image(&self, coverage_ratio_threshold: f64) -> bool {
        if self.image_rects.is_empty() {
            return false;
        }
        let page_area = self.page_rect.area().max(1.0);
        if self
            .image_rects
            .iter()
            .any(|rect| rect.intersect(&self.page_rect).area() / page_area >= coverage_ratio_threshold)
        {
            return true;
        }
        page_has_tiled_background_images_from_rects(&self.page_rect, &self.image_rects)
    }
}

/// `raw_bbox_rect(bbox)` — a 4-float list → non-empty `Rect`, else `None`.
pub fn raw_bbox_rect(bbox: &[f64]) -> Option<Rect> {
    if bbox.len() != 4 {
        return None;
    }
    let rect = Rect::new(bbox[0], bbox[1], bbox[2], bbox[3]);
    if rect.is_empty() {
        None
    } else {
        Some(rect)
    }
}

/// `int(round(x * 10))` per corner — the probe dedup key.
fn probe_key(rect: &Rect) -> (i64, i64, i64, i64) {
    (
        round_ties_even(rect.x0 * 10.0) as i64,
        round_ties_even(rect.y0 * 10.0) as i64,
        round_ties_even(rect.x1 * 10.0) as i64,
        round_ties_even(rect.y1 * 10.0) as i64,
    )
}

/// `choose_page_coordinate_candidate_with_inverse_ctm` — the candidate with the
/// max `(text_overlap_count, text_overlap_area)` over all raw bboxes; ties keep
/// the first candidate.
pub fn choose_page_coordinate_candidate_with_inverse_ctm(
    inverse_ctm: &Matrix,
    bboxes: &[Vec<f64>],
    text_index: &TextRectIndex,
) -> BBoxCoordinateCandidate {
    let raw_rects: Vec<Rect> = bboxes.iter().filter_map(|bbox| raw_bbox_rect(bbox)).collect();
    if raw_rects.is_empty() {
        return BBOX_COORDINATE_CANDIDATES[0].clone();
    }
    let mut best: Option<(usize, f64, &'static str)> = None;
    for candidate in BBOX_COORDINATE_CANDIDATES.iter() {
        let (count, area) = aggregate_candidate_score_with_inverse_ctm(inverse_ctm, candidate, &raw_rects, text_index);
        match best {
            None => best = Some((count, area, candidate.name)),
            Some((best_count, best_area, _)) => {
                if count > best_count || (count == best_count && area > best_area) {
                    best = Some((count, area, candidate.name));
                }
            }
        }
    }
    let name = best.map(|(_, _, name)| name).unwrap_or("pdf_matrix");
    BBOX_COORDINATE_CANDIDATES
        .iter()
        .find(|candidate| candidate.name == name)
        .cloned()
        .unwrap_or_else(|| BBOX_COORDINATE_CANDIDATES[0].clone())
}

/// `aggregate_candidate_score_with_inverse_ctm` — union rect + total
/// (count, area) over all raw rects under a candidate.
pub fn aggregate_candidate_score_with_inverse_ctm(
    inverse_ctm: &Matrix,
    candidate: &BBoxCoordinateCandidate,
    raw_rects: &[Rect],
    text_index: &TextRectIndex,
) -> (usize, f64) {
    let mut count = 0usize;
    let mut area = 0.0f64;
    for raw_rect in raw_rects {
        let rect = (candidate.transform)(inverse_ctm, raw_rect);
        let (rect_count, rect_area_sum) = text_index.score(&rect);
        count += rect_count;
        area += rect_area_sum;
    }
    (count, area)
}

/// `page_bboxlog_rect_groups` — split bboxlog entries into text / image /
/// unsafe-vector rect groups.
pub fn page_bboxlog_rect_groups(entries: &[(String, Rect)]) -> (Vec<Rect>, Vec<Rect>, Vec<Rect>) {
    let mut rects: Vec<Rect> = Vec::new();
    let mut image_rects: Vec<Rect> = Vec::new();
    let mut unsafe_vector_rects: Vec<Rect> = Vec::new();
    for (kind, rect) in entries {
        if kind.contains("text") {
            rects.push(*rect);
            continue;
        }
        if kind.contains("image") {
            image_rects.push(*rect);
            continue;
        }
        if drawing_classifier::bboxlog_path_blocks_text_strip(kind, rect) {
            unsafe_vector_rects.push(*rect);
        }
    }
    (rects, image_rects, unsafe_vector_rects)
}

/// `page_has_tiled_background_images_from_rects` — vertical-band coverage test.
pub fn page_has_tiled_background_images_from_rects(
    page_rect: &Rect,
    image_rects: &[Rect],
) -> bool {
    const COVERAGE_RATIO_THRESHOLD: f64 = 0.65;
    const MIN_IMAGE_COUNT: usize = 8;
    const MIN_WIDTH_RATIO: f64 = 0.60;
    if image_rects.len() < MIN_IMAGE_COUNT {
        return false;
    }
    let page_area = page_rect.area().max(1.0);
    let page_width = page_rect.width().max(1.0);
    let page_wide_rects: Vec<Rect> = image_rects
        .iter()
        .map(|rect| rect.intersect(page_rect))
        .filter(|intersection| {
            !intersection.is_empty() && intersection.width() / page_width >= MIN_WIDTH_RATIO
        })
        .collect();
    if page_wide_rects.len() < MIN_IMAGE_COUNT {
        return false;
    }
    let covered_area: f64 = merge_vertical_image_bands(&page_wide_rects)
        .iter()
        .map(|rect| rect.intersect(page_rect).area())
        .sum();
    covered_area / page_area >= COVERAGE_RATIO_THRESHOLD
}

fn merge_vertical_image_bands(rects: &[Rect]) -> Vec<Rect> {
    let mut sorted: Vec<Rect> = rects.to_vec();
    sorted.sort_by(|a, b| {
        let ka = (
            crate::rect::round_to_digits(a.y0, 3),
            crate::rect::round_to_digits(a.x0, 3),
        );
        let kb = (
            crate::rect::round_to_digits(b.y0, 3),
            crate::rect::round_to_digits(b.x0, 3),
        );
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut merged: Vec<Rect> = Vec::new();
    for rect in sorted {
        match merged.last() {
            None => merged.push(rect),
            Some(previous) => {
                if rect.y0 <= previous.y1 + 1.0 {
                    let index = merged.len() - 1;
                    merged[index] = previous.include(&rect);
                } else {
                    merged.push(rect);
                }
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PlanningPageContext {
        PlanningPageContext {
            page_index: 0,
            page_rect: Rect::new(0.0, 0.0, 612.0, 792.0),
            bboxlog_entries: vec![
                // fill-path of glyph scale (height 20, area 3000 <= 3500) is text-like
                ("fill-path".to_string(), Rect::new(50.0, 90.0, 200.0, 110.0)),
                ("text".to_string(), Rect::new(50.0, 95.0, 200.0, 110.0)),
                ("image".to_string(), Rect::new(300.0, 300.0, 500.0, 400.0)),
            ],
            content_stream_size: 100,
            has_form_xobjects: false,
            inverse_ctm: Matrix::identity(),
        }
    }

    #[test]
    fn groups_bboxlog_by_kind() {
        let (text, image, unsafe_vec) = page_bboxlog_rect_groups(&ctx().bboxlog_entries);
        assert_eq!(text.len(), 1);
        assert_eq!(image.len(), 1);
        assert_eq!(unsafe_vec.len(), 1);
    }

    #[test]
    fn score_counts_overlaps() {
        let index = TextRectIndex::build(vec![Rect::new(50.0, 95.0, 200.0, 110.0)]);
        let (count, area) = index.score(&Rect::new(60.0, 100.0, 100.0, 108.0));
        assert_eq!(count, 1);
        assert!(area > 0.0);
    }
}
