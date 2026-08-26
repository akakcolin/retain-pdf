// Port of services/rendering/analysis/profile/{geometry,text_layer,image_background,
// vector_layer,ocr_blocks,background_coverage,drawing_count,editable_text,large_background,
// primary_image,rect_area,text_traces,vector_cover_preference,vector_heavy,vector_thresholds,
// builder}.py — the fitz.Page collectors. Inputs are PageSnapshot (data shape) instead of a
// live fitz.Page; all pure logic is preserved.

use crate::page::PageSnapshot;
use crate::profile::{
    classify_profile_kind, ImageBackgroundProfile, OcrBlockProfile, PageGeometryProfile,
    RenderPageProfile, TextLayerProfile, VectorLayerProfile,
};
use crate::rect::Rect;

pub const EDITABLE_TEXT_MIN_WORDS: i64 = 20;
pub const VECTOR_HEAVY_DRAWINGS_THRESHOLD: i64 = 2000;
pub const VECTOR_COVER_ONLY_DRAWINGS_THRESHOLD: i64 = 5000;
pub const DEFAULT_BACKGROUND_THRESHOLD: f64 = 0.75;

pub fn has_editable_text(visible_text_traces: i64) -> bool {
    visible_text_traces > 0
}

pub fn has_large_background(coverage_ratio: f64, threshold: f64) -> bool {
    coverage_ratio >= threshold
}

pub fn is_vector_heavy(drawing_count: i64) -> bool {
    drawing_count >= VECTOR_HEAVY_DRAWINGS_THRESHOLD
}

pub fn prefers_vector_cover_only(drawing_count: i64) -> bool {
    drawing_count >= VECTOR_COVER_ONLY_DRAWINGS_THRESHOLD
}

pub fn text_trace_visibility_counts(snapshot: &PageSnapshot) -> (i64, i64) {
    let mut visible = 0i64;
    let mut hidden = 0i64;
    for trace in &snapshot.text_traces {
        if trace.trace_type == 3 || trace.opacity <= 0.0 {
            hidden += 1;
        } else {
            visible += 1;
        }
    }
    (visible, hidden)
}

pub fn build_text_layer_profile(snapshot: &PageSnapshot) -> TextLayerProfile {
    let (visible, hidden) = text_trace_visibility_counts(snapshot);
    let words = snapshot.word_count;
    TextLayerProfile {
        visible_traces: visible,
        hidden_traces: hidden,
        has_visible_text: visible > 0 || words >= EDITABLE_TEXT_MIN_WORDS,
        has_hidden_text: hidden > 0,
        editable: has_editable_text(visible) || words >= EDITABLE_TEXT_MIN_WORDS,
    }
}

pub fn background_coverage_ratio(snapshot: &PageSnapshot, rect: Option<&Rect>) -> f64 {
    match rect {
        None => 0.0,
        Some(r) => {
            let page_area = snapshot.rect.area().max(1.0);
            r.intersect(&snapshot.rect).area() / page_area
        }
    }
}

fn pick_primary_from_xref_rects(
    snapshot: &PageSnapshot,
    coverage_ratio_threshold: f64,
) -> Option<(f64, i64, Rect)> {
    let page_area = snapshot.rect.area().max(1.0);
    let mut best: Option<(f64, i64, Rect)> = None;
    for &xref in &snapshot.image_entries {
        if xref <= 0 {
            continue;
        }
        let rects = snapshot.image_rects.get(&xref);
        for rect in rects.into_iter().flatten() {
            if rect.is_empty() {
                continue;
            }
            let coverage_ratio = rect.intersect(&snapshot.rect).area() / page_area;
            if coverage_ratio < coverage_ratio_threshold {
                continue;
            }
            if best.is_none() || coverage_ratio > best.as_ref().unwrap().0 {
                best = Some((coverage_ratio, xref, *rect));
            }
        }
    }
    best
}

