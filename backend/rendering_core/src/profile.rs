// Port of services/rendering/analysis/profile/{models,geometry,text_layer,image_background,
// vector_layer,ocr_blocks,kind}.py. Only the dataclasses and the kind classifier are ported;
// the `build_*_profile(page)` functions need fitz and are deferred to Phase 3.

#[derive(Debug, Clone, PartialEq)]
pub struct PageGeometryProfile {
    pub page_index: i64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub rotation: i64,
    pub cropbox: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayerProfile {
    pub visible_traces: i64,
    pub hidden_traces: i64,
    pub has_visible_text: bool,
    pub has_hidden_text: bool,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageBackgroundProfile {
    pub has_large_background: bool,
    pub coverage_ratio: f64,
    pub xref: Option<i64>,
    pub bbox: Option<[f64; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorLayerProfile {
    pub drawing_count: i64,
    pub vector_heavy: bool,
    pub cover_only_preferred: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrBlockProfile {
    pub block_count: i64,
    pub valid_bbox_count: i64,
    pub total_bbox_area: f64,
    pub page_area_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPageKind {
    EditableText,
    ScanImage,
    PseudoEditableScan,
    VectorHeavy,
    MixedComplex,
}

impl RenderPageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RenderPageKind::EditableText => "editable_text",
            RenderPageKind::ScanImage => "scan_image",
            RenderPageKind::PseudoEditableScan => "pseudo_editable_scan",
            RenderPageKind::VectorHeavy => "vector_heavy",
            RenderPageKind::MixedComplex => "mixed_complex",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPageProfile {
    pub geometry: PageGeometryProfile,
    pub text_layer: TextLayerProfile,
    pub image_background: ImageBackgroundProfile,
    pub vector_layer: VectorLayerProfile,
    pub ocr_blocks: OcrBlockProfile,
    pub kind: RenderPageKind,
}

pub fn classify_profile_kind(
    text_layer: &TextLayerProfile,
    image_background: &ImageBackgroundProfile,
    vector_layer: &VectorLayerProfile,
) -> RenderPageKind {
    if image_background.has_large_background
        && (text_layer.has_hidden_text || text_layer.has_visible_text)
    {
        return RenderPageKind::PseudoEditableScan;
    }
    if image_background.has_large_background && !text_layer.has_visible_text {
        return RenderPageKind::ScanImage;
    }
    if vector_layer.vector_heavy {
        return RenderPageKind::VectorHeavy;
    }
    if image_background.has_large_background {
        return RenderPageKind::MixedComplex;
    }
    RenderPageKind::EditableText
}
