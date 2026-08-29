// Port of services/rendering/layout/payload/fit_common.py — binary-fit tuning
// constants. `fit_inner_bbox` lives in `crate::layout::render_item` (shared with
// the seed boundary) and is not re-declared here.

/// `TYPST_BINARY_*` fit-trigger ratios consumed by `fit_typst_context` and
/// `fit_typst_bounds`.
pub const TYPST_BINARY_OVERFLOW_TRIGGER: f64 = 1.08;
pub const TYPST_BINARY_DEMAND_TRIGGER: f64 = 1.10;
pub const TYPST_BINARY_DENSE_LAYOUT_TRIGGER: f64 = 0.92;
pub const TYPST_BINARY_FORMULA_RATIO_TRIGGER: f64 = 0.08;
pub const TYPST_BINARY_FORMULA_OVERFLOW_TRIGGER: f64 = 1.04;
pub const TYPST_BINARY_COLLISION_OVERFLOW_TRIGGER: f64 = 1.02;
pub const TYPST_BINARY_SOURCE_HEIGHT_TRIGGER: f64 = 1.01;
