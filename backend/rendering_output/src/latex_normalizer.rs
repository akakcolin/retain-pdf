//! Port of `inline_content/fallback/latex_normalizer.py::normalize_formula_for_latex_math`.
//! Used by `sanitize_direct_typst_inline_math` for display-math tokens.

use crate::py_re;

fn _strip_trailing_formula_punctuation(expr: &str) -> String {
    py_re!(r"\s*([,.;:])\s*$").replace_all(expr, "").into_owned()
}

fn _collapse_mathrm_letters(inner: &str) -> String {
    let collapsed: String = inner.chars().filter(|c| !is_ws(*c)).collect();
    format!(r"\mathrm{{{collapsed}}}")
}

fn _compact_mathrm_payload(expr: &str) -> String {
    let re1 = py_re!(r"\\mathrm\s*\{\s*\{\s*([A-Za-z0-9](?:\s+[A-Za-z0-9])+)\s*\}\s*\}");
    let expr = re1
        .replace_all(expr, |caps: &fancy_regex::Captures<'_, str>| {
            _collapse_mathrm_letters(caps.get(1).unwrap().as_str())
        })
        .into_owned();
    let re2 = py_re!(r"\\mathrm\s*\{\s*([A-Za-z0-9](?:\s+[A-Za-z0-9])+)\s*\}");
    re2.replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
        _collapse_mathrm_letters(caps.get(1).unwrap().as_str())
    })
    .into_owned()
}

fn _compact_superscript_mathrm(m1: &str, superscript: &str) -> String {
    let superscript: String = superscript.chars().filter(|c| !is_ws(*c)).collect();
    format!(r"\mathrm{{{m1}^{{{superscript}}}}}")
}

fn _repair_common_ocr_formula_noise(expr: &str) -> String {
    let expr = py_re!(r"\bC\s*0\s*0\s*H(?=\s*\^\s*\{\s*\*\s*\})")
        .replace_all(expr, "COOH")
        .into_owned();
    let expr = py_re!(r"\\mathrm\s*\{\s*([A-Za-z0-9]+)\s*\^\s*\{\s*([^{}]+?)\s*\}\s*\}")
        .replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
            _compact_superscript_mathrm(caps.get(1).unwrap().as_str(), caps.get(2).unwrap().as_str())
        })
        .into_owned();
    let expr = py_re!(r"(?<=\d)\s*\.\s*(?=\d)").replace_all(&expr, ".").into_owned();
    let expr = py_re!(r"(?<=\d)\s+(?=\d)").replace_all(&expr, "").into_owned();
    let expr = py_re!(r"~\s*\\mathrm\s*\{\s*e\s*V\s*\}\s*\.?")
        .replace_all(&expr, r" \mathrm{eV}")
        .into_owned();
    let expr = py_re!(r"\\vec\s*\{\s*([A-Za-z])\s*\}")
        .replace_all(&expr, "$1")
        .into_owned();
    let expr = py_re!(r"\\bf\s*\{\s*([A-Za-z0-9\-+*/]+)\s*\}")
        .replace_all(&expr, "$1")
        .into_owned();
    let expr = py_re!(r"\{\s*\\bf\s+([^{}]+?)\s*\}")
        .replace_all(&expr, "{$1}")
        .into_owned();
    let expr = py_re!(r"^\{\s*([A-Za-z])\s*\}(?=\s+\{)")
        .replace_all(&expr, "$1")
        .into_owned();
    _strip_trailing_formula_punctuation(&expr)
}

fn _unwrap_style_wrappers(expr: &str) -> String {
    let group_prefixed = py_re!(r"\{\s*\\(?:pmb|bf|rm|it|sf|tt|em)\s+([^{}]+?)\s*\}");
    let direct_group = py_re!(r"\\(?:pmb|bf|rm|it|sf|tt|em)\s*\{\s*([^{}]+?)\s*\}");
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        expr = group_prefixed
            .replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
                format!("{{{}}}", caps.get(1).unwrap().as_str().trim())
            })
            .into_owned();
        expr = direct_group
            .replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
                format!("{{{}}}", caps.get(1).unwrap().as_str().trim())
            })
            .into_owned();
        if expr == prev {
            break;
        }
    }
    expr
}

fn _find_balanced_group(text: &str, start: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if start >= chars.len() || chars[start] != '{' {
        return None;
    }
    let mut depth = 0usize;
    for idx in start..chars.len() {
        if chars[idx] == '{' {
            depth += 1;
        } else if chars[idx] == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((chars[start + 1..idx].iter().collect(), idx + 1));
            }
        }
    }
    None
}

