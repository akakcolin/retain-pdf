// Port of services/rendering/layout/payload/formula_safety.py — the subset the
// emit boundary consumes: `formula_safe_inner_bbox` (via insets + deep-formula
// detection). The output crate carries its own copy of this module for the
// typst render path (fancy-regex based); this one is hand-rolled on std only.

use crate::item::FormulaEntry;
use crate::text::analysis::formula_texts_for_render;
use crate::util::py_round;

pub const MIN_SAFE_CONTENT_HEIGHT_PT: f64 = 8.0;
pub const MAX_FORMULA_INSET_HEIGHT_RATIO: f64 = 0.18;

const TALL_COMMANDS: &[&str] = &[
    "frac", "dfrac", "tfrac", "sqrt", "sum", "prod", "int", "iint", "iiint", "lim", "underset", "overset",
    "substack",
];

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

/// Mirrors `SCRIPT_OR_TALL_MATH_RE.search` — a `_`/`^` anywhere, or a backslash
/// command in the tall set whose name is followed by a non-word boundary.
fn has_script_or_tall_math(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'_' | b'^' => return true,
            b'\\' => {
                let mut j = i + 1;
                let start = j;
                while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let cmd = &text[start..j];
                if TALL_COMMANDS.contains(&cmd) {
                    let after_word = bytes.get(j).map_or(true, |c| !c.is_ascii_alphanumeric() && *c != b'_');
                    if after_word {
                        return true;
                    }
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// `formula_needs_extra_descent`.
pub fn formula_needs_extra_descent(formula_text: &str) -> bool {
    has_script_or_tall_math(formula_text)
}

/// `formula_safety_insets_pt`.
pub fn formula_safety_insets_pt(
    text: &str,
    formula_map: &[FormulaEntry],
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

/// `formula_safe_inner_bbox`: shrink `inner_bbox` top/bottom by the active
/// formula insets; pass-through when no formula-driven insets apply.
pub fn formula_safe_inner_bbox(
    inner_bbox: &[f64],
    text: &str,
    formula_map: &[FormulaEntry],
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
        top_pt: py_round(top, 2),
        bottom_pt: py_round(bottom, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_or_tall_formula_needs_descent() {
        assert!(formula_needs_extra_descent(r"x^2"));
        assert!(formula_needs_extra_descent(r"x_2"));
        assert!(formula_needs_extra_descent(r"\frac{a}{b}"));
        assert!(formula_needs_extra_descent(r"\sqrt{x}"));
        assert!(formula_needs_extra_descent(r"\sum_{i}"));
        assert!(!formula_needs_extra_descent("abc"));
        assert!(!formula_needs_extra_descent(r"\alpha"));
    }

    #[test]
    fn tall_command_requires_boundary() {
        assert!(formula_needs_extra_descent(r"\frac x"));
        assert!(!formula_needs_extra_descent(r"\fracx"));
        assert!(formula_needs_extra_descent(r"\sum"));
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
        assert!(insets.total_pt() <= 30.0 * MAX_FORMULA_INSET_HEIGHT_RATIO);
    }

    #[test]
    fn safe_inner_bbox_shrinks_top_and_bottom() {
        let (bbox, insets) = formula_safe_inner_bbox(&[0.0, 0.0, 100.0, 30.0], "$x^2$", &[], 10.0);
        assert!(insets.active());
        assert!(bbox[1] > 0.0);
        assert!(bbox[3] < 30.0);
    }

    #[test]
    fn safe_inner_bbox_passthrough_without_formula() {
        let (bbox, insets) = formula_safe_inner_bbox(&[0.0, 0.0, 100.0, 30.0], "plain text", &[], 10.0);
        assert!(!insets.active());
        assert_eq!(bbox, vec![0.0, 0.0, 100.0, 30.0]);
    }
}
