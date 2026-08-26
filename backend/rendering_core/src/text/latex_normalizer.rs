// Port of services/rendering/layout/inline_content/fallback/latex_normalizer.py.
// Every `re.sub` is hand-rolled into a scan-and-replace pass. Python LRU caches
// are not ported.

const STRUCTURAL_LATEX_COMMANDS: [&str; 17] = [
    "begin", "end", "frac", "sqrt", "left", "right", "overline", "underline", "overset", "underset",
    "stackrel", "operatorname", "text", "textbf", "textit", "boxed", "binom",
];
const STYLE_WRAPPER_MACROS: [&str; 7] = ["pmb", "bf", "rm", "it", "sf", "tt", "em"];
const LEGACY_LAYOUT_WRAPPER_MACROS: [&str; 9] = [
    "smash", "mbox", "hbox", "vbox", "fbox", "textnormal", "textrm", "textsf", "texttt",
];
const INLINE_TEXT_WRAPPER_MACROS: [&str; 4] = ["textbf", "textit", "emph", "em"];

/// Advance `i` past whitespace (Unicode), returning a byte index.
fn skip_ws(s: &str, i: usize) -> usize {
    let mut i = i;
    while i < s.len() {
        let ch = s[i..].chars().next().unwrap();
        if ch.is_whitespace() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    i
}

/// Remove all whitespace (`\s+` → "").
fn collapse_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Read an ASCII alphabetic command name starting after the backslash at index i.
/// Returns (name, end_index_after_name).
fn read_command_name(s: &str, i: usize) -> (String, usize) {
    let bytes = s.as_bytes();
    let mut j = i;
    while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    (s[i..j].to_string(), j)
}

pub fn strip_trailing_formula_punctuation(expr: &str) -> String {
    let s = expr.trim_end();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return s.to_string();
    }
    if matches!(bytes[bytes.len() - 1], b',' | b'.' | b';' | b':') {
        let mut i = bytes.len() - 1;
        while i > 0 {
            let prev = s[..i].chars().next_back().unwrap();
            if prev.is_whitespace() {
                i -= prev.len_utf8();
            } else {
                break;
            }
        }
        return s[..i].to_string();
    }
    s.to_string()
}

