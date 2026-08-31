//! Layout / payload pipeline entry points (C3-N2..N8): block seeding, emit,
//! body pipeline, font-target resolution, collision marking, render-field
//! seeding, per-page payload preparation, and render-pages policy fields. Pure
//! JSON DTO transformations — no PDF editing. Ports of `payload/` and
//! `policy/cleanup_policy.py`.

use std::collections::BTreeMap;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// `build_block_payloads(translated_items_json, page_width, page_height) ->
/// str` — the C3-N2 seed boundary (port of
/// `payload/block_seed.build_block_payloads`). `translated_items_json` is a JSON
/// array of the translated-item dicts (post `seed_render_fields`); returns
/// `{"block_payloads": [...], "page_text_width_med": float}`.
#[pyfunction(name = "build_block_payloads")]
fn build_block_payloads(
    translated_items_json: &str,
    page_width: Option<f64>,
    page_height: Option<f64>,
) -> PyResult<String> {
    let raw_items: Vec<serde_json::Value> = serde_json::from_str(translated_items_json)
        .map_err(|e| PyValueError::new_err(format!("translated_items_json: {e}")))?;
    let items: Vec<rendering_core::item::Item> =
        raw_items.iter().map(rendering_core::item::Item::from_json_value).collect();
    let (block_payloads, page_text_width_med) =
        rendering_core::payload::block_seed::build_block_payloads(&items, &raw_items, page_width, page_height);
    let out = serde_json::json!({
        "block_payloads": block_payloads,
        "page_text_width_med": page_text_width_med,
    });
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

#[pyfunction]
fn emit_render_blocks(block_payloads_json: &str) -> PyResult<String> {
    let block_payloads: Vec<serde_json::Value> = serde_json::from_str(block_payloads_json)
        .map_err(|e| PyValueError::new_err(format!("block_payloads_json: {e}")))?;
    let blocks = rendering_core::payload::emit::emit_render_blocks(&block_payloads);
    serde_json::to_string(&blocks).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `apply_body_pipeline(ordered_payloads_json, page_text_width_med,
/// book_body_font_target, font_unify_mode) -> str` — the C3-N3 body-pipeline
/// boundary (ports `payload/body_pipeline.apply_body_payload_pipeline` plus the
/// post-pipeline annotation stages `annotation_font_policy.unify_annotation_fonts`
/// when `font_unify_mode != "off"` and `recover_underfilled_annotation_density`).
/// Mutates the ordered payload dicts in place and returns the updated array, so
/// the Python shim can write the dicts back onto the shared references.
#[pyfunction]
fn apply_body_pipeline(
    ordered_payloads_json: &str,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
    font_unify_mode: &str,
) -> PyResult<String> {
    let mut ordered_payloads: Vec<serde_json::Value> = serde_json::from_str(ordered_payloads_json)
        .map_err(|e| PyValueError::new_err(format!("ordered_payloads_json: {e}")))?;
    rendering_core::payload::body_pipeline::apply_body_payload_pipeline(
        &mut ordered_payloads,
        page_text_width_med,
        book_body_font_target,
        font_unify_mode,
    );
    if font_unify_mode != "off" {
        rendering_core::payload::body_policy_facade::unify_annotation_fonts(&mut ordered_payloads);
    }
    rendering_core::payload::body_policy_facade::recover_underfilled_annotation_density(&mut ordered_payloads);
    serde_json::to_string(&ordered_payloads).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `resolve_book_body_font_target(pages_json) -> str` — the whole-book body font
/// target (port of `payload/body_font_unify_policy.resolve_book_body_font_target`).
/// `pages_json` is an array of `[block_payloads, page_text_width_med]` tuples;
/// returns `null` or the low stable body font.
#[pyfunction]
fn resolve_book_body_font_target(pages_json: &str) -> PyResult<String> {
    let pages: Vec<(Vec<serde_json::Value>, f64)> = serde_json::from_str(pages_json)
        .map_err(|e| PyValueError::new_err(format!("pages_json: {e}")))?;
    let target = rendering_core::layout::body_font_unify_policy::resolve_book_body_font_target(&pages);
    serde_json::to_string(&target).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `mark_adjacent_collision_risk(ordered_payloads_json) -> str` — the C3-N4
/// adjacent-body collision-risk boundary (port of
/// `payload/collision.mark_adjacent_collision_risk`). Mutates the ordered
/// payload dicts in place and returns the updated array, so the Python shim can
/// write the dicts back onto the shared references.
#[pyfunction]
fn mark_adjacent_collision_risk(ordered_payloads_json: &str) -> PyResult<String> {
    let mut ordered_payloads: Vec<serde_json::Value> = serde_json::from_str(ordered_payloads_json)
        .map_err(|e| PyValueError::new_err(format!("ordered_payloads_json: {e}")))?;
    rendering_core::layout::collision::mark_adjacent_collision_risk(&mut ordered_payloads);
    serde_json::to_string(&ordered_payloads).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `seed_render_fields(translated_items_json) -> str` — the C3-N5 seed boundary
/// (port of `payload/render_item.seed_render_fields`). Seeds every translated
/// item dict in place (render_protected_text / render_source_text /
/// render_formula_map plus the preserve-line-break flags) and returns the
/// updated array so the Python shim can write the dicts back.
#[pyfunction]
fn seed_render_fields(translated_items_json: &str) -> PyResult<String> {
    let mut translated_items: Vec<serde_json::Value> = serde_json::from_str(translated_items_json)
        .map_err(|e| PyValueError::new_err(format!("translated_items_json: {e}")))?;
    for item in translated_items.iter_mut() {
        rendering_core::layout::render_item::seed_render_fields(item);
    }
    serde_json::to_string(&translated_items)
        .map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `prepare_render_payloads_by_page(translated_pages_json,
/// first_line_indent_lookup_json, effective_inner_bbox_lookup_json) -> str` —
/// the C3-N7 boundary (port of `payload/prepare.prepare_render_payloads_by_page`).
/// `translated_pages_json` is a `{page_idx: [item, ...]}` object; the two
/// lookups are optional precomputed `{item_id: value}` maps (the Python shim
/// resolves `source_pdf_path` into the first-line-indent lookup before calling).
/// Deep-copies the input and returns the prepared page map, leaving the caller's
/// dicts untouched.
#[pyfunction]
fn prepare_render_payloads_by_page(
    translated_pages_json: &str,
    first_line_indent_lookup_json: Option<&str>,
    effective_inner_bbox_lookup_json: Option<&str>,
) -> PyResult<String> {
    let translated_pages: BTreeMap<i64, Vec<serde_json::Value>> =
        serde_json::from_str(translated_pages_json)
            .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let first_line_indent_lookup: Option<BTreeMap<String, f64>> =
        match first_line_indent_lookup_json {
            Some(s) => Some(
                serde_json::from_str(s)
                    .map_err(|e| PyValueError::new_err(format!("first_line_indent_lookup_json: {e}")))?,
            ),
            None => None,
        };
    let effective_inner_bbox_lookup: Option<BTreeMap<String, Vec<f64>>> =
        match effective_inner_bbox_lookup_json {
            Some(s) => Some(
                serde_json::from_str(s).map_err(|e| {
                    PyValueError::new_err(format!("effective_inner_bbox_lookup_json: {e}"))
                })?,
            ),
            None => None,
        };
    let prepared = rendering_core::payload::prepare::prepare_render_payloads_by_page(
        &translated_pages,
        first_line_indent_lookup.as_ref(),
        effective_inner_bbox_lookup.as_ref(),
    );
    serde_json::to_string(&prepared).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// `apply_render_pages_policy_fields(translated_pages_json,
/// use_typst_fill_cleanup, use_default_text_overlay_cover_fill) -> str` — the
/// C3-N8 boundary (port of `policy/cleanup_policy.apply_render_pages_policy_fields`).
/// Patches `_render_policy` onto the translated items per page and returns the
/// patched page map. The two config flags are runtime settings the Python shim
/// resolves from the layout config at call time.
#[pyfunction]
fn apply_render_pages_policy_fields(
    translated_pages_json: &str,
    use_typst_fill_cleanup: bool,
    use_default_text_overlay_cover_fill: bool,
) -> PyResult<String> {
    let translated_pages: BTreeMap<i64, Vec<serde_json::Value>> =
        serde_json::from_str(translated_pages_json)
            .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let prepared = rendering_core::layout::policy_fields::apply_render_pages_policy_fields(
        &translated_pages,
        use_typst_fill_cleanup,
        use_default_text_overlay_cover_fill,
    );
    serde_json::to_string(&prepared).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build_block_payloads, m)?)?;
    m.add_function(wrap_pyfunction!(emit_render_blocks, m)?)?;
    m.add_function(wrap_pyfunction!(apply_body_pipeline, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_book_body_font_target, m)?)?;
    m.add_function(wrap_pyfunction!(mark_adjacent_collision_risk, m)?)?;
    m.add_function(wrap_pyfunction!(seed_render_fields, m)?)?;
    m.add_function(wrap_pyfunction!(prepare_render_payloads_by_page, m)?)?;
    m.add_function(wrap_pyfunction!(apply_render_pages_policy_fields, m)?)?;
    Ok(())
}
