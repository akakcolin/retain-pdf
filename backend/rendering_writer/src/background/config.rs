//! Port of `source/background/config.py` plus the vertical-merge constants from
//! `source/background/patch.py`. Every threshold/ratio the fill/sampling logic
//! relies on, as `pub const` so the whole background module shares one source
//! of truth.

/// Light-background patch: median brightness (0-255) must be at least this.
pub const BACKGROUND_PATCH_LIGHT_BG_MEDIAN_MIN: u8 = 245;
/// Light-background patch: p90 brightness (0-255) must be at least this.
pub const BACKGROUND_PATCH_LIGHT_BG_P90_MIN: u8 = 250;
/// Pixels strictly below this brightness count as dark text contamination.
pub const BACKGROUND_PATCH_TEXT_CONTAMINATION_DARK_VALUE: u8 = 220;
/// Minimum dark-pixel fraction that marks a patch as text-contaminated.
pub const BACKGROUND_PATCH_TEXT_CONTAMINATION_DARK_RATIO: f64 = 0.015;

/// Expand the sampling rect by this margin (points) when reading a clean border.
pub const BACKGROUND_COVER_SAMPLE_MARGIN_PT: f64 = 6.0;
/// Clip-render scale for background sampling.
pub const BACKGROUND_COVER_SAMPLE_SCALE: f64 = 2.0;
/// Minimum pixel count before a fill estimate is trusted.
pub const BACKGROUND_COVER_MIN_SAMPLE_PIXELS: usize = 24;
/// Cap on sampled pixels; larger regions subsample by stride.
pub const BACKGROUND_COVER_MAX_SAMPLE_PIXELS: usize = 4096;
/// p90-p10 spread above which a region is "too complex" for a plain median fill.
pub const BACKGROUND_COVER_COMPLEXITY_BRIGHTNESS_SPREAD: u8 = 72;

/// Dominant-fill histogram bin edge.
pub const BACKGROUND_FILL_DOMINANT_BIN_SIZE: u8 = 8;
/// Minimum dominant-bin share before a fill is accepted.
pub const BACKGROUND_FILL_DOMINANT_MIN_RATIO: f64 = 0.35;
/// A fill with any channel at/above this is treated as white and rejected.
pub const BACKGROUND_FILL_NONWHITE_MAX_CHANNEL: f64 = 0.98;

/// Fewer valid rects than this skips the batched clip sampler.
pub const BACKGROUND_CLIP_SAMPLER_MIN_RECTS: usize = 8;
/// Clip union larger than this fraction of the page falls back to full-page.
pub const BACKGROUND_CLIP_SAMPLER_MAX_PAGE_AREA_RATIO: f64 = 0.35;
/// Extra margin added to each sampled rect when forming the batch clip.
pub const BACKGROUND_CLIP_SAMPLER_EXTRA_MARGIN_PT: f64 = 18.0;
/// Valid rects at/above this permit the full-page fallback.
pub const BACKGROUND_FULL_PAGE_SAMPLER_MIN_RECTS: usize = 24;

/// Vertical merge accepts rects separated by a gap within this range (points).
pub const STRICT_VERTICAL_MERGE_GAP_PT: f64 = 2.0;
/// Vertical merge requires this horizontal overlap before merging.
pub const STRICT_VERTICAL_MERGE_MIN_WIDTH_OVERLAP_RATIO: f64 = 0.72;
