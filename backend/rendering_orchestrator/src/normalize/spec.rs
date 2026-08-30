// serde mirror of `foundation/shared/stage_specs.py::NormalizeStageSpec`
// (`normalize.stage.v1`), the normalize worker's spec. Only the fields the
// worker consumes are typed; unknown fields are ignored.

use std::path::Path;

use anyhow::{bail, Context, Result};

pub const NORMALIZE_STAGE_SCHEMA_VERSION: &str = "normalize.stage.v1";

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NormalizeStageSpec {
    pub schema_version: String,
    pub stage: String,
    pub job: NormalizeStageJob,
    pub inputs: NormalizeStageInputs,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NormalizeStageJob {
    pub job_root: std::path::PathBuf,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NormalizeStageInputs {
    #[serde(default)]
    pub provider: String,
    pub source_json: std::path::PathBuf,
    pub source_pdf: std::path::PathBuf,
    #[serde(default)]
    pub provider_version: String,
    #[serde(default)]
    pub provider_result_json: Option<std::path::PathBuf>,
    #[serde(default)]
    pub provider_zip: Option<std::path::PathBuf>,
    #[serde(default)]
    pub provider_raw_dir: Option<std::path::PathBuf>,
}

impl NormalizeStageSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read normalize spec: {}", path.display()))?;
        let spec: NormalizeStageSpec = serde_json::from_str(&text)
            .with_context(|| format!("parse normalize spec: {}", path.display()))?;
        if spec.schema_version != NORMALIZE_STAGE_SCHEMA_VERSION {
            bail!(
                "unsupported normalize schema_version: {} (expected {NORMALIZE_STAGE_SCHEMA_VERSION})",
                spec.schema_version
            );
        }
        if spec.stage != "normalize" {
            bail!("unexpected stage spec kind: {}", spec.stage);
        }
        Ok(spec)
    }
}
