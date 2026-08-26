// Port of the composition half of
// backend/scripts/services/rendering/analysis/document/builder.py +
// route/builder.py: given a RenderPageProfile, build_render_page_analysis must
// produce the route-derived fields and the profile facts.

use rendering_core::document_builder::{build_render_page_analysis, RenderDocumentAnalysis};
use rendering_core::profile::{
    classify_profile_kind, ImageBackgroundProfile, OcrBlockProfile, PageGeometryProfile,
    RenderPageKind, RenderPageProfile, TextLayerProfile, VectorLayerProfile,
};
use rendering_core::route::build_render_page_route;

fn profile(
    kind: RenderPageKind,
    image: ImageBackgroundProfile,
    text: TextLayerProfile,
    vector: VectorLayerProfile,
) -> RenderPageProfile {
    RenderPageProfile {
        geometry: PageGeometryProfile {
            page_index: 3,
            width_pt: 595.0,
            height_pt: 842.0,
            rotation: 0,
            cropbox: [0.0, 0.0, 595.0, 842.0],
        },
        text_layer: text,
        image_background: image,
        vector_layer: vector,
        ocr_blocks: OcrBlockProfile {
            block_count: 0,
            valid_bbox_count: 0,
            total_bbox_area: 0.0,
            page_area_ratio: 0.0,
        },
        kind,
    }
}

#[test]
fn editable_text_page_composes_overlay_route() {
    let image = ImageBackgroundProfile {
        has_large_background: false,
        coverage_ratio: 0.0,
        xref: None,
        bbox: None,
    };
    let text = TextLayerProfile {
        visible_traces: 3,
        hidden_traces: 0,
        has_visible_text: true,
        has_hidden_text: false,
        editable: true,
    };
    let vector = VectorLayerProfile {
        drawing_count: 2,
        vector_heavy: false,
        cover_only_preferred: false,
    };
    let p = profile(RenderPageKind::EditableText, image, text, vector);

    let a = build_render_page_analysis(&p);
    assert_eq!(a.page_index, 3);
    assert_eq!(a.kind, RenderPageKind::EditableText);
    assert_eq!(a.redaction, "text_layer_only");
    assert_eq!(a.background, "source_pdf_page");
    assert_eq!(a.compose, "typst_overlay");
    assert_eq!(a.layout, "ocr_bbox_overlay");
    assert_eq!(a.reason, "visible editable text layer");
    assert!(!a.has_large_background);
    assert!(a.visible_text);
    assert!(!a.hidden_text);
    assert!(a.editable_text);
    assert_eq!(a.drawing_count, 2);
    assert!(!a.vector_heavy);
}

#[test]
fn scan_image_page_composes_visual_cover() {
    let image = ImageBackgroundProfile {
        has_large_background: true,
        coverage_ratio: 0.98,
        xref: Some(7),
        bbox: Some([0.0, 0.0, 595.0, 842.0]),
    };
    let text = TextLayerProfile {
        visible_traces: 0,
        hidden_traces: 0,
        has_visible_text: false,
        has_hidden_text: false,
        editable: false,
    };
    let vector = VectorLayerProfile {
        drawing_count: 0,
        vector_heavy: false,
        cover_only_preferred: false,
    };
    let p = profile(RenderPageKind::ScanImage, image, text, vector);

    let a = build_render_page_analysis(&p);
    assert_eq!(a.kind, RenderPageKind::ScanImage);
    assert_eq!(a.redaction, "visual_cover");
    assert_eq!(a.background, "image_background");
    assert_eq!(a.compose, "typst_background");
    assert_eq!(a.reason, "large background image without visible text");
    assert!(a.has_large_background);
    assert!((a.background_coverage_ratio - 0.98).abs() < 1e-9);
    assert!(!a.visible_text);
}

#[test]
fn pseudo_editable_scan_composes_cover_and_remove_text() {
    let image = ImageBackgroundProfile {
        has_large_background: true,
        coverage_ratio: 0.9,
        xref: Some(7),
        bbox: Some([0.0, 0.0, 595.0, 842.0]),
    };
    let text = TextLayerProfile {
        visible_traces: 1,
        hidden_traces: 4,
        has_visible_text: true,
        has_hidden_text: true,
        editable: false,
    };
    let vector = VectorLayerProfile {
        drawing_count: 0,
        vector_heavy: false,
        cover_only_preferred: false,
    };
    let p = profile(RenderPageKind::PseudoEditableScan, image, text, vector);

    let a = build_render_page_analysis(&p);
    assert_eq!(a.kind, RenderPageKind::PseudoEditableScan);
    assert_eq!(a.redaction, "visual_cover_and_remove_text");
    assert_eq!(a.background, "hidden_text_stripped_source");
    assert_eq!(a.compose, "typst_background");
    assert_eq!(a.reason, "large background image with hidden text layer");
    assert!(a.hidden_text);
}

