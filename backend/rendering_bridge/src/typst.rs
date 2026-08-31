//! Typst output-layer bridge: pure emitter + book-overlay source builder +
//! CLI compile orchestration (ports of `output/typst/emitter.py`,
//! `output/typst/source_builder.py`, `output/typst/compiler.py`).

use std::path::{Path, PathBuf};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// `emit_typst_source` entry point (pure; no PDF editing).
#[pyfunction]
fn emit_typst_source(
    page_specs_json: &str,
    background_pdf_path: &str,
    work_dir: &str,
    font_family: &str,
) -> PyResult<String> {
    let specs: Vec<rendering_output::dto::RenderPageSpec> = serde_json::from_str(page_specs_json)
        .map_err(|e| PyValueError::new_err(format!("page_specs_json: {e}")))?;
    Ok(rendering_output::emitter::build_typst_source_from_page_specs(
        Path::new(background_pdf_path),
        &specs,
        Path::new(work_dir),
        font_family,
    ))
}

/// `emit_typst_book_overlay_source` entry point (pure; no PDF editing). Payload
/// is `[[page_width_pt, page_height_pt, [RenderBlock, ...]], ...]` — the
/// prebuilt `RenderBlock` DTOs the layout pipeline `build_render_blocks`
/// produces on the Python side.
#[pyfunction]
fn emit_typst_book_overlay_source(
    page_specs_json: &str,
    font_family: &str,
    include_cover_rect: bool,
) -> PyResult<String> {
    let page_specs: Vec<(f64, f64, Vec<rendering_output::dto::RenderBlock>)> =
        serde_json::from_str(page_specs_json)
            .map_err(|e| PyValueError::new_err(format!("page_specs_json: {e}")))?;
    Ok(rendering_output::source_builder::build_typst_book_overlay_source(
        &page_specs,
        font_family,
        include_cover_rect,
    ))
}

/// `compile_typst_source` entry point (orchestrates the `typst` CLI). Returns
/// the compiled PDF path. `root`, `font_paths`, `typ_bin`, and
/// `timeout_seconds` override the compile context defaults.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn compile_typst_source(
    source: &str,
    stem: &str,
    phase: &str,
    work_dir: &str,
    root: Option<&str>,
    font_paths: Vec<String>,
    typ_bin: Option<&str>,
    timeout_seconds: Option<f64>,
) -> PyResult<String> {
    let ctx = rendering_output::compile::CompileContext {
        typ_bin: typ_bin
            .unwrap_or(rendering_output::compile::DEFAULT_TYPST_BIN)
            .to_string(),
        timeout_seconds: timeout_seconds
            .unwrap_or(rendering_output::compile::DEFAULT_COMPILE_TIMEOUT_SECONDS),
        backends_fonts_dir: None,
        env_font_dirs: None,
    };
    let fonts: Vec<PathBuf> = font_paths.into_iter().map(PathBuf::from).collect();
    let out = rendering_output::compile::compile_typst_source(
        source,
        stem,
        phase,
        Path::new(work_dir),
        root.map(Path::new),
        &fonts,
        &ctx,
        serde_json::Map::new(),
    )
    .map_err(|e| PyRuntimeError::new_err(e.message()))?;
    Ok(out.to_string_lossy().into_owned())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(emit_typst_source, m)?)?;
    m.add_function(wrap_pyfunction!(emit_typst_book_overlay_source, m)?)?;
    m.add_function(wrap_pyfunction!(compile_typst_source, m)?)?;
    Ok(())
}
