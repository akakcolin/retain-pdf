//! N11b: whole-document render analysis + auto-mode resolution.
//!
//! Mirrors the native analysis the bridge exposes (`build_render_document_analysis`:
//! `page_snapshot` -> `rendering_core::profile_build::build_render_page_profile` ->
//! `rendering_core::document_builder::build_render_page_analysis`) so the
//! orchestrator can resolve `auto` render mode and (from N11c on) drive
//! render-source prep without spawning python3. The derived page-index sets port
//! `contracts/document_analysis.py` (`RenderDocumentAnalysis` properties), and
//! `resolve_effective_render_mode` ports `runtime/pipeline/render_mode.py`
//! (`is_pseudo_editable_scan_analysis`/`is_editable_analysis`, sampled on the
//! leading 3 pages). Only the analysis-backed auto branch is reachable natively —
//! `build_bundle` always computes the analysis before resolving, so the
//! `_sample_pdf_analysis` fallback (which re-opens the PDF for a 3-page sample)
//! is dead code here.
//!
//! Same `render_document_profile_v1` algorithm and the same empty-`text_traces`
//! fallback as the bridge: mupdf-rs exposes no text-trace opacity signal, so
//! `visible_text`/`editable_text` fall back to `word_count >= 20` and
//! `hidden_text` is false. The routing fields (`kind`, `redaction`, and
//! everything derived from them) stay parity.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{anyhow, Result};
use rendering_core::document_builder::{build_render_page_analysis, RenderPageAnalysis};
use rendering_core::profile::RenderPageKind;
use rendering_core::profile_build::{build_render_page_profile, DEFAULT_BACKGROUND_THRESHOLD};
use rendering_reader::{open_document, PdfDocument};
use serde_json::{json, Value};

pub const RENDER_DOCUMENT_PROFILE_ALGORITHM_VERSION: &str = "render_document_profile_v1";

/// `RenderDocumentAnalysis` — pages keyed by page index (sorted; the auto-mode
/// sampling takes the leading 3 in page order, matching Python dict insertion
/// order).
pub struct RenderDocumentAnalysis {
    pub pages: BTreeMap<i64, RenderPageAnalysis>,
}

impl RenderDocumentAnalysis {
    /// `pikepdf_text_strip_page_indices`: `allows_pikepdf_text_strip`.
    pub fn pikepdf_text_strip_page_indices(&self) -> BTreeSet<i64> {
        self.filter_pages(|p| {
            p.redaction == "text_layer_only" && p.kind == RenderPageKind::EditableText
        })
    }

    /// `visual_cover_page_indices`: `needs_visual_cover`.
    pub fn visual_cover_page_indices(&self) -> BTreeSet<i64> {
        self.filter_pages(|p| {
            matches!(p.redaction, "visual_cover" | "visual_cover_and_remove_text")
        })
    }

    /// `hidden_text_strip_page_indices`: `needs_hidden_text_strip`.
    pub fn hidden_text_strip_page_indices(&self) -> BTreeSet<i64> {
        self.filter_pages(|p| p.redaction == "visual_cover_and_remove_text")
    }

