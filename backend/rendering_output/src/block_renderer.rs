//! Port of `output/typst/block_renderer.py`: turn a single `RenderBlock` into
//! its Typst source fragment (`build_typst_block` and friends).

use crate::block_config as typst_config;
use crate::block_fit::fit_dimensions;
use crate::block_fields::{typst_block_fields, typst_rgb};
use crate::block_markup::{
    typst_markdown_block, typst_markdown_fit_call, typst_place_context, typst_plain_markdown_expr,
    typst_plain_text_expr, typst_preserved_lines_expr, typst_single_line_fit_call,
};
use crate::dto::RenderBlock;
use crate::escape::escape_typst_string;
use crate::formula_safety::{formula_safety_insets_pt, has_long_inline_math_layout_risk};
use crate::inline_passthrough::build_direct_typst_passthrough_text;
use crate::py_re;
use crate::util::{fmt_f, round_2dp};

pub const PLAIN_LINE_FIT_MAX_CHARS: usize = 40;
pub const TOC_ENTRY_FONT_PT: f64 = 9.6;

fn or_default(x: f64, default: f64) -> f64 {
    if x != 0.0 {
        x
    } else {
        default
    }
}

/// `sanitize_typst_markdown_for_compile`.
pub fn sanitize_typst_markdown_for_compile(markdown: &str) -> String {
    let mut text = markdown.to_string();
    text = py_re!(r"\$\s*\^\s*\{\s*\\(?:circled|textcircled)\s*R\s*\}\s*\$")
        .replace_all(&text, "®")
        .into_owned();
    text = py_re!(r"\$\s*\^\s*\{\s*\\(?:circled|textcircled)\s*\{\s*R\s*\}\s*\}\s*\$")
        .replace_all(&text, "®")
        .into_owned();
    text = py_re!(r"\$\s*\^\s*\{\s*\\(?:textregistered|registered)\s*\}\s*\$")
        .replace_all(&text, "®")
        .into_owned();
    text = py_re!(r"\$\s*\^\s*\{\s*®\s*\}\s*\$")
        .replace_all(&text, "®")
        .into_owned();
    text = text.replace("$^®$", "®").replace("$^{®}$", "®");
    text = text.replace(r"$^\circled{R}$", "®").replace(r"$^\textcircled{R}$", "®");
    text = py_re!(r"\\langlen\b").replace_all(&text, r"\langle n").into_owned();
    text = text.replace(r"\circled{\times}", r"\otimes");
    text = text.replace(r"\circled{\parallel}", r"\circ");
    text = text.replace(r"\textcircled{\times}", r"\otimes");
    text = text.replace(r"\textcircled{\parallel}", r"\circ");
    text
}

/// `_typst_string_array`.
fn typst_string_array(values: &[String]) -> String {
    let inner: Vec<String> = values
        .iter()
        .map(|v| format!("\"{}\"", escape_typst_string(v)))
        .collect();
    let trailing = if values.len() == 1 { "," } else { "" };
    format!("({}{})", inner.join(", "), trailing)
}

