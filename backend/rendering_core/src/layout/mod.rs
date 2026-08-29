// Ports of the services/rendering/layout leaf modules that feed the C3-N2 seed
// boundary (`build_block_payloads`): render_text / render_item model helpers,
// the block-seed body policy, fit metrics, preserved-line and title binary fit,
// and the typography-memory feature hash. Assemblere-level modules
// (block_seed_metrics / block_seed_payload_factory / block_seed) live under
// `crate::payload` in a later slice.

pub mod annotation_font_policy;
pub mod block_seed_body_policy;
pub mod body_common;
pub mod body_context;
pub mod body_fit_policy;
pub mod body_font_dense_policy;
pub mod body_font_harmonize_policy;
pub mod body_font_inheritance_policy;
pub mod body_font_underfill_policy;
pub mod body_font_unify_policy;
pub mod body_leading_policy;
pub mod body_leading_solver;
pub mod body_page_anchor_policy;
pub mod body_smoothing_policy;
pub mod collision;
pub mod collision_context;
pub mod fit_metrics;
pub mod fit_vertical;
pub mod line_structure;
pub mod payload_dict;
pub mod render_item;
pub mod render_text;
pub mod title_binary_fit;
pub mod typography_decision;
pub mod typography_memory;
pub mod typography_policy;
