//! `source/cleanup/redaction_padding.py` — rect expansion pads. Only the
//! word-level pads are needed by the 7R-2 safe-direct path; item/formula/image
//! pads arrive with 7R-3.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{WORD_REDACTION_PAD_X, WORD_REDACTION_PAD_Y};

/// `expand_word_rect` — inflate by the word redaction pads.
pub fn expand_word_rect(rect: &RectTuple) -> RectTuple {
    [
        rect[0] - WORD_REDACTION_PAD_X,
        rect[1] - WORD_REDACTION_PAD_Y,
        rect[2] + WORD_REDACTION_PAD_X,
        rect[3] + WORD_REDACTION_PAD_Y,
    ]
}