/// Python `str.splitlines()` — split on the Unicode line boundaries, dropping
/// the break characters, with no trailing empty line after a final break.
fn py_splitlines(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if matches!(
            chars[i],
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(chars[start..i].iter().collect());
            i += 1;
            if chars[i - 1] == '\r' && i < chars.len() && chars[i] == '\n' {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    if start < chars.len() {
        lines.push(chars[start..].iter().collect());
    }
    lines
}

fn resolved_font_weight(block: &RenderBlock) -> String {
    if block.font_weight.trim().is_empty() {
        "regular".to_string()
    } else {
        block.font_weight.clone()
    }
}

/// `_build_preserved_line_box_typst`.
fn build_preserved_line_box_typst(block_id: &str, block: &RenderBlock, text_fill: &str, block_fill: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let font_weight = resolved_font_weight(block);
    if block.use_cover_fill {
        let cover_name = format!("{}_cover", block_id.replace('-', "_"));
        let cover_x0 = block.cover_bbox[0];
        let cover_y0 = block.cover_bbox[1];
        let cover_x1 = block.cover_bbox[2];
        let cover_y1 = block.cover_bbox[3];
        let cover_width = (cover_x1 - cover_x0).max(typst_config::MIN_BLOCK_SIZE_PT);
        let cover_height = (cover_y1 - cover_y0).max(typst_config::MIN_BLOCK_SIZE_PT);
        parts.push(format!(
            "#let {cover_name} = rect(width: {}pt, height: {}pt, fill: {})",
            fmt_f(cover_width),
            fmt_f(cover_height),
            typst_rgb(block.cover_fill),
        ));
        parts.push(typst_place_context(cover_x0, cover_y0, &cover_name).trim_end().to_string());
    }
    for (index, line) in block.preserved_line_boxes.iter().enumerate() {
        if line.bbox.len() != 4 || line.text.trim().is_empty() {
            continue;
        }
        let line_markdown = build_direct_typst_passthrough_text(&line.text);
        let x0 = line.bbox[0];
        let y0 = line.bbox[1];
        let x1 = line.bbox[2];
        let y1 = line.bbox[3];
        let width = (x1 - x0).max(typst_config::MIN_BLOCK_SIZE_PT);
        let height = (y1 - y0).max(typst_config::MIN_BLOCK_SIZE_PT);
        let max_font_pt = round_2dp(1.0_f64.max(block.font_size_pt.min(height * 0.86)));
        let text_units = 1usize.max(line_markdown.trim().chars().count());
        let dense_single_line = text_units as f64 / width.max(1.0) > 0.38;
        let min_scale: f64 = if dense_single_line { 0.36 } else { 0.58 };
        let min_floor: f64 = if dense_single_line { 4.8 } else { 1.0 };
        let min_font_pt = round_2dp(min_floor.max(max_font_pt.min(height * min_scale)));
        let var = block_id.replace('-', "_");
        let line_name = format!("{var}_line_{index}_md");
        let body_name = format!("{var}_line_{index}_body");
        parts.push(format!("#let {line_name} = \"{}\"", escape_typst_string(&line_markdown)));
        parts.push(format!(
            "#let {body_name} = block(width: {}pt, height: {}pt{block_fill})[#{{ set text(fill: {text_fill}); pdftr_fit_single_line_markdown({line_name}, max_size: {}pt, min_size: {}pt, fit_width: {}pt, fit_height: {}pt, weight: \"{font_weight}\", justify: false) }}]",
            fmt_f(width),
            fmt_f(height),
            fmt_f(max_font_pt),
            fmt_f(min_font_pt),
            fmt_f(width),
            fmt_f(height),
        ));
        parts.push(typst_place_context(x0, y0, &body_name).trim_end().to_string());
    }
    let joined = parts.join("\n");
    if parts.is_empty() {
        String::new()
    } else {
        format!("{joined}\n")
    }
}

/// `_build_toc_entry_typst`.
fn build_toc_entry_typst(block_id: &str, block: &RenderBlock, text_fill: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let font_weight = resolved_font_weight(block);
    for (index, entry) in block.toc_entries.iter().enumerate() {
        if entry.bbox.len() != 4 || entry.title.trim().is_empty() {
            continue;
        }
        let x0 = entry.bbox[0];
        let y0 = entry.bbox[1];
        let x1 = entry.bbox[2];
        let y1 = entry.bbox[3];
        let width = (x1 - x0).max(typst_config::MIN_BLOCK_SIZE_PT);
        let height = (y1 - y0).max(typst_config::MIN_BLOCK_SIZE_PT);
        let level = if entry.level != 0 { entry.level } else { 1 };
        let indent = round_2dp((0_i64.max(level - 1) as f64) * (18.0_f64.min(width * 0.06)));
        let max_font_pt = round_2dp(1.0_f64.max(TOC_ENTRY_FONT_PT.min(height * 0.82)));
        let prefix = if entry.number.trim().is_empty() {
            String::new()
        } else {
            format!("{} ", entry.number)
        };
        let line_width = round_2dp(8.0_f64.max(width - indent));
        let prefix_title = build_direct_typst_passthrough_text(&format!("{prefix}{}", entry.title));
        let page_label = entry.page_label.trim().to_string();
        let var = block_id.replace('-', "_");
        let title_name = format!("{var}_toc_{index}_title");
        let page_name = format!("{var}_toc_{index}_page");
        let body_name = format!("{var}_toc_{index}_body");
        let title_y = round_2dp(0.0_f64.max(height * 0.08));
        let leader_y = round_2dp(height * 0.55);
        parts.push(format!("#let {title_name} = \"{}\"", escape_typst_string(&prefix_title)));
        parts.push(format!("#let {page_name} = \"{}\"", escape_typst_string(&page_label)));
        parts.push(format!(
            "#let {body_name} = block(width: {}pt, height: {}pt)[#{{ set text(size: {}pt, weight: \"{font_weight}\", fill: {text_fill}); set par(leading: 0.15em, justify: false); layout(size => {{ let page-body = box[#{{ {page_name} }}]; let page-size = measure(page-body); let title-body = box[#{{ cmarker.render({title_name}, math: mitex) }}]; let title-size = measure(title-body); let title-max = calc.max(8pt, size.width - page-size.width - 8pt); let title-width = calc.min(title-size.width, title-max); let leader-start = title-width + 2pt; let leader-end = size.width - page-size.width - 4pt; let leader-len = calc.max(0pt, leader-end - leader-start); place(top + left, dx: 0pt, dy: {}pt, box(width: title-width, clip: false)[#{{ title-body }}]); if leader-len > 2pt {{ place(top + left, dx: leader-start, dy: {}pt, line(length: leader-len, stroke: (paint: rgb(120, 120, 120), thickness: 0.45pt, dash: (1pt, 2pt)))) }}; place(top + left, dx: size.width - page-size.width, dy: {}pt, page-body) }}) }}]",
            fmt_f(line_width),
            fmt_f(height),
            fmt_f(max_font_pt),
            fmt_f(title_y),
            fmt_f(leader_y),
            fmt_f(title_y),
        ));
        parts.push(typst_place_context(x0 + indent, y0, &body_name).trim_end().to_string());
    }
    let joined = parts.join("\n");
    if parts.is_empty() {
        String::new()
    } else {
        format!("{joined}\n")
    }
}

/// `build_typst_block`.
pub fn build_typst_block(block_id: &str, block: &RenderBlock, include_fill: bool) -> String {
    let fields = typst_block_fields(
        block_id,
        &block.inner_bbox,
        block.font_size_pt,
        block.leading_em,
        Some(&block.font_weight),
    );
    let text_fill = typst_rgb(block.text_color);
    let block_fill = typst_config::cover_fill_arg(
        include_fill,
        block.use_cover_fill,
        &typst_rgb(block.cover_fill),
    );

    if matches!(block.render_kind.as_str(), "plain" | "plain_line") {
        let plain_text = &block.plain_text;
        if plain_text.chars().count() > PLAIN_LINE_FIT_MAX_CHARS {
            let text_name = format!("{}_txt", fields.var_prefix);
            let body_name = format!("{}_body", fields.var_prefix);
            let body_expr = typst_plain_text_expr(
                &text_name,
                fields.font_size,
                fields.leading,
                Some(&fields.font_weight),
                Some(&text_fill),
                typst_config::first_line_indent_pt(block.first_line_indent_pt),
                typst_config::typst_bool(block.justify_text),
            );
            let parts = [
                format!("#let {text_name} = \"{}\"", escape_typst_string(plain_text)),
                typst_markdown_block(
                    &body_name,
                    fields.width,
                    fields.height,
                    &block_fill,
                    &body_expr,
                    0.0,
                    0.0,
                ),
                typst_place_context(fields.x0, fields.y0, &body_name),
            ];
            return format!("{}\n", parts.join("\n"));
        }
        let text_name = format!("{}_txt", fields.var_prefix);
        let base_name = format!("{}_base", fields.var_prefix);
        let scaled_name = format!("{}_scaled", fields.var_prefix);
        let parts = [
            format!("#let {text_name} = \"{}\"", escape_typst_string(plain_text)),
            format!(
                "#let {base_name} = box[#{{ set text(size: {}pt, weight: \"{}\", fill: {text_fill}); {text_name} }}]",
                fmt_f(fields.font_size),
                fields.font_weight,
            ),
            "#context {".to_string(),
            format!("  let base-size = measure({base_name})"),
            format!(
                "  let scaled-font = if base-size.width > {}pt {{ {}pt * ({}pt / base-size.width) }} else {{ {}pt }}",
                fmt_f(fields.width),
                fmt_f(fields.font_size),
                fmt_f(fields.width),
                fmt_f(fields.font_size),
            ),
            format!(
                "  let {scaled_name} = block(width: {}pt, height: {}pt{block_fill})[#{{ set text(size: scaled-font, weight: \"{}\", fill: {text_fill}); {text_name} }}]",
                fmt_f(fields.width),
                fmt_f(fields.height),
                fields.font_weight,
            ),
            format!("  place(top + left, dx: {}pt, dy: {}pt, {scaled_name})", fmt_f(fields.x0), fmt_f(fields.y0)),
            "}".to_string(),
        ];
        return format!("{}\n", parts.join("\n"));
    }

    let markdown_name = format!("{}_md", fields.var_prefix);
    let body_name = format!("{}_body", fields.var_prefix);
    let markdown = sanitize_typst_markdown_for_compile(&block.markdown_text);
    let formula_insets = formula_safety_insets_pt(&markdown, &block.math_map, fields.font_size, fields.height);
    let long_inline_math_risk =
        has_long_inline_math_layout_risk(&markdown, &block.math_map, fields.font_size, fields.width);
    let content_fit_height = (fields.height - formula_insets.total_pt()).max(typst_config::MIN_BLOCK_SIZE_PT);
    let first_line_indent = typst_config::first_line_indent_pt(block.first_line_indent_pt);
    let justify_text = typst_config::typst_bool(block.justify_text && !long_inline_math_risk);

    if !block.toc_entries.is_empty() {
        return build_toc_entry_typst(block_id, block, &text_fill);
    }
    if block.preserve_line_breaks && !block.preserved_line_boxes.is_empty() {
        return build_preserved_line_box_typst(block_id, block, &text_fill, &block_fill);
    }
    if block.preserve_line_breaks && markdown.contains('\n') {
        let lines_name = format!("{}_lines", fields.var_prefix);
        let line_values: Vec<String> = py_splitlines(&markdown)
            .into_iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        let body_expr = typst_preserved_lines_expr(
            &lines_name,
            fields.font_size,
            fields.leading,
            Some(&fields.font_weight),
            Some(&text_fill),
            "false",
        );
        let parts = [
            format!("#let {lines_name} = {}", typst_string_array(&line_values)),
            typst_markdown_block(
                &body_name,
                fields.width,
                fields.height,
                &block_fill,
                &body_expr,
                formula_insets.top_pt,
                formula_insets.bottom_pt,
            ),
            typst_place_context(fields.x0, fields.y0, &body_name),
        ];
        return format!("{}\n", parts.join("\n"));
    }
    if block.fit_to_box {
        if block.fit_single_line {
            let single_line_fit = typst_config::single_line_fit_config(
                fields.width,
                content_fit_height,
                fields.font_size,
                block.fit_min_font_size_pt,
                block.fit_max_font_size_pt,
                content_fit_height.min(or_default(block.fit_max_height_pt, content_fit_height)),
                block.fit_target_width_pt,
                content_fit_height.min(or_default(block.fit_target_height_pt, content_fit_height)),
                block.fit_shift_up_pt,
            );
            let fit_call = typst_single_line_fit_call(
                &markdown_name,
                &single_line_fit,
                &fields.font_weight,
                justify_text,
            );
            let parts = [
                format!("#let {markdown_name} = \"{}\"", escape_typst_string(&markdown)),
                typst_markdown_block(
                    &body_name,
                    single_line_fit.width_pt,
                    fields.height,
                    &block_fill,
                    &format!("set text(fill: {text_fill}); {fit_call}"),
                    formula_insets.top_pt,
                    formula_insets.bottom_pt,
                ),
                typst_place_context(fields.x0, fields.y0 - single_line_fit.shift_up_pt, &body_name),
            ];
            return format!("{}\n", parts.join("\n"));
        }
        let fit_min_font = if long_inline_math_risk {
            or_default(block.fit_min_font_size_pt, fields.font_size).min(fields.font_size * 0.72)
        } else {
            block.fit_min_font_size_pt
        };
        let fit_min_leading = if long_inline_math_risk {
            or_default(block.fit_min_leading_em, fields.leading).max(fields.leading.min(0.62))
        } else {
            block.fit_min_leading_em
        };
        let fit = fit_dimensions(
            fields.width,
            content_fit_height,
            fields.font_size,
            fields.leading,
            fit_min_font,
            fit_min_leading,
            content_fit_height.min(or_default(block.fit_max_height_pt, content_fit_height)),
        );
        let fit_call = typst_markdown_fit_call(
            &markdown_name,
            fields.font_size,
            fit.fit_min_font,
            fields.leading,
            fit.fit_min_leading,
            fit.fit_target_height,
            &fields.font_weight,
            first_line_indent,
            justify_text,
        );
        let parts = [
            format!("#let {markdown_name} = \"{}\"", escape_typst_string(&markdown)),
            typst_markdown_block(
                &body_name,
                fit.width,
                fields.height,
                &block_fill,
                &format!("set text(fill: {text_fill}); {fit_call}"),
                formula_insets.top_pt,
                formula_insets.bottom_pt,
            ),
            typst_place_context(fields.x0, fields.y0, &body_name),
        ];
        return format!("{}\n", parts.join("\n"));
    }
    let body_expr = typst_plain_markdown_expr(
        &markdown_name,
        fields.font_size,
        fields.leading,
        Some(&fields.font_weight),
        Some(&text_fill),
        first_line_indent,
        justify_text,
    );
    let parts = [
        format!("#let {markdown_name} = \"{}\"", escape_typst_string(&markdown)),
        typst_markdown_block(
            &body_name,
            fields.width,
            fields.height,
            &block_fill,
            &body_expr,
            formula_insets.top_pt,
            formula_insets.bottom_pt,
        ),
        typst_place_context(fields.x0, fields.y0, &body_name),
    ];
    format!("{}\n", parts.join("\n"))
}

/// `build_typst_cover_rect`.
pub fn build_typst_cover_rect(block_id: &str, block: &RenderBlock) -> String {
    let rect_name = format!("{}_cover", block_id.replace('-', "_"));
    let x0 = block.cover_bbox[0];
    let y0 = block.cover_bbox[1];
    let x1 = block.cover_bbox[2];
    let y1 = block.cover_bbox[3];
    let width = (x1 - x0).max(typst_config::MIN_BLOCK_SIZE_PT);
    let height = (y1 - y0).max(typst_config::MIN_BLOCK_SIZE_PT);
    let cover_fill = typst_rgb(block.cover_fill);
    let parts = [
        format!("#let {rect_name} = rect(width: {}pt, height: {}pt, fill: {cover_fill})", fmt_f(width), fmt_f(height)),
        typst_place_context(x0, y0, &rect_name),
    ];
    format!("{}\n", parts.join("\n"))
}
