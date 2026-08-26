// Port of services/rendering/layout/typography/*.py. The measurement.py facade
// (`__all__`) is exposed here as re-exports rather than a separate module.
pub mod baseline;
pub mod compactness;
pub mod constants;
pub mod content;
pub mod cover_geometry;
pub mod geometry;
pub mod line_count;
pub mod line_metrics;
pub mod scalars;

pub use baseline::candidate_text_items;
pub use baseline::page_baseline_font_size;
pub use compactness::{line_widths, occupied_ratio, occupied_ratio_x, source_compactness_score};
pub use constants::*;
pub use content::{formula_ratio, plain_text_chars_per_line};
pub use cover_geometry::expanded_cover_bbox;
pub use geometry::{cover_bbox, inner_bbox};
pub use line_count::{is_tall_single_line_glue, source_visual_line_count, visual_line_count};
pub use line_metrics::{
    bbox_height, bbox_width, effective_text_height, line_centers, line_height, local_font_metric,
    local_glyph_height, local_line_pitch, median_line_height, median_line_pitch,
    source_text_height_limit_pt,
};
pub use scalars::{clamp, percentile_value};
