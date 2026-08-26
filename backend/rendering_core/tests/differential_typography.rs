// Differential (golden replay) tests: typography (line_metrics / geometry /
// cover_geometry / compactness / content / line_count / scalars / baseline).

mod common;

use common::*;
use rendering_core::typography::{
    bbox_height, bbox_width, cover_bbox, effective_text_height, expanded_cover_bbox, formula_ratio,
    inner_bbox, line_height, line_widths, local_font_metric, local_glyph_height, local_line_pitch,
    median_line_height, median_line_pitch, occupied_ratio, occupied_ratio_x,
    page_baseline_font_size, percentile_value, plain_text_chars_per_line, source_compactness_score,
    source_visual_line_count, visual_line_count,
};

macro_rules! item_float_test {
    ($name:ident, $fn_id:literal, $f:path) => {
        #[test]
        fn $name() {
            let c = corpus();
            for (_i, case) in cases(c, $fn_id).iter().enumerate() {
                let item: ItemDto = from_value(&case.input["item"]);
                let expected: f64 = from_value(&case.expected);
                let actual = $f(&item.to_item());
                assert_close_f64(actual, expected);
            }
        }
    };
}

#[test]
fn test_percentile_value() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.percentile_value").iter().enumerate() {
        let values: Vec<f64> = from_value(&case.input["values"]);
        let q: f64 = from_value(&case.input["q"]);
        let expected: f64 = from_value(&case.expected);
        let actual = percentile_value(&values, q);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_visual_line_count() {
    let c = corpus();
    for (i, case) in cases(c, "typography.visual_line_count").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: i64 = from_value(&case.expected);
        let actual = visual_line_count(&item.to_item());
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_source_visual_line_count() {
    let c = corpus();
    for (i, case) in cases(c, "typography.source_visual_line_count").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: i64 = from_value(&case.expected);
        let actual = source_visual_line_count(&item.to_item());
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_line_height() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.line_height").iter().enumerate() {
        let line: LineDto = from_value(&case.input["line"]);
        let expected: f64 = from_value(&case.expected);
        let actual = line_height(&line.to_line());
        assert_close_f64(actual, expected);
    }
}

item_float_test!(test_median_line_height, "typography.median_line_height", median_line_height);
item_float_test!(test_median_line_pitch, "typography.median_line_pitch", median_line_pitch);
item_float_test!(test_local_line_pitch, "typography.local_line_pitch", local_line_pitch);
item_float_test!(test_local_glyph_height, "typography.local_glyph_height", local_glyph_height);
item_float_test!(test_local_font_metric, "typography.local_font_metric", local_font_metric);
item_float_test!(test_bbox_width, "typography.bbox_width", bbox_width);
item_float_test!(test_bbox_height, "typography.bbox_height", bbox_height);
item_float_test!(test_effective_text_height, "typography.effective_text_height", effective_text_height);
item_float_test!(test_occupied_ratio, "typography.occupied_ratio", occupied_ratio);
item_float_test!(test_occupied_ratio_x, "typography.occupied_ratio_x", occupied_ratio_x);
item_float_test!(test_source_compactness_score, "typography.source_compactness_score", source_compactness_score);
item_float_test!(test_plain_text_chars_per_line, "typography.plain_text_chars_per_line", plain_text_chars_per_line);
item_float_test!(test_formula_ratio, "typography.formula_ratio", formula_ratio);

#[test]
fn test_inner_bbox() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.inner_bbox").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: Vec<f64> = from_value(&case.expected);
        let actual = inner_bbox(&item.to_item());
        assert_close_f64_slice(&actual, &expected);
    }
}

#[test]
fn test_cover_bbox() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.cover_bbox").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: Vec<f64> = from_value(&case.expected);
        let actual = cover_bbox(&item.to_item());
        assert_close_f64_slice(&actual, &expected);
    }
}

#[test]
fn test_expanded_cover_bbox() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.expanded_cover_bbox").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let bbox: [f64; 4] = from_value(&case.input["bbox"]);
        let expected: Vec<f64> = from_value(&case.expected);
        let actual = expanded_cover_bbox(&item.to_item(), &bbox);
        assert_close_f64_slice(&actual.to_vec(), &expected);
    }
}

#[test]
fn test_line_widths() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.line_widths").iter().enumerate() {
        let item: ItemDto = from_value(&case.input["item"]);
        let expected: Vec<f64> = from_value(&case.expected);
        let actual = line_widths(&item.to_item());
        assert_close_f64_slice(&actual, &expected);
    }
}

#[test]
fn test_page_baseline_font_size() {
    let c = corpus();
    for (_i, case) in cases(c, "typography.page_baseline_font_size").iter().enumerate() {
        let items: Vec<ItemDto> = from_value(&case.input["items"]);
        let expected: [f64; 4] = from_value(&case.expected);
        let items: Vec<_> = items.iter().map(|d| d.to_item()).collect();
        let refs: Vec<&_> = items.iter().collect();
        let (size, pitch, height, density) = page_baseline_font_size(&refs);
        assert_close_f64(size, expected[0]);
        assert_close_f64(pitch, expected[1]);
        assert_close_f64(height, expected[2]);
        assert_close_f64(density, expected[3]);
    }
}
