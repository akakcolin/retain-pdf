// Port of services/rendering/layout/payload/toc_structure.py — translating a
// block payload item's TOC entries (source `toc_entries` or geometric fallback)
// into RenderTocEntry JSON. Operates on the raw JSON item dict.

use serde_json::{json, Value};

use crate::payload::toc_document::{build_toc_entries, coerce_bbox, order_toc_lines_by_geometry};

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_roman_byte(b: u8) -> bool {
    matches!(b, b'i' | b'v' | b'x' | b'l' | b'c' | b'd' | b'm' | b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M')
}

fn match_page_from(s: &str, start: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = start;
    if i < len && bytes[i].is_ascii_digit() {
        let ds = i;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < len && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        return Some((s[ds..i].to_string(), i));
    }
    if i < len && is_roman_byte(bytes[i]) {
        let rs = i;
        while i < len && is_roman_byte(bytes[i]) {
            i += 1;
        }
        return Some((s[rs..i].to_string(), i));
    }
    None
}

/// `_translated_lines`.
fn translated_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let s = line.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

/// `_translated_lines_by_source_geometry`: reorder translated lines to match
/// the source lines' geometric column order.
fn translated_lines_by_source_geometry(item: &Value, translated_text: &str) -> Vec<String> {
    let lines = translated_lines(translated_text);
    let source_lines = item.get("source_line_texts").and_then(|v| v.as_array());
    let source_line_boxes = item.get("lines").and_then(|v| v.as_array());
    let (Some(source_lines), Some(source_line_boxes)) = (source_lines, source_line_boxes) else {
        return lines;
    };
    if lines.len() != source_lines.len() {
        return lines;
    }
    let lines_value = Value::Array(source_line_boxes.clone());
    let texts_value = Value::Array(source_lines.clone());
    let ordered = order_toc_lines_by_geometry(&lines_value, &texts_value);
    if ordered.len() != lines.len() {
        return lines;
    }
    ordered.iter().map(|(index, _text)| lines[*index].clone()).collect()
}

fn role_is_toc(item: &Value) -> bool {
    let structure_role = item
        .get("structure_role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let semantic_role = item
        .get("semantic_role")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("layout_role").and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_lowercase();
    structure_role == "table_of_contents" || semantic_role == "table_of_contents"
}

/// `_translated_lines_by_source_geometry` reorder uses the doc-span `line_bbox`
/// fallback; the entry-level bbox helpers below are bbox-only like Python.

/// `_bbox_from_line`.
fn bbox_from_line(item: &Value, entry: &Value) -> Option<[f64; 4]> {
    let line_index = entry.get("line_index").and_then(|v| v.as_i64())?;
    if line_index < 0 {
        return None;
    }
    let lines = item.get("lines").and_then(|v| v.as_array())?;
    let line = lines.get(line_index as usize)?;
    line.get("bbox").and_then(coerce_bbox)
}

/// `_bbox_from_entry`.
fn bbox_from_entry(entry: &Value) -> Option<[f64; 4]> {
    entry.get("bbox").and_then(coerce_bbox)
}

/// `_strip_toc_page_label` suffix removal: `(?:leader)?\s*\(?page\)?\s*$` —
/// the leader group is optional.
fn strip_page_suffix(value: &str, page: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let len = bytes.len();
    let mut i = len;
    while i > 0 && is_ws(bytes[i - 1]) {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b')' {
        i -= 1;
    }
    if i < page.len() {
        return None;
    }
    let page_start = i - page.len();
    if !value.is_char_boundary(page_start) || &value[page_start..i] != page {
        return None;
    }
    i = page_start;
    if i > 0 && bytes[i - 1] == b'(' {
        i -= 1;
    }
    while i > 0 && is_ws(bytes[i - 1]) {
        i -= 1;
    }
    let leader_start = i;
    let mut i_ellipsis = i;
    while i_ellipsis >= 3 && bytes[i_ellipsis - 3] == 0xE2 && bytes[i_ellipsis - 2] == 0x80 && bytes[i_ellipsis - 1] == 0xA6 {
        i_ellipsis -= 3;
    }
    let mut i_dots = i;
    while i_dots > 0 && bytes[i_dots - 1] == b'.' {
        i_dots -= 1;
    }
    if i - i_ellipsis > 0 {
        i = i_ellipsis;
    } else if i - i_dots >= 2 {
        i = i_dots;
    } else {
        i = leader_start;
    }
    Some(value[..i].to_string())
}

/// `_strip_toc_page_label`.
fn strip_toc_page_label(text: &str, page_label: &str) -> String {
    let mut value = text.trim().to_string();
    let page = page_label.trim();
    if !page.is_empty() {
        if let Some(stripped) = strip_page_suffix(&value, page) {
            value = stripped;
        }
    }
    value
        .trim_matches(|c| c == ' ' || c == '.' || c == '\t')
        .to_string()
}

/// `_strip_toc_number`.
fn strip_toc_number(text: &str, number: &str) -> String {
    let value = text.trim().to_string();
    let number_text = number.trim().to_string();
    if !number_text.is_empty() && value.starts_with(&number_text) {
        value[number_text.len()..].trim().to_string()
    } else {
        value
    }
}

/// `_fallback_toc_entries`.
fn fallback_toc_entries(item: &Value) -> Vec<Value> {
    if !role_is_toc(item) {
        return vec![];
    }
    let line_texts = item.get("source_line_texts").and_then(|v| v.as_array());
    let lines = item.get("lines").and_then(|v| v.as_array());
    let (Some(line_texts), Some(lines)) = (line_texts, lines) else {
        return vec![];
    };
    build_toc_entries(&Value::Array(lines.clone()), &Value::Array(line_texts.clone()))
}

/// `_toc_line_count`.
fn toc_line_count(item: &Value) -> i64 {
    if let Some(line_texts) = item.get("source_line_texts").and_then(|v| v.as_array()) {
        if !line_texts.is_empty() {
            return line_texts
                .iter()
                .filter(|v| !v.as_str().unwrap_or("").trim().is_empty())
                .count() as i64;
        }
    }
    if let Some(lines) = item.get("lines").and_then(|v| v.as_array()) {
        return lines.len() as i64;
    }
    0
}

/// `_should_rebuild_partial_toc_entries`.
fn should_rebuild_partial_toc_entries(item: &Value, entries: &[Value]) -> bool {
    if entries.is_empty() {
        return false;
    }
    let line_count = toc_line_count(item);
    line_count > 0 && (entries.len() as i64) < line_count
}

/// `_line_bbox`.
fn line_bbox_at(item: &Value, index: i64) -> Option<[f64; 4]> {
    if index < 0 {
        return None;
    }
    let lines = item.get("lines").and_then(|v| v.as_array())?;
    let line = lines.get(index as usize)?;
    line.get("bbox").and_then(coerce_bbox)
}

/// `PAREN_PAGE_SUFFIX_RE` suffix: `\s*\((page)\)\s*$`.
fn match_paren_suffix_at(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len && is_ws(bytes[i]) {
        i += 1;
    }
    if i >= len || bytes[i] != b'(' {
        return None;
    }
    let (page, after) = match_page_from(s, i + 1)?;
    if after >= len || bytes[after] != b')' {
        return None;
    }
    let mut k = after + 1;
    while k < len && is_ws(bytes[k]) {
        k += 1;
    }
    if k != len {
        return None;
    }
    Some(page)
}

/// `^\s*(title.+?)\s*\((page)\)\s*$` — first (shortest) title split wins.
fn match_paren_suffix(value: &str) -> Option<(usize, String)> {
    for split in 1..=value.len() {
        if !value.is_char_boundary(split) {
            continue;
        }
        if let Some(page) = match_paren_suffix_at(&value[split..]) {
            return Some((split, page));
        }
    }
    None
}

/// Leader separator `\s*(\.{2,}|…+)\s*`; returns end position after the ws.
fn try_leader_sep_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = start;
    while i < len && is_ws(bytes[i]) {
        i += 1;
    }
    let leader_start = i;
    if i < len && bytes[i] == b'.' {
        while i < len && bytes[i] == b'.' {
            i += 1;
        }
        if i - leader_start < 2 {
            return None;
        }
    } else if i + 2 < len && bytes[i] == 0xE2 && bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA6 {
        while i + 2 < len && bytes[i] == 0xE2 && bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA6 {
            i += 3;
        }
    } else {
        return None;
    }
    while i < len && is_ws(bytes[i]) {
        i += 1;
    }
    Some(i)
}

/// `\s+` separator; returns end position after the whitespace run.
fn try_ws_sep_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if start >= len || !is_ws(bytes[start]) {
        return None;
    }
    let mut i = start;
    while i < len && is_ws(bytes[i]) {
        i += 1;
    }
    Some(i)
}

/// `\(?page\)?\s*$` after a separator.
fn match_page_after_optional_paren(s: &str, pos: usize) -> Option<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = pos;
    if i < len && bytes[i] == b'(' {
        i += 1;
    }
    let (page, after) = match_page_from(s, i)?;
    let mut k = after;
    if k < len && bytes[k] == b')' {
        k += 1;
    }
    while k < len && is_ws(bytes[k]) {
        k += 1;
    }
    if k != len {
        return None;
    }
    Some(page)
}

