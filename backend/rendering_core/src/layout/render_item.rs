// Port of services/rendering/layout/payload/render_item.py + model/render_text.py
// (plus the protected-token restore helpers from
// services/translation/core/payload/formula_protection.py the seed boundary
// consumes). `seed_render_fields` is the Value-level, byte-exact dict-parity
// boundary; the Item-level helpers (`get_render_protected_text`,
// `get_render_formula_map`, `should_render_source_block`, ...) back it.
//
// `should_render_source_block` intentionally follows the seed's authoritative
// reference `payload/render_item.py` (final heuristic `latex_command_count >
// 0`), not `render_text.py`'s `"\\" in source_text` — the seed boundary must be
// byte-exact with `render_item.py`, and a bare backslash that is not a latex
// command must not flip the decision. The group-render-unit helpers
// (`group_render_unit_items`, `group_unit_*`) are consumed by Python's group
// seeding before the native call and are not ported here.

use crate::item::{FormulaEntry, Item, ProtectedEntry};
use crate::layout::line_structure::maybe_preserve_structured_line_breaks;
use crate::payload::text_common::same_meaningful_render_text;
use crate::text::analysis::analyze_text;
use crate::typography::geometry::inner_bbox;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

const FORMULA_MAP_CHAIN: [&str; 4] = [
    "render_formula_map",
    "translation_unit_formula_map",
    "group_formula_map",
    "formula_map",
];

pub const MODEL_KEEP_ORIGIN_REASONS: [&str; 1] = ["skip_model_keep_origin"];

/// `get_render_first_line_indent_pt`:
/// `max(0.0, float(item.get("_render_first_line_indent_pt") or 0.0))`.
pub fn get_render_first_line_indent_pt(item: &Item) -> f64 {
    item.render_first_line_indent_pt.max(0.0)
}

/// `get_render_inner_bbox`: the `_render_inner_bbox` value when it is a
/// 4-float list, else None. The deserializer already rejects non-4-float shapes.
pub fn get_render_inner_bbox(item: &Item) -> Option<[f64; 4]> {
    item.render_inner_bbox
}

/// `set_render_inner_bbox`: write a 4-float `_render_inner_bbox` back onto a
/// cloned item (used for the title-fit and fit-item copies).
pub fn set_render_inner_bbox(item: &mut Item, bbox: [f64; 4]) {
    item.render_inner_bbox = Some(bbox);
}

/// `fit_inner_bbox`: `get_render_inner_bbox(item) or inner_bbox(item)`.
pub fn fit_inner_bbox(item: &Item) -> Vec<f64> {
    match get_render_inner_bbox(item) {
        Some(bbox) => vec![bbox[0], bbox[1], bbox[2], bbox[3]],
        None => inner_bbox(item),
    }
}

