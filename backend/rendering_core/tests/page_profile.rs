// Port of backend/scripts/devtools/tests/rendering/test_page_profile.py +
// test_page_classifier.py. Uses PageSnapshot (data shape) instead of live
// fitz.Page documents; each snapshot mirrors what a real page yields.

use rendering_core::classifier::classify_render_page;
use rendering_core::page::{ImageInfo, PageSnapshot, TextTrace};
use rendering_core::profile::RenderPageKind;
use rendering_core::profile_build::build_render_page_profile;
use rendering_core::rect::Rect;
use rendering_core::registry::PageProfileRegistry;
use std::collections::HashMap;

fn editable_text_snapshot() -> PageSnapshot {
    PageSnapshot {
        number: 0,
        rotation: 0,
        rect: Rect::new(0.0, 0.0, 200.0, 300.0),
        cropbox: Rect::new(0.0, 0.0, 200.0, 300.0),
        text_traces: vec![TextTrace { trace_type: 0, opacity: 1.0 }],
        word_count: 2,
        drawing_count: 0,
        image_infos: vec![],
        image_entries: vec![],
        image_rects: HashMap::new(),
    }
}

fn scan_image_snapshot() -> PageSnapshot {
    PageSnapshot {
        number: 0,
        rotation: 0,
        rect: Rect::new(0.0, 0.0, 200.0, 300.0),
        cropbox: Rect::new(0.0, 0.0, 200.0, 300.0),
        text_traces: vec![],
        word_count: 0,
        drawing_count: 0,
        image_infos: vec![ImageInfo {
            xref: 1,
            bbox: Rect::new(0.0, 0.0, 200.0, 300.0),
        }],
        image_entries: vec![],
        image_rects: HashMap::new(),
    }
}

#[test]
fn build_render_page_profile_collects_base_dimensions() {
    let profile = build_render_page_profile(&scan_image_snapshot(), &[[10.0, 20.0, 100.0, 80.0]], 0.75);

    assert_eq!(profile.geometry.width_pt, 200.0);
    assert_eq!(profile.geometry.height_pt, 300.0);
    assert!(profile.image_background.has_large_background);
    assert!(profile.vector_layer.drawing_count >= 0);
    assert_eq!(profile.ocr_blocks.block_count, 1);
    assert_eq!(profile.ocr_blocks.valid_bbox_count, 1);
    assert_eq!(profile.kind, RenderPageKind::ScanImage);
}

#[test]
fn page_profile_registry_allows_additive_collectors() {
    let page = editable_text_snapshot();
    let registry = PageProfileRegistry::<HashMap<String, i64>, HashMap<String, i64>>::new()
        .register("probe", |page, ctx| {
            let mut value = HashMap::new();
            value.insert("page".to_string(), page.number);
            value.insert("value".to_string(), ctx["value"]);
            value
        });

    let mut context = HashMap::new();
    context.insert("value".to_string(), 42);
    let collected = registry.collect(&page, &context);

    let mut expected = HashMap::new();
    expected.insert("page".to_string(), 0);
    expected.insert("value".to_string(), 42);
    assert_eq!(collected, vec![("probe".to_string(), expected)]);

    let empty = PageProfileRegistry::<(), ()>::new();
    assert!(empty.collect(&page, &()).is_empty());
}

#[test]
fn classify_render_page_detects_editable_text() {
    let classification = classify_render_page(&editable_text_snapshot(), 0.75);

    assert_eq!(classification.kind, RenderPageKind::EditableText);
    assert!(!classification.large_background_image);
    assert_eq!(classification.route.redaction, "text_layer_only");
}

#[test]
fn classify_render_page_detects_scan_image() {
    let classification = classify_render_page(&scan_image_snapshot(), 0.75);

    assert_eq!(classification.kind, RenderPageKind::ScanImage);
    assert!(classification.large_background_image);
    assert!(classification.background_coverage_ratio >= 0.75);
    assert_eq!(classification.route.background, "image_background");
}
