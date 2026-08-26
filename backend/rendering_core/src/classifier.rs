// Port of services/rendering/analysis/classifier.py.

use crate::page::PageSnapshot;
use crate::profile::RenderPageKind;
use crate::profile_build::build_render_page_profile;
use crate::route::{build_render_page_route, RenderPageRoute};

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPageClassification {
    pub kind: RenderPageKind,
    pub large_background_image: bool,
    pub visible_text_traces: i64,
    pub hidden_text_traces: i64,
    pub drawing_count: i64,
    pub background_coverage_ratio: f64,
    pub route: RenderPageRoute,
}

pub fn classify_render_page(snapshot: &PageSnapshot, background_threshold: f64) -> RenderPageClassification {
    let profile = build_render_page_profile(snapshot, &[], background_threshold);
    let route = build_render_page_route(&profile);
    RenderPageClassification {
        kind: profile.kind,
        large_background_image: profile.image_background.has_large_background,
        visible_text_traces: profile.text_layer.visible_traces,
        hidden_text_traces: profile.text_layer.hidden_traces,
        drawing_count: profile.vector_layer.drawing_count,
        background_coverage_ratio: profile.image_background.coverage_ratio,
        route,
    }
}
