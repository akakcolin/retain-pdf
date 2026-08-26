// Differential (golden replay) tests: profile kind classifier + render page route.

mod common;

use common::*;
use rendering_core::profile::classify_profile_kind;
use rendering_core::route::build_render_page_route;

#[test]
fn test_classify_profile_kind() {
    let c = corpus();
    for (i, case) in cases(c, "route.classify_profile_kind").iter().enumerate() {
        let tl: TextLayerDto = from_value(&case.input["text_layer"]);
        let ib: ImageBackgroundDto = from_value(&case.input["image_background"]);
        let vl: VectorLayerDto = from_value(&case.input["vector_layer"]);
        let expected: String = from_value(&case.expected);
        let actual = classify_profile_kind(&tl.to_text_layer(), &ib.to_image_background(), &vl.to_vector_layer());
        assert_eq!(actual.as_str(), expected, "mismatch at case {i}");
    }
}

#[test]
fn test_build_render_page_route() {
    let c = corpus();
    for (i, case) in cases(c, "route.build_render_page_route").iter().enumerate() {
        let p: ProfileDto = from_value(&case.input["profile"]);
        let e: RouteExpected = from_value(&case.expected);
        let route = build_render_page_route(&p.to_profile());
        assert_eq!(route.redaction, e.redaction, "redaction at case {i}");
        assert_eq!(route.background, e.background, "background at case {i}");
        assert_eq!(route.compose, e.compose, "compose at case {i}");
        assert_eq!(route.layout, e.layout, "layout at case {i}");
        assert_eq!(route.reason, e.reason, "reason at case {i}");
        assert_eq!(route.render_mode_hint(), e.render_mode_hint, "render_mode_hint at case {i}");
        assert_eq!(route.text_cleanup(), e.text_cleanup, "text_cleanup at case {i}");
        assert_eq!(route.overlay_fallback(), e.overlay_fallback, "overlay_fallback at case {i}");
    }
}
