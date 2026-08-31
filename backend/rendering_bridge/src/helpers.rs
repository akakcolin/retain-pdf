//! Shared pyfunction helpers: rect-map parsing, temp-dir round-trips, and
//! write-path metadata serialization. Consumed by the write / background / read
//! / source_cleanup / color modules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_writer::save::{delete_trailer_id, save_atomic};

/// Deserialize `{"<page>": [[x0, y0, x1, y1], ...]}` into strip-API rects.
pub(crate) fn parse_rect_map(json: &str, label: &str) -> PyResult<HashMap<i32, Vec<RectTuple>>> {
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
pub(crate) fn temp_dir() -> PyResult<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rpbridge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| PyRuntimeError::new_err(format!("mkdir {dir:?}: {e}")))?;
    Ok(dir)
}

pub(crate) fn save_to_bytes(pdf: &PdfDocument, dir: &Path) -> PyResult<Vec<u8>> {
    delete_trailer_id(pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_atomic(pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

/// Open `pdf_bytes`, run `edit` (which may replace/append content), and return
/// the deterministically saved PDF bytes plus the edit's result value.
pub(crate) fn with_pdf_edit<F, R>(pdf_bytes: &[u8], edit: F) -> PyResult<(Vec<u8>, R)>
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
pub(crate) fn meta_json<T: serde::Serialize>(value: &T) -> PyResult<String> {
    serde_json::to_string(value)
        .map_err(|e| PyRuntimeError::new_err(format!("metadata json: {e}")))
}
