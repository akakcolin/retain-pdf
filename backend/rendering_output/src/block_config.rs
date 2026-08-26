//! Port of `output/typst/block_config.py`.

pub const MIN_BLOCK_SIZE_PT: f64 = 8.0;
pub const DEFAULT_FONT_SIZE_PT: f64 = 11.4;
pub const TYPST_DEFAULT_FONT_FAMILY: &str = "Source Han Serif SC";
pub const MIN_FIT_FONT_SIZE_PT: f64 = 1.0;
pub const MIN_FIT_LEADING_EM: f64 = 0.1;
pub const MIN_FIRST_LINE_INDENT_PT: f64 = 0.0;
pub const NO_TARGET_SIZE_PT: f64 = 0.0;
pub const DEFAULT_COVER_FILL: [f64; 3] = [1.0, 1.0, 1.0];
pub const CMARKER_PACKAGE: &str = "cmarker";
pub const CMARKER_VERSION: &str = "0.1.8";
pub const MITEX_PACKAGE: &str = "mitex";
pub const MITEX_VERSION: &str = "0.2.6";
pub const FIT_SIZE_HELPER: &str = "pdftr_fit_size";
pub const FIT_LEADING_HELPER: &str = "pdftr_fit_leading";
pub const FIT_MARKDOWN_HELPER: &str = "pdftr_fit_markdown";
pub const FIT_SINGLE_LINE_MARKDOWN_HELPER: &str = "pdftr_fit_single_line_markdown";
pub const FIT_SIZE_EPS_PT: f64 = 0.08;
pub const FIT_LEADING_EPS_EM: f64 = 0.01;
pub const FIT_EMERGENCY_MIN_SIZE_PT: f64 = 4.2;
pub const FIT_EMERGENCY_MIN_SIZE_RATIO: f64 = 0.65;
pub const FIT_EMERGENCY_MIN_LEADING_EM: f64 = 0.20;
pub const FIT_EMERGENCY_MIN_LEADING_RATIO: f64 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SingleLineFitConfig {
    pub min_font_pt: f64,
    pub max_font_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub shift_up_pt: f64,
}

pub fn typst_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// `x or default` — Python `or` treats `0.0` as falsy.
fn or_default(x: f64, default: f64) -> f64 {
    if x != 0.0 {
        x
    } else {
        default
    }
}

pub fn typst_package_imports() -> Vec<String> {
    vec![
        format!("#import \"@preview/{CMARKER_PACKAGE}:{CMARKER_VERSION}\""),
        format!("#import \"@preview/{MITEX_PACKAGE}:{MITEX_VERSION}\": mitex"),
    ]
}

pub fn cover_fill_arg(include_fill: bool, use_cover_fill: bool, cover_fill: &str) -> String {
    if include_fill || use_cover_fill {
        format!(", fill: {cover_fill}")
    } else {
        String::new()
    }
}

pub fn first_line_indent_pt(value: f64) -> f64 {
    value.max(MIN_FIRST_LINE_INDENT_PT)
}

pub fn single_line_fit_config(
    width_pt: f64,
    height_pt: f64,
    font_size_pt: f64,
    fit_min_font_size_pt: f64,
    fit_max_font_size_pt: f64,
    fit_max_height_pt: f64,
    fit_target_width_pt: f64,
    fit_target_height_pt: f64,
    fit_shift_up_pt: f64,
) -> SingleLineFitConfig {
    SingleLineFitConfig {
        min_font_pt: MIN_FIT_FONT_SIZE_PT.max(or_default(fit_min_font_size_pt, font_size_pt).min(font_size_pt)),
        max_font_pt: font_size_pt.max(or_default(fit_max_font_size_pt, font_size_pt)),
        width_pt: width_pt.max(or_default(fit_target_width_pt, NO_TARGET_SIZE_PT)),
        height_pt: MIN_BLOCK_SIZE_PT.max(
            height_pt.min(or_default(fit_max_height_pt, height_pt))
                .max(or_default(fit_target_height_pt, NO_TARGET_SIZE_PT)),
        ),
        shift_up_pt: 0.0_f64.max(fit_shift_up_pt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_renders_typst_literal() {
        assert_eq!(typst_bool(true), "true");
        assert_eq!(typst_bool(false), "false");
    }

    #[test]
    fn package_imports_match_production() {
        assert_eq!(
            typst_package_imports(),
            vec![
                "#import \"@preview/cmarker:0.1.8\"".to_string(),
                "#import \"@preview/mitex:0.2.6\": mitex".to_string(),
            ]
        );
    }

    #[test]
    fn cover_fill_arg_omitted_without_fill() {
        assert_eq!(cover_fill_arg(false, false, "rgb(1, 1, 1)"), "");
        assert_eq!(cover_fill_arg(true, false, "rgb(1, 1, 1)"), ", fill: rgb(1, 1, 1)");
        assert_eq!(cover_fill_arg(false, true, "rgb(1, 1, 1)"), ", fill: rgb(1, 1, 1)");
    }

    #[test]
    fn single_line_fit_clamps_and_ors_zeros() {
        let cfg = single_line_fit_config(100.0, 50.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(cfg.min_font_pt, 12.0); // fit_min 0 -> font_size
        assert_eq!(cfg.max_font_pt, 12.0); // fit_max 0 -> font_size
        assert_eq!(cfg.width_pt, 100.0);
        assert_eq!(cfg.height_pt, 50.0);
        assert_eq!(cfg.shift_up_pt, 0.0);

        let cfg = single_line_fit_config(100.0, 5.0, 12.0, 20.0, 30.0, 40.0, 200.0, 0.0, -3.0);
        assert_eq!(cfg.min_font_pt, 12.0); // min(20, 12)
        assert_eq!(cfg.max_font_pt, 30.0);
        assert_eq!(cfg.width_pt, 200.0); // max(100, 200)
        assert_eq!(cfg.height_pt, 8.0); // max(8, max(min(5,40), 0)) -> max(8, 5)
        assert_eq!(cfg.shift_up_pt, 0.0); // max(0, -3)
    }
}