pub fn primary_background_image(
    snapshot: &PageSnapshot,
    coverage_ratio_threshold: f64,
) -> Option<(i64, Rect)> {
    let page_area = snapshot.rect.area().max(1.0);
    let mut best: Option<(f64, i64, Rect)> = None;
    for info in &snapshot.image_infos {
        if info.xref <= 0 || info.bbox.is_empty() {
            continue;
        }
        let coverage_ratio = info.bbox.intersect(&snapshot.rect).area() / page_area;
        if coverage_ratio < coverage_ratio_threshold {
            continue;
        }
        if best.is_none() || coverage_ratio > best.as_ref().unwrap().0 {
            best = Some((coverage_ratio, info.xref, info.bbox));
        }
    }
    if best.is_none() {
        best = pick_primary_from_xref_rects(snapshot, coverage_ratio_threshold);
    }
    best.map(|(_, xref, rect)| (xref, rect))
}

pub fn build_image_background_profile(
    snapshot: &PageSnapshot,
    background_threshold: f64,
) -> ImageBackgroundProfile {
    match primary_background_image(snapshot, 0.0) {
        None => ImageBackgroundProfile {
            has_large_background: false,
            coverage_ratio: 0.0,
            xref: None,
            bbox: None,
        },
        Some((xref, rect)) => {
            let coverage_ratio = background_coverage_ratio(snapshot, Some(&rect));
            ImageBackgroundProfile {
                has_large_background: has_large_background(coverage_ratio, background_threshold),
                coverage_ratio,
                xref: Some(xref),
                bbox: Some([rect.x0, rect.y0, rect.x1, rect.y1]),
            }
        }
    }
}

pub fn build_vector_layer_profile(snapshot: &PageSnapshot) -> VectorLayerProfile {
    let count = snapshot.drawing_count;
    VectorLayerProfile {
        drawing_count: count,
        vector_heavy: is_vector_heavy(count),
        cover_only_preferred: prefers_vector_cover_only(count),
    }
}

fn bbox_area(bbox: &[f64; 4]) -> f64 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

pub fn build_ocr_block_profile(
    ocr_items: &[[f64; 4]],
    page_width: f64,
    page_height: f64,
) -> OcrBlockProfile {
    let areas: Vec<f64> = ocr_items.iter().map(bbox_area).collect();
    let valid_areas: Vec<f64> = areas.iter().copied().filter(|a| *a > 0.0).collect();
    let page_area = (page_width * page_height).max(1.0);
    let total_area: f64 = valid_areas.iter().sum();
    OcrBlockProfile {
        block_count: ocr_items.len() as i64,
        valid_bbox_count: valid_areas.len() as i64,
        total_bbox_area: total_area,
        page_area_ratio: total_area / page_area,
    }
}

pub fn build_page_geometry_profile(snapshot: &PageSnapshot) -> PageGeometryProfile {
    PageGeometryProfile {
        page_index: snapshot.number,
        width_pt: snapshot.rect.width(),
        height_pt: snapshot.rect.height(),
        rotation: snapshot.rotation,
        cropbox: [snapshot.cropbox.x0, snapshot.cropbox.y0, snapshot.cropbox.x1, snapshot.cropbox.y1],
    }
}

pub fn build_render_page_profile(
    snapshot: &PageSnapshot,
    ocr_items: &[[f64; 4]],
    background_threshold: f64,
) -> RenderPageProfile {
    let geometry = build_page_geometry_profile(snapshot);
    let text_layer = build_text_layer_profile(snapshot);
    let image_background = build_image_background_profile(snapshot, background_threshold);
    let vector_layer = build_vector_layer_profile(snapshot);
    let ocr_blocks = build_ocr_block_profile(ocr_items, geometry.width_pt, geometry.height_pt);
    RenderPageProfile {
        kind: classify_profile_kind(&text_layer, &image_background, &vector_layer),
        geometry,
        text_layer,
        image_background,
        vector_layer,
        ocr_blocks,
    }
}