#[test]
fn vector_heavy_page_composes_cleaned_background() {
    let image = ImageBackgroundProfile {
        has_large_background: false,
        coverage_ratio: 0.0,
        xref: None,
        bbox: None,
    };
    let text = TextLayerProfile {
        visible_traces: 2,
        hidden_traces: 0,
        has_visible_text: true,
        has_hidden_text: false,
        editable: false,
    };
    let vector = VectorLayerProfile {
        drawing_count: 42,
        vector_heavy: true,
        cover_only_preferred: true,
    };
    let p = profile(RenderPageKind::VectorHeavy, image, text, vector);

    let a = build_render_page_analysis(&p);
    assert_eq!(a.kind, RenderPageKind::VectorHeavy);
    assert_eq!(a.redaction, "visual_cover");
    assert_eq!(a.background, "cleaned_background");
    assert_eq!(a.compose, "typst_background");
    assert_eq!(a.reason, "vector drawing count exceeds safe text-layer cleanup threshold");
    assert_eq!(a.drawing_count, 42);
    assert!(a.vector_heavy);
}

#[test]
fn classifier_kinds_map_to_reachable_routes() {
    // Route/analysis stay consistent with classify_profile_kind.
    let no_bg = ImageBackgroundProfile {
        has_large_background: false,
        coverage_ratio: 0.0,
        xref: None,
        bbox: None,
    };
    let big_bg = ImageBackgroundProfile {
        has_large_background: true,
        coverage_ratio: 0.9,
        xref: Some(1),
        bbox: None,
    };

    let editable_text = TextLayerProfile {
        visible_traces: 1,
        hidden_traces: 0,
        has_visible_text: true,
        has_hidden_text: false,
        editable: true,
    };
    let hidden_text = TextLayerProfile {
        visible_traces: 0,
        hidden_traces: 1,
        has_visible_text: false,
        has_hidden_text: true,
        editable: false,
    };
    let scan_text = TextLayerProfile {
        visible_traces: 0,
        hidden_traces: 0,
        has_visible_text: false,
        has_hidden_text: false,
        editable: false,
    };
    let heavy = VectorLayerProfile {
        drawing_count: 50,
        vector_heavy: true,
        cover_only_preferred: true,
    };
    let light = VectorLayerProfile {
        drawing_count: 1,
        vector_heavy: false,
        cover_only_preferred: false,
    };

    assert_eq!(
        classify_profile_kind(&editable_text, &no_bg, &light),
        RenderPageKind::EditableText
    );
    assert_eq!(
        classify_profile_kind(&scan_text, &big_bg, &light),
        RenderPageKind::ScanImage
    );
    assert_eq!(
        classify_profile_kind(&hidden_text, &big_bg, &light),
        RenderPageKind::PseudoEditableScan
    );
    assert_eq!(
        classify_profile_kind(&editable_text, &no_bg, &heavy),
        RenderPageKind::VectorHeavy
    );
}

#[test]
fn document_analysis_collects_pages() {
    let mut doc = RenderDocumentAnalysis::default();
    let mut p = profile(
        RenderPageKind::EditableText,
        ImageBackgroundProfile {
            has_large_background: false,
            coverage_ratio: 0.0,
            xref: None,
            bbox: None,
        },
        TextLayerProfile {
            visible_traces: 1,
            hidden_traces: 0,
            has_visible_text: true,
            has_hidden_text: false,
            editable: true,
        },
        VectorLayerProfile {
            drawing_count: 0,
            vector_heavy: false,
            cover_only_preferred: false,
        },
    );
    p.geometry.page_index = 0;
    doc.pages.insert(0, build_render_page_analysis(&p));
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.pages[&0].page_index, 0);
    assert_eq!(doc.pages[&0].compose, "typst_overlay");
}

#[test]
fn route_helpers_drive_cleanup_and_fallback() {
    let image = ImageBackgroundProfile {
        has_large_background: false,
        coverage_ratio: 0.0,
        xref: None,
        bbox: None,
    };
    let text = TextLayerProfile {
        visible_traces: 1,
        hidden_traces: 0,
        has_visible_text: true,
        has_hidden_text: false,
        editable: true,
    };
    let vector = VectorLayerProfile {
        drawing_count: 0,
        vector_heavy: false,
        cover_only_preferred: false,
    };
    let p = profile(RenderPageKind::EditableText, image, text, vector);
    let route = build_render_page_route(&p);
    assert_eq!(route.render_mode_hint(), "overlay");
    assert_eq!(route.text_cleanup(), "pikepdf_text_strip");
    assert_eq!(route.overlay_fallback(), "none");

    let mut scan = profile(
        RenderPageKind::ScanImage,
        ImageBackgroundProfile {
            has_large_background: true,
            coverage_ratio: 0.99,
            xref: Some(2),
            bbox: None,
        },
        TextLayerProfile {
            visible_traces: 0,
            hidden_traces: 0,
            has_visible_text: false,
            has_hidden_text: false,
            editable: false,
        },
        VectorLayerProfile {
            drawing_count: 0,
            vector_heavy: false,
            cover_only_preferred: false,
        },
    );
    scan.kind = RenderPageKind::ScanImage;
    let route = build_render_page_route(&scan);
    assert_eq!(route.render_mode_hint(), "typst_visual");
    assert_eq!(route.text_cleanup(), "visual_cover");
    assert_eq!(route.overlay_fallback(), "page_visual_cover");
}
