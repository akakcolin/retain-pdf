// Ports of the services/rendering/layout leaf modules that feed the C3-N2 seed
// boundary (`build_block_payloads`): render_text / render_item model helpers,
// the block-seed body policy, fit metrics, preserved-line and title binary fit,
// and the typography-memory feature hash. Assemblere-level modules
// (block_seed_metrics / block_seed_payload_factory / block_seed) live under
// `crate::payload` in a later slice.

pub mod annotation_font_policy;
pub mod block_seed_body_policy;
pub mod body_context;
pub mod body_font_policy;
pub mod body_font_unify;
pub mod body_leading;
pub mod collision;
pub mod fit;
pub mod line_structure;
pub mod payload_dict;
pub mod policy_fields;
pub mod render_item;
pub mod suspicious_ocr;
pub mod typography_capacity;
