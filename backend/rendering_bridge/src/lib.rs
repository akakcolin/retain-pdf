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
//!   * `emit_render_blocks(block_payloads_json) -> str` — the C3-N2 emit
//!     boundary (port of `payload/emit.py`); `block_payloads_json` is the
//!     `build_block_payloads` output (+ body-pipeline keys), the result is the
//!     `RenderBlock` DTO array.
//!   * `apply_body_pipeline(ordered_payloads_json, page_text_width_med,
//!     book_body_font_target, font_unify_mode) -> str` — the C3-N3 body-pipeline
//!     boundary (port of `payload/body_pipeline.apply_body_payload_pipeline`
//!     plus the post-pipeline annotation stages); returns the mutated payloads.
//!   * `resolve_book_body_font_target(pages_json) -> str` — the whole-book body
//!     font target (port of
//!     `payload/body_font_unify_policy.resolve_book_body_font_target`).
//!   * `mark_adjacent_collision_risk(ordered_payloads_json) -> str` — the C3-N4
//!     adjacent-body collision-risk boundary (port of
//!     `payload/collision.mark_adjacent_collision_risk`); returns the mutated
//!     payloads.
//!   * `seed_render_fields(translated_items_json) -> str` — the C3-N5 block-seed
//!     boundary (port of `payload/render_item.seed_render_fields`); seeds each
//!     translated item in place and returns the updated array.
//!   * `prepare_render_payloads_by_page(translated_pages_json,
//!     first_line_indent_lookup_json, effective_inner_bbox_lookup_json) -> str` —
//!     the C3-N7 boundary (port of `payload/prepare.prepare_render_payloads_by_page`);
//!     deep-copies the translated pages, seeds/splits/drops, returns the prepared
//!     page map.
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
//!
//! The crate is organised as a per-domain module split (each module owns its
//! `#[pyfunction]`s verbatim plus a `pub(crate) fn register` that adds them to
//! the module); this file is the composition root wiring each domain in.

use pyo3::prelude::*;

mod analysis;
mod background;
mod color;
mod helpers;
mod layout;
mod read;
mod source_cleanup;
mod typst;
mod write;

#[pymodule]
fn rendering_bridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    analysis::register(m)?;
    background::register(m)?;
    color::register(m)?;
    layout::register(m)?;
    read::register(m)?;
    source_cleanup::register(m)?;
    typst::register(m)?;
    write::register(m)?;
    Ok(())
}
