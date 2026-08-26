// Port of services/rendering/analysis/route/*.py.

use crate::profile::{RenderPageKind, RenderPageProfile};

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPageRoute {
    pub redaction: &'static str,
    pub background: &'static str,
    pub compose: &'static str,
    pub layout: &'static str,
    pub reason: String,
}

impl RenderPageRoute {
    pub fn render_mode_hint(&self) -> &'static str {
        if self.compose == "typst_overlay" {
            "overlay"
        } else {
            "typst_visual"
        }
    }

    pub fn text_cleanup(&self) -> &'static str {
        match self.redaction {
            "text_layer_only" => "pikepdf_text_strip",
            "visual_cover_and_remove_text" => "visual_cover_and_remove_text",
            "visual_cover" => "visual_cover",
            _ => "skip",
        }
    }

    pub fn overlay_fallback(&self) -> &'static str {
        if self.redaction == "text_layer_only" {
            "none"
        } else {
            "page_visual_cover"
        }
    }
}

pub fn decide_page_layout_route(_profile: &RenderPageProfile) -> &'static str {
    "ocr_bbox_overlay"
}

pub fn decide_page_background_route(profile: &RenderPageProfile) -> &'static str {
    match profile.kind {
        RenderPageKind::ScanImage => "image_background",
        RenderPageKind::PseudoEditableScan => "hidden_text_stripped_source",
        RenderPageKind::VectorHeavy | RenderPageKind::MixedComplex => "cleaned_background",
        RenderPageKind::EditableText => "source_pdf_page",
    }
}

pub fn decide_page_compose_route(profile: &RenderPageProfile) -> &'static str {
    if profile.kind == RenderPageKind::EditableText {
        "typst_overlay"
    } else {
        "typst_background"
    }
}

pub fn decide_page_redaction_route(profile: &RenderPageProfile) -> &'static str {
    match profile.kind {
        RenderPageKind::EditableText => "text_layer_only",
        RenderPageKind::PseudoEditableScan => "visual_cover_and_remove_text",
        _ => "visual_cover",
    }
}

pub fn page_route_reason(profile: &RenderPageProfile) -> String {
    match profile.kind {
        RenderPageKind::PseudoEditableScan => "large background image with hidden text layer".to_string(),
        RenderPageKind::ScanImage => "large background image without visible text".to_string(),
        RenderPageKind::VectorHeavy => {
            "vector drawing count exceeds safe text-layer cleanup threshold".to_string()
        }
        RenderPageKind::MixedComplex => "large background image with additional page content".to_string(),
        RenderPageKind::EditableText => "visible editable text layer".to_string(),
    }
}

pub fn build_render_page_route(profile: &RenderPageProfile) -> RenderPageRoute {
    RenderPageRoute {
        redaction: decide_page_redaction_route(profile),
        background: decide_page_background_route(profile),
        compose: decide_page_compose_route(profile),
        layout: decide_page_layout_route(profile),
        reason: page_route_reason(profile),
    }
}
