//! `source/cleanup/redaction_padding.py` — rect expansion pads.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{
    ITEM_REDACTION_PAD_X, ITEM_REDACTION_PAD_Y, WORD_REDACTION_PAD_X, WORD_REDACTION_PAD_Y,
};

/// `expand_word_rect` — inflate by the word redaction pads.
pub fn expand_word_rect(rect: &RectTuple) -> RectTuple {
    [
        rect[0] - WORD_REDACTION_PAD_X,
        rect[1] - WORD_REDACTION_PAD_Y,
        rect[2] + WORD_REDACTION_PAD_X,
        rect[3] + WORD_REDACTION_PAD_Y,
    ]
}

/// `expand_item_rect` — inflate by the item redaction pads.
pub fn expand_item_rect(rect: &RectTuple) -> RectTuple {
    [
        rect[0] - ITEM_REDACTION_PAD_X,
        rect[1] - ITEM_REDACTION_PAD_Y,
        rect[2] + ITEM_REDACTION_PAD_X,
        rect[3] + ITEM_REDACTION_PAD_Y,
    ]
}
