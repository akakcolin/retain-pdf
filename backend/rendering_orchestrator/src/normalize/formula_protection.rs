// Port of `translation/core/payload/formula_protection.py::protect_inline_formulas`
// for the glossary=None path (the only path the normalize worker exercises). Emits
// the forward protection step — inline formula spans replaced by `<fN-xxx/>` tags —
// so the paddle/pseudo line rebuild can split text/formula segments. The wrap /
// re-protect helpers already live in `rendering_core::inline_content::protected_tokens`.

use std::collections::HashMap;
use std::sync::OnceLock;

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2sVar;
use regex::Regex;
use serde_json::{json, Value};

/// `LATEX_FORMULA_RE` (re.VERBOSE) — a LaTeX-ish token run.
fn latex_formula_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"((?:\\[A-Za-z]+|[A-Za-z])(?:\s*(?:_\s*\{[^{}]*\}|\^\s*\{[^{}]*\}|_\s*[A-Za-z0-9]|\^\s*[A-Za-z0-9]|\{[^{}]*\}|\([^()]*\)|\[[^\[\]]*\]|[=+\-−*/<>.,]|[A-Za-z0-9]|\\[A-Za-z]+))+)")
            .expect("latex formula regex")
    })
}

/// `GREEK_RUN_RE` (re.VERBOSE) — a Greek-symbol run.
fn greek_run_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"((?:\\[A-Za-z]+|[α-ωΑ-Ωωγβμφαζη∂])(?:\s*(?:_\s*\{[^{}]*\}|\^\s*\{[^{}]*\}|[A-Za-z0-9]|\\[A-Za-z]+))*)")
            .expect("greek run regex")
    })
}

/// `GREEK_COMMA_PAIR_RE`. `\alpha`/`\beta` keep Python's escape interpretation
/// (`\a` = BEL, `\b` = word boundary) so the pattern never matches organic text.
fn greek_comma_pair_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:\alpha|α)\s*(?:\{\s*,\s*\}|,)\s*(?:\beta|β)(?:\s*-\s*[A-Za-z]+)?$")
            .expect("greek comma pair regex")
    })
}

/// `SIMPLE_DISPLAY_COMMAND_RE` — `\mathrm{...}` style wrappers to unwrap.
fn simple_display_command_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\\(?:mathrm|mathit|mathbf|mathcal|text)\s*\{\s*([^{}]+?)\s*\}")
            .expect("simple display command regex")
    })
}

fn standalone_greek_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:\\[A-Za-z]+|[α-ωΑ-Ωωγβμφαζη∂])$").expect("standalone greek regex")
    })
}

fn short_bond_like_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z]{1,3}-[A-Za-z]{1,3}$").expect("short bond regex"))
}

fn citationish_pseudo_formula_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:\d+\s*[A-Za-z]|[A-Za-z])(?:\s*,\s*(?:\d+\s*[A-Za-z]|[A-Za-z])){2,}$")
            .expect("citationish pseudo formula regex")
    })
}

fn prose_heavy_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z]{3,}").expect("prose heavy word regex"))
}

fn reference_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:\d+\s*[A-Za-z](?:\s*-\s*[A-Za-z])?|[A-Za-z](?:\s*-\s*[A-Za-z])?)$")
            .expect("reference token regex")
    })
}

/// `PROTECTED_TOKEN_RE` — the 3-way token alternation used to re-split text.
pub(crate) fn protected_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"<[futnvc]\d+-[0-9a-z]{3}/>|\[\[FORMULA_\d+\]\]|@@F\d+@@")
            .expect("protected token regex")
    })
}

const TOKEN_TYPE_PREFIX: [(&str, &str); 6] = [
    ("formula", "f"),
    ("term", "t"),
    ("unit", "u"),
    ("numeric", "n"),
    ("variable", "v"),
    ("citation", "c"),
];

/// `_prepare_text`: insert a space after `}`/`]` that is glued to a word start.
fn prepare_text(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"([}\]])([A-Za-z][a-z]{2,})").expect("prose boundary regex"));
    re.replace_all(text, "$1 $2").into_owned()
}

/// `_checksum`: blake2s(f"{token_type}\0{value}", digest_size=2).hexdigest()[:3].
fn checksum(value: &str, token_type: &str) -> String {
    let mut hasher = Blake2sVar::new(2).expect("blake2s output size 2");
    hasher.update(format!("{token_type}\0{value}").as_bytes());
    let mut out = [0u8; 2];
    hasher.finalize_variable(&mut out).expect("hash output fits");
    let hex = format!("{:02x}{:02x}", out[0], out[1]);
    hex[..3].to_string()
}