fn _unwrap_named_macros(expr: &str, macro_names: &[&str]) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            let macro_name: String = chars[i + 1..j].iter().collect();
            if macro_names.contains(&macro_name.as_str()) {
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if k < chars.len() && chars[k] == '{' {
                    if let Some((inner, end)) = _find_balanced_group(expr, k) {
                        let normalized = normalize_formula_for_latex_math(&inner);
                        out.extend(normalized.chars());
                        i = end;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.into_iter().collect()
}

fn _unwrap_legacy_layout_wrappers(expr: &str) -> String {
    _unwrap_named_macros(
        expr,
        &[
            "smash", "mbox", "hbox", "vbox", "fbox", "textnormal", "textrm", "textsf", "texttt",
        ],
    )
}

fn _unwrap_inline_text_wrappers(expr: &str) -> String {
    _unwrap_named_macros(expr, &["textbf", "textit", "emph", "em"])
}

fn _compact_script_groups(expr: &str) -> String {
    let re_sub = py_re!(r"_\s*\{\s*([^{}]+?)\s*\}");
    let re_sup = py_re!(r"\^\s*\{\s*([^{}]+?)\s*\}");
    let re_sub_tight = py_re!(r"(?<=[A-Za-z0-9\}\)\]])\s*_\s*([A-Za-z0-9*+\-]+)");
    let re_sup_tight = py_re!(r"(?<=[A-Za-z0-9\}\)\]])\s*\^\s*([A-Za-z0-9*+\-]+)");
    let re_sup_cmd = py_re!(r"(?<=[A-Za-z0-9\}\)\]])\s*\^\s*(\\[A-Za-z]+)");
    let re_group_sub = py_re!(r"(\\[A-Za-z]+|[A-Za-z0-9\}\)\]])\s+(_\{[^{}]*\})");
    let re_group_sup = py_re!(r"(\\[A-Za-z]+|[A-Za-z0-9\}\)\]])\s+(\^\{[^{}]*\})");
    let re_sup_star = py_re!(r"\^\s+\*");
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        expr = re_sub
            .replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
                format!("_{{{}}}", _compact_script_groups(caps.get(1).unwrap().as_str().trim()))
            })
            .into_owned();
        expr = re_sup
            .replace_all(&expr, |caps: &fancy_regex::Captures<'_, str>| {
                format!("^{{{}}}", _compact_script_groups(caps.get(1).unwrap().as_str().trim()))
            })
            .into_owned();
        expr = re_sub_tight.replace_all(&expr, "_$1").into_owned();
        expr = re_sup_tight.replace_all(&expr, "^$1").into_owned();
        expr = re_sup_cmd.replace_all(&expr, "^$1").into_owned();
        expr = re_group_sub.replace_all(&expr, "$1$2").into_owned();
        expr = re_group_sup.replace_all(&expr, "$1$2").into_owned();
        expr = re_sup_star.replace_all(&expr, "*").into_owned();
        if expr == prev {
            break;
        }
    }
    expr
}

fn _compact_letter_hyphen_runs(expr: &str) -> String {
    let re = py_re!(
        r"(?P<left>(?:\\[A-Za-z]+|[A-Za-z])[A-Za-z0-9]*)\s*-\s*(?P<right>(?:\\[A-Za-z]+|[A-Za-z])[A-Za-z0-9]*)"
    );
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        expr = re.replace_all(&expr, "${left}-${right}").into_owned();
        if expr == prev {
            break;
        }
    }
    expr
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b')
}

