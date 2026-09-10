// Port of the retired `ocr_provider/paddle_normalize.py::rescale_document_geometry_to_pdf` —
// scale every bbox to the real PDF page geometry (fitz -> mupdf). Runs
// unconditionally after the contract enrichment. `round(x, 3)` uses banker's
// rounding via `rendering_core::util::py_round`, matching CPython exactly.

use std::path::Path;

use anyhow::{anyhow, Result};
use rendering_core::util::py_round;
use rendering_reader::{open_document, PdfDocument};
use serde_json::{json, Value};

/// `scale_bbox` — unchanged unless a 4-number list, then round(x*s, 3) each.
fn scale_bbox(value: &Value, scale_x: f64, scale_y: f64) -> Value {
    match value.as_array() {
        Some(arr) if arr.len() == 4 => {
            let x0 = arr[0].as_f64().unwrap_or(0.0) * scale_x;
            let y0 = arr[1].as_f64().unwrap_or(0.0) * scale_y;
            let x1 = arr[2].as_f64().unwrap_or(0.0) * scale_x;
            let y1 = arr[3].as_f64().unwrap_or(0.0) * scale_y;
            json!([py_round(x0, 3), py_round(y0, 3), py_round(x1, 3), py_round(y1, 3)])
        }
        _ => value.clone(),
    }
}

/// `scale_point_list` — 2-element list items scaled and rounded, others kept.
fn scale_point_list(value: &Value, scale_x: f64, scale_y: f64) -> Value {
    match value.as_array() {
        Some(items) => {
            let mut out: Vec<Value> = Vec::with_capacity(items.len());
            for item in items {
                match item.as_array() {
                    Some(arr) if arr.len() == 2 => {
                        let x = arr[0].as_f64().unwrap_or(0.0) * scale_x;
                        let y = arr[1].as_f64().unwrap_or(0.0) * scale_y;
                        out.push(json!([py_round(x, 3), py_round(y, 3)]));
                    }
                    _ => out.push(item.clone()),
                }
            }
            Value::Array(out)
        }
        _ => value.clone(),
    }
}

/// `rescale_document_geometry_to_pdf` — in-place rescale of the normalized
/// document's page/block/line/span/source/metadata geometry to the source PDF.
pub fn rescale_document_geometry_to_pdf(document: &mut Value, source_pdf_path: &Path) -> Result<()> {
    let doc = open_document(source_pdf_path)
        .map_err(|e| anyhow!("rescale open {}: {e}", source_pdf_path.display()))?;
    let pdf_count = PdfDocument::page_count(&doc).map_err(|e| anyhow!("page_count: {e}"))?;
    let Some(pages) = document.get_mut("pages").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for (page_index, page) in pages.iter_mut().enumerate() {
        if page_index >= pdf_count as usize {
            break;
        }
        let rect = doc
            .page_rect(page_index as i64)
            .map_err(|e| anyhow!("page_rect p{page_index}: {e}"))?;
        let pdf_w = rect.width();
        let pdf_h = rect.height();
        let raw_w = page.get("width").and_then(Value::as_f64).unwrap_or(0.0);
        let raw_h = page.get("height").and_then(Value::as_f64).unwrap_or(0.0);
        if raw_w <= 0.0 || raw_h <= 0.0 {
            page["width"] = Value::from(pdf_w);
            page["height"] = Value::from(pdf_h);
            continue;
        }
        let scale_x = pdf_w / raw_w;
        let scale_y = pdf_h / raw_h;
        page["width"] = Value::from(pdf_w);
        page["height"] = Value::from(pdf_h);
        let Some(blocks) = page.get_mut("blocks").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks.iter_mut() {
            if let Some(b) = block.get_mut("bbox") {
                let scaled = scale_bbox(b, scale_x, scale_y);
                *b = scaled;
            }
            if let Some(lines) = block.get_mut("lines").and_then(Value::as_array_mut) {
                for line in lines.iter_mut() {
                    if let Some(lb) = line.get_mut("bbox") {
                        let scaled = scale_bbox(lb, scale_x, scale_y);
                        *lb = scaled;
                    }
                    if let Some(spans) = line.get_mut("spans").and_then(Value::as_array_mut) {
                        for span in spans.iter_mut() {
                            if let Some(sb) = span.get_mut("bbox") {
                                let scaled = scale_bbox(sb, scale_x, scale_y);
                                *sb = scaled;
                            }
                        }
                    }
                }
            }
            if let Some(segments) = block.get_mut("segments").and_then(Value::as_array_mut) {
                for segment in segments.iter_mut() {
                    if !segment.is_object() {
                        continue;
                    }
                    if let Some(sb) = segment.get_mut("bbox") {
                        let scaled = scale_bbox(sb, scale_x, scale_y);
                        *sb = scaled;
                    }
                }
            }
            if let Some(source) = block.get_mut("source") {
                if source.is_object() {
                    if let Some(rb) = source.get_mut("raw_bbox") {
                        let scaled = scale_bbox(rb, scale_x, scale_y);
                        *rb = scaled;
                    }
                }
            }
            if let Some(metadata) = block.get_mut("metadata").and_then(Value::as_object_mut) {
                // Python `if metadata:` is truthy-gated — an empty dict (defaulted
                // onto blocks that lack one) skips the polygon insert entirely.
                if !metadata.is_empty() {
                    let raw_polygon = metadata
                        .get("raw_polygon")
                        .cloned()
                        .unwrap_or_else(|| Value::Array(vec![]));
                    metadata.insert(
                        "raw_polygon".to_string(),
                        scale_point_list(&raw_polygon, scale_x, scale_y),
                    );
                    let layout_det_polygon = metadata
                        .get("layout_det_polygon")
                        .cloned()
                        .unwrap_or_else(|| Value::Array(vec![]));
                    metadata.insert(
                        "layout_det_polygon".to_string(),
                        scale_point_list(&layout_det_polygon, scale_x, scale_y),
                    );
                }
            }
        }
    }
    Ok(())
}
