// Port of services/document_schema/toc.py — TOC line parsing and geometric
// column ordering, consumed by the emit boundary via toc_structure. All regexes
// are hand-rolled on std only (anchored full-match patterns over normalized
// single-space text, where the `\s{2,}` separator alternative is dead).

use serde_json::Value;

pub const MIN_COLUMN_GAP_PT: f64 = 32.0;

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub number: String,
    pub title: String,
    pub page_label: String,
    pub level: i64,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_roman_byte(b: u8) -> bool {
    matches!(b, b'i' | b'v' | b'x' | b'l' | b'c' | b'd' | b'm' | b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M')
}

fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn toc_level(number: &str) -> i64 {
    let trimmed = number.trim_end_matches('.');
    let dots = trimmed.matches('.').count() as i64;
    (dots + 1).clamp(1, 6)
}

/// Forward page matcher: `\d+[A-Za-z]?` then `[ivxlcdmIVXLCDM]+`, greedy.
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

/// `(?:[A-Z]|\d+)(?:\.\d+)*\.?` followed by `\s+`; returns (number, end-of-ws).
fn parse_number_prefix(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    if i < len && bytes[i].is_ascii_uppercase() {
        i += 1;
    } else {
        let start = i;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    while i + 1 < len && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 2;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < len && bytes[i] == b'.' {
        i += 1;
    }
    if i >= len || !is_ws(bytes[i]) {
        return None;
    }
    let number_end = i;
    while i < len && is_ws(bytes[i]) {
        i += 1;
    }
    Some((s[..number_end].to_string(), i))
}

/// Leader separator `\s* \.{2,}|…+ \s*` then page; returns (page, end-of-page).
fn try_leader_sep(s: &str, start: usize) -> Option<(String, usize)> {
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
    match_page_from(s, i)
}

/// `\s* leader \s* page \s*$` over normalized text (no closing paren).
fn match_doc_leader_suffix(s: &str) -> Option<String> {
    let (page, after) = try_leader_sep(s, 0)?;
    let bytes = s.as_bytes();
    let mut k = after;
    while k < bytes.len() && is_ws(bytes[k]) {
        k += 1;
    }
    if k != bytes.len() {
        return None;
    }
    Some(page)
}

fn scan_doc_title_suffix(s: &str) -> Option<(String, String)> {
    for split in 1..=s.len() {
        if let Some(page) = match_doc_leader_suffix(&s[split..]) {
            return Some((s[..split].to_string(), page));
        }
    }
    None
}

/// `TOC_LINE_WITH_LEADER_RE`: optional number, then non-greedy title, leader
/// separator, page. Tries number-first (greedy optional group), then bare.
fn match_toc_with_leader(raw: &str) -> Option<(String, String, String)> {
    if let Some((number, num_end)) = parse_number_prefix(raw) {
        if let Some((title, page)) = scan_doc_title_suffix(&raw[num_end..]) {
            return Some((number, title, page));
        }
    }
    if let Some((title, page)) = scan_doc_title_suffix(raw) {
        return Some((String::new(), title, page));
    }
    None
}

/// `NUMBERED_TOC_LINE_RE`: required number, non-greedy title, `\s+`, page.
fn match_toc_numbered(raw: &str) -> Option<(String, String, String)> {
    let (number, num_end) = parse_number_prefix(raw)?;
    let bytes = raw.as_bytes();
    let len = bytes.len();
    for split in (num_end + 1)..=len {
        if split >= len || !is_ws(bytes[split]) {
            continue;
        }
        let mut j = split;
        while j < len && is_ws(bytes[j]) {
            j += 1;
        }
        if let Some((page, after)) = match_page_from(raw, j) {
            let mut k = after;
            while k < len && is_ws(bytes[k]) {
                k += 1;
            }
            if k == len {
                return Some((number.clone(), raw[num_end..split].to_string(), page));
            }
        }
    }
    None
}

/// `parse_toc_line`.
pub fn parse_toc_line(text: &str) -> Option<TocEntry> {
    let raw = normalize_ws(text);
    if raw.is_empty() {
        return None;
    }
    if let Some((number, title, page)) = match_toc_with_leader(&raw) {
        return finish_toc(number, title, page);
    }
    if let Some((number, title, page)) = match_toc_numbered(&raw) {
        return finish_toc(number, title, page);
    }
    None
}

fn finish_toc(number: String, title: String, page: String) -> Option<TocEntry> {
    let title = title.trim_matches(|c| c == ' ' || c == '.' || c == '\t').to_string();
    let page_label = page.trim().to_string();
    if title.is_empty() || page_label.is_empty() {
        return None;
    }
    let number = number.trim().to_string();
    let level = if number.is_empty() { 1 } else { toc_level(&number) };
    Some(TocEntry {
        number,
        title,
        page_label,
        level,
    })
}

/// `_coerce_bbox`.
pub fn coerce_bbox(value: &Value) -> Option<[f64; 4]> {
    let arr = value.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let mut out = [0.0; 4];
    for (i, v) in arr.iter().enumerate() {
        out[i] = v.as_f64()?;
    }
    if out[2] <= out[0] || out[3] <= out[1] {
        return None;
    }
    Some(out)
}

/// `_line_bbox` with the spans fallback (document_schema semantics).
pub fn line_bbox(line: &Value) -> Option<[f64; 4]> {
    if !line.is_object() {
        return None;
    }
    if let Some(bbox) = line.get("bbox").and_then(coerce_bbox) {
        return Some(bbox);
    }
    let span_boxes: Vec<[f64; 4]> = line
        .get("spans")
        .and_then(|v| v.as_array())
        .map(|spans| {
            spans
                .iter()
                .filter_map(|span| span.get("bbox").and_then(coerce_bbox))
                .collect()
        })
        .unwrap_or_default();
    if span_boxes.is_empty() {
        return None;
    }
    let x0 = span_boxes.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
    let y0 = span_boxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
    let x1 = span_boxes.iter().map(|b| b[2]).fold(f64::NEG_INFINITY, f64::max);
    let y1 = span_boxes.iter().map(|b| b[3]).fold(f64::NEG_INFINITY, f64::max);
    Some([x0, y0, x1, y1])
}

fn column_groups(mut indexed: Vec<(usize, String, [f64; 4])>) -> Vec<Vec<(usize, String, [f64; 4])>> {
    indexed.sort_by(|a, b| {
        (a.2[0], a.2[1], a.0)
            .partial_cmp(&(b.2[0], b.2[1], b.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut columns: Vec<Vec<(usize, String, [f64; 4])>> = Vec::new();
    for item in &indexed {
        let center_x = (item.2[0] + item.2[2]) / 2.0;
        let mut best_index: Option<usize> = None;
        let mut best_distance = f64::INFINITY;
        for (idx, column) in columns.iter().enumerate() {
            let column_centers: Vec<f64> = column.iter().map(|e| (e.2[0] + e.2[2]) / 2.0).collect();
            let column_center = column_centers.iter().sum::<f64>() / column_centers.len() as f64;
            let distance = (center_x - column_center).abs();
            if distance < best_distance {
                best_distance = distance;
                best_index = Some(idx);
            }
        }
        match best_index {
            Some(idx) if best_distance <= MIN_COLUMN_GAP_PT => columns[idx].push(item.clone()),
            _ => columns.push(vec![item.clone()]),
        }
    }
    columns.sort_by(|a, b| {
        let min_a = a.iter().map(|e| e.2[0]).fold(f64::INFINITY, f64::min);
        let min_b = b.iter().map(|e| e.2[0]).fold(f64::INFINITY, f64::min);
        min_a.partial_cmp(&min_b).unwrap_or(std::cmp::Ordering::Equal)
    });
    columns
}

/// `order_toc_lines_by_geometry`: group lines into columns, order each column
/// top-down, emit left-to-right; lines without a bbox fall back at the end.
pub fn order_toc_lines_by_geometry(lines: &Value, line_texts: &Value) -> Vec<(usize, String)> {
    let texts = line_texts.as_array();
    let lines_arr = lines.as_array();
    let mut indexed: Vec<(usize, String, [f64; 4])> = Vec::new();
    let mut fallback: Vec<(usize, String)> = Vec::new();
    if let Some(texts) = texts {
        for (index, text_val) in texts.iter().enumerate() {
            let text = text_val.as_str().unwrap_or("").to_string();
            let line = lines_arr.and_then(|arr| arr.get(index)).cloned().unwrap_or(Value::Null);
            match line_bbox(&line) {
                Some(bbox) => indexed.push((index, text, bbox)),
                None => fallback.push((index, text)),
            }
        }
    }
    if indexed.is_empty() {
        return fallback;
    }
    let mut ordered: Vec<(usize, String)> = Vec::new();
    for mut column in column_groups(indexed) {
        column.sort_by(|a, b| {
            (a.2[1], a.2[0], a.0)
                .partial_cmp(&(b.2[1], b.2[0], b.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (index, text, _bbox) in column {
            ordered.push((index, text));
        }
    }
    ordered.extend(fallback);
    ordered
}

/// `build_toc_entries`: parse ordered lines into toc-entry dicts with
/// `line_index` / `order_index` / optional `bbox`.
pub fn build_toc_entries(lines: &Value, line_texts: &Value) -> Vec<Value> {
    let ordered = order_toc_lines_by_geometry(lines, line_texts);
    let mut entries: Vec<Value> = Vec::new();
    for (order, (index, text)) in ordered.iter().enumerate() {
        let Some(parsed) = parse_toc_line(text) else {
            continue;
        };
        let line = lines
            .as_array()
            .and_then(|arr| arr.get(*index))
            .cloned()
            .unwrap_or(Value::Null);
        let mut obj = serde_json::Map::new();
        obj.insert("number".into(), Value::String(parsed.number));
        obj.insert("title".into(), Value::String(parsed.title));
        obj.insert("page_label".into(), Value::String(parsed.page_label));
        obj.insert("level".into(), Value::from(parsed.level));
        obj.insert("line_index".into(), Value::from(*index as i64));
        obj.insert("order_index".into(), Value::from(order as i64));
        if let Some(bbox) = line_bbox(&line) {
            obj.insert("bbox".into(), Value::from(bbox.to_vec()));
        }
        entries.push(Value::Object(obj));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_numbered_title_page_without_leader() {
        let parsed = parse_toc_line("3 Thermochemistry output from Gaussian 8").unwrap();
        assert_eq!(parsed.number, "3");
        assert_eq!(parsed.title, "Thermochemistry output from Gaussian");
        assert_eq!(parsed.page_label, "8");
        assert_eq!(parsed.level, 1);
    }

    #[test]
    fn parses_dot_leader_and_numbered_levels() {
        let parsed = parse_toc_line("2.1 Contributions from translation ..... 3").unwrap();
        assert_eq!(parsed.number, "2.1");
        assert_eq!(parsed.title, "Contributions from translation");
        assert_eq!(parsed.page_label, "3");
        assert_eq!(parsed.level, 2);
    }

    #[test]
    fn rejects_ordinary_sentence() {
        assert!(parse_toc_line("The final energy is evaluated in 2 steps").is_none());
    }

    #[test]
    fn rejects_title_without_leading_number_and_page() {
        assert!(parse_toc_line("Thermochemistry output from Gaussian 8").is_none());
        assert!(parse_toc_line("Chapter 1 Introduction 5").is_none());
    }

    #[test]
    fn orders_two_column_toc_by_geometry() {
        let line_texts = json!([
            "1 Left first 1", "4 Right first 4", "2 Left second 2",
            "5 Right second 5", "3 Left third 3", "6 Right third 6",
        ]);
        let lines = json!([
            {"bbox": [50.0, 100.0, 240.0, 112.0]},
            {"bbox": [300.0, 100.0, 520.0, 112.0]},
            {"bbox": [50.0, 120.0, 240.0, 132.0]},
            {"bbox": [300.0, 120.0, 520.0, 132.0]},
            {"bbox": [50.0, 140.0, 240.0, 152.0]},
            {"bbox": [300.0, 140.0, 520.0, 152.0]},
        ]);
        let ordered = order_toc_lines_by_geometry(&lines, &line_texts);
        let indices: Vec<usize> = ordered.iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, vec![0, 2, 4, 1, 3, 5]);
        let entries = build_toc_entries(&lines, &line_texts);
        let numbers: Vec<&str> = entries
            .iter()
            .map(|e| e.get("number").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(numbers, vec!["1", "2", "3", "4", "5", "6"]);
        assert_eq!(entries[0].get("line_index").unwrap().as_i64(), Some(0));
        assert_eq!(entries[5].get("order_index").unwrap().as_i64(), Some(5));
    }

    #[test]
    fn builds_entries_with_mixed_leader_and_plain_lines() {
        let line_texts = json!([
            "1 Introduction 2",
            "2 Sources of components for thermodynamic quantities 2",
            "2.1 Contributions from translation ..... 3",
            "3.2 Output from compound model chemistries 11",
            "5 Summary 17",
        ]);
        let lines = json!([
            {"bbox": [10.0, 20.0, 200.0, 30.0]},
            {"bbox": [10.0, 30.0, 200.0, 40.0]},
            {"bbox": [10.0, 40.0, 200.0, 50.0]},
            {"bbox": [10.0, 50.0, 200.0, 60.0]},
            {"bbox": [10.0, 60.0, 200.0, 70.0]},
        ]);
        let entries = build_toc_entries(&lines, &line_texts);
        let numbers: Vec<&str> = entries
            .iter()
            .map(|e| e.get("number").unwrap().as_str().unwrap())
            .collect();
        let pages: Vec<&str> = entries
            .iter()
            .map(|e| e.get("page_label").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(numbers, vec!["1", "2", "2.1", "3.2", "5"]);
        assert_eq!(pages, vec!["2", "2", "3", "11", "17"]);
        assert_eq!(entries[4].get("bbox").unwrap(), &json!([10.0, 60.0, 200.0, 70.0]));
    }
}
