//! N11d: native `prepare_translated_pages_for_render` —
//! (`book_support.prepare_translated_pages_for_render`): build the per-page
//! metrics, derive the first-line-indent lookup over the RAW source PDF
//! (`spec.inputs.source_pdf`), run the C3-N7 prepare boundary, then the C3-N8
//! policy boundary. The delegate passes `first_line_indent_lookup=None` and
//! `effective_inner_bbox_lookup=None`, so candidates are always derived from the
//! source PDF (never a passthrough lookup), and the lookup never comes back
//! `None` in the bundle path (empty candidate set yields `{}`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use rendering_core::first_line_indent::is_first_line_indent_candidate;
use rendering_core::item::Item;
use rendering_core::layout::policy_fields::apply_render_pages_policy_fields;
use rendering_core::payload::block_seed_metrics::block_metrics;
use rendering_core::payload::prepare::{
    build_page_metrics, prepare_render_payloads_by_page, PageMetrics,
};
use serde_json::Value;

use super::render_source::normalize_source_cleanup_strategy;

/// `book_support.prepare_translated_pages_for_render`: prepare boundary with the
/// derived first-line-indent lookup, then the policy boundary. The policy's
/// `use_typst_fill_cleanup` mirrors `layout.use_typst_fill_cleanup()`: the
/// global is only overridden when the spec sets `source_cleanup_strategy`, so an
/// absent param keeps the module default `pikepdf_text_strip` (false).
/// `use_default_text_overlay_cover_fill` is always the module default `True`.
pub fn prepare_translated_pages_for_render(
    source_pdf_path: &Path,
    translated_pages: &BTreeMap<i64, Vec<Value>>,
    source_cleanup_strategy: Option<&str>,
) -> Result<BTreeMap<i64, Vec<Value>>> {
    let page_metrics = build_page_metrics(translated_pages);
    let indent_lookup =
        resolve_first_line_indent_lookup(source_pdf_path, translated_pages, &page_metrics)?;
    let prepared = prepare_render_payloads_by_page(translated_pages, indent_lookup.as_ref(), None);
    Ok(apply_render_pages_policy_fields(
        &prepared,
        use_typst_fill_cleanup(source_cleanup_strategy),
        true,
    ))
}

/// `layout.use_typst_fill_cleanup()` after `apply_layout_tuning` with the spec's
/// raw `source_cleanup_strategy`: None keeps the module default
/// `pikepdf_text_strip`, so only an explicit typst_fill-normalized value yields
/// true.
fn use_typst_fill_cleanup(source_cleanup_strategy: Option<&str>) -> bool {
    source_cleanup_strategy
        .map(|raw| normalize_source_cleanup_strategy(raw) == "typst_fill")
        .unwrap_or(false)
}

/// `prepare._resolve_first_line_indent_lookup` with `source_pdf_path` always set
/// and `first_line_indent_lookup=None`: per page gate each item via
/// `block_metrics` + `is_first_line_indent_candidate`, then run the batched
/// reader detection over the source PDF. Returns `Some({})` when no page has
/// candidates (the bundle path never returns `None`).
fn resolve_first_line_indent_lookup(
    source_pdf_path: &Path,
    translated_pages: &BTreeMap<i64, Vec<Value>>,
    page_metrics: &BTreeMap<i64, PageMetrics>,
) -> Result<Option<BTreeMap<String, f64>>> {
    let mut by_page: BTreeMap<i64, (f64, Vec<(String, [f64; 4], f64)>)> = BTreeMap::new();
    for (&page_idx, items) in translated_pages {
        let Some([page_font_size, page_line_pitch, page_line_height, density_baseline, page_text_width_med]) =
            page_metrics.get(&page_idx)
        else {
            continue;
        };
        let mut candidates: Vec<(String, [f64; 4], f64)> = Vec::new();
        for item in items {
            let item_id = py_item_id(item);
            if item_id.is_empty() {
                continue;
            }
            let typed = Item::from_json_value(item);
            let (font_size_pt, _leading_em) = block_metrics(
                &typed,
                *page_font_size,
                *page_line_pitch,
                *page_line_height,
                *density_baseline,
                *page_text_width_med,
            );
            if is_first_line_indent_candidate(&typed, *page_text_width_med) {
                // The gate requires a 4-number bbox, so this is always Some; the
                // zeros fallback mirrors the shim's [0,0,0,0] bbox path.
                candidates.push((item_id, typed.bbox.unwrap_or([0.0; 4]), font_size_pt));
            }
        }
        if !candidates.is_empty() {
            by_page.insert(page_idx, (*page_text_width_med, candidates));
        }
    }
    if by_page.is_empty() {
        return Ok(Some(BTreeMap::new()));
    }

    let doc = mupdf::Document::open(source_pdf_path).map_err(|e| {
        anyhow!(
            "open source pdf for first-line-indent detection {}: {e}",
            source_pdf_path.display()
        )
    })?;
    let page_indices: Vec<i64> = by_page.keys().copied().collect();
    let parsed: BTreeMap<i64, Vec<([f64; 4], f64)>> = by_page
        .iter()
        .map(|(&page_idx, (_width, candidates))| {
            (
                page_idx,
                candidates
                    .iter()
                    .map(|(_, bbox, font_size_pt)| (*bbox, *font_size_pt))
                    .collect(),
            )
        })
        .collect();
    let raw = rendering_reader::indent::detect_first_line_indents(&doc, &page_indices, &parsed);

    // `_native.detect_first_line_indents` maps per-candidate `None` (unreadable
    // page / failed clip) to `0.0`, matching the reference.
    let mut lookup: BTreeMap<String, f64> = BTreeMap::new();
    for (page_idx, (_width, candidates)) in &by_page {
        let Some(page_out) = raw.get(page_idx) else {
            continue;
        };
        for (index, (item_id, _bbox, _font_size_pt)) in candidates.iter().enumerate() {
            let indent_pt = page_out.get(index).copied().flatten().unwrap_or(0.0);
            lookup.insert(item_id.clone(), indent_pt);
        }
    }
    Ok(Some(lookup))
}

