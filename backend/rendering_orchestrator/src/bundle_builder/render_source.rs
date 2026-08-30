//! N11c: native `build_render_source_pdf` — the render-source prep chain
//! (`services/rendering/source/render_source.py`): invalid-xobject sanitize ->
//! hidden-text strip -> bbox-text strip -> image compress, producing the
//! bundle's `source_pdf` path and `precleaned_page_indices` (the bbox-strip
//! changed page set). `work_root` is the typst temp root (artifact_mode=False).
//!
//! Hidden-text strip is gated on `document_analysis.hidden_text_strip_page_indices`
//! (empty for the parity corpus, so skipped). When non-empty the candidates are
//! the analysis pseudo-scan set (the fitz `get_texttrace` pre-scan in
//! `_collect_hidden_text_scan_pages` is the hard boundary — the analysis encodes
//! the same pseudo-scan fallback natively, `word_count >= 20`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use mupdf::pdf::PdfDocument;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_core::source_cleanup::planning::planner::plan_source_cleanup;
use rendering_reader::cleanup_context::build_planning_contexts;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::analysis::RenderDocumentAnalysis;
use super::{RENDERED_DIR_NAME, TYPST_DIR_NAME};

/// `RenderSourcePdf` (the subset the bundle consumes).
pub struct RenderSourcePdf {
    pub path: PathBuf,
    pub source_text_precleaned_page_indices: Vec<i32>,
}

// `intermediate_paths.py` byte caps (POSIX path limit; Windows branch unused).
const SAFE_FILENAME_BYTES: usize = 240;
const SAFE_FULL_PATH_BYTES_POSIX: usize = 1000;

