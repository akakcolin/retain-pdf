//! Native `build_bundle` (C3-N11): in-process mirror of
//! `entrypoints/run_render_delegate.py::build_bundle`, producing the
//! `render.bundle.v1` hand-off so `render_rs` no longer spawns python3 for the
//! prepare/page-specs segment. Built incrementally in sub-batches N11a..N11f,
//! each reproducing one `build_bundle` step over the public Rust leaf
//! functions; keys that have not landed yet carry placeholder values and are
//! excluded from the differential smoke until their batch.
//!
//! Gate: `RETAINPDF_RENDER_BUNDLE_NATIVE` (default off until N11f flips it; an
//! explicit `0/false/off/no` forces off). Until N11f the only sanctioned native
//! entry is `render_rs --dump-bundle` (the differential producer); running a
//! full render with the flag set early would feed placeholder keys into the
//! stages and is NOT supported.

pub mod assemble;

use std::path::PathBuf;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::spec::RenderStageSpec;
use crate::translations::{load_translated_pages, select_translated_pages};

use assemble::AssembleInputs;

pub const RENDER_BUNDLE_SCHEMA_VERSION: &str = "render.bundle.v1";

const RENDERED_DIR_NAME: &str = "rendered";
const TYPST_DIR_NAME: &str = "typst";
const BACKGROUND_BOOK_SUBDIR: &str = "background-book";

/// `RETAINPDF_RENDER_BUNDLE_NATIVE` — truthy unless `0/false/off/no`.
pub fn native_enabled() -> bool {
    match std::env::var("RETAINPDF_RENDER_BUNDLE_NATIVE") {
        Ok(raw) => !["0", "false", "off", "no"].contains(&raw.trim().to_lowercase().as_str()),
        Err(_) => false,
    }
}

/// Mirrors `build_bundle`'s 15-key assembly. N11a: mode/font/output/work_dir/
/// start/end/page_map/fill_map/overlay-specs are real; source_pdf,
/// precleaned_page_indices, translated_pages and page_specs are placeholders
/// until N11c/N11d/N11f respectively.
pub fn build_bundle(spec: &RenderStageSpec) -> Result<Value> {
    let mode = spec.params.render_mode_str();
    if !matches!(mode.as_str(), "typst" | "typst_visual") {
        bail!(
            "native build_bundle supports typst/typst_visual only, got {mode:?} \
             (overlay/dual land in N11e, auto resolution in N11b)"
        );
    }

    // `job_dirs.resolve_job_dirs(root).rendered_dir` — Python `Path.resolve()`
    // canonicalizes the existing job root.
    let job_root = resolved_path(&spec.job.job_root);
    let output_pdf = job_root.join(RENDERED_DIR_NAME).join(translated_pdf_name(spec));

    let start_page = spec.params.start_page().max(0);
    let translated_pages = load_translated_pages(
        &spec.inputs.translations_dir,
        spec.inputs.translation_manifest.as_deref(),
    )?;
    let selected_pages =
        select_translated_pages(&translated_pages, start_page, spec.params.end_page())?;
    // `stop_page = max(selected_pages) if end_page < 0 else end_page`.
    let end_page = if spec.params.end_page() < 0 {
        *selected_pages.keys().next_back().unwrap_or(&0)
    } else {
        spec.params.end_page() as i32
    };

    let work_dir = background_work_dir(&output_pdf);
    prepare_work_dir(&work_dir)?;

    Ok(assemble::assemble(AssembleInputs {
        mode,
        source_pdf: spec.inputs.source_pdf.clone(),
        output_pdf,
        work_dir,
        font_family: font_family(spec),
        start_page: start_page as i32,
        end_page,
        page_map_indices: selected_pages.keys().copied().collect(),
        translated_pages: json!({}),
        page_specs: json!([]),
    }))
}

/// `spec.params.translated_pdf_name.strip() or f"{source.stem}-translated.pdf"`.
fn translated_pdf_name(spec: &RenderStageSpec) -> String {
    let name = spec
        .params
        .translated_pdf_name
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        let stem = spec
            .inputs
            .source_pdf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        format!("{stem}-translated.pdf")
    } else {
        name.to_string()
    }
}

/// Bundle `font_family` is the RAW stripped spec value (Python defaults to ""
/// when the spec omits it; the typst stage applies its own fallback).
fn font_family(spec: &RenderStageSpec) -> String {
    spec.params
        .typst_font_family
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

/// `default_typst_temp_root(output_pdf) / "background-book"`: walk the parents
/// for the `rendered` dir and return `<rendered>/typst/background-book`.
fn background_work_dir(output_pdf: &std::path::Path) -> PathBuf {
    for parent in output_pdf.ancestors().skip(1) {
        if parent.file_name().map(|n| n == RENDERED_DIR_NAME).unwrap_or(false) {
            return parent.join(TYPST_DIR_NAME).join(BACKGROUND_BOOK_SUBDIR);
        }
    }
    output_pdf
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(TYPST_DIR_NAME)
        .join(BACKGROUND_BOOK_SUBDIR)
}

/// `prepare_typst_work_dir`: rmtree (or unlink a stray file) then recreate.
fn prepare_work_dir(dir: &std::path::Path) -> Result<()> {
    if dir.exists() {
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        } else {
            std::fs::remove_file(dir)?;
        }
    }
    std::fs::create_dir_all(dir)?;
    Ok(())
}

/// Python `Path.resolve()` (strict=False): canonicalize when the path exists,
/// otherwise lexical `.`/`..` normalization.
fn resolved_path(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_work_dir_nested_under_rendered() {
        let out = std::path::Path::new("/tmp/job/rendered/out.pdf");
        assert_eq!(
            background_work_dir(out),
            PathBuf::from("/tmp/job/rendered/typst/background-book")
        );
    }

    #[test]
    fn env_flag_truthy_unless_off() {
        std::env::remove_var("RETAINPDF_RENDER_BUNDLE_NATIVE");
        assert!(!native_enabled());
        std::env::set_var("RETAINPDF_RENDER_BUNDLE_NATIVE", "1");
        assert!(native_enabled());
        std::env::set_var("RETAINPDF_RENDER_BUNDLE_NATIVE", "0");
        assert!(!native_enabled());
        std::env::set_var("RETAINPDF_RENDER_BUNDLE_NATIVE", "false");
        assert!(!native_enabled());
        std::env::remove_var("RETAINPDF_RENDER_BUNDLE_NATIVE");
    }
}