/// Port of `normalize_formula_for_latex_math`.
pub fn normalize_formula_for_latex_math(formula_text: &str) -> String {
    let mut expr = formula_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if expr.is_empty() {
        return expr;
    }
    if expr == "^®" || expr == "^{®}" || expr == r"^\circled{R}" || expr == r"^\textcircled{R}" {
        return r"\text{®}".to_string();
    }

    expr = py_re!(r"\\begin\{array\}\s*\{[^{}]*\}\s*").replace_all(&expr, "").into_owned();
    expr = py_re!(r"\s*\\end\{array\}").replace_all(&expr, "").into_owned();
    expr = py_re!(r"\\cal\s+([A-Za-z])")
        .replace_all(&expr, r"\mathcal{$1}")
        .into_owned();
    expr = py_re!(r"\\mathscr\b").replace_all(&expr, r"\mathcal").into_owned();
    expr = py_re!(r"\\rrangle\b").replace_all(&expr, r"\rangle").into_owned();
    expr = py_re!(r"\\llangle\b").replace_all(&expr, r"\langle").into_owned();
    expr = py_re!(r"\\langlen\b").replace_all(&expr, r"\langle n").into_owned();
    expr = py_re!(r"\\Breve\b").replace_all(&expr, r"\breve").into_owned();
    expr = py_re!(r"\\Vec\b").replace_all(&expr, r"\vec").into_owned();
    expr = py_re!(r"\\textsuperscript\s*\{\s*([^{}]+?)\s*\}")
        .replace_all(&expr, "^{$1}")
        .into_owned();
    expr = py_re!(r"\\circled\s*\{\s*\\times\s*\}")
        .replace_all(&expr, r"\otimes")
        .into_owned();
    expr = py_re!(r"\\circled\s*\{\s*\\parallel\s*\}")
        .replace_all(&expr, r"\circ")
        .into_owned();
    expr = py_re!(r"\\circled\s*\{\s*([^{}]+?)\s*\}")
        .replace_all(&expr, "$1")
        .into_owned();
    expr = py_re!(r"\\textcircled\s*\{\s*\\times\s*\}")
        .replace_all(&expr, r"\otimes")
        .into_owned();
    expr = py_re!(
        r"\\textcircled\s*\{\s*\\scriptsize\s*\{\s*\\parallel\s*\}\s*\}"
    )
    .replace_all(&expr, r"\circ")
    .into_owned();
    expr = py_re!(r"\\textcircled\s*\{\s*\\parallel\s*\}")
        .replace_all(&expr, r"\circ")
        .into_owned();
    expr = py_re!(r"\\textcircled\s*\{\s*([^{}]+?)\s*\}")
        .replace_all(&expr, "$1")
        .into_owned();
    expr = py_re!(r"\\(?:scriptstyle|scriptscriptstyle|textstyle|displaystyle)\b")
        .replace_all(&expr, "")
        .into_owned();

    expr = _compact_mathrm_payload(&expr);

    expr = py_re!(r"(?<=\d)\s*\\dot\b(?=\s*$)").replace_all(&expr, ".").into_owned();
    expr = py_re!(r"(?<=\d)\s*\\dot\b(?=\s*[\)\],;])")
        .replace_all(&expr, ".")
        .into_owned();

    expr = _unwrap_style_wrappers(&expr);
    expr = _unwrap_legacy_layout_wrappers(&expr);
    expr = _unwrap_inline_text_wrappers(&expr);
    expr = py_re!(r"\{\s*\\(?:bf|rm|it|tt|sf|pmb)\s*\}")
        .replace_all(&expr, "")
        .into_owned();
    expr = py_re!(r"^\{\s*([^{}]+?)\s*\}$").replace_all(&expr, "$1").into_owned();

    expr = _repair_common_ocr_formula_noise(&expr);
    expr = _compact_script_groups(&expr);
    expr = _compact_letter_hyphen_runs(&expr);
    expr = py_re!(r"\.\s+([\)\],;])").replace_all(&expr, ".$1").into_owned();
    expr = py_re!(r"(?<=\d)\s*\.\s*(?=\d)").replace_all(&expr, ".").into_owned();
    expr = py_re!(r"(?<=\d)\s+(?=\d)").replace_all(&expr, "").into_owned();
    expr = py_re!(r"\s*([=+*/<>:,;])\s*").replace_all(&expr, " $1 ").into_owned();
    expr = py_re!(
        r"(?P<left>(?:\d+|[)\]}]|\\[A-Za-z]+))\s*-\s*(?P<right>(?:\d+|[(\[{]|\\[A-Za-z]+))"
    )
    .replace_all(&expr, "${left} - ${right}")
    .into_owned();
    expr = _compact_script_groups(&expr);
    expr = py_re!(r"(?<=\^)\s+([*+\-])").replace_all(&expr, "$1").into_owned();
    expr = py_re!(r"\^([*+\-])\s+(?=[}\)])").replace_all(&expr, "^$1").into_owned();
    expr = py_re!(r"\s+").replace_all(&expr, " ").into_owned();
    let expr = expr.trim().to_string();
    let expr = py_re!(r"\\mathrm\s*\{\s*COOH\s*\^\s*\{\s*\*\s*\}\s*\}")
        .replace_all(&expr, r"\mathrm{COOH^{*}}")
        .into_owned();
    let expr = py_re!(r"^([_^])\{([^{}]+)\}$").replace_all(&expr, "$1{{$2}}").into_owned();
    if expr.starts_with('_') || expr.starts_with('^') {
        return format!("{{}} {expr}");
    }
    expr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace() {
        assert_eq!(
            normalize_formula_for_latex_math("  a +   b "),
            "a + b"
        );
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(normalize_formula_for_latex_math("  "), "");
    }

    #[test]
    fn registered_symbol_shortcut() {
        assert_eq!(normalize_formula_for_latex_math(r"^\circled{R}"), r"\text{®}");
    }

    #[test]
    fn script_groups_compact() {
        assert_eq!(normalize_formula_for_latex_math(r"x^{ 2 }"), "x^{2}");
    }

    #[test]
    fn prefix_script_gets_prepended() {
        assert_eq!(normalize_formula_for_latex_math(r"^{N}"), "{} ^{{N}}");
    }
}
