// Port of services/rendering/document_schema/semantics.py. The Python functions
// take a `payload: dict`; here they take `&Item`, reading the equivalent fields.

use crate::item::Item;

pub const CAPTION_TAGS: [&str; 6] = [
    "caption",
    "figure_caption",
    "image_caption",
    "table_caption",
    "table_footnote",
    "image_footnote",
];
pub const FOOTNOTE_TAGS: [&str; 4] = ["footnote", "image_footnote", "table_footnote", "vision_footnote"];
pub const REFERENCE_HEADING_TAGS: [&str; 1] = ["reference_heading"];
pub const REFERENCE_ENTRY_TAGS: [&str; 2] = ["reference_entry", "reference_zone"];
pub const ALGORITHM_TAGS: [&str; 1] = ["algorithm"];
pub const CAPTION_BLOCK_TYPES: [&str; 4] = ["figure_caption", "image_caption", "table_caption", "table_footnote"];
pub const FOOTNOTE_BLOCK_TYPES: [&str; 4] = ["footnote", "image_footnote", "table_footnote", "vision_footnote"];
pub const BODYLIKE_LAYOUT_ROLES: [&str; 2] = ["paragraph", "list_item"];
pub const BODYLIKE_SEMANTIC_ROLES: [&str; 2] = ["body", "abstract"];
pub const BODYLIKE_STRUCTURE_ROLES: [&str; 7] = [
    "",
    "body",
    "abstract",
    "example_line",
    "option_header",
    "option_description",
    "example_intro",
];
pub const TITLE_LIKE_LAYOUT_ROLES: [&str; 2] = ["title", "heading"];
pub const TITLE_LIKE_STRUCTURE_ROLES: [&str; 3] = ["title", "heading", "section_heading"];
pub const TEXTUAL_LAYOUT_ROLES: [&str; 5] = ["title", "heading", "paragraph", "list_item", "caption"];

fn field(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("").trim().to_lowercase()
}

pub fn normalize_tags(tags: Option<&[String]>) -> Vec<String> {
    let mut out = Vec::new();
    for tag in tags.into_iter().flatten() {
        let t = tag.trim();
        if !t.is_empty() {
            out.push(t.to_lowercase());
        }
    }
    out
}

pub fn derived_role(item: &Item) -> String {
    field(&item.derived_role)
}

pub fn normalized_sub_type(item: &Item) -> String {
    if item.normalized_sub_type.is_some() {
        return field(&item.normalized_sub_type);
    }
    field(&item.sub_type)
}

pub fn layout_role(item: &Item) -> String {
    field(&item.layout_role)
}

pub fn semantic_role(item: &Item) -> String {
    field(&item.semantic_role)
}

pub fn block_kind(item: &Item) -> String {
    if let Some(k) = &item.block_kind {
        let k = k.trim();
        if !k.is_empty() {
            return k.to_lowercase();
        }
    }
    let bt = item.block_type.as_deref().unwrap_or("unknown").trim().to_lowercase();
    if bt.is_empty() {
        "unknown".to_string()
    } else {
        bt
    }
}

pub fn policy_translate(item: &Item) -> Option<bool> {
    item.policy_translate
}

pub fn has_any_tag(item: &Item, tags: &[&str]) -> bool {
    let normalized = normalize_tags(Some(&item.tags));
    normalized.iter().any(|t| tags.contains(&t.as_str()))
}

pub fn structure_role(item: &Item) -> String {
    field(&item.structure_role)
}

pub fn is_caption_semantic(item: &Item) -> bool {
    if structure_role(item) == "figure_caption" {
        return true;
    }
    if layout_role(item) == "caption" {
        return true;
    }
    derived_role(item) == "caption"
        || derived_role(item) == "figure_caption"
        || has_any_tag(item, &CAPTION_TAGS)
}

pub fn is_caption_like_block(item: &Item) -> bool {
    if is_caption_semantic(item) {
        return true;
    }
    let block_type = item
        .block_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    CAPTION_BLOCK_TYPES.contains(&block_type.as_str())
}

