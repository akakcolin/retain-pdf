// Port of services/rendering/layout/inline_content/{mode_router.py, core/,
// fallback/placeholder_markdown.py} plus the protected-token restore helper the
// placeholder path depends on.

pub mod inline_math;
pub mod markdown;
pub mod placeholder_markdown;
pub mod protected_tokens;

use serde_json::Value;

use crate::item::FormulaEntry;

pub const DEFAULT_RENDER_MATH_MODE: &str = "placeholder";
pub const DIRECT_TYPST_MATH_MODE: &str = "direct_typst";

/// `item_render_math_mode`.
pub fn item_render_math_mode(item: &Value) -> String {
    let mode = item
        .get("math_mode")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_RENDER_MATH_MODE)
        .trim();
    if mode.is_empty() {
        DEFAULT_RENDER_MATH_MODE.to_string()
    } else {
        mode.to_string()
    }
}

/// `is_direct_typst_math_mode`.
pub fn is_direct_typst_math_mode(item: &Value) -> bool {
    item_render_math_mode(item) == DIRECT_TYPST_MATH_MODE
}

/// `build_render_markdown`.
pub fn build_render_markdown(
    protected_text: &str,
    formula_map: &[FormulaEntry],
    math_mode: &str,
) -> String {
    let normalized_mode = math_mode.trim();
    let normalized_mode = if normalized_mode.is_empty() {
        DEFAULT_RENDER_MATH_MODE
    } else {
        normalized_mode
    };
    if normalized_mode == DIRECT_TYPST_MATH_MODE {
        return markdown::build_direct_typst_passthrough_text(protected_text);
    }
    placeholder_markdown::build_markdown_from_parts(protected_text, formula_map)
}

/// `build_item_render_markdown`.
pub fn build_item_render_markdown(
    item: &Value,
    protected_text: &str,
    formula_map: &[FormulaEntry],
) -> String {
    build_render_markdown(protected_text, formula_map, &item_render_math_mode(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::FormulaEntry;
    use serde_json::json;

    fn entry(placeholder: &str, formula_text: &str) -> FormulaEntry {
        FormulaEntry {
            placeholder: placeholder.to_string(),
            formula_text: formula_text.to_string(),
        }
    }

    #[test]
    fn reads_math_mode_from_item() {
        let item = json!({"math_mode": "direct_typst"});
        assert_eq!(item_render_math_mode(&item), "direct_typst");
        assert!(is_direct_typst_math_mode(&item));
        let item = json!({});
        assert_eq!(item_render_math_mode(&item), "placeholder");
        let item = json!({"math_mode": ""});
        assert_eq!(item_render_math_mode(&item), "placeholder");
    }

    #[test]
    fn direct_typst_passthrough() {
        let out = build_render_markdown("$x$ and y", &[], "direct_typst");
        assert!(out.contains("$x$"));
    }

    #[test]
    fn placeholder_mode_substitutes_formulas() {
        let map = vec![entry("<f1-abc/>", "a+b")];
        let out = build_render_markdown("a <f1-abc/> b", &map, "placeholder");
        assert!(out.contains("$a + b$"));
    }
}
