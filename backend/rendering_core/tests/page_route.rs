// Port of backend/scripts/devtools/tests/rendering/test_page_route.py.

use rendering_core::profile::classify_profile_kind;
use rendering_core::profile::{
    ImageBackgroundProfile, OcrBlockProfile, PageGeometryProfile, RenderPageKind,
    RenderPageProfile, TextLayerProfile, VectorLayerProfile,
};
use rendering_core::route::{
    build_render_page_route, decide_page_background_route, decide_page_compose_route,
    decide_page_layout_route, decide_page_redaction_route,
};

fn sample_profile(kind: RenderPageKind) -> RenderPageProfile {
    let large_background = matches!(
        kind,
        RenderPageKind::ScanImage | RenderPageKind::PseudoEditableScan | RenderPageKind::MixedComplex
    );
    RenderPageProfile {
        geometry: PageGeometryProfile {
            page_index: 0,
            width_pt: 200.0,
            height_pt: 300.0,
            rotation: 0,
            cropbox: [0.0, 0.0, 200.0, 300.0],
        },
        text_layer: TextLayerProfile {
            visible_traces: if kind == RenderPageKind::EditableText { 1 } else { 0 },
            hidden_traces: if kind == RenderPageKind::PseudoEditableScan { 1 } else { 0 },
            has_visible_text: kind == RenderPageKind::EditableText,
            has_hidden_text: kind == RenderPageKind::PseudoEditableScan,
            editable: kind == RenderPageKind::EditableText,
        },
        image_background: ImageBackgroundProfile {
            has_large_background: large_background,
            coverage_ratio: if large_background { 0.9 } else { 0.0 },
            xref: if large_background { Some(1) } else { None },
            bbox: if large_background {
                Some([0.0, 0.0, 200.0, 300.0])
            } else {
                None
            },
        },
        vector_layer: VectorLayerProfile {
            drawing_count: if kind == RenderPageKind::VectorHeavy { 1200 } else { 0 },
            vector_heavy: kind == RenderPageKind::VectorHeavy,
            cover_only_preferred: kind == RenderPageKind::VectorHeavy,
        },
        ocr_blocks: OcrBlockProfile {
            block_count: 1,
            valid_bbox_count: 1,
            total_bbox_area: 100.0,
            page_area_ratio: 0.01,
        },
        kind,
    }
}

#[test]
fn page_route_decisions_are_split_by_concern() {
    let editable = sample_profile(RenderPageKind::EditableText);
    assert_eq!(decide_page_redaction_route(&editable), "text_layer_only");
    assert_eq!(decide_page_background_route(&editable), "source_pdf_page");
    assert_eq!(decide_page_compose_route(&editable), "typst_overlay");
    assert_eq!(decide_page_layout_route(&editable), "ocr_bbox_overlay");
}

#[test]
fn build_render_page_route_for_scan_page() {
    let route = build_render_page_route(&sample_profile(RenderPageKind::ScanImage));
    assert_eq!(route.redaction, "visual_cover");
    assert_eq!(route.background, "image_background");
    assert_eq!(route.compose, "typst_background");
    assert_eq!(route.layout, "ocr_bbox_overlay");
    assert!(route.reason.contains("large background image"));
}

#[test]
fn build_render_page_route_for_pseudo_editable_scan() {
    let route = build_render_page_route(&sample_profile(RenderPageKind::PseudoEditableScan));
    assert_eq!(route.redaction, "visual_cover_and_remove_text");
    assert_eq!(route.background, "hidden_text_stripped_source");
    assert_eq!(route.compose, "typst_background");
}

#[test]
fn page_route_exposes_pipeline_decisions() {
    let editable = build_render_page_route(&sample_profile(RenderPageKind::EditableText));
    let pseudo_scan = build_render_page_route(&sample_profile(RenderPageKind::PseudoEditableScan));

    assert_eq!(editable.render_mode_hint(), "overlay");
    assert_eq!(editable.text_cleanup(), "pikepdf_text_strip");
    assert_eq!(editable.overlay_fallback(), "none");

    assert_eq!(pseudo_scan.render_mode_hint(), "typst_visual");
    assert_eq!(pseudo_scan.text_cleanup(), "visual_cover_and_remove_text");
    assert_eq!(pseudo_scan.overlay_fallback(), "page_visual_cover");
}

#[test]
fn large_background_with_visible_text_is_pseudo_editable_scan() {
    let profile = sample_profile(RenderPageKind::MixedComplex);
    let kind = classify_profile_kind(
        &TextLayerProfile {
            visible_traces: 8,
            hidden_traces: 0,
            has_visible_text: true,
            has_hidden_text: false,
            editable: true,
        },
        &profile.image_background,
        &profile.vector_layer,
    );
    assert_eq!(kind, RenderPageKind::PseudoEditableScan);
}

#[test]
fn build_render_page_route_for_vector_heavy_page() {
    let route = build_render_page_route(&sample_profile(RenderPageKind::VectorHeavy));
    assert_eq!(route.redaction, "visual_cover");
    assert_eq!(route.background, "cleaned_background");
    assert_eq!(route.compose, "typst_background");
}