/// `TRANSLATED_TOC_LINE_RE` suffix: `(sep)(\(?page\)?\s*$)`.
fn match_translated_suffix_at(s: &str) -> Option<String> {
    if let Some(after_sep) = try_leader_sep_end(s, 0) {
        if let Some(page) = match_page_after_optional_paren(s, after_sep) {
            return Some(page);
        }
    }
    if let Some(after_sep) = try_ws_sep_end(s, 0) {
        if let Some(page) = match_page_after_optional_paren(s, after_sep) {
            return Some(page);
        }
    }
    None
}

/// `_split_translated_toc_line`.
fn split_translated_toc_line(line: &str) -> (String, String) {
    let value = line.trim().to_string();
    if value.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((split, page)) = match_paren_suffix(&value) {
        let title = value[..split]
            .trim_matches(|c| c == ' ' || c == '.' || c == '\t')
            .to_string();
        return (title, page);
    }
    for split in 1..=value.len() {
        if !value.is_char_boundary(split) {
            continue;
        }
        if let Some(page) = match_translated_suffix_at(&value[split..]) {
            let title = value[..split]
                .trim_matches(|c| c == ' ' || c == '.' || c == '\t')
                .to_string();
            return (title, page);
        }
    }
    (
        value
            .trim_matches(|c| c == ' ' || c == '.' || c == '\t')
            .to_string(),
        String::new(),
    )
}