pub fn is_footnote_like_block(item: &Item) -> bool {
    if layout_role(item) == "footnote" {
        return true;
    }
    if FOOTNOTE_TAGS.contains(&structure_role(item).as_str()) {
        return true;
    }
    if FOOTNOTE_TAGS.contains(&derived_role(item).as_str()) {
        return true;
    }
    if has_any_tag(item, &FOOTNOTE_TAGS) {
        return true;
    }
    let block_type = item.block_type.as_deref().unwrap_or("").trim().to_lowercase();
    FOOTNOTE_BLOCK_TYPES.contains(&block_type.as_str())
}

pub fn is_reference_heading_semantic(item: &Item) -> bool {
    if structure_role(item) == "reference_heading" {
        return true;
    }
    derived_role(item) == "reference_heading" || has_any_tag(item, &REFERENCE_HEADING_TAGS)
}

pub fn is_reference_entry_semantic(item: &Item) -> bool {
    if semantic_role(item) == "reference" {
        return true;
    }
    if structure_role(item) == "reference_entry" {
        return true;
    }
    derived_role(item) == "reference_entry" || has_any_tag(item, &REFERENCE_ENTRY_TAGS)
}

pub fn is_algorithm_semantic(item: &Item) -> bool {
    let block_type = item
        .raw_block_type
        .as_deref()
        .or_else(|| item.block_type.as_deref())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    normalized_sub_type(item) == "algorithm"
        || block_type == "algorithm"
        || derived_role(item) == "algorithm"
        || has_any_tag(item, &ALGORITHM_TAGS)
}

pub fn is_metadata_semantic(item: &Item) -> bool {
    normalized_sub_type(item) == "metadata" || semantic_role(item) == "metadata"
}

pub fn is_title_like_block(item: &Item) -> bool {
    if TITLE_LIKE_LAYOUT_ROLES.contains(&layout_role(item).as_str()) {
        return true;
    }
    TITLE_LIKE_STRUCTURE_ROLES.contains(&structure_role(item).as_str())
}

pub fn is_body_structure_role(item: &Item) -> bool {
    let role = structure_role(item);
    role.is_empty() || role == "body"
}

pub fn is_body_like_structure_role(item: &Item) -> bool {
    let role = structure_role(item);
    role.is_empty() || role == "body" || role == "example_line"
}

pub fn is_bodylike_block(item: &Item) -> bool {
    BODYLIKE_SEMANTIC_ROLES.contains(&semantic_role(item).as_str())
        || BODYLIKE_STRUCTURE_ROLES.contains(&structure_role(item).as_str())
        || BODYLIKE_LAYOUT_ROLES.contains(&layout_role(item).as_str())
}

pub fn is_textual_block(item: &Item) -> bool {
    if block_kind(item) == "text" {
        return true;
    }
    TEXTUAL_LAYOUT_ROLES.contains(&layout_role(item).as_str())
}

pub fn is_plain_text_block(item: &Item) -> bool {
    block_kind(item) == "text"
        && !(is_caption_like_block(item)
            || is_footnote_like_block(item)
            || is_reference_entry_semantic(item)
            || is_title_like_block(item))
}

pub fn is_plain_bodylike_block(item: &Item) -> bool {
    is_plain_text_block(item) && is_bodylike_block(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_like_via_layout_role() {
        let mut item = Item::default();
        item.layout_role = Some("caption".to_string());
        assert!(is_caption_like_block(&item));
    }

    #[test]
    fn footnote_like_via_structure_role() {
        let mut item = Item::default();
        item.structure_role = Some("footnote".to_string());
        assert!(is_footnote_like_block(&item));
    }

    #[test]
    fn footnote_like_via_tag() {
        let mut item = Item::default();
        item.tags = vec!["footnote".to_string()];
        assert!(is_footnote_like_block(&item));
    }

    #[test]
    fn block_kind_falls_back_to_block_type() {
        let mut item = Item::default();
        item.block_type = Some("Text".to_string());
        assert_eq!(block_kind(&item), "text");
    }

    #[test]
    fn block_kind_defaults_to_unknown() {
        let item = Item::default();
        assert_eq!(block_kind(&item), "unknown");
    }

    #[test]
    fn title_like_by_layout_role() {
        let mut item = Item::default();
        item.layout_role = Some("title".to_string());
        assert!(is_title_like_block(&item));
    }

    #[test]
    fn plain_text_block() {
        let mut item = Item::default();
        item.block_type = Some("text".to_string());
        assert!(is_plain_text_block(&item));
        assert!(is_plain_bodylike_block(&item));
    }
}
