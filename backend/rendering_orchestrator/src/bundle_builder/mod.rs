//! Native `build_bundle` (C3-N11): in-process mirror of the retired
//! `entrypoints/run_render_delegate.py::build_bundle`, producing the
//! `render.bundle.v1` hand-off so `render_rs` no longer spawns python3 for the
//! prepare/page-specs segment. Built incrementally in sub-batches N11a..N11f,
//! each reproducing one `build_bundle` step over the public Rust leaf
//! functions; N11f lands the final key (`page_specs`) and flips the default on.

pub mod analysis;
pub mod assemble;
pub mod color_adapt;
pub mod overlay;
pub mod page_specs;
pub mod prepare;
pub mod render_source;
pub mod visual_profile;

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;

use crate::native_stats::subsystem as sub;
use crate::native_stats::NativeStats;
use crate::spec::RenderStageSpec;
use crate::translations::{load_translated_pages, select_translated_pages};

use assemble::AssembleInputs;

pub const RENDER_BUNDLE_SCHEMA_VERSION: &str = "render.bundle.v1";

const RENDERED_DIR_NAME: &str = "rendered";
const TYPST_DIR_NAME: &str = "typst";
const BACKGROUND_BOOK_SUBDIR: &str = "background-book";

/// Mirrors `build_bundle`'s 15-key assembly. N11d: translated_pages (prepare +
/// first-line indent + policy) is real. N11e: color adapt runs over the prepared
/// pages for every mode (writing `_render_cover_fill` / `_render_text_color`),
/// and overlay/dual additionally assemble `overlay_page_specs` (page geometry +
/// RenderBlock DTOs). N11f: `page_specs` is real (full emitter dicts).
pub fn build_bundle(spec: &RenderStageSpec, stats: &mut NativeStats) -> Result<Value> {
    let mut mode = spec.params.render_mode_str();

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

    // N11b: whole-document analysis (page_snapshot -> profile -> analysis), then
    // auto-mode resolution. The analysis also drives render-source prep from N11c.
    let document_analysis =
        analysis::build_render_document_analysis(&spec.inputs.source_pdf, &selected_pages)?;
    stats.record_hit(sub::ANALYSIS);
    if mode == "auto" {
        mode = analysis::resolve_effective_render_mode(&mode, !selected_pages.is_empty(), Some(&document_analysis));
    }

    // N11c: protected pages (for render-source prep) load eagerly so a
    // wrong-schema normalized document errors here exactly like the retired
    // Python `build_bundle`.
    let protected_pages = crate::protected_pages::protected_pages_from_document_path(
        spec.inputs
            .translations_dir
            .parent()
            .map(|parent| parent.join("ocr").join("normalized").join("document.v1.json"))
            .as_deref(),
    )?;
    stats.record_hit(sub::PDF_STRUCTURE_PROFILE);

    // N11c: render-source prep chain (sanitize -> hidden strip -> bbox strip ->
    // compress), producing the real `source_pdf` + `precleaned_page_indices`.
    let cleanup_strategy = spec
        .params
        .source_cleanup_strategy
        .as_deref()
        .map(render_source::normalize_source_cleanup_strategy)
        .unwrap_or_else(|| "typst_fill".to_string());
    let render_source_pdf = render_source::build_render_source_pdf(
        &spec.inputs.source_pdf,
        &output_pdf,
        spec.params.pdf_compress_dpi(),
        &selected_pages,
        &protected_pages,
        mode != "overlay",
        &cleanup_strategy,
        &document_analysis,
        stats,
    )?;

    // N11d: prepare boundary + first-line-indent detection + policy boundary,
    // producing the bundle's `translated_pages` (indent pdf = the RAW source).
    let selected_pages_i64: BTreeMap<i64, Vec<Value>> = selected_pages
        .iter()
        .map(|(&idx, items)| (idx as i64, items.clone()))
        .collect();
    let prepared_pages = prepare::prepare_translated_pages_for_render(
        &spec.inputs.source_pdf,
        &selected_pages_i64,
        spec.params.source_cleanup_strategy.as_deref(),
    )?;
    stats.record_hit(sub::LAYOUT);

    // N11e: color adapt runs for every mode (`precomputed_colors_by_item_id={}`),
    // writing `_render_cover_fill` / `_render_text_color` onto each item.
    let adapted_pages = color_adapt::apply_adaptive_overlay_colors_batch(
        &spec.inputs.source_pdf,
        &prepared_pages,
    )?;

    // N11e: overlay/dual assemble `overlay_page_specs` (geometry + blocks); the
    // typst modes leave the bundle key null.
    let overlay_page_specs = if matches!(mode.as_str(), "overlay" | "dual") {
        Some(serde_json::to_value(overlay::build_overlay_page_specs(
            &spec.inputs.source_pdf,
            &adapted_pages,
            &font_unify_mode(spec),
        )?)?)
    } else {
        None
    };

    let work_dir = background_work_dir(&output_pdf);
    prepare_work_dir(&work_dir)?;

    // N11f: `page_specs` — full emitter dicts (`_page_spec_to_dict`, 29-key
    // blocks) from the RENDER-SOURCE pdf over the color-adapted pages. The
    // retired Python `build_bundle` re-applied the C3-N8 policy boundary
    // (`prepared=True`); the pages are already policy-applied, so the
    // pass-through is idempotent.
    let page_specs = page_specs::build_render_page_specs(
        &render_source_pdf.path,
        &adapted_pages,
        &font_unify_mode(spec),
    )?;
    stats.record_hit(sub::LAYOUT_PAYLOAD);

    // Load the prewarmed visual profile (empty map when absent; mirrors the
    // retired Python `build_bundle`'s `load_visual_profile_runtime` over the
    // prewarm artifacts dir).
    let visual_profile_fill_map = serde_json::to_value(visual_profile::visual_profile_fill_map(
        &spec.inputs.translations_dir,
    )?)?;
    stats.record_hit(sub::VISUAL_PROFILE);

    Ok(assemble::assemble(AssembleInputs {
        mode,
        source_pdf: render_source_pdf.path,
        output_pdf,
        work_dir,
        font_family: font_family(spec),
        start_page: start_page as i32,
        end_page,
        page_map_indices: selected_pages.keys().copied().collect(),
        precleaned_page_indices: render_source_pdf.source_text_precleaned_page_indices,
        translated_pages: serde_json::to_value(&adapted_pages)?,
        overlay_page_specs,
        page_specs,
        visual_profile_fill_map,
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

/// `layout.FONT_UNIFY_MODE`: `role_min` unless the spec sets a valid explicit
/// mode (`role_min` / `off`); any other value falls back to the module default.
fn font_unify_mode(spec: &RenderStageSpec) -> String {
    let mode = spec
        .params
        .font_unify_mode
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if mode == "off" {
        "off".to_string()
    } else {
        "role_min".to_string()
    }
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
}