/// `_render_toc_entries_from_translated_lines`.
fn render_toc_entries_from_translated_lines(item: &Value, translated_text: &str) -> Vec<Value> {
    if !role_is_toc(item) {
        return vec![];
    }
    let mut rendered: Vec<Value> = Vec::new();
    for (index, line) in translated_lines_by_source_geometry(item, translated_text).iter().enumerate() {
        let Some(bbox) = line_bbox_at(item, index as i64) else {
            continue;
        };
        let (title, page_label) = split_translated_toc_line(line);
        if title.is_empty() {
            continue;
        }
        rendered.push(toc_entry_value(&title, &page_label, &bbox, "", 1));
    }
    rendered
}

fn toc_entry_value(title: &str, page_label: &str, bbox: &[f64; 4], number: &str, level: i64) -> Value {
    json!({
        "title": title,
        "page_label": page_label,
        "bbox": bbox.to_vec(),
        "number": number,
        "level": level,
    })
}

/// `render_toc_entries_for_item`.
pub fn render_toc_entries_for_item(item: &Value, translated_text: &str) -> Vec<Value> {
    let raw_entries = item.get("toc_entries").and_then(|v| v.as_array()).cloned();
    let mut entries: Vec<Value> = match raw_entries {
        Some(list) if !list.is_empty() => list,
        _ => fallback_toc_entries(item),
    };
    if should_rebuild_partial_toc_entries(item, &entries) {
        let rebuilt = fallback_toc_entries(item);
        if rebuilt.len() > entries.len() {
            entries = rebuilt;
        }
    }
    if entries.is_empty() {
        return render_toc_entries_from_translated_lines(item, translated_text);
    }
    let lines = translated_lines_by_source_geometry(item, translated_text);
    let mut rendered: Vec<Value> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if !entry.is_object() {
            continue;
        }
        let Some(line_bbox) = bbox_from_line(item, entry).or_else(|| bbox_from_entry(entry)) else {
            continue;
        };
        let source_title = entry
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let page_label = entry
            .get("page_label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let number = entry
            .get("number")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let translated_line = lines.get(index).cloned().unwrap_or_default();
        let title = strip_toc_number(&strip_toc_page_label(&translated_line, &page_label), &number);
        let title = if title.is_empty() { source_title } else { title };
        let level = entry.get("level").and_then(|v| v.as_i64()).unwrap_or(1).clamp(1, 6);
        rendered.push(toc_entry_value(&title, &page_label, &line_bbox, &number, level));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splits_translated_toc_lines() {
        assert_eq!(split_translated_toc_line("第一章 ..... 5"), ("第一章".to_string(), "5".to_string()));
        assert_eq!(split_translated_toc_line("第一章 5"), ("第一章".to_string(), "5".to_string()));
        assert_eq!(split_translated_toc_line("abc def 12"), ("abc def".to_string(), "12".to_string()));
        assert_eq!(split_translated_toc_line("x (3)"), ("x".to_string(), "3".to_string()));
        assert_eq!(split_translated_toc_line("目录"), ("目录".to_string(), String::new()));
        assert_eq!(split_translated_toc_line("12a"), ("12a".to_string(), String::new()));
        assert_eq!(split_translated_toc_line("iv."), ("iv".to_string(), String::new()));
    }

    #[test]
    fn strips_page_label_suffix() {
        assert_eq!(strip_toc_page_label("标题 ..... 12", "12"), "标题");
        assert_eq!(strip_toc_page_label("标题 12", "12"), "标题");
        assert_eq!(strip_toc_page_label("标题(12)", "12"), "标题");
        assert_eq!(strip_toc_page_label("标题（12）", "12"), "标题（12）");
        assert_eq!(strip_toc_page_label("3 热化学 8", "8"), "3 热化学");
    }

    #[test]
    fn renders_fallback_entries_for_toc_item() {
        let item = json!({
            "structure_role": "table_of_contents",
            "layout_role": "toc",
            "source_line_texts": [
                "1 Introduction 2",
                "2 Sources of components 2",
                "2.1 Contributions from translation ..... 3",
            ],
            "lines": [
                {"bbox": [10.0, 20.0, 200.0, 30.0]},
                {"bbox": [10.0, 30.0, 200.0, 40.0]},
                {"bbox": [10.0, 40.0, 200.0, 50.0]},
            ],
        });
        let rendered = render_toc_entries_for_item(&item, "1 引言 2\n2 组成部分来源 2\n2.1 翻译贡献 ..... 3");
        assert_eq!(rendered.len(), 3);
        assert_eq!(rendered[0].get("title").unwrap(), &json!("引言"));
        assert_eq!(rendered[0].get("page_label").unwrap(), &json!("2"));
        assert_eq!(rendered[0].get("number").unwrap(), &json!("1"));
        assert_eq!(rendered[2].get("title").unwrap(), &json!("翻译贡献"));
        assert_eq!(rendered[2].get("level").unwrap(), &json!(2));
    }

    #[test]
    fn non_toc_item_renders_empty() {
        let item = json!({"layout_role": "paragraph", "lines": []});
        assert!(render_toc_entries_for_item(&item, "hello").is_empty());
    }
}