fn compact_mathrm_payload(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if expr[i..].starts_with("\\mathrm") {
            let mut j = i + 7;
            j = skip_ws(expr, j);
            if j < bytes.len() && bytes[j] == b'{' {
                j += 1;
                j = skip_ws(expr, j);
                let nested = j < bytes.len() && bytes[j] == b'{';
                if nested {
                    j += 1;
                    j = skip_ws(expr, j);
                }
                let mut tokens: Vec<&str> = Vec::new();
                let mut p = j;
                let alnum_start = p;
                while p < bytes.len() && bytes[p].is_ascii_alphanumeric() {
                    p += 1;
                }
                if p > alnum_start {
                    tokens.push(&expr[alnum_start..p]);
                    loop {
                        let wstart = p;
                        while p < bytes.len() && expr[p..].chars().next().unwrap().is_whitespace() {
                            p += expr[p..].chars().next().unwrap().len_utf8();
                        }
                        if p == wstart {
                            break;
                        }
                        let astart = p;
                        while p < bytes.len() && bytes[p].is_ascii_alphanumeric() {
                            p += 1;
                        }
                        if p == astart {
                            break;
                        }
                        tokens.push(&expr[astart..p]);
                    }
                }
                if tokens.len() >= 2 {
                    let e = skip_ws(expr, p);
                    if e < bytes.len() && bytes[e] == b'}' {
                        let mut matched = false;
                        let mut e2 = 0usize;
                        if !nested {
                            e2 = e + 1;
                            matched = true;
                        } else {
                            let t = skip_ws(expr, e + 1);
                            if t < bytes.len() && bytes[t] == b'}' {
                                e2 = t + 1;
                                matched = true;
                            }
                        }
                        if matched {
                            out.push_str("\\mathrm{");
                            out.push_str(&tokens.concat());
                            out.push('}');
                            i = e2;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\bC\s*0\s*0\s*H(?=\s*\^\s*\{\s*\*\s*\})` → "COOH".
fn replace_c00h(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let boundary = i == 0 || {
            let prev = expr[..i].chars().next_back().unwrap();
            !(prev.is_ascii_alphanumeric() || prev == '_')
        };
        if boundary && bytes[i] == b'C' {
            let mut j = i + 1;
            j = skip_ws(expr, j);
            if j < bytes.len() && bytes[j] == b'0' {
                j = skip_ws(expr, j + 1);
                if j < bytes.len() && bytes[j] == b'0' {
                    j = skip_ws(expr, j + 1);
                    if j < bytes.len() && bytes[j] == b'H' {
                        let after_h = skip_ws(expr, j + 1);
                        let lookahead_ok = after_h < bytes.len()
                            && bytes[after_h] == b'^'
                            && {
                                let a = skip_ws(expr, after_h + 1);
                                a < bytes.len() && bytes[a] == b'{'
                            }
                            && {
                                let a = skip_ws(expr, {
                                    let b = skip_ws(expr, after_h + 1);
                                    if b < bytes.len() && bytes[b] == b'{' {
                                        b + 1
                                    } else {
                                        bytes.len()
                                    }
                                });
                                a < bytes.len() && bytes[a] == b'*'
                            }
                            && {
                                let a = skip_ws(expr, {
                                    let b = skip_ws(expr, {
                                        let c = skip_ws(expr, after_h + 1);
                                        if c < bytes.len() && bytes[c] == b'{' {
                                            c + 1
                                        } else {
                                            bytes.len()
                                        }
                                    });
                                    if b < bytes.len() && bytes[b] == b'*' {
                                        b + 1
                                    } else {
                                        bytes.len()
                                    }
                                });
                                a < bytes.len() && bytes[a] == b'}'
                            };
                        if lookahead_ok {
                            out.push_str("COOH");
                            i = j + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\mathrm\s*\{\s*([A-Za-z0-9]+)\s*\^\s*\{\s*([^{}]+?)\s*\}\s*\}` →
/// `\mathrm{<g1>^{<g2-no-ws>}}`.
fn compact_mathrm_superscript(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if expr[i..].starts_with("\\mathrm") {
            let mut j = i + 7;
            j = skip_ws(expr, j);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(expr, j + 1);
                let mut g1_start = k;
                while g1_start < bytes.len() && bytes[g1_start].is_ascii_alphanumeric() {
                    g1_start += 1;
                }
                if g1_start > k {
                    let m = skip_ws(expr, g1_start);
                    if m < bytes.len() && bytes[m] == b'^' {
                        let n = skip_ws(expr, m + 1);
                        if n < bytes.len() && bytes[n] == b'{' {
                            let p = skip_ws(expr, n + 1);
                            // g2 = chars up to first '}'.
                            let mut q = p;
                            while q < bytes.len() && bytes[q] != b'}' {
                                q += 1;
                            }
                            if q > p {
                                let g2 = &expr[p..q];
                                let r = skip_ws(expr, q + 1);
                                if r < bytes.len() && bytes[r] == b'}' {
                                    out.push_str("\\mathrm{");
                                    out.push_str(&expr[k..g1_start]);
                                    out.push_str("^{");
                                    out.push_str(&collapse_ws(g2));
                                    out.push_str("}}");
                                    i = r + 1;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `(?<=\d)\s*\.\s*(?=\d)` → "." and `(?<=\d)\s+(?=\d)` → "".
fn compact_digit_dots(expr: &str) -> String {
    // First pass: digit . digit with ws around dot → single ".".
    let mut out = String::new();
    let mut i = 0usize;
    while i < expr.len() {
        let ch = expr[i..].chars().next().unwrap();
        if ch == '.' {
            // ws before dot
            let mut ws_start = i;
            while ws_start > 0 {
                let prev = expr[..ws_start].chars().next_back().unwrap();
                if prev.is_whitespace() {
                    ws_start -= prev.len_utf8();
                } else {
                    break;
                }
            }
            // ws after dot
            let mut ws_end = i + 1;
            while ws_end < expr.len() {
                let n = expr[ws_end..].chars().next().unwrap();
                if n.is_whitespace() {
                    ws_end += n.len_utf8();
                } else {
                    break;
                }
            }
            let prev_ok = ws_start > 0 && expr[..ws_start].chars().next_back().unwrap().is_ascii_digit();
            let next_ok = ws_end < expr.len() && expr[ws_end..].chars().next().unwrap().is_ascii_digit();
            if prev_ok && next_ok {
                out.push('.');
                i = ws_end;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    // Second pass: remove ws between digits.
    let mut out2 = String::new();
    let mut i = 0usize;
    while i < out.len() {
        let ch = out[i..].chars().next().unwrap();
        if ch.is_whitespace() {
            let ws_start = i;
            let ws_end = {
                let mut e = i;
                while e < out.len() {
                    let n = out[e..].chars().next().unwrap();
                    if n.is_whitespace() {
                        e += n.len_utf8();
                    } else {
                        break;
                    }
                }
                e
            };
            let prev_ok = ws_start > 0 && out[..ws_start].chars().next_back().unwrap().is_ascii_digit();
            let next_ok = ws_end < out.len() && out[ws_end..].chars().next().unwrap().is_ascii_digit();
            if prev_ok && next_ok {
                i = ws_end;
                continue;
            }
        }
        out2.push(ch);
        i += ch.len_utf8();
    }
    out2
}

/// `~\s*\\mathrm\s*\{\s*e\s*V\s*\}\s*\.?` → ` \mathrm{eV}`.
fn compact_tilde_e_v(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'~' {
            let mut j = i + 1;
            j = skip_ws(expr, j);
            if expr[j..].starts_with("\\mathrm") {
                let k = skip_ws(expr, j + 7);
                if k < bytes.len() && bytes[k] == b'{' {
                    let l = skip_ws(expr, k + 1);
                    if l < bytes.len() && bytes[l] == b'e' {
                        let m = skip_ws(expr, l + 1);
                        if m < bytes.len() && bytes[m] == b'V' {
                            let n = skip_ws(expr, m + 1);
                            if n < bytes.len() && bytes[n] == b'}' {
                                let o = skip_ws(expr, n + 1);
                                let mut end = o;
                                if o < bytes.len() && bytes[o] == b'.' {
                                    end = o + 1;
                                }
                                out.push_str(" \\mathrm{eV}");
                                i = end;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\vec\s*\{\s*([A-Za-z])\s*\}` → `\1`.
fn unwrap_vec(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if expr[i..].starts_with("\\vec") {
            let j = skip_ws(expr, i + 4);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(expr, j + 1);
                if k < bytes.len() && bytes[k].is_ascii_alphabetic() {
                    let l = skip_ws(expr, k + 1);
                    if l < bytes.len() && bytes[l] == b'}' {
                        out.push(expr[k..k + 1].chars().next().unwrap());
                        i = l + 1;
                        continue;
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\bf\s*\{\s*([A-Za-z0-9\-+*/]+)\s*\}` → `\1`.
fn unwrap_bf_group(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if expr[i..].starts_with("\\bf") {
            let j = skip_ws(expr, i + 3);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(expr, j + 1);
                let mut run_end = k;
                while run_end < bytes.len() && matches!(bytes[run_end], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'+' | b'*' | b'/') {
                    run_end += 1;
                }
                if run_end > k {
                    let l = skip_ws(expr, run_end);
                    if l < bytes.len() && bytes[l] == b'}' {
                        out.push_str(&expr[k..run_end]);
                        i = l + 1;
                        continue;
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\{\s*\\bf\s+([^{}]+?)\s*\}` → `{\1}`.
fn unwrap_brace_bf(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let j = skip_ws(expr, i + 1);
            if expr[j..].starts_with("\\bf") {
                let k = skip_ws(expr, j + 3);
                if k > j + 3 && k < bytes.len() && bytes[k] != b'}' {
                    // \s+ required between \bf and content: k > j+3 ensures at least one ws char.
                    let mut run_end = k;
                    while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                        run_end += 1;
                    }
                    if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                        let l = skip_ws(expr, run_end + 1);
                        if l < bytes.len() && bytes[l] == b'}' {
                            let inner = expr[k..run_end].trim();
                            out.push('{');
                            out.push_str(inner);
                            out.push('}');
                            i = l + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `^\{\s*([A-Za-z])\s*\}(?=\s+\{)` → `\1`.
fn unwrap_leading_single_letter(expr: &str) -> String {
    let bytes = expr.as_bytes();
    if bytes.is_empty() || bytes[0] != b'{' {
        return expr.to_string();
    }
    let j = skip_ws(expr, 1);
    if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        let k = skip_ws(expr, j + 1);
        if k < bytes.len() && bytes[k] == b'}' {
            let l = skip_ws(expr, k + 1);
            if l > k + 1 && l < bytes.len() && bytes[l] == b'{' {
                // lookahead: at least one ws char then `{`.
                let mut replaced = String::new();
                replaced.push(expr[j..j + 1].chars().next().unwrap());
                replaced.push_str(&expr[l..]);
                return replaced;
            }
        }
    }
    expr.to_string()
}

fn repair_common_ocr_formula_noise(expr: &str) -> String {
    let s = replace_c00h(expr);
    let s = compact_mathrm_superscript(&s);
    let s = compact_digit_dots(&s);
    let s = compact_tilde_e_v(&s);
    let s = unwrap_vec(&s);
    let s = unwrap_bf_group(&s);
    let s = unwrap_brace_bf(&s);
    let s = unwrap_leading_single_letter(&s);
    strip_trailing_formula_punctuation(&s)
}

fn unwrap_style_wrappers(expr: &str) -> String {
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        // { \bf text } → {text}
        expr = scan_group_prefixed_wrapper(&expr);
        // \bf{text} → {text}
        expr = scan_direct_group_wrapper(&expr);
        if expr == prev {
            break;
        }
    }
    expr
}

fn scan_group_prefixed_wrapper(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let j = skip_ws(expr, i + 1);
            if j < bytes.len() && bytes[j] == b'\\' {
                let (name, name_end) = read_command_name(expr, j + 1);
                if STYLE_WRAPPER_MACROS.contains(&name.as_str()) {
                    let k = skip_ws(expr, name_end);
                    if k > name_end {
                        let mut run_end = k;
                        while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                            run_end += 1;
                        }
                        if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                            let l = skip_ws(expr, run_end + 1);
                            if l < bytes.len() && bytes[l] == b'}' {
                                let inner = expr[k..run_end].trim();
                                out.push('{');
                                out.push_str(inner);
                                out.push('}');
                                i = l + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn scan_direct_group_wrapper(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (name, name_end) = read_command_name(expr, i + 1);
            if STYLE_WRAPPER_MACROS.contains(&name.as_str()) {
                let j = skip_ws(expr, name_end);
                if j < bytes.len() && bytes[j] == b'{' {
                    let k = skip_ws(expr, j + 1);
                    let mut run_end = k;
                    while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                        run_end += 1;
                    }
                    if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                        let l = skip_ws(expr, run_end + 1);
                        if l < bytes.len() && bytes[l] == b'}' {
                            let inner = expr[k..run_end].trim();
                            out.push('{');
                            out.push_str(inner);
                            out.push('}');
                            i = l + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_balanced_group(text: &str, start: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'{' {
        return (String::new(), start);
    }
    let mut depth = 0usize;
    let mut idx = start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (text[start + 1..idx].to_string(), idx + 1);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    (String::new(), start)
}

fn unwrap_named_macros(expr: &str, macros: &[&str]) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (name, name_end) = read_command_name(expr, i + 1);
            if macros.contains(&name.as_str()) {
                let k = skip_ws(expr, name_end);
                if k < bytes.len() && bytes[k] == b'{' {
                    let (inner, end) = find_balanced_group(expr, k);
                    out.push_str(&normalize_formula_for_latex_math(&inner));
                    i = end;
                    continue;
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn unwrap_legacy_layout_wrappers(expr: &str) -> String {
    unwrap_named_macros(expr, &LEGACY_LAYOUT_WRAPPER_MACROS)
}

fn unwrap_inline_text_wrappers(expr: &str) -> String {
    unwrap_named_macros(expr, &INLINE_TEXT_WRAPPER_MACROS)
}

fn compact_script_groups(expr: &str) -> String {
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        expr = compact_script_group_sub(&expr);
        expr = compact_script_spacing(&expr);
        if expr == prev {
            break;
        }
    }
    expr
}

fn compact_script_group_sub(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'_' || c == b'^' {
            let marker = c;
            let j = skip_ws(expr, i + 1);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(expr, j + 1);
                let mut run_end = k;
                while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                    run_end += 1;
                }
                if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                    let inner = expr[k..run_end].trim();
                    out.push(marker as char);
                    out.push('{');
                    out.push_str(&compact_script_groups(inner));
                    out.push('}');
                    i = run_end + 1;
                    continue;
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Remove whitespace around `_`/`^` after alnum/`}`/`]`, and between an
/// atom and a brace script group. Applied in one pass each fixpoint iteration
/// to keep it deterministic.
fn compact_script_spacing(expr: &str) -> String {
    // Python rules 3-5, then 6-7, then 8, applied as separate passes (each a
    // single left-to-right `re.sub`), then the fixpoint loop re-runs them.
    let s = compact_script_after_atom(expr);
    let s = compact_script_group_after_atom(&s);
    hat_star(&s)
}

fn is_script_atom(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '}' || c == ']'
}

/// Rules 3/4/5: `(?<=[A-Za-z0-9\}\)\]])\s*[_\^]\s*...` — a `_`/`^` whose previous
/// non-whitespace char is an atom, with whitespace stripped on both sides.
fn compact_script_after_atom(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = expr[i..].chars().next().unwrap();
        if ch == '_' || ch == '^' {
            let prev_atom = {
                let mut p = i;
                let mut found = false;
                while p > 0 {
                    let c = expr[..p].chars().next_back().unwrap();
                    p -= c.len_utf8();
                    if c.is_whitespace() {
                        continue;
                    }
                    found = is_script_atom(c);
                    break;
                }
                found
            };
            if prev_atom {
                let j = skip_ws(expr, i + 1);
                let mut matched = false;
                if j < bytes.len() && bytes[j] == b'\\' {
                    // Rule 5: `^\s*(\\[A-Za-z]+)` only for `^` + command.
                    if ch == '^' {
                        let (name, _) = read_command_name(expr, j + 1);
                        if !name.is_empty() {
                            while out.chars().next_back().map_or(false, |c| c.is_whitespace()) {
                                out.pop();
                            }
                            out.push('^');
                            out.push_str(&expr[j..j + 1 + name.len()]);
                            i = j + 1 + name.len();
                            matched = true;
                        }
                    }
                } else if j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'*' | b'+' | b'-'))
                {
                    let mut k = j;
                    while k < bytes.len()
                        && (bytes[k].is_ascii_alphanumeric() || matches!(bytes[k], b'*' | b'+' | b'-'))
                    {
                        k += 1;
                    }
                    while out.chars().next_back().map_or(false, |c| c.is_whitespace()) {
                        out.pop();
                    }
                    out.push(ch);
                    out.push_str(&expr[j..k]);
                    i = k;
                    matched = true;
                }
                if matched {
                    continue;
                }
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Rules 6/7: `(\\[A-Za-z]+|[A-Za-z0-9\}\)\]])\s+([_\^]\{[^{}]*\})` — an atom
/// followed by whitespace then a brace script group; drop the whitespace.
fn compact_script_group_after_atom(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let is_command = bytes[i] == b'\\'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_alphabetic();
        let atom_end = if is_command {
            let (_, e) = read_command_name(expr, i + 1);
            e
        } else {
            let ch = expr[i..].chars().next().unwrap();
            if !is_script_atom(ch) {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
            i + ch.len_utf8()
        };
        let j = skip_ws(expr, atom_end);
        if j > atom_end && j < bytes.len() && (bytes[j] == b'_' || bytes[j] == b'^') {
            let k = skip_ws(expr, j + 1);
            if k < bytes.len() && bytes[k] == b'{' {
                let mut run_end = k + 1;
                while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                    run_end += 1;
                }
                if run_end >= k + 1 && run_end < bytes.len() && bytes[run_end] == b'}' {
                    out.push_str(&expr[i..atom_end]);
                    out.push(bytes[j] as char);
                    out.push('{');
                    out.push_str(&expr[k + 1..run_end]);
                    out.push('}');
                    i = run_end + 1;
                    continue;
                }
            }
        }
        out.push_str(&expr[i..atom_end]);
        i = atom_end;
    }
    out
}

/// Rule 8: `\^\s+\*` → `^*`
fn hat_star(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = expr[i..].chars().next().unwrap();
        if ch == '^' {
            let j = skip_ws(expr, i + 1);
            if j < bytes.len() && bytes[j] == b'*' {
                out.push('^');
                out.push('*');
                i = j + 1;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn compact_letter_hyphen_runs(expr: &str) -> String {
    let mut expr = expr.to_string();
    loop {
        let prev = expr.clone();
        expr = scan_letter_hyphen(&expr);
        if expr == prev {
            break;
        }
    }
    expr
}

fn scan_letter_hyphen(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let left_start = i;
        // left: (\\cmd | letter) + alnum*
        let consume_atom = |at: usize| -> (usize, bool) {
            let mut e = at;
            if e < bytes.len() && bytes[e] == b'\\' {
                if e + 1 >= bytes.len() || !bytes[e + 1].is_ascii_alphabetic() {
                    return (at, false);
                }
                e += 2;
                while e < bytes.len() && bytes[e].is_ascii_alphabetic() {
                    e += 1;
                }
            } else if e < bytes.len() && bytes[e].is_ascii_alphabetic() {
                e += 1;
            } else {
                return (at, false);
            }
            while e < bytes.len() && bytes[e].is_ascii_alphanumeric() {
                e += 1;
            }
            (e, true)
        };
        if b.is_ascii_alphabetic() || b == b'\\' {
            let (left_end, ok) = consume_atom(i);
            if ok {
                let after_left = skip_ws(expr, left_end);
                if after_left < bytes.len() && bytes[after_left] == b'-' {
                    let after_hyphen = skip_ws(expr, after_left + 1);
                    let (right_end, ok2) = consume_atom(after_hyphen);
                    if ok2 {
                        out.push_str(&expr[left_start..left_end]);
                        out.push('-');
                        out.push_str(&expr[after_hyphen..right_end]);
                        i = right_end;
                        continue;
                    }
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Wrap operators `= + * / < > : , ;` in single spaces, and hyphens between
/// number/close-brace/command and number/open-brace/command in single spaces.
fn space_operators(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if matches!(b, b'=' | b'+' | b'*' | b'/' | b'<' | b'>' | b':' | b',' | b';') {
            // trim trailing ws already emitted
            while out.chars().next_back().map_or(false, |c| c.is_whitespace()) {
                out.pop();
            }
            out.push(' ');
            out.push(b as char);
            out.push(' ');
            let j = skip_ws(expr, i + 1);
            i = j;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn space_selective_hyphens(expr: &str) -> String {
    // `(?P<left>(?:\d+|[)\]}]|\\[A-Za-z]+))\s*-\s*(?P<right>(?:\d+|[(\[{]|\\[A-Za-z]+))`
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let left_end = {
            let mut e = i;
            let mut matched = false;
            if bytes[e].is_ascii_digit() {
                while e < bytes.len() && bytes[e].is_ascii_digit() {
                    e += 1;
                }
                matched = true;
            } else if matches!(bytes[e], b')' | b']' | b'}') {
                e += 1;
                matched = true;
            } else if bytes[e] == b'\\' && e + 1 < bytes.len() && bytes[e + 1].is_ascii_alphabetic() {
                e += 2;
                while e < bytes.len() && bytes[e].is_ascii_alphabetic() {
                    e += 1;
                }
                matched = true;
            }
            if !matched {
                e = i;
            }
            e
        };
        if left_end > i {
            let after_left = skip_ws(expr, left_end);
            if after_left < bytes.len() && bytes[after_left] == b'-' {
                let after_hyphen = skip_ws(expr, after_left + 1);
                let mut right_end = after_hyphen;
                let mut right_matched = false;
                if right_end < bytes.len() && bytes[right_end].is_ascii_digit() {
                    while right_end < bytes.len() && bytes[right_end].is_ascii_digit() {
                        right_end += 1;
                    }
                    right_matched = true;
                } else if right_end < bytes.len() && matches!(bytes[right_end], b'(' | b'[' | b'{') {
                    right_end += 1;
                    right_matched = true;
                } else if right_end < bytes.len()
                    && bytes[right_end] == b'\\'
                    && right_end + 1 < bytes.len()
                    && bytes[right_end + 1].is_ascii_alphabetic()
                {
                    right_end += 2;
                    while right_end < bytes.len() && bytes[right_end].is_ascii_alphabetic() {
                        right_end += 1;
                    }
                    right_matched = true;
                }
                if right_matched {
                    out.push_str(&expr[i..left_end]);
                    out.push_str(" - ");
                    out.push_str(&expr[after_hyphen..right_end]);
                    i = right_end;
                    continue;
                }
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn normalize_formula_for_latex_math(formula_text: &str) -> String {
    let expr = collapse_ws_segments(formula_text);
    if expr.is_empty() {
        return expr;
    }
    if matches!(expr.as_str(), "^®" | "^{®}" | r"^\circled{R}" | r"^\textcircled{R}") {
        return r"\text{®}".to_string();
    }

    let mut s = remove_begin_array(&expr);
    s = remove_end_array(&s);
    s = cal_to_mathcal(&s);
    s = sub_macro(&s, "mathscr", "mathcal");
    s = sub_macro(&s, "rrangle", "rangle");
    s = sub_macro(&s, "llangle", "langle");
    s = sub_macro(&s, "langlen", "langle n");
    s = sub_macro(&s, "Breve", "breve");
    s = sub_macro(&s, "Vec", "vec");
    s = textsuperscript_to_superscript(&s);
    s = circled_times_to_otimes(&s);
    s = circled_parallel_to_circ(&s);
    s = circled_unwrap(&s);
    s = textcircled_times_to_otimes(&s);
    s = textcircled_scriptsize_parallel_to_circ(&s);
    s = textcircled_parallel_to_circ(&s);
    s = textcircled_unwrap(&s);
    s = strip_style_mode_commands(&s);

    s = compact_mathrm_payload(&s);

    s = strip_trailing_dot_after_digit(&s);

    s = unwrap_style_wrappers(&s);
    s = unwrap_legacy_layout_wrappers(&s);
    s = unwrap_inline_text_wrappers(&s);
    s = remove_empty_style_brace(&s);
    s = strip_outer_braces(&s);

    s = repair_common_ocr_formula_noise(&s);
    s = compact_script_groups(&s);
    s = compact_letter_hyphen_runs(&s);
    s = fix_dot_before_close(&s);
    s = compact_digit_dots(&s);
    s = space_operators(&s);
    s = space_selective_hyphens(&s);
    s = compact_script_groups(&s);
    s = fix_superscript_signs(&s);
    s = collapse_ws_segments(&s);
    s = compact_mathrm_cooh(&s);
    s = brace_escape_single_script(&s);
    if s.starts_with('_') || s.starts_with('^') {
        s = format!("{{}} {s}");
    }
    s
}

/// Collapse runs of whitespace to single spaces and trim.
fn collapse_ws_segments(s: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        }
    }
    out
}

fn remove_begin_array(s: &str) -> String {
    // `\\begin\{array\}\s*\{[^{}]*\}\s*` → ""
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("\\begin{array}") {
            let j = skip_ws(s, i + 13);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = j + 1;
                let mut l = k;
                while l < bytes.len() && bytes[l] != b'}' && bytes[l] != b'{' {
                    l += 1;
                }
                if l < bytes.len() && bytes[l] == b'}' {
                    let m = skip_ws(s, l + 1);
                    i = m;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn remove_end_array(s: &str) -> String {
    // `\s*\\end\{array\}` → ""
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let ws_start = i;
        let mut j = i;
        while j < bytes.len() && s[j..].chars().next().unwrap().is_whitespace() {
            j += s[j..].chars().next().unwrap().len_utf8();
        }
        if j < bytes.len() && s[j..].starts_with("\\end{array}") {
            i = j + 11;
            continue;
        }
        if j == ws_start {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        } else {
            out.push_str(&s[ws_start..j]);
            i = j;
        }
    }
    out
}

fn cal_to_mathcal(s: &str) -> String {
    // `\\cal\s+([A-Za-z])` → `\mathcal{\1}`
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("\\cal") {
            let j = skip_ws(s, i + 4);
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                out.push_str("\\mathcal{");
                out.push(bytes[j] as char);
                out.push('}');
                i = j + 1;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Replace a command name `\name` with `\replacement` when followed by a word
/// boundary (i.e. the command word ends there).
fn sub_macro(s: &str, name: &str, replacement: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (cmd, end) = read_command_name(s, i + 1);
            if cmd == name {
                out.push_str("\\");
                out.push_str(replacement);
                i = end;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\textsuperscript\s*\{\s*([^{}]+?)\s*\}` → `^{g1}`
fn textsuperscript_to_superscript(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("\\textsuperscript") {
            let j = skip_ws(s, i + 15);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(s, j + 1);
                let mut run_end = k;
                while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                    run_end += 1;
                }
                if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                    let l = skip_ws(s, run_end + 1);
                    if l < bytes.len() && bytes[l] == b'}' {
                        out.push_str("^{");
                        out.push_str(&s[k..run_end]);
                        out.push('}');
                        i = l + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\circled\s*\{\s*\\times\s*\}` → `\otimes`
fn circled_times_to_otimes(s: &str) -> String {
    replace_named_brace(s, "circled", Some("\\times"), Some("\\otimes"))
}

/// `\\circled\s*\{\s*\\parallel\s*\}` → `\circ`
fn circled_parallel_to_circ(s: &str) -> String {
    replace_named_brace(s, "circled", Some("\\parallel"), Some("\\circ"))
}

/// `\\circled\s*\{\s*([^{}]+?)\s*\}` → `\1`
fn circled_unwrap(s: &str) -> String {
    replace_named_brace(s, "circled", None, None)
}

/// `\\textcircled\s*\{\s*\\times\s*\}` → `\otimes`
fn textcircled_times_to_otimes(s: &str) -> String {
    replace_named_brace(s, "textcircled", Some("\\times"), Some("\\otimes"))
}

/// `\\textcircled\s*\{\s*\\scriptsize\s*\{\s*\\parallel\s*\}\s*\}` → `\circ`
fn textcircled_scriptsize_parallel_to_circ(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("\\textcircled") {
            let j = skip_ws(s, i + 11);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(s, j + 1);
                if s[k..].starts_with("\\scriptsize") {
                    let l = skip_ws(s, k + 10);
                    if l < bytes.len() && bytes[l] == b'{' {
                        let m = skip_ws(s, l + 1);
                        if s[m..].starts_with("\\parallel") {
                            let n = skip_ws(s, m + 8);
                            if n < bytes.len() && bytes[n] == b'}' {
                                let o = skip_ws(s, n + 1);
                                if o < bytes.len() && bytes[o] == b'}' {
                                    let p = skip_ws(s, o + 1);
                                    if p < bytes.len() && bytes[p] == b'}' {
                                        out.push_str("\\circ");
                                        i = p + 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\textcircled\s*\{\s*\\parallel\s*\}` → `\circ`
fn textcircled_parallel_to_circ(s: &str) -> String {
    replace_named_brace(s, "textcircled", Some("\\parallel"), Some("\\circ"))
}

/// `\\textcircled\s*\{\s*([^{}]+?)\s*\}` → `\1`
fn textcircled_unwrap(s: &str) -> String {
    replace_named_brace(s, "textcircled", None, None)
}

/// Helper: `\name\s*{\s*<payload>\s*}`. If payload is Some(exact), replace with
/// replacement. If payload is None, replace with the inner content (unwrap).
fn replace_named_brace(s: &str, name: &str, payload: Option<&str>, replacement: Option<&str>) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let full = format!("\\{name}");
        if s[i..].starts_with(&full) {
            let j = skip_ws(s, i + full.len());
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(s, j + 1);
                let mut run_end = k;
                while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                    run_end += 1;
                }
                if run_end > k && run_end < bytes.len() && bytes[run_end] == b'}' {
                    let trimmed = s[k..run_end].trim();
                    match payload {
                        Some(p) if trimmed == p => {
                            out.push_str(replacement.unwrap());
                            i = run_end + 1;
                            continue;
                        }
                        Some(_) => {}
                        None => {
                            out.push_str(trimmed);
                            i = run_end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\\(?:scriptstyle|scriptscriptstyle|textstyle|displaystyle)\b` → ""
fn strip_style_mode_commands(s: &str) -> String {
    let bytes = s.as_bytes();
    let names = ["scriptstyle", "scriptscriptstyle", "textstyle", "displaystyle"];
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (cmd, end) = read_command_name(s, i + 1);
            if names.contains(&cmd.as_str()) {
                i = end;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `(?<=\d)\s*\\dot\b(?=\s*$)` and `(?<=\d)\s*\\dot\b(?=\s*[\)\],;])` → "."
fn strip_trailing_dot_after_digit(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && s[i..].starts_with("\\dot") {
            let prev = if i > 0 {
                let mut p = i;
                while p > 0 {
                    let c = s[..p].chars().next_back().unwrap();
                    if c.is_whitespace() {
                        p -= c.len_utf8();
                    } else {
                        break;
                    }
                }
                if p > 0 {
                    Some(s[..p].chars().next_back().unwrap())
                } else {
                    None
                }
            } else {
                None
            };
            if matches!(prev, Some(c) if c.is_ascii_digit()) {
                let after = skip_ws(s, i + 4);
                let ok_end = after >= bytes.len()
                    || matches!(bytes[after], b')' | b']' | b',' | b';')
                    || after + 1 >= bytes.len();
                if ok_end {
                    // strip trailing ws before \dot
                    while out.chars().next_back().map_or(false, |c| c.is_whitespace()) {
                        out.pop();
                    }
                    out.push('.');
                    i = i + 4;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\{\s*\\(?:bf|rm|it|tt|sf|pmb)\s*\}` → ""
fn remove_empty_style_brace(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let j = skip_ws(s, i + 1);
            if j < bytes.len() && bytes[j] == b'\\' {
                let (cmd, end) = read_command_name(s, j + 1);
                if matches!(cmd.as_str(), "bf" | "rm" | "it" | "tt" | "sf" | "pmb") {
                    let k = skip_ws(s, end);
                    if k < bytes.len() && bytes[k] == b'}' {
                        i = k + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `^\{\s*([^{}]+?)\s*\}$` → `\1`
fn strip_outer_braces(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'{' && bytes[bytes.len() - 1] == b'}' {
        let inner = &s[1..s.len() - 1];
        if !inner.contains('{') && !inner.contains('}') {
            return inner.trim().to_string();
        }
    }
    s.to_string()
}

/// `\.\s+([\)\],;])` → `.\1`
fn fix_dot_before_close(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let j = skip_ws(s, i + 1);
            if j < bytes.len() && matches!(bytes[j], b')' | b']' | b',' | b';') {
                while out.chars().next_back().map_or(false, |c| c.is_whitespace()) {
                    out.pop();
                }
                out.push('.');
                out.push(bytes[j] as char);
                i = j + 1;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `(?<=\^)\s+([*+\-])` → `\1` and `\^([*+\-])\s+(?=[}\)])` → `^\1`
fn fix_superscript_signs(s: &str) -> String {
    let bytes = s.as_bytes();
    // pass 1: strip ws right after ^ when followed by sign
    let mut pass1 = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'^' {
            let j = skip_ws(s, i + 1);
            if j < bytes.len() && matches!(bytes[j], b'*' | b'+' | b'-') {
                out_push_char(&mut pass1, '^');
                out_push_char(&mut pass1, bytes[j] as char);
                i = j + 1;
                continue;
            }
        }
        out_push_char(&mut pass1, s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }
    // pass 2: strip ws after sign after ^ when followed by close
    let p1 = pass1;
    let bytes = p1.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if i > 0 && bytes[i - 1] == b'^' && matches!(bytes[i], b'*' | b'+' | b'-') {
            let j = skip_ws(&p1, i + 1);
            if j < bytes.len() && matches!(bytes[j], b')' | b'}') {
                out.push('^');
                out.push(bytes[i] as char);
                i = j;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn out_push_char(out: &mut String, c: char) {
    out.push(c);
}

/// `\\mathrm\s*\{\s*COOH\s*\^\s*\{\s*\*\s*\}\s*\}` → `\mathrm{COOH^{*}}`
fn compact_mathrm_cooh(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("\\mathrm") {
            let j = skip_ws(s, i + 7);
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(s, j + 1);
                if s[k..].starts_with("COOH") {
                    let m = k + 4;
                    let n = skip_ws(s, m);
                    if n < bytes.len() && bytes[n] == b'^' {
                        let o = skip_ws(s, n + 1);
                        if o < bytes.len() && bytes[o] == b'{' {
                            let p = skip_ws(s, o + 1);
                            if p < bytes.len() && bytes[p] == b'*' {
                                let q = skip_ws(s, p + 1);
                                if q < bytes.len() && bytes[q] == b'}' {
                                    let r = skip_ws(s, q + 1);
                                    if r < bytes.len() && bytes[r] == b'}' {
                                        out.push_str("\\mathrm{COOH^{*}}");
                                        i = r + 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `^([_^])\{([^{}]+)\}$` → `\1{{\2}}`
fn brace_escape_single_script(s: &str) -> String {
    let bytes = s.as_bytes();
    let last = bytes.len();
    if bytes.len() >= 4 && matches!(bytes[0], b'_' | b'^') && bytes[1] == b'{' && bytes[last - 1] == b'}' {
        let inner = &s[2..last - 1];
        if !inner.contains('{') && !inner.contains('}') {
            return format!("{}{{{{{inner}}}}}", bytes[0] as char);
        }
    }
    s.to_string()
}

pub fn aggressively_simplify_formula_for_latex_math(formula_text: &str) -> String {
    let mut expr = normalize_formula_for_latex_math(formula_text);
    if expr.is_empty() {
        return expr;
    }
    loop {
        let prev = expr.clone();
        expr = unwrap_single_arg_commands(&expr);
        if expr == prev {
            break;
        }
    }
    expr = collapse_ws_segments(&expr);
    if expr.starts_with('_') || expr.starts_with('^') {
        expr = format!("{{}} {expr}");
    }
    expr
}

/// `\\([A-Za-z]+)\s*(?:\[[^\]]*])?\s*\{\s*([^{}]*)\s*\}(?!\s*\{)` — unwrap
/// single-argument commands unless structural.
fn unwrap_single_arg_commands(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (cmd, cmd_end) = read_command_name(s, i + 1);
            // Python regex `\\([A-Za-z]+)`: a backslash with no letter (e.g. `\{`)
            // is not a command — copy it through unchanged.
            if cmd.is_empty() {
                let ch = s[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
            let mut j = skip_ws(s, cmd_end);
            if j < bytes.len() && bytes[j] == b'[' {
                // optional [ ... ]
                let k = j + 1;
                let mut l = k;
                while l < bytes.len() && bytes[l] != b']' {
                    l += 1;
                }
                if l < bytes.len() {
                    j = skip_ws(s, l + 1);
                } else {
                    j = bytes.len();
                }
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let k = skip_ws(s, j + 1);
                let mut run_end = k;
                while run_end < bytes.len() && bytes[run_end] != b'}' && bytes[run_end] != b'{' {
                    run_end += 1;
                }
                if run_end < bytes.len() && bytes[run_end] == b'}' {
                    let l = skip_ws(s, run_end + 1);
                    // negative lookahead (?!\s*\{): next non-ws must not be '{'
                    let no_follow_brace = l >= bytes.len() || bytes[l] != b'{';
                    if no_follow_brace {
                        let inner = s[k..run_end].trim();
                        if !STRUCTURAL_LATEX_COMMANDS.contains(&cmd.as_str()) {
                            out.push_str(inner);
                            i = l;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_trailing_punct() {
        assert_eq!(strip_trailing_formula_punctuation("a, "), "a");
        assert_eq!(strip_trailing_formula_punctuation("ab,"), "ab");
        assert_eq!(strip_trailing_formula_punctuation("ab"), "ab");
        assert_eq!(strip_trailing_formula_punctuation("( x ) ."), "( x )");
    }

    #[test]
    fn normalizer_basics() {
        assert_eq!(normalize_formula_for_latex_math(r"\frac{\partial E}{\partial R}"), r"\frac{\partial E}{\partial R}");
        assert_eq!(normalize_formula_for_latex_math(r"g^{IJ}(R_x)"), r"g^{IJ}(R_x)");
        assert_eq!(normalize_formula_for_latex_math(r"x_1"), "x_1");
    }

    #[test]
    fn compact_mathrm() {
        assert_eq!(compact_mathrm_payload(r"\mathrm{ a b }"), r"\mathrm{ab}");
        assert_eq!(compact_mathrm_payload(r"\mathrm{ { a b } }"), r"\mathrm{ab}");
    }

    #[test]
    fn aggressively_simplifies() {
        assert_eq!(
            aggressively_simplify_formula_for_latex_math(r"\sqrt{\delta\mathbf{R}^{IJ}}"),
            r"\sqrt{\deltaR^{IJ}}"
        );
    }

    #[test]
    fn subscript_spacing() {
        assert_eq!(normalize_formula_for_latex_math(r"E_{ I J }"), r"E_{I J}");
        assert_eq!(normalize_formula_for_latex_math(r"x^{ 1 2 }"), r"x^{12}");
    }

    #[test]
    fn compact_script_spacing_ground_truth() {
        // Cross-checked against Python _compact_script_groups.
        assert_eq!(compact_script_groups(r"a _ b"), r"a_b");
        assert_eq!(compact_script_groups(r"a ^ b"), r"a^b");
        assert_eq!(compact_script_groups(r"a ^\mathbf R"), r"a^\mathbf R");
        assert_eq!(compact_script_groups(r"a _{b}"), r"a_{b}");
        assert_eq!(compact_script_groups(r"x ^ { 1 }"), r"x^{1}");
        assert_eq!(compact_script_groups(r"\alpha _ 1"), r"\alpha_1");
        assert_eq!(compact_script_groups(r"x ^ *"), r"x^*");
        assert_eq!(compact_script_groups(r"E _ {IJ} ^ {KL}"), r"E_{IJ}^{KL}");
        assert_eq!(compact_script_groups(r"a _b"), r"a_b");
        assert_eq!(compact_script_groups(r"a_ b"), r"a_b");
        assert_eq!(compact_script_groups(r"c ^\times"), r"c^\times");
        assert_eq!(compact_script_groups(r"\Gamma ^ { - 1 }"), r"\Gamma^{- 1}");
        assert_eq!(compact_script_groups(r"X _{i j}"), r"X_{i j}");
        assert_eq!(compact_script_groups(r"^{ 2 }"), r"^{2}");
    }

    #[test]
    fn capacity_formula_ground_truth() {
        // Cross-checked against Python aggressively_simplify_formula_for_latex_math.
        // These exact formulas drive the payload capacity tests.
        assert_eq!(aggressively_simplify_formula_for_latex_math(r"x_1"), r"x_1");
        assert_eq!(
            aggressively_simplify_formula_for_latex_math(r"\frac{\partial E}{\partial R}"),
            r"\frac{\partial E}{\partial R}"
        );
        assert_eq!(
            aggressively_simplify_formula_for_latex_math(r"\sqrt{\delta R}"),
            r"\sqrt{\delta R}"
        );
        assert_eq!(
            aggressively_simplify_formula_for_latex_math(r"g-h(\mathbf{R}_x)"),
            r"g-h(R_x)"
        );
        assert_eq!(
            aggressively_simplify_formula_for_latex_math(r"\\frac{\partial E}{\partial R}"),
            r"\\frac{\partial E}{\partial R}"
        );
    }
}
