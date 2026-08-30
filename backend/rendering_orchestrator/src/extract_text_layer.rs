//! Native `extract_text_layer` worker mirroring `entrypoints/run_extract_text_layer.py`:
//! reads a PDF's embedded text layer and emits a `generic_flat_ocr` payload the
//! document_schema adapter normalizes into `document.v1` downstream — the "skip
//! OCR" path. `render_rs --extract-text-layer --spec` replaces the python3
//! subprocess so a skip-OCR job needs no interpreter.
//!
//! Block extraction uses `rendering_reader::PdfDocument::page_text_blocks`
//! (fitz `get_text("blocks")` semantics: text blocks with lines joined by
//! "\n"), which matches the python reference's `get_text("dict")` `_block_text`
//! for text-layer documents (the reader's span aggregation uses the same
//! PRESERVE_LIGATURES|PRESERVE_WHITESPACE flags; known whitespace-edge
//! divergence is documented in the parity smoke, not a render-critical gap).

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rendering_reader::PdfDocument;
use serde_json::{json, Value};

pub const EXTRACT_TEXT_LAYER_STAGE_SCHEMA_VERSION: &str = "extract_text_layer.stage.v1";
pub const PROVIDER_GENERIC_FLAT_OCR: &str = "generic_flat_ocr";
pub const DEFAULT_UNIT: &str = "pt";

/// serde mirror of `foundation/shared/stage_specs.py::ExtractTextLayerStageSpec`
/// (`extract_text_layer.stage.v1`). Only the inputs the worker consumes are typed.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractTextLayerStageSpec {
    pub schema_version: String,
    pub stage: String,
    pub job: ExtractTextLayerStageJob,
    pub inputs: ExtractTextLayerStageInputs,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractTextLayerStageJob {
    pub job_root: std::path::PathBuf,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractTextLayerStageInputs {
    pub source_pdf: std::path::PathBuf,
    pub output_json: std::path::PathBuf,
}

impl ExtractTextLayerStageSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read extract_text_layer spec: {}", path.display()))?;
        let spec: ExtractTextLayerStageSpec = serde_json::from_str(&text)
            .with_context(|| format!("parse extract_text_layer spec: {}", path.display()))?;
        if spec.schema_version != EXTRACT_TEXT_LAYER_STAGE_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported extract_text_layer schema_version: {} (expected {EXTRACT_TEXT_LAYER_STAGE_SCHEMA_VERSION})",
                spec.schema_version
            );
        }
        if spec.stage != "extract_text_layer" {
            anyhow::bail!("unexpected stage spec kind: {}", spec.stage);
        }
        Ok(spec)
    }
}

/// One page's text-layer block (the json `blocks[]` entry source).
struct TextLayerBlock {
    bbox: [f64; 4],
    text: String,
}

/// `build_text_layer_document` — open the PDF and walk every page, extracting
/// text blocks. A page whose text layer cannot be read yields no blocks
/// (mirrors the python `try: get_text("dict") except: {"blocks": []}`); a page
/// whose geometry cannot be read propagates (python raises on `page.rect`).
pub fn build_text_layer_document(source_pdf: &Path) -> Result<Value> {
    let doc = mupdf::Document::open(source_pdf)
        .map_err(|e| anyhow!("text layer open {}: {e}", source_pdf.display()))?;
    let page_count = PdfDocument::page_count(&doc).map_err(|e| anyhow!("page_count: {e}"))?;
    let mut pages_out = Vec::with_capacity(page_count.max(0) as usize);
    let mut total_blocks: usize = 0;
    for page_index in 0..page_count {
        let rect = doc
            .page_rect(page_index)
            .map_err(|e| anyhow!("page_rect p{page_index}: {e}"))?;
        let blocks: Vec<TextLayerBlock> = doc
            .page_text_blocks(page_index)
            .into_iter()
            .map(|(bbox, text)| TextLayerBlock {
                bbox: [bbox.x0, bbox.y0, bbox.x1, bbox.y1],
                text,
            })
            .collect();
        total_blocks += blocks.len();
        pages_out.push(json!({
            "page_index": page_index,
            "width": rect.width(),
            "height": rect.height(),
            "unit": DEFAULT_UNIT,
            "blocks": blocks
                .into_iter()
                .map(|block| json!({
                    "bbox": block.bbox,
                    "type": "text",
                    "sub_type": "body",
                    "text": block.text,
                }))
                .collect::<Vec<Value>>(),
        }));
    }
    if total_blocks == 0 {
        anyhow::bail!("源 PDF 不含可提取的文本层，无法跳过 OCR；请改用 OCR 识别");
    }
    Ok(json!({ "provider": PROVIDER_GENERIC_FLAT_OCR, "pages": pages_out }))
}

