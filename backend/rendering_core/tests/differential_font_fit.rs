// Differential (golden replay) tests: font_size_fit / leading_fit /
// title_fit_limits / font_roles.

mod common;

use common::*;
use rendering_core::font_roles::is_body_text_candidate;
use rendering_core::font_size_fit::{estimate_font_size_pt, local_font_size_pt};
use rendering_core::leading_fit::{estimate_leading_em, normalize_leading_em_for_font_size};
use rendering_core::title_fit_limits::resolve_title_fill_max_font_size_pt;

#[test]
fn test_local_font_size_pt() {
    let c = corpus();
    for (_i, case) in cases(c, "font_fit.local_font_size_pt").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: f64 = from_value(&case.expected);
        let actual = local_font_size_pt(&item.to_item());
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_estimate_font_size_pt() {
    let c = corpus();
    for (_i, case) in cases(c, "font_fit.estimate_font_size_pt").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let page_font: f64 = from_value(&case.input["page_font_size"]);
        let page_pitch: f64 = from_value(&case.input["page_line_pitch"]);
        let page_height: f64 = from_value(&case.input["page_line_height"]);
        let expected: f64 = from_value(&case.expected);
        let actual = estimate_font_size_pt(&item.to_item(), page_font, page_pitch, page_height, 0.0);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_estimate_leading_em() {
    let c = corpus();
    for (_i, case) in cases(c, "font_fit.estimate_leading_em").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let pitch: f64 = from_value(&case.input["page_line_pitch"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let expected: f64 = from_value(&case.expected);
        let actual = estimate_leading_em(&item.to_item(), pitch, font);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_normalize_leading_em_for_font_size() {
    let c = corpus();
    for (_i, case) in cases(c, "font_fit.normalize_leading_em_for_font_size").iter().enumerate() {
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let reference: f64 = from_value(&case.input["reference_font_size_pt"]);
        let min_leading: f64 = from_value(&case.input["min_leading_em"]);
        let max_leading: f64 = from_value(&case.input["max_leading_em"]);
        let strength: f64 = from_value(&case.input["strength"]);
        let floor_min: Option<f64> = from_value(&case.input["floor_min_leading_em"]);
        let expected: f64 = from_value(&case.expected);
        let actual = normalize_leading_em_for_font_size(
            font,
            leading,
            reference,
            min_leading,
            max_leading,
            strength,
            floor_min,
        );
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_resolve_title_fill_max_font_size_pt() {
    let c = corpus();
    for (_i, case) in cases(c, "font_fit.resolve_title_fill_max_font_size_pt").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let base: f64 = from_value(&case.input["base_font_size_pt"]);
        let expected: f64 = from_value(&case.expected);
        let actual = resolve_title_fill_max_font_size_pt(&item.to_item(), base);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_is_body_text_candidate() {
    let c = corpus();
    for (i, case) in cases(c, "font_fit.is_body_text_candidate").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let width_med: f64 = from_value(&case.input["page_text_width_med"]);
        let expected: bool = from_value(&case.expected);
        let actual = is_body_text_candidate(&item.to_item(), width_med);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}
