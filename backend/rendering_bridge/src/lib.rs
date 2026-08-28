//! Native (pyo3/maturin) bridge exposing the ported Rust rendering pipeline to
//! production Python.
//!
//! Functions:
//!   * `emit_typst_source(page_specs_json, background_pdf_path, work_dir,
//!     font_family) -> str` — the pure Typst emitter (port of
//!     `output/typst/emitter.py`).
//!   * `emit_typst_book_overlay_source(page_specs_json, font_family,
//!     include_cover_rect) -> str` — the whole-book overlay emitter (port of
//!     `output/typst/source_builder.build_typst_book_overlay_source`); payload
//!     is prebuilt `RenderBlock` DTOs.
//!   * `compile_typst_source(...) -> str` — orchestrate the `typst` CLI
//!     (port of `output/typst/compiler.py`).
//!   * Phase 5 write-path entries operating on PDF bytes in -> bytes out:
//!     `strip_bbox_text_rects`, `strip_hidden_text`, `sanitize_invalid_xobjects`,
//!     `compress_images`, `extract_pages`, `overlay_page`. The transformations
//!     that expose a production result contract (`strip_hidden_text`,
//!     `sanitize_invalid_xobjects`, `compress_images`) return
//!     `(pdf_bytes, metadata_json)` so the Python shim can rebuild the result
//!     dataclass without re-deriving it.
//!   * `clean_background(source_pdf_bytes, config_json) -> bytes` — sample a
//!     fill per rect (ported `background::fill`) and draw opaque covers.
//!     This is the fill-cover capability validated by the background
//!     differential; the full production `stage.build_clean_background_pdf`
//!     (redaction-based) is not ported.
//!
//! The Python shim `output/typst/_native.py` imports this module and falls
//! back to the pure-Python implementations on `ImportError`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::rect::Rect as CoreRect;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::PdfDocument as _;
use rendering_writer::background::color_adapt::{
    build_text_page_for_extraction, extract_span_dicts_from_text_page, foreground_color_from_pixmap,
    title_foreground_color_from_pixmap,
};
use rendering_writer::background::fill::{
    batch_sampler_clip_rect, build_local_background_sampler, draw_white_covers, sample_local_background_fill, RgbPixmap,
};
use rendering_writer::background::patch::{rect_intersection, rect_is_empty, rect_is_finite};
use rendering_writer::background::redaction::page_specs::{self, RenderPageSpec};
use rendering_writer::background::redaction::RedactionItem;
use rendering_writer::background::vector_text;
use rendering_writer::save::{delete_trailer_id, save_atomic, save_optimized};
use serde::Deserialize;

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

// --- write-path helpers ------------------------------------------------------

/// Deserialize `{"<page>": [[x0, y0, x1, y1], ...]}` into strip-API rects.
fn parse_rect_map(json: &str, label: &str) -> PyResult<HashMap<i32, Vec<RectTuple>>> {
    let raw: HashMap<String, Vec<Vec<f64>>> = serde_json::from_str(json)
        .map_err(|e| PyValueError::new_err(format!("{label}: {e}")))?;
    let mut out = HashMap::new();
    for (k, rects) in raw {
        let idx: i32 = k
            .parse()
            .map_err(|e| PyValueError::new_err(format!("{label}: bad page key {k}: {e}")))?;
        out.insert(idx, rects.into_iter().map(|r| [r[0], r[1], r[2], r[3]]).collect());
    }
    Ok(out)
}

/// Fresh temp working dir for in-memory PDF round-trips.
fn temp_dir() -> PyResult<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rpbridge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| PyRuntimeError::new_err(format!("mkdir {dir:?}: {e}")))?;
    Ok(dir)
}

