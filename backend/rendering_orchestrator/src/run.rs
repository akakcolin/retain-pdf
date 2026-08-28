//! Orchestration sequence + mode dispatch.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;

use crate::delegate;
use crate::spec::RenderStageSpec;
use crate::stages;

pub struct RenderOutcome {
    pub output_pdf: PathBuf,
    pub source_pdf: PathBuf,
    pub translations_dir: PathBuf,
    pub mode: String,
    pub page_count: usize,
    pub elapsed_seconds: f64,
}

pub fn run(spec_path: &Path) -> Result<RenderOutcome> {
    let started = Instant::now();
    let spec = RenderStageSpec::load(spec_path)?;
    let mode = spec.params.render_mode_str();
    if !matches!(mode.as_str(), "typst" | "typst_visual") {
        anyhow::bail!(
            "render mode {mode:?} not supported by render_rs (C1 supports typst/typst_visual only)"
        );
    }

    let bundle_out = spec.job.job_root.join("render-bundle.json");
    let bundle = delegate::run_delegate(spec_path, &bundle_out)?;
    let cleaned_bg = stages::background::run_background(&bundle)?;
    let compiled_pdf = stages::typst::run_typst(&bundle, &cleaned_bg)?;
    stages::save::run_save(&bundle, &compiled_pdf)?;

    Ok(RenderOutcome {
        output_pdf: bundle.output_pdf,
        source_pdf: bundle.source_pdf,
        translations_dir: spec.inputs.translations_dir,
        mode,
        page_count: bundle.page_map.source_page_indices.len(),
        elapsed_seconds: started.elapsed().as_secs_f64(),
    })
}
