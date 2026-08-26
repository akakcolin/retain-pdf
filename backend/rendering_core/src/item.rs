// Rust replacement for the Python `item: dict` (layout block) consumed by the
// typography / font_fit / payload modules. Tests construct these directly, so
// every field mirrors a Python dict key from the fixtures.

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// "text" or "inline_equation".
    pub span_type: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub bbox: Option<[f64; 4]>,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormulaEntry {
    pub placeholder: String,
    /// From `formula_text`, falling back to `latex`.
    pub formula_text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub bbox: Option<[f64; 4]>,
    pub lines: Vec<Line>,
    pub source_text: String,
    pub layout_role: Option<String>,
    pub semantic_role: Option<String>,
    pub structure_role: Option<String>,
    pub block_kind: Option<String>,
    pub block_type: Option<String>,
    pub sub_type: Option<String>,
    pub normalized_sub_type: Option<String>,
    pub raw_block_type: Option<String>,
    pub tags: Vec<String>,
    /// Mirrors `payload["derived"]["role"]`.
    pub derived_role: Option<String>,
    /// Mirrors `payload["policy"]["translate"]`.
    pub policy_translate: Option<bool>,
    pub formula_map: Vec<FormulaEntry>,
    /// Mirrors `payload["body_repair_applied"]`.
    pub body_repair_applied: Option<bool>,
    /// Mirrors `payload["provider_body_repair_applied"]`.
    pub provider_body_repair_applied: Option<bool>,
    /// Mirrors `payload["body_repair_role"]`.
    pub body_repair_role: Option<String>,
    /// Mirrors `payload["provider_body_repair_role"]`.
    pub provider_body_repair_role: Option<String>,
    /// Mirrors `payload["body_repair_peer_block_id"]`.
    pub body_repair_peer_block_id: Option<String>,
    /// Mirrors `payload["provider_suspected_peer_block_id"]`.
    pub provider_suspected_peer_block_id: Option<String>,
    // Internal layout flags.
    pub is_body_text_candidate: bool,
    pub wide_aspect_body_text: bool,
    pub cover_with_inner_bbox: bool,
}