fn token_tag(token_type: &str, index: usize, checksum: &str) -> String {
    let prefix = TOKEN_TYPE_PREFIX
        .iter()
        .find(|(t, _)| *t == token_type)
        .map(|(_, p)| *p)
        .unwrap_or("f");
    format!("<{prefix}{index}-{checksum}/>")
}

/// `_unwrap_display_commands` — repeatedly unwrap `\mathrm{...}` etc.
fn unwrap_display_commands(value: &str) -> String {
    let mut previous = value.to_string();
    loop {
        let replaced = simple_display_command_re().replace_all(&previous, "$1").into_owned();
        if replaced == previous {
            return replaced;
        }
        previous = replaced;
    }
}

/// `_normalize_formula_candidate`.
fn normalize_formula_candidate(value: &str) -> String {
    let text = unwrap_display_commands(value.trim());
    let text = text.replace('{', "").replace('}', "");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn looks_like_standalone_greek_symbol(value: &str) -> bool {
    let normalized = normalize_formula_candidate(value);
    if normalized
        .chars()
        .any(|c| matches!(c, '_' | '^' | '(' | ')' | '[' | ']' | '+' | '=' | '/'))
    {
        return false;
    }
    standalone_greek_re().is_match(&normalized)
}

fn looks_like_short_bond_token(value: &str) -> bool {
    let normalized = normalize_formula_candidate(value).replace(' ', "");
    if normalized
        .chars()
        .any(|c| matches!(c, '_' | '^' | '+' | '=' | '/' | '*'))
    {
        return false;
    }
    short_bond_like_re().is_match(&normalized)
}

fn looks_like_citationish_pseudo_formula(value: &str) -> bool {
    let normalized = normalize_formula_candidate(value);
    if normalized
        .chars()
        .any(|c| matches!(c, '_' | '^' | '(' | ')' | '[' | ']' | '+' | '=' | '/'))
    {
        return false;
    }
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if citationish_pseudo_formula_re().is_match(&normalized) {
        return true;
    }
    let parts: Vec<&str> = normalized
        .split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 4 {
        return false;
    }
    parts.iter().all(|part| reference_token_re().is_match(part))
}

fn looks_like_prose_heavy_formula_candidate(value: &str) -> bool {
    static COMMAND_RE: OnceLock<Regex> = OnceLock::new();
    let command_re = COMMAND_RE
        .get_or_init(|| Regex::new(r"\\[A-Za-z]+").expect("latex command regex"));
    let command_stripped = command_re.replace_all(value, " ").into_owned();
    let normalized = normalize_formula_candidate(&command_stripped);
    let words: Vec<&str> = prose_heavy_word_re().find_iter(&normalized).map(|m| m.as_str()).collect();
    if words.len() < 4 {
        return false;
    }
    let lowercase_words = words
        .iter()
        .filter(|word| word.chars().any(|c| c.is_lowercase()))
        .count();
    lowercase_words >= 3
}

fn should_skip_formula_candidate(value: &str) -> bool {
    looks_like_prose_heavy_formula_candidate(value)
        || looks_like_citationish_pseudo_formula(value)
        || looks_like_standalone_greek_symbol(value)
        || looks_like_short_bond_token(value)
}

/// `_iter_formula_matches` — every formula candidate from both patterns, filtered.
fn iter_formula_matches(text: &str) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for re in [latex_formula_re(), greek_run_re()] {
        for m in re.find_iter(text) {
            let value = m.as_str().trim().to_string();
            if greek_comma_pair_re().is_match(&value) {
                continue;
            }
            if should_skip_formula_candidate(&value) {
                continue;
            }
            if value.chars().any(|c| {
                matches!(
                    c,
                    '\\' | '_' | '^' | '{' | '}' | 'α' | 'β' | 'γ' | 'μ' | 'φ' | 'ζ' | 'η' | '∂'
                )
            }) {
                out.push((m.start(), m.end(), value));
            }
        }
    }
    out
}

#[derive(Clone)]
struct Span {
    start: usize,
    end: usize,
    token_type: String,
    original_text: String,
    restore_text: String,
}

/// `_collect_formula_spans`.
fn collect_formula_spans(text: &str) -> Vec<Span> {
    let mut raw = iter_formula_matches(text);
    raw.sort_by(|a, b| a.0.cmp(&b.0).then((b.1 - b.0).cmp(&(a.1 - a.0))));
    let mut selected: Vec<Span> = Vec::new();
    let mut cursor = 0usize;
    for (start, end, value) in raw {
        if end <= cursor || start < cursor {
            continue;
        }
        selected.push(Span {
            start,
            end,
            token_type: "formula".to_string(),
            original_text: value.clone(),
            restore_text: value,
        });
        cursor = end;
    }
    selected
}

