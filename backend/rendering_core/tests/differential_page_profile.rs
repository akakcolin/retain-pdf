// Differential (golden replay) tests: Phase 3 profile collectors
// (build_render_page_profile / ocr_blocks / text_layer / image_background /
// vector_layer / text_traces / background_coverage / detect / classifier).

mod common;

use common::*;
use rendering_core::classifier::classify_render_page;
use rendering_core::page::PageSnapshot;
use rendering_core::profile::{
    ImageBackgroundProfile, OcrBlockProfile, PageGeometryProfile, RenderPageProfile,
    TextLayerProfile, VectorLayerProfile,
};
use rendering_core::profile_build::{
    background_coverage_ratio, build_image_background_profile, build_ocr_block_profile,
    build_render_page_profile, build_text_layer_profile, build_vector_layer_profile,
    primary_background_image, text_trace_visibility_counts,
};
use rendering_core::rect::Rect;

fn snapshot_of(case: &Case) -> PageSnapshot {
    let dto: PageSnapshotDto = from_value(&case.input["page_snapshot"]);
    dto.to_snapshot()
}

// PyMuPDF fitz.Rect stores coordinates as f32, so Python-derived area/ratio
// values differ from our f64 port at ~1e-7. Compare floats tolerantly and
// ints/bools/options exactly.
fn assert_close_geometry(a: &PageGeometryProfile, b: &PageGeometryProfile) {
    assert_eq!(a.page_index, b.page_index);
    assert_eq!(a.rotation, b.rotation);
    assert_close_f64(a.width_pt, b.width_pt);
    assert_close_f64(a.height_pt, b.height_pt);
    assert_close_f64_slice(&a.cropbox, &b.cropbox);
}

fn assert_close_text_layer(a: &TextLayerProfile, b: &TextLayerProfile) {
    assert_eq!(a.visible_traces, b.visible_traces);
    assert_eq!(a.hidden_traces, b.hidden_traces);
    assert_eq!(a.has_visible_text, b.has_visible_text);
    assert_eq!(a.has_hidden_text, b.has_hidden_text);
    assert_eq!(a.editable, b.editable);
}

fn assert_close_image_background(a: &ImageBackgroundProfile, b: &ImageBackgroundProfile) {
    assert_eq!(a.has_large_background, b.has_large_background);
    assert_close_f64(a.coverage_ratio, b.coverage_ratio);
    assert_eq!(a.xref, b.xref);
    match (a.bbox, b.bbox) {
        (Some(x), Some(y)) => assert_close_f64_slice(&x, &y),
        (None, None) => {}
        _ => panic!("bbox mismatch: actual={:?}, expected={:?}", a.bbox, b.bbox),
    }
}

fn assert_close_vector_layer(a: &VectorLayerProfile, b: &VectorLayerProfile) {
    assert_eq!(a.drawing_count, b.drawing_count);
    assert_eq!(a.vector_heavy, b.vector_heavy);
    assert_eq!(a.cover_only_preferred, b.cover_only_preferred);
}

fn assert_close_ocr_blocks(a: &OcrBlockProfile, b: &OcrBlockProfile) {
    assert_eq!(a.block_count, b.block_count);
    assert_eq!(a.valid_bbox_count, b.valid_bbox_count);
    assert_close_f64(a.total_bbox_area, b.total_bbox_area);
    assert_close_f64(a.page_area_ratio, b.page_area_ratio);
}

fn assert_close_profile(a: &RenderPageProfile, b: &RenderPageProfile) {
    assert_eq!(a.kind, b.kind);
    assert_close_geometry(&a.geometry, &b.geometry);
    assert_close_text_layer(&a.text_layer, &b.text_layer);
    assert_close_image_background(&a.image_background, &b.image_background);
    assert_close_vector_layer(&a.vector_layer, &b.vector_layer);
    assert_close_ocr_blocks(&a.ocr_blocks, &b.ocr_blocks);
}

#[test]
fn test_build_render_page_profile() {
    let c = corpus();
    for case in cases(c, "profile.build_render_page_profile").iter() {
        let ocr_items: Vec<[f64; 4]> = from_value(&case.input["ocr_items"]);
        let threshold: f64 = from_value(&case.input["background_threshold"]);
        let expected: ProfileDto = from_value(&case.expected);
        let actual = build_render_page_profile(&snapshot_of(case), &ocr_items, threshold);
        assert_close_profile(&actual, &expected.to_profile());
    }
}