fn save_to_bytes(pdf: &PdfDocument, dir: &Path) -> PyResult<Vec<u8>> {
    delete_trailer_id(pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_atomic(pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

/// Open `pdf_bytes`, run `edit` (which may replace/append content), and return
/// the deterministically saved PDF bytes plus the edit's result value.
fn with_pdf_edit<F, R>(pdf_bytes: &[u8], edit: F) -> PyResult<(Vec<u8>, R)>
where
    F: FnOnce(&mut PdfDocument) -> Result<R, mupdf::Error>,
{
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let result = edit(&mut pdf).map_err(|e| PyRuntimeError::new_err(format!("edit: {e}")))?;
    let bytes = save_to_bytes(&pdf, &dir)?;
    Ok((bytes, result))
}

/// Serialize a write-path outcome struct to the JSON metadata string returned
/// alongside the PDF bytes (the Python shim parses it to rebuild the production
/// result contract: `changed`, page/count fields, etc.).
fn meta_json<T: serde::Serialize>(value: &T) -> PyResult<String> {
    serde_json::to_string(value)
        .map_err(|e| PyRuntimeError::new_err(format!("metadata json: {e}")))
}

/// Strip text show ops inside the given per-page rects (port of
/// `strip_bbox_text_rects_from_pdf_copy`). Returns bytes only (the Python
/// side's rich skip/candidate metadata has no bridge equivalent yet).
#[pyfunction]
fn strip_bbox_text_rects(
    pdf_bytes: &[u8],
    page_rects_json: &str,
    page_protected_rects_json: Option<&str>,
    recurse_forms: Option<bool>,
) -> PyResult<Vec<u8>> {
    let page_rects = parse_rect_map(page_rects_json, "page_rects")?;
    let protected = match page_protected_rects_json {
        Some(s) => parse_rect_map(s, "page_protected_rects")?,
        None => HashMap::new(),
    };
    let recurse = recurse_forms.unwrap_or(true);
    let (bytes, ()) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::cleanup_writer::strip_bbox_text_rects_from_pdf(
            pdf, &page_rects, &protected, recurse,
        )
        .map(|_| ())
    })?;
    Ok(bytes)
}

/// Remove hidden text (all-zero / zero-size / all-white color) show ops (port
/// of `hidden_text.py::strip_hidden_text`). Returns bytes plus the
/// `HiddenTextStripResult` metadata.
#[pyfunction]
fn strip_hidden_text(pdf_bytes: &[u8]) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) =
        with_pdf_edit(pdf_bytes, |pdf| rendering_writer::hidden_text::strip_hidden_text(pdf))?;
    Ok((bytes, meta_json(&result)?))
}

/// Replace invalid image XObjects with the recorded fill (port of
/// `sanitize.py::sanitize_invalid_xobjects`). Returns bytes plus the
/// `SanitizeResult` metadata.
#[pyfunction]
fn sanitize_invalid_xobjects(pdf_bytes: &[u8]) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::sanitize::sanitize_invalid_xobjects(pdf)
    })?;
    Ok((bytes, meta_json(&result)?))
}

/// Recompress displayed images toward `dpi` (port of
/// `image_compress.py::compress_images`). Returns bytes plus the
/// `ImageCompressResult` metadata.
#[pyfunction]
fn compress_images(pdf_bytes: &[u8], dpi: i32) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::image_compress::compress_images(pdf, dpi)
    })?;
    Ok((bytes, meta_json(&result)?))
}

/// Extract `start..=end` pages into a new document (port of
/// `pikepdf_pages.py::extract_pages_with_pikepdf`).
#[pyfunction]
fn extract_pages(pdf_bytes: &[u8], start: i64, end: i64) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let mut source = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let out = rendering_writer::page_subset::extract_pages(&mut source, start, end)
        .map_err(|e| PyRuntimeError::new_err(format!("extract_pages: {e}")))?;
    save_to_bytes(&out, &dir)
}