/// `render_unit_kind`: `str(item.get("translation_unit_kind", "") or "").strip().lower()`.
pub(crate) fn render_unit_kind(item: &Value) -> String {
    item.get("translation_unit_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// `render_continuation_group_id`:
/// `str(item.get("continuation_group") or item.get("continuation_group_id") or "")`.
/// `continuation_group` may be a bool in the dict; Python stringifies the truthy
/// value, so `true` becomes `"True"`.
pub(crate) fn render_continuation_group_id(item: &Value) -> String {
    match item.get("continuation_group") {
        Some(Value::Bool(true)) => return "True".to_string(),
        Some(Value::String(s)) if !s.is_empty() => return s.clone(),
        _ => {}
    }
    item.get("continuation_group_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// `_render_should_use_unit_translation`.
pub(crate) fn render_should_use_unit_translation(item: &Value) -> bool {
    render_unit_kind(item) == "group" || !render_continuation_group_id(item).is_empty()
}

/// `_member_translation_text`: `protected_translated_text or translated_text`.
pub(crate) fn member_translation_text(item: &Value) -> String {
    first_non_empty(item, &["protected_translated_text", "translated_text"])
        .trim()
        .to_string()
}

/// `render_protected_translation_text` — the unit/group text chain, stripped.
pub(crate) fn render_protected_translation_text(item: &Value) -> String {
    let text = if !render_should_use_unit_translation(item) {
        first_non_empty(
            item,
            &[
                "protected_translated_text",
                "translated_text",
                "translation_unit_protected_translated_text",
                "translation_unit_translated_text",
            ],
        )
    } else if !render_continuation_group_id(item).is_empty()
        && !member_translation_text(item).is_empty()
    {
        member_translation_text(item)
    } else {
        first_non_empty(
            item,
            &[
                "translation_unit_protected_translated_text",
                "group_protected_translated_text",
                "protected_translated_text",
                "translation_unit_translated_text",
                "group_translated_text",
                "translated_text",
            ],
        )
    };
    text.trim().to_string()
}

/// `render_protected_source_text` — the group/non-group source chain, stripped.
/// Unlike the model's `_render_source_text` this does NOT read
/// `render_source_text` (the seed writes that key afterwards).
pub(crate) fn render_protected_source_text(item: &Value) -> String {
    let text = if render_unit_kind(item) != "group" {
        first_non_empty(
            item,
            &[
                "protected_source_text",
                "source_text",
                "translation_unit_protected_source_text",
                "translation_unit_source_text",
            ],
        )
    } else {
        first_non_empty(
            item,
            &[
                "translation_unit_protected_source_text",
                "group_protected_source_text",
                "protected_source_text",
                "translation_unit_source_text",
                "group_source_text",
                "source_text",
            ],
        )
    };
    text.trim().to_string()
}

/// Python `a or b or c ...` over string keys: the first non-empty string value.
fn first_non_empty(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = item.get(*key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

/// `clear_render_fields`: reset the render text/formula-map seeds (skip branch).
pub(crate) fn clear_render_fields(item: &mut Value) {
    item["render_protected_text"] = Value::String(String::new());
    item["render_formula_map"] = Value::Array(Vec::new());
}

/// `get_render_formula_map`: the first non-empty formula-map array. A present
/// non-list value mirrors Python's `isinstance(formula_map, list)` guard and
/// yields an empty result.
fn get_render_formula_map_value(item: &Value) -> Value {
    for key in FORMULA_MAP_CHAIN {
        match item.get(key) {
            Some(Value::Array(arr)) if !arr.is_empty() => return Value::Array(arr.clone()),
            Some(Value::Array(_)) => {}
            Some(_) => return Value::Array(Vec::new()),
            None => {}
        }
    }
    Value::Array(Vec::new())
}

/// `render_translation_unit_id`: `str(item.get("translation_unit_id", "") or "")`.
pub(crate) fn render_translation_unit_id(item: &Value) -> String {
    match item.get("translation_unit_id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// `group_render_unit_items`: group by `render_continuation_group_id(item) or
/// render_translation_unit_id(item)` when `_render_should_use_unit_translation`.
/// Continuation units carrying member text are dropped (Python's
/// `continuation_units_with_member_text` set). Returns unit-id -> item indices
/// in first-seen order so callers can mutate the flat items in place.
pub(crate) fn group_render_unit_items(items: &[Value]) -> BTreeMap<String, Vec<usize>> {
    let mut continuation_units_with_member_text: HashSet<String> = HashSet::new();
    for item in items {
        let unit_id = render_continuation_group_id(item);
        if !unit_id.is_empty() && !member_translation_text(item).is_empty() {
            continuation_units_with_member_text.insert(unit_id);
        }
    }
    let mut units: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        let continuation_id = render_continuation_group_id(item);
        if continuation_units_with_member_text.contains(&continuation_id) {
            continue;
        }
        let unit_id = if continuation_id.is_empty() {
            render_translation_unit_id(item)
        } else {
            continuation_id
        };
        if render_should_use_unit_translation(item) && !unit_id.is_empty() {
            units.entry(unit_id).or_default().push(index);
        }
    }
    units
}

/// `item_has_group_render_text`: `bool(render_protected_translation_text(item))`.
pub(crate) fn item_has_group_render_text(item: &Value) -> bool {
    !render_protected_translation_text(item).is_empty()
}

/// `group_unit_formula_map`: `get_render_formula_map(items[0])` or `[]`.
pub(crate) fn group_unit_formula_map(items: &[&Value]) -> Value {
    match items.first() {
        Some(first) => get_render_formula_map_value(first),
        None => Value::Array(Vec::new()),
    }
}

/// `group_unit_protected_text`: the longest member text, first wins on ties
/// (Python `max(..., key=len, default="")` keeps the first maximum).
pub(crate) fn group_unit_protected_text(items: &[&Value]) -> String {
    group_longest(items, render_protected_translation_text)
}

/// `group_unit_source_text`: the longest member source text, first wins on ties.
pub(crate) fn group_unit_source_text(items: &[&Value]) -> String {
    group_longest(items, render_protected_source_text)
}

fn group_longest(items: &[&Value], text_of: impl Fn(&Value) -> String) -> String {
    let mut best: Option<(String, usize)> = None;
    for item in items {
        let text = text_of(item);
        let len = text.chars().count();
        match &best {
            Some((_, best_len)) if *best_len >= len => {}
            _ => best = Some((text, len)),
        }
    }
    best.map(|(text, _)| text).unwrap_or_default()
}

/// `seed_render_fields`: compute the render-text / source-text / formula-map
/// seeds on a raw item dict. Mutates the Value in place, byte-exact with the
/// Python seed — including the `_render_preserve_line_breaks` /
/// `_render_line_structure` writes from the line-structure boundary. Reuses the
/// Item-based render decisions via `Item::from_json_value`.
pub fn seed_render_fields(item: &mut Value) {
    if should_skip_display_math_render(&Item::from_json_value(item)) {
        clear_render_fields(item);
        item["render_source_text"] = Value::String(render_protected_source_text(item));
        return;
    }
    let source_text = render_protected_source_text(item);
    let mut render_text = render_protected_translation_text(item);
    render_text = maybe_preserve_structured_line_breaks(item, &render_text);
    if render_text.is_empty() && should_render_source_block(&Item::from_json_value(item)) {
        render_text = source_text.clone();
    }
    let protected = if should_render_source_block(&Item::from_json_value(item)) {
        render_text
    } else if same_meaningful_render_text(&source_text, &render_text) {
        String::new()
    } else {
        render_text
    };
    item["render_protected_text"] = Value::String(protected);
    item["render_source_text"] = Value::String(source_text);
    item["render_formula_map"] = get_render_formula_map_value(item);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn first_line_indent_clamps_negative() {
        let item = Item { render_first_line_indent_pt: -2.0, ..Default::default() };
        assert_eq!(get_render_first_line_indent_pt(&item), 0.0);
    }

    #[test]
    fn fit_inner_bbox_prefers_render_bbox() {
        let mut item = Item::default();
        set_render_inner_bbox(&mut item, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(get_render_inner_bbox(&item), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(fit_inner_bbox(&item), vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn fit_inner_bbox_falls_back_to_geometry() {
        let item = Item { bbox: Some([10.0, 20.0, 110.0, 120.0]), ..Default::default() };
        assert_eq!(fit_inner_bbox(&item), vec![10.0, 20.0, 110.0, 120.0]);
    }

    #[test]
    fn seed_normal_body_paragraph() {
        let mut item = json!({
            "source_text": "Hello world",
            "translated_text": "你好世界",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!("你好世界"));
        assert_eq!(item["render_source_text"], json!("Hello world"));
        assert_eq!(item["render_formula_map"], json!([]));
    }

    #[test]
    fn seed_same_meaningful_text_empties_protected() {
        let mut item = json!({
            "source_text": "Same text",
            "translated_text": "  Same   text ",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(""));
        assert_eq!(item["render_source_text"], json!("Same text"));
    }

    #[test]
    fn seed_skip_display_math_clears_fields() {
        let mut item = json!({
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": false,
            "block_kind": "formula",
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(""));
        assert_eq!(item["render_formula_map"], json!([]));
        assert_eq!(item["render_source_text"], json!("x^2"));
    }

    #[test]
    fn seed_formula_block_keeps_translation() {
        let mut item = json!({
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": true,
            "block_kind": "formula",
        });
        seed_render_fields(&mut item);
        // should_render_source_block -> render_text kept verbatim.
        assert_eq!(item["render_protected_text"], json!("$x^2$"));
        assert_eq!(item["render_source_text"], json!("x^2"));
    }

    #[test]
    fn seed_empty_translation_falls_back_to_source() {
        let mut item = json!({
            "source_text": "x^2",
            "should_translate": true,
            "block_kind": "formula",
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!("x^2"));
        assert_eq!(item["render_source_text"], json!("x^2"));
    }

    #[test]
    fn seed_present_empty_block_kind_does_not_fall_through_to_block_type() {
        // render_text.py's inline block_kind is present-wins: a present-but-empty
        // `block_kind` yields "" and is NOT treated as a formula. The native
        // side must not fall through to block_type="formula" here.
        let mut item = json!({
            "source_text": "Hello world",
            "translated_text": "",
            "should_translate": true,
            "block_kind": "",
            "block_type": "formula",
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(""));
        assert_eq!(item["render_source_text"], json!("Hello world"));
    }

    #[test]
    fn seed_present_empty_block_kind_keeps_translation() {
        let mut item = json!({
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": false,
            "block_kind": "",
            "block_type": "formula",
        });
        seed_render_fields(&mut item);
        // Not a formula to the seed: skip-display-math is False and the
        // translation is kept, matching the Python reference.
        assert_eq!(item["render_protected_text"], json!("$x^2$"));
        assert_eq!(item["render_formula_map"], json!([]));
    }

    #[test]
    fn seed_absent_block_kind_falls_back_to_block_type_formula() {
        let mut item = json!({
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": false,
            "block_type": "formula",
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(""));
        assert_eq!(item["render_source_text"], json!("x^2"));
    }

    #[test]
    fn seed_bare_backslash_not_latex_command_keeps_empty() {
        // The seed reference `render_item.py::should_render_source_block` uses
        // `latex_command_count > 0`, NOT `"\\" in source_text`: `\(x^2\)` has a
        // backslash but no latex command, so an empty translation must not fall
        // back to the source.
        let mut item = json!({
            "source_text": r"\(x^2\)",
            "translated_text": "",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(""));
        assert_eq!(item["render_source_text"], json!(r"\(x^2\)"));
    }

    #[test]
    fn seed_latex_command_falls_back_to_source() {
        let mut item = json!({
            "source_text": r"\frac{a}{b}",
            "translated_text": "",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!(r"\frac{a}{b}"));
    }

    #[test]
    fn seed_group_unit_text_chain() {
        let mut item = json!({
            "translation_unit_kind": "group",
            "group_protected_translated_text": "组翻译",
            "protected_source_text": "来源",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!("组翻译"));
        assert_eq!(item["render_source_text"], json!("来源"));
    }

    #[test]
    fn seed_continuation_member_text() {
        let mut item = json!({
            "continuation_group": "g1",
            "protected_translated_text": "成员翻译",
            "protected_source_text": "来源",
            "should_translate": true,
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_protected_text"], json!("成员翻译"));
    }

    #[test]
    fn seed_formula_map_chain() {
        let mut item = json!({
            "source_text": "a+b",
            "translated_text": "a+b",
            "should_translate": true,
            "translation_unit_formula_map": [{"placeholder": "[[F1]]", "formula_text": "a+b"}],
            "formula_map": [{"x": 1}],
        });
        seed_render_fields(&mut item);
        assert_eq!(
            item["render_formula_map"],
            json!([{"placeholder": "[[F1]]", "formula_text": "a+b"}])
        );
        // Present non-list value yields an empty map.
        let mut item = json!({
            "source_text": "a+b",
            "translated_text": "a+b",
            "should_translate": true,
            "render_formula_map": {"not": "a list"},
        });
        seed_render_fields(&mut item);
        assert_eq!(item["render_formula_map"], json!([]));
    }

    #[test]
    fn seed_propagates_preserve_line_break_flag() {
        let mut item = json!({
            "source_text": "1. 甲\n2. 乙",
            "translated_text": "1. 甲内容 2. 乙内容",
            "should_translate": true,
            "text_flow": "preserve_lines",
            "source_line_texts": ["1. 甲", "2. 乙"],
        });
        seed_render_fields(&mut item);
        assert_eq!(item["_render_preserve_line_breaks"], json!(true));
        assert_eq!(item["_render_line_structure"], json!("structured_lines"));
        assert_eq!(item["render_protected_text"], json!("1. 甲内容\n2. 乙内容"));
    }

    #[test]
    fn translation_unit_id_stringifies() {
        assert_eq!(render_translation_unit_id(&json!({"translation_unit_id": "u1"})), "u1");
        assert_eq!(render_translation_unit_id(&json!({"translation_unit_id": 5})), "5");
        assert_eq!(render_translation_unit_id(&json!({})), "");
        assert_eq!(render_translation_unit_id(&json!({"translation_unit_id": null})), "");
    }

    #[test]
    fn group_units_by_continuation_then_unit_id() {
        let items = vec![
            json!({"translation_unit_kind": "group", "translation_unit_id": "g1", "group_protected_translated_text": "一"}),
            json!({"translation_unit_kind": "group", "translation_unit_id": "g1", "group_protected_translated_text": "二"}),
            json!({"translation_unit_kind": "group", "translation_unit_id": "g2", "group_protected_translated_text": "三"}),
            json!({"translation_unit_id": "g2", "should_translate": true}), // not a unit-translation item
            json!({"continuation_group": "cg", "translation_unit_kind": "group", "translation_unit_id": "g3", "group_protected_translated_text": "四"}),
        ];
        let units = group_render_unit_items(&items);
        assert_eq!(units.keys().collect::<Vec<_>>(), vec!["cg", "g1", "g2"]);
        assert_eq!(units["g1"], vec![0, 1]);
        assert_eq!(units["g2"], vec![2]);
        // The continuation "cg" unit's own id groups under "cg" (not g3).
        assert_eq!(units["cg"], vec![4]);
    }

    #[test]
    fn group_skips_continuation_units_with_member_text() {
        let items = vec![
            json!({"continuation_group": "cg", "protected_translated_text": "成员"}),
            json!({"continuation_group": "cg", "translation_unit_kind": "group", "group_protected_translated_text": "组"}),
        ];
        // The first member carries member text, so the whole "cg" unit is skipped.
        assert!(group_render_unit_items(&items).is_empty());
    }

    #[test]
    fn group_unit_text_and_formula_map() {
        let first = json!({"translation_unit_kind": "group", "protected_translated_text": "short", "protected_source_text": "s", "render_formula_map": [{"placeholder": "F1", "formula_text": "x"}]});
        let second = json!({"translation_unit_kind": "group", "protected_translated_text": "longer text", "protected_source_text": "longer source", "formula_map": [{"placeholder": "F2", "formula_text": "y"}]});
        let refs: Vec<&Value> = vec![&first, &second];
        assert_eq!(group_unit_protected_text(&refs), "longer text");
        assert_eq!(group_unit_source_text(&refs), "longer source");
        // First item's render_formula_map wins; empty group -> [].
        assert_eq!(group_unit_formula_map(&refs), json!([{"placeholder": "F1", "formula_text": "x"}]));
        assert_eq!(group_unit_formula_map(&[]), json!([]));
    }

    #[test]
    fn group_longest_uses_char_count_and_first_wins_on_ties() {
        // CJK "甲乙" is 2 chars / 6 bytes; "abcdef" is 6 chars / 6 bytes. Python
        // `len` is char count, so "abcdef" wins over "甲乙" even though the byte
        // lengths are equal.
        let cjk = json!({"translation_unit_kind": "group", "protected_translated_text": "甲乙"});
        let ascii = json!({"translation_unit_kind": "group", "protected_translated_text": "abcdef"});
        let refs: Vec<&Value> = vec![&cjk, &ascii];
        assert_eq!(group_unit_protected_text(&refs), "abcdef");
        // Ties keep the first item.
        let a = json!({"translation_unit_kind": "group", "protected_translated_text": "abc"});
        let b = json!({"translation_unit_kind": "group", "protected_translated_text": "def"});
        let refs: Vec<&Value> = vec![&a, &b];
        assert_eq!(group_unit_protected_text(&refs), "abc");
        assert_eq!(group_unit_source_text(&[]), "");
    }

    #[test]
    fn group_has_render_text_true_only_for_nonempty() {
        assert!(item_has_group_render_text(&json!({"protected_translated_text": "x"})));
        assert!(!item_has_group_render_text(&json!({"protected_translated_text": "  "})));
        assert!(!item_has_group_render_text(&json!({})));
    }
}
/// `protected_token_re().search(text)` — the three exact patterns
/// `<[futnvc]\d+-[0-9a-z]{3}/>`, `[[FORMULA_\d+]]`, `@@F\d+@@`.
pub fn has_protected_token(text: &str) -> bool {
    crate::text::tokens::has_protected_token(text)
}

/// `wrap_formula_inline_math`: strip, unwrap a `$...$` body when the whole
/// string is already inline math, re-wrap in `$...$`.
pub fn wrap_formula_inline_math(formula_text: &str) -> String {
    let text = formula_text.trim();
    if text.is_empty() {
        return String::new();
    }
    let body = inline_math_fullmatch_body(text).unwrap_or(text);
    format!("${body}$")
}

fn inline_math_fullmatch_body(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'$' || bytes[bytes.len() - 1] != b'$' {
        return None;
    }
    let body = &text[1..text.len() - 1];
    if body.is_empty() || body.contains('$') || body.contains('\n') {
        return None;
    }
    Some(body.trim())
}

/// `restore_protected_tokens`: for each entry, token_tag (or placeholder) →
/// restore_text (or formula_text or original_text); formula-type entries are
/// inline-math wrapped before substitution.
pub fn restore_protected_tokens(text: &str, protected_map: &[ProtectedEntry]) -> String {
    let mut restored = text.to_string();
    for entry in protected_map {
        let token_tag = &entry.token_tag;
        let mut restore_text = entry.restore_text.clone();
        if entry.token_type == "formula" {
            restore_text = wrap_formula_inline_math(&restore_text);
        }
        if !token_tag.is_empty() {
            restored = restored.replace(token_tag, &restore_text);
        }
    }
    restored
}

/// `_skip_reason`: `skip_reason or classification_label`, lowercased.
fn skip_reason(item: &Item) -> String {
    let mut raw = item.skip_reason.as_deref().unwrap_or("").to_string();
    if raw.is_empty() {
        raw = item.classification_label.as_deref().unwrap_or("").to_string();
    }
    raw.trim().to_lowercase()
}

/// `_render_source_text` chain, stripped.
fn render_source_text(item: &Item) -> String {
    for text in [
        &item.render_source_text,
        &item.protected_source_text,
        &item.source_text,
        &item.translation_unit_protected_source_text,
        &item.translation_unit_source_text,
    ] {
        if !text.is_empty() {
            return text.trim().to_string();
        }
    }
    String::new()
}

/// `continuation_group or continuation_group_id`.
fn continuation_active(item: &Item) -> bool {
    if item.continuation_group.unwrap_or(false) {
        return true;
    }
    item.continuation_group_id
        .as_deref()
        .map_or(false, |s| !s.is_empty())
}

/// `_should_use_unit_translation`: unit kind "group" or a continuation marker.
fn should_use_unit_translation(item: &Item) -> bool {
    let unit_kind = item
        .translation_unit_kind
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    unit_kind == "group" || continuation_active(item)
}

/// `_member_translation_text`: `protected_translated_text or translated_text`.
fn member_translation_text_item(item: &Item) -> String {
    let text = if !item.protected_translated_text.is_empty() {
        &item.protected_translated_text
    } else {
        &item.translated_text
    };
    text.trim().to_string()
}

/// `normalized_sub_type` read directly (no fallback to `sub_type`), matching
/// the model's `item.get("normalized_sub_type", "")`.
fn normalized_sub_type_raw(item: &Item) -> String {
    item.normalized_sub_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// Present-wins `block_kind` matching `render_text.py`'s inline
/// `str(item.get("block_kind", item.get("block_type", "")) or "").strip().lower()`:
/// a present-but-empty `block_kind` yields `""`, unlike `semantics::block_kind`
/// which falls through to `block_type`. The Python seed reference never falls
/// through, so the seed boundary must not either.
fn seed_block_kind(item: &Item) -> String {
    if let Some(k) = &item.block_kind {
        return k.trim().to_lowercase();
    }
    item.block_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

pub fn should_skip_display_math_render(item: &Item) -> bool {
    if item.should_translate.unwrap_or(true) {
        return false;
    }
    let source_text = render_source_text(item);
    if source_text.is_empty() {
        return false;
    }
    let block_kind = seed_block_kind(item);
    let sub_type = normalized_sub_type_raw(item);
    let skip_reason = skip_reason(item);
    if block_kind == "formula" || sub_type == "display_formula" {
        return true;
    }
    let analysis = analyze_text(&source_text);
    matches!(skip_reason.as_str(), "skip_display_formula" | "skip_model_keep_origin")
        && analysis.has_display_math()
        && analysis.plain_text.trim().is_empty()
}

pub fn should_render_source_block(item: &Item) -> bool {
    if should_skip_display_math_render(item) {
        return false;
    }
    if !item.should_translate.unwrap_or(true)
        && MODEL_KEEP_ORIGIN_REASONS.contains(&skip_reason(item).as_str())
    {
        return false;
    }
    let source_text = render_source_text(item);
    if source_text.is_empty() {
        return false;
    }
    let block_kind = seed_block_kind(item);
    let sub_type = normalized_sub_type_raw(item);
    if block_kind == "formula" || sub_type == "formula" || sub_type == "display_formula" {
        return true;
    }
    let analysis = analyze_text(&source_text);
    // Seed reference is `render_item.py::should_render_source_block`
    // (`latex_command_count > 0`), NOT `render_text.py`'s `"\\" in source_text`:
    // a bare backslash that is not a latex command must not flip the decision.
    analysis.raw_math_count() > 0 || analysis.latex_command_count() > 0
}

pub fn should_render_source_when_untranslated(item: &Item) -> bool {
    if item.should_translate.unwrap_or(true) {
        return false;
    }
    if MODEL_KEEP_ORIGIN_REASONS.contains(&skip_reason(item).as_str()) {
        return false;
    }
    should_render_source_block(item)
}

/// `_render_protected_map`: the first non-empty map in the chain, used directly
/// when it carries token-tag entries, otherwise converted to a formula-type
/// protected map.
fn render_protected_map(item: &Item) -> Vec<ProtectedEntry> {
    let unit_kind = item
        .translation_unit_kind
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if unit_kind != "group" {
        select_chain_map(
            item,
            &[
                ProtectedSlot::RenderFormulaMap,
                ProtectedSlot::ProtectedMap,
                ProtectedSlot::FormulaMap,
            ],
        )
    } else {
        select_chain_map(
            item,
            &[
                ProtectedSlot::TranslationUnitProtectedMap,
                ProtectedSlot::RenderFormulaMap,
                ProtectedSlot::TranslationUnitFormulaMap,
                ProtectedSlot::GroupFormulaMap,
                ProtectedSlot::ProtectedMap,
                ProtectedSlot::FormulaMap,
            ],
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ProtectedSlot {
    RenderFormulaMap,
    ProtectedMap,
    FormulaMap,
    TranslationUnitProtectedMap,
    TranslationUnitFormulaMap,
    GroupFormulaMap,
}

fn slot_has_token_tag(item: &Item, slot: ProtectedSlot) -> bool {
    match slot {
        ProtectedSlot::RenderFormulaMap => item.render_formula_map_has_token_tag,
        ProtectedSlot::ProtectedMap => item.protected_map_has_token_tag,
        ProtectedSlot::FormulaMap => item.formula_map_has_token_tag,
        ProtectedSlot::TranslationUnitProtectedMap => true,
        ProtectedSlot::TranslationUnitFormulaMap => item.translation_unit_formula_map_has_token_tag,
        ProtectedSlot::GroupFormulaMap => item.group_formula_map_has_token_tag,
    }
}

fn slot_entries(item: &Item, slot: ProtectedSlot) -> Vec<ProtectedEntry> {
    match slot {
        ProtectedSlot::RenderFormulaMap => formula_map_to_protected(&item.render_formula_map),
        ProtectedSlot::ProtectedMap => item.protected_map.clone(),
        ProtectedSlot::FormulaMap => formula_map_to_protected(&item.formula_map),
        ProtectedSlot::TranslationUnitProtectedMap => item.translation_unit_protected_map.clone(),
        ProtectedSlot::TranslationUnitFormulaMap => {
            formula_map_to_protected(&item.translation_unit_formula_map)
        }
        ProtectedSlot::GroupFormulaMap => formula_map_to_protected(&item.group_formula_map),
    }
}

/// Formula-map entries kept verbatim: `token_tag` from placeholder, `token_type`
/// left empty (so the restore does NOT inline-math-wrap), `restore_text` from
/// formula_text — mirroring a raw dict that has no `token_type` key.
fn formula_map_to_protected(formula_map: &[FormulaEntry]) -> Vec<ProtectedEntry> {
    formula_map
        .iter()
        .map(|e| ProtectedEntry {
            token_tag: e.placeholder.clone(),
            token_type: String::new(),
            restore_text: e.formula_text.clone(),
        })
        .collect()
}

fn select_chain_map(item: &Item, chain: &[ProtectedSlot]) -> Vec<ProtectedEntry> {
    for slot in chain {
        let has_token_tag = slot_has_token_tag(item, *slot);
        let entries = slot_entries(item, *slot);
        if entries.is_empty() {
            continue;
        }
        if has_token_tag {
            return entries;
        }
        // protected_map_from_formula_map: token_tag=placeholder,
        // token_type="formula", restore_text=formula_text.
        return entries
            .into_iter()
            .map(|e| ProtectedEntry {
                token_tag: e.token_tag,
                token_type: "formula".to_string(),
                restore_text: e.restore_text,
            })
            .collect();
    }
    Vec::new()
}

pub fn restore_render_protected_text(text: &str, item: &Item) -> String {
    let current = text.trim();
    if current.is_empty() || !has_protected_token(current) {
        return current.to_string();
    }
    let restored = restore_protected_tokens(current, &render_protected_map(item));
    restored.trim().to_string()
}

pub fn get_render_protected_text(item: &Item) -> String {
    if should_skip_display_math_render(item) {
        return String::new();
    }
    if item.has_render_protected_text {
        return restore_render_protected_text(&item.render_protected_text, item);
    }
    if !should_use_unit_translation(item) {
        let translated = unit_translated_plain(item);
        if translated.is_empty() && should_render_source_block(item) {
            return restore_render_protected_text(&render_source_text(item), item);
        }
        return restore_render_protected_text(&translated, item);
    }
    if continuation_active(item) && !member_translation_text_item(item).is_empty() {
        let member = member_translation_text_item(item);
        return restore_render_protected_text(&member, item);
    }
    let group_text = group_translated_plain(item);
    restore_render_protected_text(&group_text, item)
}

fn unit_translated_plain(item: &Item) -> String {
    first_non_empty_str(&[
        &item.protected_translated_text,
        &item.translated_text,
        &item.translation_unit_protected_translated_text,
        &item.translation_unit_translated_text,
    ])
    .trim()
    .to_string()
}

fn group_translated_plain(item: &Item) -> String {
    first_non_empty_str(&[
        &item.translation_unit_protected_translated_text,
        &item.group_protected_translated_text,
        &item.protected_translated_text,
        &item.translation_unit_translated_text,
        &item.group_translated_text,
        &item.translated_text,
    ])
    .trim()
    .to_string()
}

fn first_non_empty_str<'a>(values: &[&'a str]) -> &'a str {
    for value in values {
        if !value.is_empty() {
            return value;
        }
    }
    ""
}

/// `get_render_formula_map`: `render_formula_map or translation_unit_formula_map
/// or group_formula_map or formula_map`.
pub fn get_render_formula_map(item: &Item) -> Vec<FormulaEntry> {
    if !item.render_formula_map.is_empty() {
        return item.render_formula_map.clone();
    }
    if !item.translation_unit_formula_map.is_empty() {
        return item.translation_unit_formula_map.clone();
    }
    if !item.group_formula_map.is_empty() {
        return item.group_formula_map.clone();
    }
    item.formula_map.clone()
}
