// Port of services/rendering/layout/font_roles.py.

use crate::item::Item;
pub use crate::semantics::{
    is_caption_like_block, is_footnote_like_block, is_title_like_block,
};
use crate::semantics::{
    block_kind, is_plain_bodylike_block, is_plain_text_block, is_textual_block, layout_role,
    semantic_role,
};
use crate::typography::content::formula_ratio;
use crate::typography::line_count::source_visual_line_count;
use crate::typography::line_metrics::bbox_width;

pub const BODY_FORMULA_RATIO_MAX: f64 = 0.5;

fn nonspace_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

pub fn item_layout_role_name(item: &Item) -> String {
    layout_role(item)
}

pub fn item_semantic_role_name(item: &Item) -> String {
    semantic_role(item)
}

pub fn is_local_textual_item(item: &Item) -> bool {
    if is_caption_like_block(item) || is_footnote_like_block(item) {
        return true;
    }
    if is_title_like_block(item) {
        return true;
    }
    if block_kind(item) == "text" {
        return true;
    }
    is_textual_block(item)
}

pub fn is_body_text_candidate(item: &Item, page_text_width_med: f64) -> bool {
    if is_caption_like_block(item) || is_footnote_like_block(item) {
        return false;
    }
    let layout_role = item_layout_role_name(item);
    let semantic_role = item_semantic_role_name(item);
    if !is_plain_text_block(item) && layout_role != "paragraph" && layout_role != "list_item" {
        return false;
    }
    if semantic_role != "" && semantic_role != "body" && semantic_role != "abstract" {
        return false;
    }
    if formula_ratio(item) > BODY_FORMULA_RATIO_MAX {
        return false;
    }
    let text_len = nonspace_len(&item.source_text);
    let width = bbox_width(item);
    if page_text_width_med > 0.0 && width < page_text_width_med * 0.75 {
        if !(is_plain_bodylike_block(item) && text_len >= 36 && source_visual_line_count(item) >= 2) {
            return false;
        }
    }
    text_len >= 40
}

pub fn is_default_text_block(item: &Item) -> bool {
    if is_title_like_block(item) {
        return true;
    }
    if !is_plain_text_block(item) {
        return false;
    }
    let line_count = item.lines.len();
    let text_len = nonspace_len(&item.source_text);
    line_count <= 1 && text_len < 60
}

pub fn resolve_font_weight(item: &Item) -> String {
    if is_title_like_block(item) {
        "bold".to_string()
    } else {
        "regular".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};

    fn body_item() -> Item {
        Item {
            block_type: Some("text".into()),
            source_text: "This is a sufficiently long body text block for the candidate test.".into(),
            bbox: Some([40.0, 100.0, 400.0, 160.0]),
            lines: vec![
                Line { bbox: Some([40.0, 100.0, 390.0, 115.0]), spans: vec![Span { span_type: "text".into(), content: "line one".into() }] },
                Line { bbox: Some([40.0, 115.0, 390.0, 130.0]), spans: vec![Span { span_type: "text".into(), content: "line two".into() }] },
                Line { bbox: Some([40.0, 130.0, 390.0, 145.0]), spans: vec![Span { span_type: "text".into(), content: "line three".into() }] },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn local_textual_for_text_block() {
        assert!(is_local_textual_item(&body_item()));
    }

    #[test]
    fn body_candidate_accepts_plain_body() {
        assert!(is_body_text_candidate(&body_item(), 600.0));
    }

    #[test]
    fn footnote_not_body_candidate() {
        let mut item = body_item();
        item.layout_role = Some("footnote".into());
        assert!(!is_body_text_candidate(&item, 600.0));
    }

    #[test]
    fn default_text_block_single_short_line() {
        let mut item = Item::default();
        item.block_type = Some("text".into());
        item.source_text = "short".into();
        item.lines = vec![Line { bbox: None, spans: vec![] }];
        assert!(is_default_text_block(&item));
    }
}