/// Overlay one page of `overlay_bytes` onto one page of `pdf_bytes` (port of
/// `pikepdf_overlay.py::overlay_page_pdfs_with_pikepdf`).
#[pyfunction]
fn overlay_page(
    pdf_bytes: &[u8],
    overlay_bytes: &[u8],
    source_page: i32,
    overlay_page_idx: i32,
) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    let ovl_path = dir.join("ovl.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    std::fs::write(&ovl_path, overlay_bytes).map_err(|e| PyRuntimeError::new_err(format!("write ovl: {e}")))?;
    let mut source = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open source: {e}")))?;
    let overlay = PdfDocument::open(ovl_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open overlay: {e}")))?;
    rendering_writer::overlay::overlay_page(&mut source, source_page, &overlay, overlay_page_idx)
        .map_err(|e| PyRuntimeError::new_err(format!("overlay: {e}")))?;
    save_to_bytes(&source, &dir)
}

/// Sample a background fill per rect from the pristine page and draw opaque
/// covers (port of `background/fill.py::draw_white_covers`). `config_json`:
/// `{"page_index": 0, "rects": [[x0, y0, x1, y1], ...], "scale": 2.0}`.
#[pyfunction]
fn clean_background(source_pdf_bytes: &[u8], config_json: &str) -> PyResult<Vec<u8>> {
    #[derive(Deserialize)]
    struct CleanConfig {
        #[serde(default)]
        page_index: i32,
        rects: Vec<Vec<f64>>,
        #[serde(default = "default_scale")]
        scale: f64,
    }
    fn default_scale() -> f64 {
        2.0
    }

    let config: CleanConfig = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(format!("config_json: {e}")))?;
    let rects: Vec<RectTuple> = config.rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, source_pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;

    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open render: {e}")))?;
    let page = doc
        .load_page(config.page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("load_page: {e}")))?;
    let b = page.bounds().map_err(|e| PyRuntimeError::new_err(format!("bounds: {e}")))?;
    let page_rect: RectTuple = [b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64];
    let scale = config.scale as f32;

    let render_clip = |clip: &RectTuple| -> Option<RgbPixmap> {
        let r = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
        doc.render_page_clip_rgb(config.page_index as i64, Some(&r), scale)
            .ok()
            .map(|px| RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            })
    };

    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open edit: {e}")))?;
    let mut edit_page = pdf
        .load_pdf_page(config.page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("load_pdf_page: {e}")))?;
    draw_white_covers(&mut edit_page, &mut pdf, &page_rect, &rects, &render_clip)
        .map_err(|e| PyRuntimeError::new_err(format!("draw_white_covers: {e}")))?;
    save_to_bytes(&pdf, &dir)
}

