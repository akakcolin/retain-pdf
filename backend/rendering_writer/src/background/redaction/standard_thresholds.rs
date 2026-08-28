//! Port of `source/cleanup/standard_thresholds.py` (the standard-subroute
//! fast-page-cover counts) plus the vector-profile count thresholds
//! (`source/vector_profile.py` HEAVY_VECTOR / VECTOR_HEAVY). The count-based
//! decision (`>=5000` cover_only_count, `>=2000` vector_heavy) uses the raw
//! drawing count, which equals `len(collect_page_drawing_rects(page))` for the
//! corpus pages (all drawings carry non-empty rects after thin-expansion).

/// `ITEM_REMOVABLE_RECTS_FAST_COVER_THRESHOLD` — a single item with this many
/// raw removable rects is force-covered.
pub const ITEM_REMOVABLE_RECTS_FAST_COVER_THRESHOLD: usize = 24;
/// `PAGE_REMOVABLE_RECTS_FAST_COVER_THRESHOLD` — total raw removable rects
/// across the page at/above which the fast page cover fires.
pub const PAGE_REMOVABLE_RECTS_FAST_COVER_THRESHOLD: usize = 180;
/// `PAGE_AVG_REMOVABLE_RECTS_FAST_COVER_THRESHOLD` — per-item average at/above
/// which the fast page cover fires.
pub const PAGE_AVG_REMOVABLE_RECTS_FAST_COVER_THRESHOLD: f64 = 24.0;
/// `PAGE_ITEM_REMOVABLE_RECTS_FAST_COVER_COUNT` — this many ≥-threshold items
/// trigger the fast page cover.
pub const PAGE_ITEM_REMOVABLE_RECTS_FAST_COVER_COUNT: usize = 8;

/// `HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD` — at/above this the decision routes
/// to `cover_only_count`.
pub const HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD: usize = 5000;
/// `VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD` — at/above this the decision routes
/// to `vector_heavy_redaction`.
pub const VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD: usize = 2000;
