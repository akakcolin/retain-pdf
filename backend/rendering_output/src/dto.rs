//! Port of `services/rendering/layout/model/models.py` — the render-spec DTOs.
//!
//! These are the input shape of the Typst emitter. Every optional field maps to
//! a Python dataclass default and is `#[serde(default)]` so partial JSON
//! (produced by the differential generator) deserializes exactly like the
//! dataclass constructor would.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RenderLineBox {
    pub text: String,
    pub bbox: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RenderTocEntry {
    pub title: String,
    pub page_label: String,
    pub bbox: Vec<f64>,
    #[serde(default)]
    pub number: String,
    #[serde(default = "default_toc_level")]
    pub level: i64,
}

fn default_toc_level() -> i64 {
    1
}

/// One `math_map` entry (`formula_text` / `latex` are the keys the emitter
/// reads; unknown dict keys are tolerated).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MathMapEntry {
    #[serde(default)]
    pub formula_text: String,
    #[serde(default)]
    pub latex: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RenderBlock {
    pub block_id: String,
    pub bbox: Vec<f64>,
    pub cover_bbox: Vec<f64>,
    pub inner_bbox: Vec<f64>,
    pub markdown_text: String,
    pub plain_text: String,
    pub render_kind: String,
    pub font_size_pt: f64,
    pub leading_em: f64,
    #[serde(default)]
    pub font_weight: String,
    #[serde(default)]
    pub fit_to_box: bool,
    #[serde(default)]
    pub fit_single_line: bool,
    #[serde(default)]
    pub fit_min_font_size_pt: f64,
    #[serde(default)]
    pub fit_max_font_size_pt: f64,
    #[serde(default)]
    pub fit_min_leading_em: f64,
    #[serde(default)]
    pub fit_max_height_pt: f64,
    #[serde(default)]
    pub fit_target_width_pt: f64,
    #[serde(default)]
    pub fit_target_height_pt: f64,
    #[serde(default)]
    pub fit_shift_up_pt: f64,
    #[serde(default)]
    pub first_line_indent_pt: f64,
    #[serde(default)]
    pub justify_text: bool,
    #[serde(default)]
    pub text_color: [f64; 3],
    #[serde(default)]
    pub cover_fill: [f64; 3],
    #[serde(default)]
    pub use_cover_fill: bool,
    #[serde(default)]
    pub math_map: Vec<MathMapEntry>,
    #[serde(default)]
    pub skip_reason: String,
    #[serde(default)]
    pub source_item_id: String,
    #[serde(default)]
    pub preserve_line_breaks: bool,
    #[serde(default)]
    pub preserved_line_boxes: Vec<RenderLineBox>,
    #[serde(default)]
    pub toc_entries: Vec<RenderTocEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RenderLayoutBlock {
    pub block_id: String,
    pub page_index: i64,
    pub background_rect: Vec<f64>,
    pub content_rect: Vec<f64>,
    pub content_kind: String,
    pub content_text: String,
    pub plain_text: String,
    #[serde(default)]
    pub math_map: Vec<MathMapEntry>,
    pub font_size_pt: f64,
    pub leading_em: f64,
    #[serde(default)]
    pub font_weight: String,
    #[serde(default)]
    pub fit_to_box: bool,
    #[serde(default)]
    pub fit_single_line: bool,
    #[serde(default)]
    pub fit_min_font_size_pt: f64,
    #[serde(default)]
    pub fit_max_font_size_pt: f64,
    #[serde(default)]
    pub fit_min_leading_em: f64,
    #[serde(default)]
    pub fit_max_height_pt: f64,
    #[serde(default)]
    pub fit_target_width_pt: f64,
    #[serde(default)]
    pub fit_target_height_pt: f64,
    #[serde(default)]
    pub fit_shift_up_pt: f64,
    #[serde(default)]
    pub first_line_indent_pt: f64,
    #[serde(default)]
    pub justify_text: bool,
    #[serde(default)]
    pub text_color: [f64; 3],
    #[serde(default)]
    pub cover_fill: [f64; 3],
    #[serde(default)]
    pub use_cover_fill: bool,
    #[serde(default)]
    pub skip_reason: String,
    #[serde(default)]
    pub preserve_line_breaks: bool,
    #[serde(default)]
    pub preserved_line_boxes: Vec<RenderLineBox>,
    #[serde(default)]
    pub toc_entries: Vec<RenderTocEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RenderPageSpec {
    pub page_index: i64,
    pub page_width_pt: f64,
    pub page_height_pt: f64,
    #[serde(default)]
    pub background_pdf_path: Option<String>,
    #[serde(default)]
    pub blocks: Vec<RenderLayoutBlock>,
}

impl RenderBlock {
    /// Port of `layout_block_to_render_block`: pure field copy; `math_map` is
    /// intentionally NOT carried (the Python conversion leaves it `None`).
    pub fn from_layout(block: &RenderLayoutBlock) -> Self {
        RenderBlock {
            block_id: block.block_id.clone(),
            bbox: block.background_rect.clone(),
            cover_bbox: block.background_rect.clone(),
            inner_bbox: block.content_rect.clone(),
            markdown_text: block.content_text.clone(),
            plain_text: block.plain_text.clone(),
            render_kind: block.content_kind.clone(),
            font_size_pt: block.font_size_pt,
            leading_em: block.leading_em,
            font_weight: block.font_weight.clone(),
            fit_to_box: block.fit_to_box,
            fit_single_line: block.fit_single_line,
            fit_min_font_size_pt: block.fit_min_font_size_pt,
            fit_max_font_size_pt: block.fit_max_font_size_pt,
            fit_min_leading_em: block.fit_min_leading_em,
            fit_max_height_pt: block.fit_max_height_pt,
            fit_target_width_pt: block.fit_target_width_pt,
            fit_target_height_pt: block.fit_target_height_pt,
            fit_shift_up_pt: block.fit_shift_up_pt,
            first_line_indent_pt: block.first_line_indent_pt,
            justify_text: block.justify_text,
            text_color: block.text_color,
            cover_fill: block.cover_fill,
            use_cover_fill: block.use_cover_fill,
            math_map: Vec::new(),
            skip_reason: block.skip_reason.clone(),
            source_item_id: String::new(),
            preserve_line_breaks: block.preserve_line_breaks,
            preserved_line_boxes: block.preserved_line_boxes.clone(),
            toc_entries: block.toc_entries.clone(),
        }
    }
}