fn overlaps_any(span: &Span, selected: &[Span]) -> bool {
    selected
        .iter()
        .any(|existing| span.start < existing.end && span.end > existing.start)
}

/// `_protect_spans` — overlap-resolve, tokenize, build the protected text.
fn protect_spans(text: &str, spans: Vec<Span>) -> (String, Vec<Value>) {
    let mut ordered = spans;
    ordered.sort_by(|a, b| a.start.cmp(&b.start).then((b.end - b.start).cmp(&(a.end - a.start))));
    let mut selected: Vec<Span> = Vec::new();
    for span in ordered {
        if overlaps_any(&span, &selected) {
            continue;
        }
        selected.push(span);
    }
    let mut counters: HashMap<String, usize> = HashMap::new();
    let mut protected_map: Vec<Value> = Vec::new();
    let mut chunks: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for span in selected {
        chunks.push(text[cursor..span.start].to_string());
        let count = counters.entry(span.token_type.clone()).or_insert(0);
        *count += 1;
        let sum = checksum(&span.original_text, &span.token_type);
        let tag = token_tag(&span.token_type, *count, &sum);
        protected_map.push(json!({
            "token_tag": tag,
            "token_type": span.token_type,
            "original_text": span.original_text,
            "restore_text": span.restore_text,
            "source_offset": span.start,
            "checksum": sum,
        }));
        chunks.push(tag);
        cursor = span.end;
    }
    chunks.push(text[cursor..].to_string());
    (chunks.concat(), protected_map)
}

/// `_formula_map_from_protected_map` — only the `formula`-type entries.
fn formula_map_from_protected_map(protected_map: &[Value]) -> Vec<Value> {
    protected_map
        .iter()
        .filter_map(|entry| {
            if !entry.is_object() {
                return None;
            }
            if entry.get("token_type").and_then(Value::as_str) != Some("formula") {
                return None;
            }
            let placeholder = entry.get("token_tag").and_then(Value::as_str).unwrap_or("");
            let formula_text = entry
                .get("restore_text")
                .or_else(|| entry.get("original_text"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if placeholder.is_empty() || formula_text.is_empty() {
                return None;
            }
            Some(json!({ "placeholder": placeholder, "formula_text": formula_text }))
        })
        .collect()
}

/// `protect_inline_formulas(text, glossary_entries=None)` — forward formula
/// protection: returns `(protected_text, formula_map)`.
pub fn protect_inline_formulas(text: &str) -> (String, Vec<Value>) {
    let prepared = prepare_text(text);
    let spans = collect_formula_spans(&prepared);
    let (protected_text, protected_map) = protect_spans(&prepared, spans);
    let formula_map = formula_map_from_protected_map(&protected_map);
    (protected_text, formula_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_latex_formula() {
        let (protected, formula_map) = protect_inline_formulas(r"值 \alpha + \beta 中");
        assert!(protected.contains("<f1-"));
        assert_eq!(formula_map.len(), 1);
        let first = &formula_map[0];
        let placeholder = first["placeholder"].as_str().unwrap();
        assert!(protected.contains(placeholder));
        assert!(first["formula_text"].as_str().unwrap().contains('\\'));
    }

    #[test]
    fn standalone_latex_command_is_skipped() {
        // Python reference: a lone `\alpha` or `\frac{a}{b}` (brace-collapsed to a
        // standalone command) is treated as an inline symbol and NOT protected.
        let (protected, formula_map) = protect_inline_formulas(r"值 \alpha 与 \frac{a}{b} 中");
        assert!(formula_map.is_empty());
        assert_eq!(protected, r"值 \alpha 与 \frac{a}{b} 中");
    }

    #[test]
    fn empty_or_plain_text_is_unchanged() {
        let (protected, formula_map) = protect_inline_formulas("普通中文段落 without formulas");
        assert_eq!(protected, "普通中文段落 without formulas");
        assert!(formula_map.is_empty());
    }

    #[test]
    fn checksum_is_stable() {
        assert_eq!(checksum("x", "formula"), checksum("x", "formula"));
        assert_ne!(checksum("x", "formula"), checksum("y", "formula"));
    }

    #[test]
    fn prepare_text_glues_boundary() {
        assert_eq!(prepare_text("a]word"), "a] word");
        assert_eq!(prepare_text("no boundary"), "no boundary");
    }
}