/// `build_clean_background_pdf` entry point — the full production stage
/// (copy_toc + per-page redaction orchestration; port of
/// `background/stage.py::build_clean_background_pdf`). `translated_pages_json`:
/// `{"<page>": [{bbox, translated_text, ...}, ...]}` — the ORIGINAL translated
/// items (page-spec replacement happens here in Rust); item dicts are the
/// stable serde DTO (unknown keys ignored). `page_specs_json`: the raw
/// `RenderPageSpec` list (only `page_index` + `blocks` consumed).
/// `visual_profile_fill_map_json`: flat first-wins `{item_id: [r,g,b]}` fill
/// table extracted by the shim from the visual profile. The page-spec
/// replacement and per-item `_visual_profile_fill` annotation are computed by
/// `page_specs::apply_page_specs_and_fills`, whose formula-source map (the
/// un-replaced originals) is what the stage's formula detection reads.
/// `precleaned_page_indices_json`: `[0, 2, ...]`. Returns the optimised PDF
/// bytes.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn build_clean_background_pdf(
    source_pdf_bytes: &[u8],
    translated_pages_json: &str,
    redaction_strategy: Option<&str>,
    precleaned_page_indices_json: &str,
    page_specs_json: &str,
    visual_profile_fill_map_json: &str,
) -> PyResult<Vec<u8>> {
    let translated_pages: BTreeMap<i32, Vec<RedactionItem>> =
        serde_json::from_str(translated_pages_json)
            .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let page_specs: Vec<RenderPageSpec> = serde_json::from_str(page_specs_json)
        .map_err(|e| PyValueError::new_err(format!("page_specs_json: {e}")))?;
    let fill_map: HashMap<String, [f64; 3]> =
        serde_json::from_str(visual_profile_fill_map_json)
            .map_err(|e| PyValueError::new_err(format!("visual_profile_fill_map_json: {e}")))?;
    let precleaned: HashSet<i32> = serde_json::from_str(precleaned_page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("precleaned_page_indices_json: {e}")))?;
    let (translated_pages, formula_source_pages) =
        page_specs::apply_page_specs_and_fills(&translated_pages, &page_specs, &fill_map);

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, source_pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;

    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open render: {e}")))?;
    let page = doc
        .load_page(0)
        .map_err(|e| PyRuntimeError::new_err(format!("load_page 0: {e}")))?;
    let b = page.bounds().map_err(|e| PyRuntimeError::new_err(format!("bounds: {e}")))?;
    let page_rect: RectTuple = [b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64];
    let scale: f32 = 2.0;

    let render_clip = |page_index: i32, clip: &RectTuple| -> Option<RgbPixmap> {
        let rect = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
        doc.render_page_clip_rgb(page_index as i64, Some(&rect), scale)
            .ok()
            .map(|px| RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            })
    };

    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open edit: {e}")))?;
    rendering_writer::background::stage::build_clean_background_pdf(
        &doc,
        &mut pdf,
        &page_rect,
        &translated_pages,
        &formula_source_pages,
        redaction_strategy,
        &precleaned,
        &render_clip,
    )
    .map_err(|e| PyRuntimeError::new_err(format!("build_clean_background_pdf: {e}")))?;

    delete_trailer_id(&pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_optimized(&pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save_optimized: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

/// `save_optimized_pdf` entry point — PDF bytes in, optimised bytes out
/// (garbage=4 + image/font stream compression). Port of the compaction half of
/// `document/pdf_ops.save_optimized_pdf`; font subsetting is intentionally NOT
/// performed here because mupdf-rs exposes no subset API — production applies
/// fitz `subset_fonts()` + `tobytes()` before handing bytes over (see the
/// `source/_native.py::save_optimized` shim).
#[pyfunction]
fn save_optimized_pdf(pdf_bytes: &[u8]) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    delete_trailer_id(&pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_optimized(&pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save_optimized: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

// --- reader entry -----------------------------------------------------------

/// Read per-page rects (rotation-applied bounds, == fitz `page.rect`) for the
/// requested page indices. Returns `{"<idx>": [x0, y0, x1, y1]}`; unreadable
/// indices are skipped (production filters `0 <= idx < page_count` before
/// asking, so an out-of-range request is not fatal).
#[pyfunction]
fn read_page_sizes(pdf_bytes: &[u8], page_indices_json: &str) -> PyResult<String> {
    let indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let mut sizes = BTreeMap::new();
    for idx in indices {
        if let Ok(rect) = doc.page_rect(idx) {
            sizes.insert(idx.to_string(), [rect.x0, rect.y0, rect.x1, rect.y1]);
        }
    }
    serde_json::to_string(&sizes).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read total page count and per-page rects (rotation-applied bounds, == fitz
/// `page.rect`) in a single open. Returns
/// `{"page_count": N, "rects": {"<idx>": [x0, y0, x1, y1]}}`; `rects` uses a
/// BTreeMap so indices stay ordered. An index whose rect cannot be read is
/// skipped (fitz-equivalent for an unreadable page); `page_count` is still
/// authoritative so `len(doc)` parity is preserved.
#[pyfunction]
fn read_page_geometry(pdf_bytes: &[u8]) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    // UFCS: mupdf's inherent `Document::page_count` (i32) shadows the trait
    // method; the trait's i64 keeps the loop index aligned with `page_rect`.
    let page_count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    let mut rects = BTreeMap::new();
    for idx in 0..page_count {
        if let Ok(rect) = doc.page_rect(idx) {
            rects.insert(idx.to_string(), [rect.x0, rect.y0, rect.x1, rect.y1]);
        }
    }
    let out = serde_json::json!({
        "page_count": page_count,
        "rects": rects,
    });
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's vector-drawing count (B2-7): `page_drawing_count` (== fitz
/// `len(page.get_cdrawings())`; count parity holds exactly — see
/// `rendering_reader::PdfDocument::page_drawing_count`). This powers the
/// count-based vector-heavy / cover-only heuristics. A page that cannot be
/// loaded raises so the shim falls back to the pure-Python reference.
///
/// Note: per-drawing rect/type/width parity is NOT exposed here — PyMuPDF's
/// `get_cdrawings` drops the last path item's first point on some stroked
/// zigzag paths, so the native `page_drawings` rect can exceed fitz's (see
/// `rendering_reader::pdf_document`). The rects consumer
/// (`collect_page_drawing_rects`) therefore stays on the Python reference.
#[pyfunction]
fn read_page_drawing_count(pdf_bytes: &[u8], page_index: i64) -> PyResult<i64> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_drawing_count(&doc, page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("page_drawing_count: {e}")))?;
    Ok(count)
}

/// Read one page's image placement rects (B2-8): `page_image_placement_rects`
/// (== fitz `page.get_image_info(hashes=False)` bboxes in content /
/// rotation-stripped page space, paint order — see
/// `rendering_reader::PdfDocument::page_image_placement_rects`). Returns
/// `[[x0,y0,x1,y1], ...]`. A PDF that cannot be opened, or a page index outside
/// `[0, page_count)`, raises so the shim falls back to the pure-Python
/// reference (mirrors the `read_page_drawing_count` contract). A page that
/// loads but has no `fill_image` placements yields `[]`.
#[pyfunction]
fn read_page_image_rects(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let rects: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_image_placement_rects(&doc, page_index)
        .iter()
        .map(|r| serde_json::json!([r.x0, r.y0, r.x1, r.y1]))
        .collect();
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's fitz `get_text("dict")` spans (B2-9):
/// `text_extract.extract_page_text_spans` — `[[x0,y0,x1,y1,text], ...]` in
/// content / rotation-stripped page space (see
/// `rendering_reader::PdfDocument::page_text_spans`). A PDF that cannot be
/// opened, or a page index outside `[0, page_count)`, raises so the shim falls
/// back to the pure-Python reference (mirrors the `read_page_image_rects`
/// contract).
#[pyfunction]
fn read_page_text_spans(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let spans: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_text_spans(&doc, page_index)
        .iter()
        .map(|(r, text)| serde_json::json!([r.x0, r.y0, r.x1, r.y1, text]))
        .collect();
    serde_json::to_string(&spans).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's fitz `get_text("blocks")` text blocks (B2-9):
/// `text_extract.extract_page_text_blocks` — `[[x0,y0,x1,y1,text], ...]`.
#[pyfunction]
fn read_page_text_blocks(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let blocks: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_text_blocks(&doc, page_index)
        .iter()
        .map(|(r, text)| serde_json::json!([r.x0, r.y0, r.x1, r.y1, text]))
        .collect();
    serde_json::to_string(&blocks).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's math-font span rects (B2-9):
/// `math_spans.collect_page_math_protection_rects` — `[[x0,y0,x1,y1], ...]`,
/// deduped by `rect_key`.
#[pyfunction]
fn read_page_math_rects(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let rects: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_math_rects(&doc, page_index)
        .iter()
        .map(|r| serde_json::json!([r.x0, r.y0, r.x1, r.y1]))
        .collect();
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's non-math span heights (B2-9):
/// `math_spans.collect_page_non_math_span_heights` — `[h0, h1, ...]`.
#[pyfunction]
fn read_page_span_heights(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let heights: Vec<f64> = rendering_reader::PdfDocument::page_span_heights(&doc, page_index);
    serde_json::to_string(&heights).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Run the ported vector-text classifier on one page (B2-7). `target_rects_json`
/// is `[[x0,y0,x1,y1], ...]`; returns the matched rects `[[x0,y0,x1,y1], ...]`
/// (mirrors `source/vector_text.py::collect_vector_text_rects`). Any failure
/// yields an empty list (Python `except Exception: return []`).
#[pyfunction]
fn collect_vector_text_rects(pdf_bytes: &[u8], page_index: i64, target_rects_json: &str) -> PyResult<String> {
    let target_rects: Vec<RectTuple> = serde_json::from_str(target_rects_json)
        .map_err(|e| PyValueError::new_err(format!("target_rects_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let rects = vector_text::collect_vector_text_rects(&doc, page_index as i32, &target_rects);
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read the source-cleanup planning contexts for `page_indices_json`:
/// `{"<idx>": {"rect": [x0,y0,x1,y1], "ctm": [a,b,c,d,e,f],
/// "bboxlog": [[kind, x0, y0, x1, y1], ...], "content_stream_size": N,
/// "has_form_xobjects": bool}}`.
///
/// `rect`/`ctm`/`bboxlog` are in rotation-stripped fitz page space (fitz
/// `page.rect` / `page.transformation_matrix` / `page.get_bboxlog()`); the
/// bboxlog keeps op order. A page whose rect or ctm cannot be read is skipped
/// (production filters `0 <= idx < page_count` first). `content_stream_threshold`
/// is the early-return cap mirrored from
/// `page_probe.page_content_stream_size`.
#[pyfunction]
fn read_page_cleanup_contexts(
    pdf_bytes: &[u8],
    page_indices_json: &str,
    content_stream_threshold: u64,
) -> PyResult<String> {
    let indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let mut out = BTreeMap::new();
    for idx in indices {
        let Ok(rect) = doc.page_rect(idx) else {
            continue;
        };
        let Some(ctm) = doc.page_ctm(idx) else {
            continue;
        };
        let entries: Vec<serde_json::Value> = doc
            .page_bboxlog(idx)
            .iter()
            .map(|e| {
                serde_json::json!([
                    e.kind,
                    [e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1],
                ])
            })
            .collect();
        let stream_size = doc.page_content_stream_size(idx, content_stream_threshold);
        let has_form_xobjects = doc.page_has_form_xobjects(idx);
        out.insert(
            idx.to_string(),
            serde_json::json!({
                "rect": [rect.x0, rect.y0, rect.x1, rect.y1],
                "ctm": ctm,
                "bboxlog": entries,
                "content_stream_size": stream_size,
                "has_form_xobjects": has_form_xobjects,
            }),
        );
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Detect per-candidate first-line indent from each page's display list.
/// `page_indices_json`: `[page_idx, ...]`; `candidates_json`:
/// `{"<page_idx>": [[x0, y0, x1, y1, font_size_pt], ...]}`. Returns
/// `{"<page_idx>": {"<cand_idx>": indent_pt}}`; unreadable pages / failed
/// renders are skipped (absent keys; the shim maps them to 0.0, matching the
/// Python reference where an empty/out-of-page clip yields empty samples and a
/// 0.0 result). See `rendering_reader::indent`.
#[pyfunction]
fn detect_first_line_indents(pdf_bytes: &[u8], page_indices_json: &str, candidates_json: &str) -> PyResult<String> {
    let page_indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let candidates: HashMap<String, Vec<[f64; 5]>> = serde_json::from_str(candidates_json)
        .map_err(|e| PyValueError::new_err(format!("candidates_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let parsed: BTreeMap<i64, Vec<([f64; 4], f64)>> = candidates
        .into_iter()
        .map(|(page, cands)| {
            (
                page.parse::<i64>().unwrap_or(-1),
                cands.into_iter()
                    .map(|[x0, y0, x1, y1, font]| ([x0, y0, x1, y1], font))
                    .collect(),
            )
        })
        .collect();
    let detected = rendering_reader::indent::detect_first_line_indents(&doc, &page_indices, &parsed);
    let mut out: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (idx, entries) in detected {
        let mut page_out = BTreeMap::new();
        for (ci, entry) in entries.iter().enumerate() {
            if let Some(indent) = entry {
                page_out.insert(ci.to_string(), *indent);
            }
        }
        if !page_out.is_empty() {
            out.insert(idx.to_string(), page_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: sample a local background fill per target rect from each
/// page's pristine content. `config_json`:
/// `{"<page>": {"batch_rects": [[x0,y0,x1,y1], ...], "target_rects": [[...], ...]}}`.
/// `batch_rects` are the cover rects that form the `LocalBackgroundSampler`
/// clip (`BACKGROUND_CLIP_SAMPLER_MIN_RECTS` gate); `target_rects` are every
/// rect whose fill the decision tree may read (an over-approximation — extra
/// results are ignored). Returns
/// `{"<page>": {"batch": {"count": N, "clip": [x0,y0,x1,y1] | null},
/// "targets": {"<i>": [r,g,b]}}}`. Mirrors
/// `color_adapt._sample_item_cover_fill` with
/// `sample_local_background_fill(page, rect, sampler=...)` where the sampler is
/// built from `batch_rects` (scale `BACKGROUND_COVER_SAMPLE_SCALE`, RGB,
/// alpha=false).
#[pyfunction]
fn sample_page_color_fills(pdf_bytes: &[u8], config_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct PageColorConfig {
        batch_rects: Vec<Vec<f64>>,
        target_rects: Vec<Vec<f64>>,
    }
    let config: HashMap<String, PageColorConfig> = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(format!("config_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 2.0;
    let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (page_key, cfg) in config {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(rect) = doc.page_rect(idx) else {
            continue;
        };
        let page_rect: RectTuple = [rect.x0, rect.y0, rect.x1, rect.y1];
        let batch_rects: Vec<RectTuple> =
            cfg.batch_rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();
        let target_rects: Vec<RectTuple> =
            cfg.target_rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();

        let render_clip = |clip: &RectTuple| -> Option<RgbPixmap> {
            let r = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
            doc.render_page_clip_rgb(idx, Some(&r), scale)
                .ok()
                .map(|px| RgbPixmap {
                    width: px.width as usize,
                    height: px.height as usize,
                    samples: px.samples,
                })
        };
        let sampler = build_local_background_sampler(&page_rect, &batch_rects, &render_clip);
        let batch_clip = batch_sampler_clip_rect(&page_rect, &batch_rects, true);
        let valid_count = batch_rects
            .iter()
            .filter(|r| !rect_is_empty(r) && rect_is_finite(r))
            .count();

        let mut targets = BTreeMap::new();
        for (i, rect) in target_rects.iter().enumerate() {
            let fill = sample_local_background_fill(&page_rect, &render_clip, rect, sampler.as_ref());
            targets.insert(i.to_string(), fill);
        }
        out.insert(
            page_key,
            serde_json::json!({
                "batch": {"count": valid_count, "clip": batch_clip},
                "targets": targets,
            }),
        );
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: structured-text span dicts per page and per clip.
/// `clips_json`: `{"<page>": [[x0,y0,x1,y1], null, ...]}` (null = full page).
/// Returns `{"<page>": {"<i>": [[x0,y0,x1,y1, color_int, "text"], ...]}}`;
/// unreadable pages are skipped. Mirrors fitz `get_text("dict", clip=...)`
/// (PRESERVE_LIGATURES; characters filtered by quad∩clip; span bbox = union of
/// surviving glyph quads).
#[pyfunction]
fn extract_page_span_dicts(pdf_bytes: &[u8], clips_json: &str) -> PyResult<String> {
    let clips: HashMap<String, Vec<Option<Vec<f64>>>> = serde_json::from_str(clips_json)
        .map_err(|e| PyValueError::new_err(format!("clips_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let mut out: BTreeMap<String, BTreeMap<String, Vec<serde_json::Value>>> = BTreeMap::new();
    for (page_key, page_clips) in clips {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(page) = doc.load_page(idx as i32) else {
            continue;
        };
        let Ok(text_page) = build_text_page_for_extraction(&page) else {
            continue;
        };
        let mut clip_out = BTreeMap::new();
        for (i, clip) in page_clips.iter().enumerate() {
            let clip_tuple: Option<RectTuple> = clip.as_ref().map(|c| [c[0], c[1], c[2], c[3]]);
            let spans = extract_span_dicts_from_text_page(&text_page, clip_tuple.as_ref())
                .map_err(|e| PyRuntimeError::new_err(format!("extract spans: {e}")))?;
            let encoded: Vec<serde_json::Value> = spans
                .iter()
                .map(|s| serde_json::json!([s.rect[0], s.rect[1], s.rect[2], s.rect[3], s.color, s.text]))
                .collect();
            clip_out.insert(i.to_string(), encoded);
        }
        out.insert(page_key, clip_out);
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: title foreground color sampled from a 3x RGB render of each
/// probe rect against its background fill. `visuals_json`:
/// `{"<page>": [{"rect": [x0,y0,x1,y1], "fill": [r,g,b]}, ...]}`. Returns
/// `{"<page>": {"<i>": [r,g,b]}}`; a probe that fails (empty clip / no text-like
/// foreground) is an absent key. Mirrors
/// `color_adapt.title_text_color_from_visual_components`
/// (`TITLE_COLOR_SAMPLE_SCALE`, device-RGB, alpha=false).
#[pyfunction]
fn sample_title_visual_colors(pdf_bytes: &[u8], visuals_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct VisualProbe {
        rect: Vec<f64>,
        fill: Vec<f64>,
    }
    let visuals: HashMap<String, Vec<VisualProbe>> = serde_json::from_str(visuals_json)
        .map_err(|e| PyValueError::new_err(format!("visuals_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 3.0;
    let mut out: BTreeMap<String, BTreeMap<String, [f64; 3]>> = BTreeMap::new();
    for (page_key, probes) in visuals {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let mut probe_out = BTreeMap::new();
        for (i, probe) in probes.iter().enumerate() {
            let rect = CoreRect::new(probe.rect[0], probe.rect[1], probe.rect[2], probe.rect[3]);
            let background = [probe.fill[0], probe.fill[1], probe.fill[2]];
            let Ok(px) = doc.render_page_clip_rgb(idx, Some(&rect), scale) else {
                continue;
            };
            let pix = RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            };
            if let Some(color) = title_foreground_color_from_pixmap(&pix, background) {
                probe_out.insert(i.to_string(), color);
            }
        }
        if !probe_out.is_empty() {
            out.insert(page_key, probe_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Visual-profile: foreground color + confidence sampled from a 3x RGB render of
/// each probe rect (clipped to the page bounds) against its background fill.
/// `probes_json`: `{"<page>": [{"rect": [x0,y0,x1,y1], "fill": [r,g,b]}, ...]}`.
/// Returns `{"<page>": {"<i>": [r,g,b, confidence]}}`; a probe that fails (rect
/// does not overlap the page / render failure / no text-like foreground) is an
/// absent key. Mirrors
/// `visual_profile/foreground.py::sample_foreground_color_from_pixels`
/// (`FOREGROUND_SAMPLE_SCALE`, device-RGB, alpha=false, rect clipped to
/// `page.rect`).
#[pyfunction]
fn sample_foreground_colors(pdf_bytes: &[u8], probes_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct VisualProbe {
        rect: Vec<f64>,
        fill: Vec<f64>,
    }
    let probes: HashMap<String, Vec<VisualProbe>> = serde_json::from_str(probes_json)
        .map_err(|e| PyValueError::new_err(format!("probes_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 3.0;
    let mut out: BTreeMap<String, BTreeMap<String, [f64; 4]>> = BTreeMap::new();
    for (page_key, page_probes) in probes {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(pr) = doc.page_rect(idx) else {
            continue;
        };
        let page_rect: RectTuple = [pr.x0, pr.y0, pr.x1, pr.y1];
        let mut probe_out = BTreeMap::new();
        for (i, probe) in page_probes.iter().enumerate() {
            let rect: RectTuple = [probe.rect[0], probe.rect[1], probe.rect[2], probe.rect[3]];
            let clipped = rect_intersection(&rect, &page_rect);
            if rect_is_empty(&clipped) {
                continue;
            }
            let clip_rect = CoreRect::new(clipped[0], clipped[1], clipped[2], clipped[3]);
            let Ok(px) = doc.render_page_clip_rgb(idx, Some(&clip_rect), scale) else {
                continue;
            };
            let pix = RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            };
            let background = [probe.fill[0], probe.fill[1], probe.fill[2]];
            let (color, confidence) = foreground_color_from_pixmap(&pix, background);
            if let Some(color) = color {
                probe_out.insert(i.to_string(), [color[0], color[1], color[2], confidence]);
            }
        }
        if !probe_out.is_empty() {
            out.insert(page_key, probe_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

#[pymodule]
fn rendering_bridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(emit_typst_source, m)?)?;
    m.add_function(wrap_pyfunction!(emit_typst_book_overlay_source, m)?)?;
    m.add_function(wrap_pyfunction!(compile_typst_source, m)?)?;
    m.add_function(wrap_pyfunction!(strip_bbox_text_rects, m)?)?;
    m.add_function(wrap_pyfunction!(strip_hidden_text, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_invalid_xobjects, m)?)?;
    m.add_function(wrap_pyfunction!(compress_images, m)?)?;
    m.add_function(wrap_pyfunction!(extract_pages, m)?)?;
    m.add_function(wrap_pyfunction!(overlay_page, m)?)?;
    m.add_function(wrap_pyfunction!(clean_background, m)?)?;
    m.add_function(wrap_pyfunction!(build_clean_background_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(save_optimized_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_sizes, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_geometry, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_drawing_count, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_image_rects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_text_spans, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_text_blocks, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_math_rects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_span_heights, m)?)?;
    m.add_function(wrap_pyfunction!(collect_vector_text_rects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_cleanup_contexts, m)?)?;
    m.add_function(wrap_pyfunction!(detect_first_line_indents, m)?)?;
    m.add_function(wrap_pyfunction!(sample_page_color_fills, m)?)?;
    m.add_function(wrap_pyfunction!(extract_page_span_dicts, m)?)?;
    m.add_function(wrap_pyfunction!(sample_title_visual_colors, m)?)?;
    m.add_function(wrap_pyfunction!(sample_foreground_colors, m)?)?;
    Ok(())
}
