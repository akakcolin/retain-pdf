//! serde mirror of `foundation/shared/stage_specs.py::RenderStageSpec`
//! (`render.stage.v1`). Only the fields the orchestrator consumes directly are
//! typed; everything else the delegate re-reads from the spec file itself.
//! Params are `Option` so a missing or `null` value falls back to the same
//! Python defaults (`render_mode` -> `"typst"`, `typst_font_family` ->
//! `TYPST_DEFAULT_FONT_FAMILY`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const RENDER_STAGE_SCHEMA_VERSION: &str = "render.stage.v1";
pub const TYPST_DEFAULT_FONT_FAMILY: &str = "Source Han Serif SC";

#[derive(Debug, Clone, Deserialize)]
pub struct RenderStageSpec {
    pub schema_version: String,
    pub stage: String,
    pub job: StageJobRef,
    pub inputs: RenderStageInputs,
    pub params: RenderStageParams,
}

impl RenderStageSpec {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let spec: RenderStageSpec = serde_json::from_str(&text)?;
        spec.validate()?;
        Ok(spec)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.schema_version != RENDER_STAGE_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported render stage schema_version: {} (expected {RENDER_STAGE_SCHEMA_VERSION})",
                self.schema_version
            );
        }
        if self.stage != "render" {
            anyhow::bail!("unexpected stage spec kind: {}", self.stage);
        }
        if !self.inputs.source_pdf.exists() {
            anyhow::bail!("source pdf not found: {}", self.inputs.source_pdf.display());
        }
        if !self.inputs.translations_dir.exists() {
            anyhow::bail!(
                "translations dir not found: {}",
                self.inputs.translations_dir.display()
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StageJobRef {
    pub job_id: String,
    pub job_root: PathBuf,
    pub workflow: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderStageInputs {
    pub source_pdf: PathBuf,
    pub translations_dir: PathBuf,
    #[serde(default)]
    pub translation_manifest: Option<PathBuf>,
}

/// `Option` fields so a missing or `null` value falls back to the Python
/// defaults via the accessors below.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RenderStageParams {
    #[serde(default)]
    pub start_page: Option<i64>,
    #[serde(default)]
    pub end_page: Option<i64>,
    #[serde(default)]
    pub render_mode: Option<String>,
    #[serde(default)]
    pub compile_workers: Option<i64>,
    #[serde(default)]
    pub typst_font_family: Option<String>,
    #[serde(default)]
    pub pdf_compress_dpi: Option<i64>,
    #[serde(default)]
    pub translated_pdf_name: Option<String>,
    #[serde(default)]
    pub body_font_size_factor: Option<f64>,
    #[serde(default)]
    pub body_leading_factor: Option<f64>,
    #[serde(default)]
    pub inner_bbox_shrink_x: Option<f64>,
    #[serde(default)]
    pub inner_bbox_shrink_y: Option<f64>,
    #[serde(default)]
    pub inner_bbox_dense_shrink_x: Option<f64>,
    #[serde(default)]
    pub inner_bbox_dense_shrink_y: Option<f64>,
    #[serde(default)]
    pub font_unify_mode: Option<String>,
    #[serde(default)]
    pub source_cleanup_strategy: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
}

impl RenderStageParams {
    /// `str(params_payload.get("render_mode", "typst") or "typst")`.
    pub fn render_mode_str(&self) -> String {
        let raw = self.render_mode.as_deref().unwrap_or("typst").trim();
        if raw.is_empty() {
            "typst".to_string()
        } else {
            raw.to_string()
        }
    }

    /// `str(... or "").strip() or fonts.TYPST_DEFAULT_FONT_FAMILY`.
    pub fn typst_font_family(&self) -> String {
        let raw = self.typst_font_family.as_deref().unwrap_or("").trim();
        if raw.is_empty() {
            TYPST_DEFAULT_FONT_FAMILY.to_string()
        } else {
            raw.to_string()
        }
    }

    pub fn start_page(&self) -> i64 {
        self.start_page.unwrap_or(0)
    }

    pub fn end_page(&self) -> i64 {
        self.end_page.unwrap_or(-1)
    }

    pub fn compile_workers(&self) -> i64 {
        self.compile_workers.unwrap_or(0)
    }

    pub fn pdf_compress_dpi(&self) -> i64 {
        self.pdf_compress_dpi.unwrap_or(0)
    }
}