#[test]
fn test_build_ocr_block_profile() {
    let c = corpus();
    for case in cases(c, "profile.ocr_blocks.build_ocr_block_profile").iter() {
        let ocr_items: Vec<[f64; 4]> = from_value(&case.input["ocr_items"]);
        let width: f64 = from_value(&case.input["page_width"]);
        let height: f64 = from_value(&case.input["page_height"]);
        let expected: OcrBlocksDto = from_value(&case.expected);
        let actual = build_ocr_block_profile(&ocr_items, width, height);
        assert_close_ocr_blocks(&actual, &expected.to_ocr_blocks());
    }
}

#[test]
fn test_build_text_layer_profile() {
    let c = corpus();
    for case in cases(c, "profile.text_layer.build_text_layer_profile").iter() {
        let expected: TextLayerDto = from_value(&case.expected);
        let actual = build_text_layer_profile(&snapshot_of(case));
        assert_close_text_layer(&actual, &expected.to_text_layer());
    }
}

#[test]
fn test_build_image_background_profile() {
    let c = corpus();
    for case in cases(c, "profile.image_background.build_image_background_profile").iter() {
        let threshold: f64 = from_value(&case.input["background_threshold"]);
        let expected: ImageBackgroundDto = from_value(&case.expected);
        let actual = build_image_background_profile(&snapshot_of(case), threshold);
        assert_close_image_background(&actual, &expected.to_image_background());
    }
}

#[test]
fn test_build_vector_layer_profile() {
    let c = corpus();
    for case in cases(c, "profile.vector_layer.build_vector_layer_profile").iter() {
        let expected: VectorLayerDto = from_value(&case.expected);
        let actual = build_vector_layer_profile(&snapshot_of(case));
        assert_close_vector_layer(&actual, &expected.to_vector_layer());
    }
}

#[test]
fn test_text_trace_visibility_counts() {
    let c = corpus();
    for (i, case) in cases(c, "profile.text_traces.text_trace_visibility_counts").iter().enumerate() {
        let expected: Vec<i64> = from_value(&case.expected);
        let (visible, hidden) = text_trace_visibility_counts(&snapshot_of(case));
        assert_eq!(vec![visible, hidden], expected, "mismatch at case {i}");
    }
}

#[test]
fn test_background_coverage_ratio() {
    let c = corpus();
    for case in cases(c, "profile.background_coverage.background_coverage_ratio").iter() {
        let rect: Option<[f64; 4]> = from_value(&case.input["rect"]);
        let expected: f64 = from_value(&case.expected);
        let rect = rect.map(|r| Rect::new(r[0], r[1], r[2], r[3]));
        let actual = background_coverage_ratio(&snapshot_of(case), rect.as_ref());
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_pick_primary_background_image() {
    let c = corpus();
    for (i, case) in cases(c, "profile.detect.pick_primary_background_image").iter().enumerate() {
        let threshold: f64 = from_value(&case.input["coverage_ratio_threshold"]);
        let expected: Option<PrimaryImageDto> = from_value(&case.expected);
        let actual = primary_background_image(&snapshot_of(case), threshold);
        match (actual, expected) {
            (Some((xref, rect)), Some(e)) => {
                assert_eq!(xref, e.xref, "xref mismatch at case {i}");
                assert_close_f64_slice(
                    &[rect.x0, rect.y0, rect.x1, rect.y1],
                    &e.bbox,
                );
            }
            (None, None) => {}
            (a, e) => panic!("mismatch at case {i}: actual={a:?}, expected={e:?}"),
        }
    }
}

#[test]
fn test_classify_render_page() {
    let c = corpus();
    for (i, case) in cases(c, "profile.classifier.classify_render_page").iter().enumerate() {
        let threshold: f64 = from_value(&case.input["background_threshold"]);
        let expected: ClassificationDto = from_value(&case.expected);
        let actual = classify_render_page(&snapshot_of(case), threshold);
        assert_eq!(actual.kind.as_str(), expected.kind, "kind mismatch at case {i}");
        assert_eq!(actual.large_background_image, expected.large_background_image, "case {i}");
        assert_eq!(actual.visible_text_traces, expected.visible_text_traces, "case {i}");
        assert_eq!(actual.hidden_text_traces, expected.hidden_text_traces, "case {i}");
        assert_eq!(actual.drawing_count, expected.drawing_count, "case {i}");
        assert_close_f64(actual.background_coverage_ratio, expected.background_coverage_ratio);
        assert_eq!(actual.route.redaction, expected.route.redaction, "case {i}");
        assert_eq!(actual.route.background, expected.route.background, "case {i}");
        assert_eq!(actual.route.compose, expected.route.compose, "case {i}");
        assert_eq!(actual.route.layout, expected.route.layout, "case {i}");
        assert_eq!(actual.route.reason, expected.route.reason, "case {i}");
    }
}
