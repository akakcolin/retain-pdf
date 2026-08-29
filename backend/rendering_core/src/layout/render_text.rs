// Port of services/rendering/layout/model/render_text.py plus the
// protected-token restore helpers from
// services/translation/core/payload/formula_protection.py (the subset the seed
// boundary consumes: wrap_formula_inline_math / restore_protected_tokens /
// protected_map_from_formula_map / the PROTECTED_TOKEN_RE gate).
//
// `should_render_source_block` intentionally follows the seed's authoritative
// reference `payload/render_item.py` (final heuristic `latex_command_count >
// 0`), not `render_text.py`'s `"\\" in source_text` — the seed boundary must be
// byte-exact with `render_item.py`, and a bare backslash that is not a latex
// command must not flip the decision.

use crate::item::{FormulaEntry, Item, ProtectedEntry};
use crate::text::analysis::analyze_text;

pub const MODEL_KEEP_ORIGIN_REASONS: [&str; 1] = ["skip_model_keep_origin"];

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
fn member_translation_text(item: &Item) -> String {
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
    if continuation_active(item) && !member_translation_text(item).is_empty() {
        let member = member_translation_text(item);
        return restore_render_protected_text(&member, item);
    }
    let group_text = group_translated_plain(item);
    restore_render_protected_text(&group_text, item)
}

fn unit_translated_plain(item: &Item) -> String {
    first_non_empty(&[
        &item.protected_translated_text,
        &item.translated_text,
        &item.translation_unit_protected_translated_text,
        &item.translation_unit_translated_text,
    ])
    .trim()
    .to_string()
}

fn group_translated_plain(item: &Item) -> String {
    first_non_empty(&[
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

fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
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
