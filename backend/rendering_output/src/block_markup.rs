//! Port of `output/typst/block_markup.py`: the Typst expression builders.

use crate::block_config::SingleLineFitConfig;
use crate::util::fmt_f;

/// `typst_block_fill_arg`: the fill string, unchanged (kept for parity).
pub fn typst_block_fill_arg(fill: &str) -> String {
    fill.to_string()
}

/// `typst_single_line_fit_call`.
pub fn typst_single_line_fit_call(
    markdown_name: &str,
    config: &SingleLineFitConfig,
    font_weight: &str,
    justify_text: &str,
) -> String {
    format!(
        "pdftr_fit_single_line_markdown({markdown_name}, max_size: {}pt, min_size: {}pt, fit_width: {}pt, fit_height: {}pt, weight: \"{font_weight}\", justify: {justify_text})",
        fmt_f(config.max_font_pt),
        fmt_f(config.min_font_pt),
        fmt_f(config.width_pt),
        fmt_f(config.height_pt),
    )
}

/// `typst_markdown_fit_call`.
pub fn typst_markdown_fit_call(
    markdown_name: &str,
    max_font_size_pt: f64,
    min_font_size_pt: f64,
    max_leading_em: f64,
    min_leading_em: f64,
    fit_height_pt: f64,
    font_weight: &str,
    first_line_indent_pt: f64,
    justify_text: &str,
) -> String {
    format!(
        "pdftr_fit_markdown({markdown_name}, max_size: {}pt, min_size: {}pt, max_leading: {}em, min_leading: {}em, fit_height: {}pt, weight: \"{font_weight}\", first_line_indent: {}pt, justify: {justify_text})",
        fmt_f(max_font_size_pt),
        fmt_f(min_font_size_pt),
        fmt_f(max_leading_em),
        fmt_f(min_leading_em),
        fmt_f(fit_height_pt),
        fmt_f(first_line_indent_pt),
    )
}

/// `typst_markdown_block`.
pub fn typst_markdown_block(
    body_name: &str,
    width_pt: f64,
    height_pt: f64,
    block_fill: &str,
    body_expr: &str,
    content_top_inset_pt: f64,
    content_bottom_inset_pt: f64,
) -> String {
    let body_expr = if content_top_inset_pt > 0.0 || content_bottom_inset_pt > 0.0 {
        format!(
            "pad(top: {}pt, bottom: {}pt)[#{{ {body_expr} }}]",
            fmt_f(content_top_inset_pt.max(0.0)),
            fmt_f(content_bottom_inset_pt.max(0.0)),
        )
    } else {
        body_expr.to_string()
    };
    format!(
        "#let {body_name} = block(width: {}pt, height: {}pt{block_fill})[#{{ {body_expr} }}]\n",
        fmt_f(width_pt),
        fmt_f(height_pt),
    )
}

/// `typst_plain_markdown_expr`.
pub fn typst_plain_markdown_expr(
    markdown_name: &str,
    font_size_pt: f64,
    leading_em: f64,
    font_weight: Option<&str>,
    text_fill: Option<&str>,
    first_line_indent_pt: f64,
    justify_text: &str,
) -> String {
    let text_args = text_args(font_size_pt, font_weight, text_fill);
    format!(
        "set text({text_args}); set par(leading: {}em, justify: {justify_text}); if {}pt > 0pt {{ h({}pt) }}; cmarker.render({markdown_name}, math: mitex)",
        fmt_f(leading_em),
        fmt_f(first_line_indent_pt),
        fmt_f(first_line_indent_pt),
    )
}

/// `typst_plain_text_expr`.
pub fn typst_plain_text_expr(
    text_name: &str,
    font_size_pt: f64,
    leading_em: f64,
    font_weight: Option<&str>,
    text_fill: Option<&str>,
    first_line_indent_pt: f64,
    justify_text: &str,
) -> String {
    let text_args = text_args(font_size_pt, font_weight, text_fill);
    format!(
        "set text({text_args}); set par(leading: {}em, justify: {justify_text}); if {}pt > 0pt {{ h({}pt) }}; {text_name}",
        fmt_f(leading_em),
        fmt_f(first_line_indent_pt),
        fmt_f(first_line_indent_pt),
    )
}

/// `typst_preserved_lines_expr`.
pub fn typst_preserved_lines_expr(
    lines_name: &str,
    font_size_pt: f64,
    leading_em: f64,
    font_weight: Option<&str>,
    text_fill: Option<&str>,
    justify_text: &str,
) -> String {
    let text_args = text_args(font_size_pt, font_weight, text_fill);
    let gap_em = leading_em.max(0.0);
    format!(
        "set text({text_args}); set par(leading: {}em, justify: {justify_text}); stack(dir: ttb, spacing: {}em, ..{lines_name}.map(line => block(line)))",
        fmt_f(leading_em),
        fmt_f(gap_em),
    )
}

/// `typst_place_context`.
pub fn typst_place_context(x_pt: f64, y_pt: f64, body_name: &str) -> String {
    format!(
        "#context {{\n  place(top + left, dx: {}pt, dy: {}pt, {body_name})\n}}\n",
        fmt_f(x_pt),
        fmt_f(y_pt),
    )
}

fn text_args(font_size_pt: f64, font_weight: Option<&str>, text_fill: Option<&str>) -> String {
    let mut args = vec![format!("size: {}pt", fmt_f(font_size_pt))];
    if let Some(weight) = font_weight {
        args.push(format!("weight: \"{weight}\""));
    }
    if let Some(fill) = text_fill {
        args.push(format!("fill: {fill}"));
    }
    args.join(", ")
}
