//! Port of `layout/payload/formula_safety.py`: formula-driven padding insets
//! and long-inline-math layout-risk detection.

use crate::dto::MathMapEntry;
use crate::py_re;
use crate::pyre::py_find_iter;
use crate::text_analysis::formula_texts_for_render;
use crate::util::round_2dp;

pub const MIN_SAFE_CONTENT_HEIGHT_PT: f64 = 8.0;
pub const MAX_FORMULA_INSET_HEIGHT_RATIO: f64 = 0.18;
pub const LONG_INLINE_MATH_MIN_BODY_CHARS: usize = 64;
pub const LONG_INLINE_MATH_WIDTH_RATIO: f64 = 0.82;
pub const FORMULA_WIDTH_UNIT_PT_RATIO: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormulaSafetyInsets {
    pub top_pt: f64,
    pub bottom_pt: f64,
}

impl Default for FormulaSafetyInsets {
    fn default() -> Self {
        FormulaSafetyInsets {
            top_pt: 0.0,
            bottom_pt: 0.0,
        }
    }
}

impl FormulaSafetyInsets {
    pub fn total_pt(self) -> f64 {
        self.top_pt + self.bottom_pt
    }

    pub fn active(self) -> bool {
        self.total_pt() > 0.01
    }
}

/// `formula_needs_extra_descent`.
pub fn formula_needs_extra_descent(formula_text: &str) -> bool {
    py_is_search(py_re!(r"[_^]|\\(?:frac|dfrac|tfrac|sqrt|sum|prod|int|iint|iiint|lim|underset|overset|substack)\b"), formula_text)
}

/// `formula_safety_insets_pt`.
pub fn formula_safety_insets_pt(
    text: &str,
    formula_map: &[MathMapEntry],
    font_size_pt: f64,
    box_height_pt: f64,
) -> FormulaSafetyInsets {
    if font_size_pt <= 0.0 || box_height_pt <= MIN_SAFE_CONTENT_HEIGHT_PT {
        return FormulaSafetyInsets::default();
    }
    let formula_texts = formula_texts_for_render(text, formula_map);
    if formula_texts.is_empty() {
        return FormulaSafetyInsets::default();
    }
    let has_deep_formula = formula_texts.iter().any(|f| formula_needs_extra_descent(f));
    let top_ratio = if has_deep_formula { 0.07 } else { 0.045 };
    let bottom_ratio = if has_deep_formula { 0.18 } else { 0.11 };
    let top = (font_size_pt * top_ratio).max(0.25).min(1.15);
    let bottom = (font_size_pt * bottom_ratio).max(0.55).min(2.6);
    fit_insets_to_box(top, bottom, box_height_pt)
}

/// `has_long_inline_math_layout_risk`.
pub fn has_long_inline_math_layout_risk(
    text: &str,
    formula_map: &[MathMapEntry],
    font_size_pt: f64,
    box_width_pt: f64,
) -> bool {
    if font_size_pt <= 0.0 || box_width_pt <= MIN_SAFE_CONTENT_HEIGHT_PT {
        return false;
    }
    formula_texts_for_render(text, formula_map)
        .iter()
        .any(|formula| inline_formula_has_layout_risk(formula, font_size_pt, box_width_pt))
}

fn inline_formula_has_layout_risk(formula_text: &str, font_size_pt: f64, box_width_pt: f64) -> bool {
    let formula = formula_text.trim();
    if formula.chars().count() < LONG_INLINE_MATH_MIN_BODY_CHARS {
        return false;
    }
    let visible_units = formula_visible_units(formula);
    let estimated_width = visible_units * font_size_pt * FORMULA_WIDTH_UNIT_PT_RATIO;
    estimated_width >= box_width_pt * LONG_INLINE_MATH_WIDTH_RATIO
}

fn formula_visible_units(formula_text: &str) -> f64 {
    let compact: String = py_re!(r"\\[A-Za-z]+|[{}\s]")
        .replace_all(formula_text, "")
        .into_owned();
    if compact.is_empty() {
        return 0.0;
    }
    let command_bonus = py_find_iter(py_re!(r"\\[A-Za-z]+"), formula_text).len() as f64 * 0.18;
    let script_bonus = py_find_iter(py_re!(r"[_^]"), formula_text).len() as f64 * 0.12;
    (compact.chars().count() as f64 * 0.42 + command_bonus + script_bonus).max(0.0)
}

/// `formula_safe_inner_bbox`.
pub fn formula_safe_inner_bbox(
    inner_bbox: &[f64],
    text: &str,
    formula_map: &[MathMapEntry],
    font_size_pt: f64,
) -> (Vec<f64>, FormulaSafetyInsets) {
    if inner_bbox.len() != 4 {
        return (inner_bbox.to_vec(), FormulaSafetyInsets::default());
    }
    let x0 = inner_bbox[0];
    let y0 = inner_bbox[1];
    let x1 = inner_bbox[2];
    let y1 = inner_bbox[3];
    let height = (y1 - y0).max(0.0);
    let insets = formula_safety_insets_pt(text, formula_map, font_size_pt, height);
    if !insets.active() {
        return (vec![x0, y0, x1, y1], insets);
    }
    (vec![x0, y0 + insets.top_pt, x1, y1 - insets.bottom_pt], insets)
}

fn fit_insets_to_box(top: f64, bottom: f64, box_height_pt: f64) -> FormulaSafetyInsets {
    let available = (box_height_pt - MIN_SAFE_CONTENT_HEIGHT_PT).max(0.0);
    let cap = (box_height_pt * MAX_FORMULA_INSET_HEIGHT_RATIO).min(available);
    let total = top + bottom;
    if cap <= 0.0 || total <= 0.0 {
        return FormulaSafetyInsets::default();
    }
    let (top, bottom) = if total > cap {
        let scale = cap / total;
        (top * scale, bottom * scale)
    } else {
        (top, bottom)
    };
    FormulaSafetyInsets {
        top_pt: round_2dp(top),
        bottom_pt: round_2dp(bottom),
    }
}

fn py_is_search(re: &fancy_regex::Regex, text: &str) -> bool {
    re.is_match(text).expect("fancy-regex is_match")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_or_tall_formula_needs_descent() {
        assert!(formula_needs_extra_descent(r"x^2"));
        assert!(formula_needs_extra_descent(r"\frac{a}{b}"));
        assert!(formula_needs_extra_descent(r"\sqrt{x}"));
        assert!(!formula_needs_extra_descent("abc"));
    }

    #[test]
    fn insets_inactive_below_min_height() {
        let insets = formula_safety_insets_pt("x^2", &[], 10.0, 7.0);
        assert!(!insets.active());
    }

    #[test]
    fn insets_scale_into_box() {
        let insets = formula_safety_insets_pt("$x^2$", &[], 10.0, 30.0);
        assert!(insets.active());
        assert!(insets.total_pt() <= 30.0 * 0.18);
    }

    #[test]
    fn long_inline_math_risk_detected() {
        let long = format!("${}$", "x".repeat(70));
        let risk = has_long_inline_math_layout_risk(long.as_str(), &[], 12.0, 200.0);
        assert!(risk);
        let short = "$x$";
        assert!(!has_long_inline_math_layout_risk(short, &[], 12.0, 200.0));
    }
}