/// Mirror of `build_render_source_pdf(...)` for the bundle path. `translated_pages`
/// are the selected pages; `protected_pages` the OCR normalized document. The
/// final `source_pdf` is whichever intermediate (or the original) the last
/// changed step produced.
pub fn build_render_source_pdf(
    source_pdf_path: &Path,
    output_pdf_path: &Path,
    pdf_compress_dpi: i64,
    translated_pages: &BTreeMap<i32, Vec<Value>>,
    protected_pages: &BTreeMap<i32, Vec<Value>>,
    strip_hidden_text: bool,
    source_cleanup_strategy: &str,
    document_analysis: &RenderDocumentAnalysis,
) -> Result<RenderSourcePdf> {
    let mut render_source_path = source_pdf_path.to_path_buf();
    let work_root = typst_temp_root(output_pdf_path);

    // 1) invalid-xobject sanitize.
    let sanitized_path =
        intermediate_pdf_path(&work_root, output_pdf_path, ".source-xobject-sanitized.pdf");
    let mut doc = open_writer(&render_source_path)?;
    let sanitize_result = rendering_writer::sanitize::sanitize_invalid_xobjects(&mut doc)
        .map_err(|e| anyhow!("sanitize invalid xobjects: {e}"))?;
    if sanitize_result.changed {
        save_copy(&doc, &sanitized_path)?;
        render_source_path = sanitized_path;
    } else {
        let _ = std::fs::remove_file(&sanitized_path);
    }

    // 2) hidden-text strip (candidates = the analysis pseudo-scan set).
    let hidden_page_indices = document_analysis.hidden_text_strip_page_indices();
    if strip_hidden_text && !hidden_page_indices.is_empty() {
        let hidden_path =
            intermediate_pdf_path(&work_root, output_pdf_path, ".source-hidden-text-stripped.pdf");
        let candidates: Vec<i32> = hidden_page_indices.iter().map(|&idx| idx as i32).collect();
        let mut doc = open_writer(&render_source_path)?;
        let hidden_result = rendering_writer::hidden_text::strip_hidden_text_pages(&mut doc, &candidates)
            .map_err(|e| anyhow!("hidden-text strip: {e}"))?;
        if hidden_result.changed {
            save_copy(&doc, &hidden_path)?;
            render_source_path = hidden_path;
        } else {
            let _ = std::fs::remove_file(&hidden_path);
        }
    }

    // 3) bbox-text strip.
    let mut source_text_precleaned_page_indices: Vec<i32> = Vec::new();
    if !translated_pages.is_empty() && use_bbox_text_strip_cleanup(source_cleanup_strategy) {
        let translated_page_indices: Vec<i32> = translated_pages
            .iter()
            .filter(|(_, items)| !items.is_empty())
            .map(|(idx, _)| *idx)
            .collect();
        let pikepdf_strip_indices = document_analysis.pikepdf_text_strip_page_indices();
        let allows_pikepdf_strip: HashMap<i64, bool> = document_analysis
            .pages
            .keys()
            .map(|idx| (*idx, pikepdf_strip_indices.contains(idx)))
            .collect();
        if pikepdf_strip_indices
            .iter()
            .any(|idx| translated_page_indices.contains(&(*idx as i32)))
        {
            let bbox_path =
                intermediate_pdf_path(&work_root, output_pdf_path, ".source-bbox-text-stripped.pdf");
            let translated_pages_i64 = pages_to_i64(translated_pages);
            let protected_pages_i64 = pages_to_i64(protected_pages);
            let mut doc = open_writer(&render_source_path)?;
            let reader = mupdf::Document::open(render_source_path.as_path())
                .map_err(|e| anyhow!("open reader {}: {e}", render_source_path.display()))?;
            let indices: Vec<i64> = translated_pages.keys().map(|idx| *idx as i64).collect();
            let contexts = build_planning_contexts(&reader, &indices);
            let candidates = plan_source_cleanup(
                &contexts,
                &translated_pages_i64,
                &protected_pages_i64,
                false, // skip_formula_pages
                false, // skip_form_xobject_pages
                Some(&allows_pikepdf_strip),
            );
            let page_rects: HashMap<i32, Vec<RectTuple>> = candidates
                .page_rects
                .into_iter()
                .map(|(idx, rects)| (idx as i32, rects))
                .collect();
            let page_protected_rects: HashMap<i32, Vec<RectTuple>> = candidates
                .page_protected_rects
                .into_iter()
                .map(|(idx, rects)| (idx as i32, rects))
                .collect();
            let strip_result = rendering_writer::cleanup_writer::strip_bbox_text_rects_from_pdf(
                &mut doc,
                &page_rects,
                &page_protected_rects,
                true, // effective_recurse_forms (recurse_forms=None -> True)
            )
            .map_err(|e| anyhow!("bbox-text strip: {e}"))?;
            source_text_precleaned_page_indices = strip_result.changed_page_indices;
            source_text_precleaned_page_indices.sort_unstable();
            if source_text_precleaned_page_indices.is_empty() {
                let _ = std::fs::remove_file(&bbox_path);
            } else {
                save_copy(&doc, &bbox_path)?;
                render_source_path = bbox_path;
            }
        }
    }

    // 4) image compress (skipped when dpi <= 0).
    if pdf_compress_dpi > 0 {
        let compressed_path =
            intermediate_pdf_path(&work_root, output_pdf_path, ".source-compressed.pdf");
        let mut doc = open_writer(&render_source_path)?;
        let compress_result = rendering_writer::image_compress::compress_images(
            &mut doc,
            pdf_compress_dpi as i32,
        )
        .map_err(|e| anyhow!("image compress: {e}"))?;
        if compress_result.changed {
            save_copy(&doc, &compressed_path)?;
            render_source_path = compressed_path;
        } else {
            let _ = std::fs::remove_file(&compressed_path);
        }
    }

    Ok(RenderSourcePdf {
        path: render_source_path,
        source_text_precleaned_page_indices,
    })
}

/// Open an editable document (writer type) for an in-place prep step.
fn open_writer(path: &Path) -> Result<PdfDocument> {
    PdfDocument::open(path).map_err(|e| anyhow!("open {}: {e}", path.display()))
}

/// `delete_trailer_id` + `save_atomic` (the bridge's deterministic edit save).
fn save_copy(doc: &PdfDocument, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rendering_writer::save::delete_trailer_id(doc).map_err(|e| anyhow!("delete_trailer_id: {e}"))?;
    rendering_writer::save::save_atomic(doc, path).map_err(|e| anyhow!("save {}: {e}", path.display()))?;
    Ok(())
}