/// Python `str(item.get("item_id", "") or "")` — falsy scalars (missing, "",
/// 0, false) become "".
pub(crate) fn py_item_id(item: &Value) -> String {
    match item.get("item_id") {
        Some(v) if value_falsy(v) => String::new(),
        Some(v) => py_str(v),
        None => String::new(),
    }
}

/// Python `bool(x)` over scalar dict values (used for the `or ""` gate).
pub(crate) fn value_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f == 0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// Python `str(v)` for scalar dict values: strings pass through, numbers and
/// bools stringify, null/absent become "".
pub(crate) fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use serde_json::json;

    const GOLDEN_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/samples/golden-pdfs"
    );

    fn paragraph_item(item_id: &str, page_idx: i64, bbox: [f64; 4]) -> Value {
        json!({
            "item_id": item_id,
            "page_idx": page_idx,
            "block_type": "text",
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "bbox": bbox,
            "source_text": "source text",
            "translated_text": "译文",
            "protected_source_text": "source text",
            "protected_translated_text": "译文",
            "should_translate": true,
            "final_status": "translated",
            "translation_unit_id": item_id,
            "translation_unit_kind": "single",
            "translation_unit_protected_source_text": "source text",
            "translation_unit_protected_translated_text": "译文",
            "translation_unit_formula_map": [],
            "formula_map": [],
            "protected_map": [],
            "continuation_group": "",
            "group_protected_source_text": "",
            "group_formula_map": [],
            "group_protected_translated_text": "",
            "group_translated_text": "",
            "lines": [{"bbox": bbox, "spans": [{"type": "text", "content": "source text", "bbox": bbox}]}],
        })
    }

    #[test]
    fn use_typst_fill_cleanup_matches_layout_flag() {
        assert!(use_typst_fill_cleanup(Some("typst_fill")));
        assert!(use_typst_fill_cleanup(Some(" typst_fill ")));
        assert!(!use_typst_fill_cleanup(Some("pikepdf_text_strip")));
        assert!(!use_typst_fill_cleanup(Some("redact_restore_formulas")));
        // None keeps the module default SOURCE_CLEANUP_STRATEGY ("pikepdf_text_strip").
        assert!(!use_typst_fill_cleanup(None));
    }

    #[test]
    fn empty_pages_short_circuit() {
        let pages = BTreeMap::new();
        let result = prepare_translated_pages_for_render(
            Path::new("/nonexistent.pdf"),
            &pages,
            Some("typst_fill"),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn derives_lookup_from_source_pdf() {
        let source = Path::new(GOLDEN_ROOT).join("2.pdf");
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        pages.insert(0, vec![paragraph_item("p001-b001", 0, [40.0, 40.0, 400.0, 100.0])]);
        let page_metrics = build_page_metrics(&pages);
        let lookup =
            resolve_first_line_indent_lookup(&source, &pages, &page_metrics).unwrap().unwrap();
        let indent_pt = lookup["p001-b001"];
        assert!(indent_pt.is_finite() && indent_pt >= 0.0, "indent {indent_pt}");
    }

    #[test]
    fn non_candidate_items_yield_empty_lookup() {
        // A caption-like item never passes the gate -> by_page empty -> Some({}).
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        let mut item = paragraph_item("p001-b001", 0, [40.0, 40.0, 400.0, 100.0]);
        item["layout_role"] = json!("caption");
        pages.insert(0, vec![item]);
        let page_metrics = build_page_metrics(&pages);
        let lookup =
            resolve_first_line_indent_lookup(Path::new("/nonexistent.pdf"), &pages, &page_metrics)
                .unwrap()
                .unwrap();
        assert!(lookup.is_empty());
    }
}