    /// `route_reason_counts`: counts by `page.kind` string.
    pub fn route_reason_counts(&self) -> BTreeMap<String, i64> {
        let mut counts: BTreeMap<String, i64> = BTreeMap::new();
        for page in self.pages.values() {
            *counts.entry(page.kind.as_str().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// Bridge-shaped manifest (`{"algorithm", "pages": [...]}` in page-index
    /// order, `background_coverage_ratio` rounded to 6 dp) — the differential
    /// check surface against the native bridge output.
    pub fn to_manifest(&self) -> Value {
        let mut pages: Vec<Value> = self
            .pages
            .iter()
            .map(|(_, page)| {
                json!({
                    "page_index": page.page_index,
                    "kind": page.kind.as_str(),
                    "redaction": page.redaction,
                    "background": page.background,
                    "compose": page.compose,
                    "layout": page.layout,
                    "reason": page.reason,
                    "has_large_background": page.has_large_background,
                    "background_coverage_ratio":
                        (page.background_coverage_ratio * 1_000_000.0).round() / 1_000_000.0,
                    "visible_text": page.visible_text,
                    "hidden_text": page.hidden_text,
                    "editable_text": page.editable_text,
                    "drawing_count": page.drawing_count,
                    "vector_heavy": page.vector_heavy,
                })
            })
            .collect();
        pages.sort_by_key(|p| p["page_index"].as_i64().unwrap_or(i64::MAX));
        json!({
            "algorithm": RENDER_DOCUMENT_PROFILE_ALGORITHM_VERSION,
            "pages": pages,
        })
    }

    fn filter_pages(&self, predicate: impl Fn(&RenderPageAnalysis) -> bool) -> BTreeSet<i64> {
        self.pages
            .iter()
            .filter(|(_, page)| predicate(page))
            .map(|(idx, _)| *idx)
            .collect()
    }
}

/// `document.builder.build_render_document_analysis` (analysis/_native.py
/// `_build_native` config path) — open the source PDF once and walk the selected
/// translated pages (in `[0, page_count)`) through `page_snapshot` ->
/// `build_render_page_profile` -> `build_render_page_analysis`. A page whose
/// snapshot cannot be read raises (mirrors the bridge shim falling back to the
/// Python reference; in the orchestrator there is no reference, so it is an
/// error).
pub fn build_render_document_analysis(
    source_pdf_path: &Path,
    selected_pages: &BTreeMap<i32, Vec<Value>>,
) -> Result<RenderDocumentAnalysis> {
    let doc = open_document(source_pdf_path)
        .map_err(|e| anyhow!("render analysis open {}: {e}", source_pdf_path.display()))?;
    // UFCS: mupdf::Document has an inherent `page_count` returning i32; the
    // reader trait's i64 version is the one used here.
    let page_count = PdfDocument::page_count(&doc).map_err(|e| anyhow!("page_count: {e}"))?;
    let mut pages = BTreeMap::new();
    for (page_idx, items) in selected_pages {
        if *page_idx < 0 || *page_idx as i64 >= page_count {
            continue;
        }
        let snapshot = doc
            .page_snapshot(*page_idx as i64)
            .map_err(|e| anyhow!("page_snapshot p{page_idx}: {e}"))?;
        let profile = build_render_page_profile(
            &snapshot,
            &page_ocr_bboxes(items),
            DEFAULT_BACKGROUND_THRESHOLD,
        );
        pages.insert(*page_idx as i64, build_render_page_analysis(&profile));
    }
    Ok(RenderDocumentAnalysis { pages })
}

/// `_native.build_render_document_analysis` config extraction: `[[float(v) for
/// v in item.get("bbox", [])] for item in translated_pages[page_idx]]`. Items
/// without a clean 4-float bbox are dropped (the Python shim would decode
/// `Vec<[f64; 4]>` and raise on a short array; the corpus never carries one).
fn page_ocr_bboxes(items: &[Value]) -> Vec<[f64; 4]> {
    items.iter().filter_map(item_bbox).collect()
}

fn item_bbox(item: &Value) -> Option<[f64; 4]> {
    let bbox = item.get("bbox")?.as_array()?;
    if bbox.len() != 4 {
        return None;
    }
    let mut out = [0.0f64; 4];
    for (slot, value) in bbox.iter().enumerate() {
        match value {
            Value::Number(n) => out[slot] = n.as_f64().unwrap_or(0.0),
            Value::String(s) => out[slot] = s.trim().parse::<f64>().ok()?,
            _ => return None,
        }
    }
    Some(out)
}

/// `render_mode.is_pseudo_editable_scan_analysis` — sampled on the leading 3
/// pages; at least half (min 1) must be `pseudo_editable_scan`.
pub fn is_pseudo_editable_scan_analysis(analysis: &RenderDocumentAnalysis) -> bool {
    let pages: Vec<&RenderPageAnalysis> = analysis.pages.values().take(3).collect();
    let sampled = pages.len() as i64;
    let pseudo_scan = pages
        .iter()
        .filter(|page| page.kind == RenderPageKind::PseudoEditableScan)
        .count() as i64;
    sampled > 0 && pseudo_scan >= ceil_half(sampled)
}

/// `render_mode.is_editable_analysis` — sampled on the leading 3 pages;
/// `editable_text && kind == "editable_text"` at least half (min 1), unless
/// every sampled page is a pseudo-scan.
pub fn is_editable_analysis(analysis: &RenderDocumentAnalysis) -> bool {
    let pages: Vec<&RenderPageAnalysis> = analysis.pages.values().take(3).collect();
    let sampled = pages.len() as i64;
    let editable = pages
        .iter()
        .filter(|page| page.editable_text && page.kind == RenderPageKind::EditableText)
        .count() as i64;
    let pseudo_scan = pages
        .iter()
        .filter(|page| page.kind == RenderPageKind::PseudoEditableScan)
        .count() as i64;
    if sampled == 0 || pseudo_scan >= sampled {
        return false;
    }
    editable >= ceil_half(sampled)
}

/// `max(1, math.ceil(sampled / 2))` (Python float division + ceiling).
fn ceil_half(sampled: i64) -> i64 {
    std::cmp::max(1, (sampled as f64 / 2.0).ceil() as i64)
}

/// `render_mode.resolve_effective_render_mode` — non-auto passes through; auto
/// resolves to `typst_visual` (pseudo-scan or non-editable) or `overlay`
/// (editable default). The analysis-backed branch always wins natively
/// (`build_bundle` computes the analysis before resolving); `None` analysis
/// falls back to `overlay` (never reached here).
pub fn resolve_effective_render_mode(
    render_mode: &str,
    has_translated_pages: bool,
    document_analysis: Option<&RenderDocumentAnalysis>,
) -> String {
    if render_mode != "auto" {
        return render_mode.to_string();
    }
    if !has_translated_pages {
        return "overlay".to_string();
    }
    match document_analysis {
        Some(analysis) => {
            if is_pseudo_editable_scan_analysis(analysis) || !is_editable_analysis(analysis) {
                "typst_visual".to_string()
            } else {
                "overlay".to_string()
            }
        }
        None => "overlay".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(kind: RenderPageKind, redaction: &'static str, editable_text: bool) -> RenderPageAnalysis {
        RenderPageAnalysis {
            page_index: 0,
            kind,
            redaction,
            background: "source_pdf_page",
            compose: "typst_overlay",
            layout: "ocr_bbox_overlay",
            reason: String::new(),
            has_large_background: false,
            background_coverage_ratio: 0.0,
            visible_text: false,
            hidden_text: false,
            editable_text,
            drawing_count: 0,
            vector_heavy: false,
        }
    }

    fn analysis(pages: Vec<(i64, RenderPageAnalysis)>) -> RenderDocumentAnalysis {
        RenderDocumentAnalysis {
            pages: pages.into_iter().collect(),
        }
    }

    #[test]
    fn derived_sets_match_contracts() {
        let doc = analysis(vec![
            (0, page(RenderPageKind::EditableText, "text_layer_only", true)),
            (1, page(RenderPageKind::ScanImage, "visual_cover", false)),
            (2, page(RenderPageKind::EditableText, "visual_cover_and_remove_text", true)),
            (3, page(RenderPageKind::EditableText, "text_layer_only", true)),
        ]);
        assert_eq!(doc.pikepdf_text_strip_page_indices(), BTreeSet::from([0, 3]));
        assert_eq!(doc.visual_cover_page_indices(), BTreeSet::from([1, 2]));
        assert_eq!(doc.hidden_text_strip_page_indices(), BTreeSet::from([2]));
        let counts = doc.route_reason_counts();
        assert_eq!(counts.get("editable_text"), Some(&3));
        assert_eq!(counts.get("scan_image"), Some(&1));
    }

    #[test]
    fn pseudo_scan_threshold_ceil_half() {
        // 3 sampled, ceil(3/2)=2 → needs >=2.
        let doc = analysis(vec![
            (0, page(RenderPageKind::PseudoEditableScan, "visual_cover", false)),
            (1, page(RenderPageKind::PseudoEditableScan, "visual_cover", false)),
            (2, page(RenderPageKind::ScanImage, "visual_cover", false)),
        ]);
        assert!(is_pseudo_editable_scan_analysis(&doc));
        // 3 sampled, only 1 pseudo-scan → false.
        let doc2 = analysis(vec![
            (0, page(RenderPageKind::PseudoEditableScan, "visual_cover", false)),
            (1, page(RenderPageKind::ScanImage, "visual_cover", false)),
            (2, page(RenderPageKind::ScanImage, "visual_cover", false)),
        ]);
        assert!(!is_pseudo_editable_scan_analysis(&doc2));
    }

    #[test]
    fn editable_requires_editable_kind_and_half() {
        // 2 sampled, ceil(2/2)=1 → needs >=1 editable_text && kind editable_text.
        let doc = analysis(vec![
            (0, page(RenderPageKind::EditableText, "text_layer_only", true)),
            (1, page(RenderPageKind::ScanImage, "visual_cover", false)),
        ]);
        assert!(is_editable_analysis(&doc));
        // editable_text=true but kind not editable_text → not counted (2 pages,
        // 0 qualifying, so 0 >= 1 is false).
        let doc2 = analysis(vec![
            (0, page(RenderPageKind::ScanImage, "visual_cover", true)),
            (1, page(RenderPageKind::ScanImage, "visual_cover", true)),
        ]);
        assert!(!is_editable_analysis(&doc2));
        // all pseudo-scan → false regardless of editable count.
        let doc3 = analysis(vec![
            (0, page(RenderPageKind::PseudoEditableScan, "visual_cover", true)),
            (1, page(RenderPageKind::PseudoEditableScan, "visual_cover", true)),
        ]);
        assert!(!is_editable_analysis(&doc3));
    }

    #[test]
    fn auto_resolution_passes_through_and_resolves() {
        assert_eq!(resolve_effective_render_mode("typst", true, None), "typst");
        assert_eq!(
            resolve_effective_render_mode("typst_visual", true, None),
            "typst_visual"
        );
        // No translated pages map → overlay.
        assert_eq!(resolve_effective_render_mode("auto", false, None), "overlay");
        // Non-editable analysis → typst_visual.
        let non_editable = analysis(vec![(0, page(RenderPageKind::ScanImage, "visual_cover", false))]);
        assert_eq!(
            resolve_effective_render_mode("auto", true, Some(&non_editable)),
            "typst_visual"
        );
        // Editable analysis → overlay.
        let editable = analysis(vec![
            (0, page(RenderPageKind::EditableText, "text_layer_only", true)),
            (1, page(RenderPageKind::EditableText, "text_layer_only", true)),
        ]);
        assert_eq!(
            resolve_effective_render_mode("auto", true, Some(&editable)),
            "overlay"
        );
    }

    #[test]
    fn ocr_bboxes_skips_non_clean_items() {
        let items = serde_json::from_str::<Vec<Value>>(
            r#"[
                {"bbox": [1, 2, 3, 4]},
                {"bbox": [1.5, 2.5, 3.5, 4.5]},
                {"bbox": []},
                {"bbox": [1, 2]},
                {"no_bbox": true},
                {"bbox": ["1", "2", "3", "4"]}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            page_ocr_bboxes(&items),
            vec![[1.0, 2.0, 3.0, 4.0], [1.5, 2.5, 3.5, 4.5], [1.0, 2.0, 3.0, 4.0]]
        );
    }

    #[test]
    fn manifest_sorts_pages_and_rounds_ratio() {
        let mut pages = BTreeMap::new();
        pages.insert(
            1,
            RenderPageAnalysis {
                page_index: 1,
                kind: RenderPageKind::EditableText,
                redaction: "text_layer_only",
                background: "source_pdf_page",
                compose: "typst_overlay",
                layout: "ocr_bbox_overlay",
                reason: "x".to_string(),
                has_large_background: false,
                background_coverage_ratio: 0.123456789,
                visible_text: true,
                hidden_text: false,
                editable_text: true,
                drawing_count: 0,
                vector_heavy: false,
            },
        );
        pages.insert(
            0,
            RenderPageAnalysis {
                page_index: 0,
                kind: RenderPageKind::ScanImage,
                redaction: "visual_cover",
                background: "image_background",
                compose: "typst_background",
                layout: "ocr_bbox_overlay",
                reason: "y".to_string(),
                has_large_background: true,
                background_coverage_ratio: 0.5,
                visible_text: false,
                hidden_text: false,
                editable_text: false,
                drawing_count: 0,
                vector_heavy: false,
            },
        );
        let doc = RenderDocumentAnalysis { pages };
        let manifest = doc.to_manifest();
        assert_eq!(manifest["algorithm"], "render_document_profile_v1");
        let pages = manifest["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0]["page_index"], 0);
        assert_eq!(pages[1]["page_index"], 1);
        assert_eq!(pages[1]["background_coverage_ratio"], json!(0.123457));
    }
}