/// `default_typst_temp_root(output_pdf_path)` — first ancestor named
/// `rendered` yields `<rendered>/typst`; the project-output fallback branch is
/// unreachable for job-tree renders.
fn typst_temp_root(output_pdf_path: &Path) -> PathBuf {
    for parent in output_pdf_path.ancestors().skip(1) {
        if parent.file_name().map(|n| n == RENDERED_DIR_NAME).unwrap_or(false) {
            return parent.join(TYPST_DIR_NAME);
        }
    }
    output_pdf_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(TYPST_DIR_NAME)
}

/// `intermediate_pdf_path` — prefer `{stem}{suffix}` under `work_root`, else a
/// sha256-of-filename hash, else a 12-hex minimal name.
fn intermediate_pdf_path(work_root: &Path, output_pdf_path: &Path, suffix: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(work_root);
    let suffix = if suffix.starts_with('.') || suffix.is_empty() {
        suffix.to_string()
    } else {
        format!(".{suffix}")
    };
    let preferred = work_root.join(format!("{}{suffix}", output_stem(output_pdf_path)));
    if is_safe_path(&preferred) {
        return preferred;
    }
    let digest = sha256_hex(output_pdf_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let short = work_root.join(format!("{digest}{suffix}"));
    if is_safe_path(&short) {
        return short;
    }
    work_root.join(format!("{}.pdf", &digest[..12]))
}

fn output_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string()
}

fn is_safe_path(path: &Path) -> bool {
    let name_bytes = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.len())
        .unwrap_or(0);
    let full_bytes = path.to_string_lossy().len();
    name_bytes <= SAFE_FILENAME_BYTES && full_bytes <= SAFE_FULL_PATH_BYTES_POSIX
}

fn sha256_hex(input: String) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// `layout.use_bbox_text_strip_cleanup(strategy)` after
/// `normalize_source_cleanup_strategy`.
fn use_bbox_text_strip_cleanup(strategy: &str) -> bool {
    matches!(
        normalize_source_cleanup_strategy(strategy).as_str(),
        "pikepdf_text_strip" | "bbox_text_strip" | "legacy" | "redact_restore_formulas"
    )
}

/// `layout.normalize_source_cleanup_strategy`.
pub fn normalize_source_cleanup_strategy(value: &str) -> String {
    let strategy = value.trim().to_lowercase();
    if strategy == "redact_restore_formulas" {
        return "pikepdf_text_strip".to_string();
    }
    match strategy.as_str() {
        "pikepdf_text_strip" | "bbox_text_strip" | "legacy" | "typst_fill" => strategy,
        _ => "typst_fill".to_string(),
    }
}

fn pages_to_i64(pages: &BTreeMap<i32, Vec<Value>>) -> BTreeMap<i64, Vec<Value>> {
    pages.iter().map(|(idx, items)| (*idx as i64, items.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typst_temp_root_walks_to_rendered() {
        let out = std::path::Path::new("/tmp/job/rendered/out.pdf");
        assert_eq!(
            typst_temp_root(out),
            PathBuf::from("/tmp/job/rendered/typst")
        );
    }

    #[test]
    fn intermediate_path_prefers_stem() {
        let root = std::path::Path::new("/tmp/job/rendered/typst");
        assert_eq!(
            intermediate_pdf_path(root, std::path::Path::new("/tmp/job/rendered/out.pdf"), ".source-bbox-text-stripped.pdf"),
            PathBuf::from("/tmp/job/rendered/typst/out.source-bbox-text-stripped.pdf")
        );
    }

    #[test]
    fn strategy_normalizes_and_gates() {
        assert_eq!(normalize_source_cleanup_strategy("redact_restore_formulas"), "pikepdf_text_strip");
        assert_eq!(normalize_source_cleanup_strategy(" typst_fill "), "typst_fill");
        assert_eq!(normalize_source_cleanup_strategy("bogus"), "typst_fill");
        assert!(use_bbox_text_strip_cleanup("pikepdf_text_strip"));
        assert!(use_bbox_text_strip_cleanup("redact_restore_formulas"));
        assert!(!use_bbox_text_strip_cleanup("typst_fill"));
    }
}
