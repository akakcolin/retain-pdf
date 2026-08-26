// Port of services/rendering/layout/payload/shared.py — the payload package's
// public facade. Names depending on markdown/render_text are not ported.

pub use crate::payload::continuation_split::{
    split_protected_text_for_boxes, CONTINUATION_REBALANCE_IMBALANCE_TRIGGER,
    CONTINUATION_REBALANCE_MAX_PASSES, CONTINUATION_REBALANCE_NON_PUNCT_MIN_MOVE_UNITS,
    CONTINUATION_REBALANCE_PUNCTUATION_PENALTY, CONTINUATION_REBALANCE_TARGET_TOLERANCE,
    CONTINUATION_REBALANCE_TOKEN_WINDOW,
};
pub use crate::payload::formula_cost::{approx_formula_visible_text, token_units};
pub use crate::payload::text_common::{
    layout_density_ratio, normalize_render_text, same_meaningful_render_text, source_word_count,
    strip_formula_placeholders, tokenize_protected_text, translated_zh_char_count,
    translation_density_ratio, trim_joined_tokens, COMPACT_SCALE, COMPACT_TRIGGER_RATIO,
    HEAVY_COMPACT_RATIO, LAYOUT_COMPACT_TRIGGER_RATIO, LAYOUT_HEAVY_COMPACT_RATIO, SPLIT_PUNCTUATION,
    WORD_RE, ZH_CHAR_RE,
};