/// `entrypoints/run_extract_text_layer.py::main` — load the stage spec, extract
/// the text layer, write the compact `generic_flat_ocr` JSON, and print the
/// production stdout labels (`job root:`/`source pdf:`/`normalized document
/// json:`/`schema version: document.v1`/`text layer extracted:`).
pub fn extract_text_layer(spec_path: &Path) -> Result<Value> {
    let spec = ExtractTextLayerStageSpec::load(spec_path)?;
    let source_pdf = &spec.inputs.source_pdf;
    let output_json = &spec.inputs.output_json;
    if !source_pdf.exists() {
        anyhow::bail!("source pdf not found: {}", source_pdf.display());
    }
    let document = build_text_layer_document(source_pdf)?;
    if let Some(parent) = output_json.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create output dir: {}", parent.display())
        })?;
    }
    std::fs::write(output_json, serde_json::to_string(&document)?)
        .with_context(|| format!("write text layer json: {}", output_json.display()))?;
    let page_count = document["pages"].as_array().map_or(0, |pages| pages.len());
    let block_count = document["pages"].as_array().map_or(0, |pages| {
        pages
            .iter()
            .map(|page| page["blocks"].as_array().map_or(0, |blocks| blocks.len()))
            .sum()
    });
    println!("job root: {}", spec.job.job_root.display());
    println!("source pdf: {}", source_pdf.display());
    println!("normalized document json: {}", output_json.display());
    println!("schema version: document.v1");
    println!(
        "text layer extracted: pages={page_count} blocks={block_count} path={}",
        output_json.display()
    );
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let unique = format!(
                "{label}-{}-{:?}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn write_spec(root: &Path, source_pdf: &Path, output_json: &Path) -> std::path::PathBuf {
        let spec_path = root.join("extract_text_layer.spec.json");
        let payload = json!({
            "schema_version": EXTRACT_TEXT_LAYER_STAGE_SCHEMA_VERSION,
            "stage": "extract_text_layer",
            "job": {"job_id": "tl-test", "job_root": root, "workflow": "normalize"},
            "inputs": {
                "source_pdf": source_pdf,
                "output_json": output_json,
            },
            "params": {},
        });
        std::fs::write(&spec_path, serde_json::to_vec(&payload).unwrap()).unwrap();
        spec_path
    }

    #[test]
    fn spec_load_accepts_written_shape() {
        let root = TempDir::new("tl-spec");
        let spec_path = write_spec(root.path(), &root.path().join("source.pdf"), &root.path().join("out.json"));
        let spec = ExtractTextLayerStageSpec::load(&spec_path).expect("load");
        assert_eq!(spec.inputs.source_pdf, root.path().join("source.pdf"));
        assert_eq!(spec.inputs.output_json, root.path().join("out.json"));
    }

    #[test]
    fn spec_load_rejects_wrong_schema() {
        let root = TempDir::new("tl-spec-bad");
        let spec_path = root.path().join("bad.json");
        std::fs::write(
            &spec_path,
            br#"{"schema_version":"render.stage.v1","stage":"extract_text_layer","inputs":{}}"#,
        )
        .unwrap();
        assert!(ExtractTextLayerStageSpec::load(&spec_path).is_err());
    }

    #[test]
    fn extract_text_layer_missing_source_fails() {
        let root = TempDir::new("tl-missing");
        let spec_path = write_spec(root.path(), &root.path().join("nope.pdf"), &root.path().join("out.json"));
        let err = extract_text_layer(&spec_path).unwrap_err();
        assert!(err.to_string().contains("source pdf not found"));
    }
}
