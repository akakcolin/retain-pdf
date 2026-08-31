//! Orchestration sequence + mode dispatch.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;

use crate::bundle::RenderBundle;
use crate::bundle_builder;
use crate::native_stats::subsystem as sub;
use crate::native_stats::{self, NativeStats};
use crate::spec::RenderStageSpec;
use crate::stages;
use crate::summary;

pub struct RenderOutcome {
    pub output_pdf: PathBuf,
    pub source_pdf: PathBuf,
    pub translations_dir: PathBuf,
    pub summary_path: PathBuf,
    pub stats_path: PathBuf,
    pub mode: String,
    pub page_count: usize,
    pub elapsed_seconds: f64,
}

pub fn run(spec_path: &Path) -> Result<RenderOutcome> {
    let started = Instant::now();
    let spec = RenderStageSpec::load(spec_path)?;
    let bundle_out = spec.job.job_root.join("render-bundle.json");
    let mut stats = NativeStats::new();
    // C3-N11: build the bundle natively (in-process mirror of the retired
    // `run_render_delegate.py` prepare/page-specs segment); the delegate and its
    // `RETAINPDF_RENDER_BUNDLE_NATIVE` gate were retired with it.
    let bundle: RenderBundle = {
        let value = bundle_builder::build_bundle(&spec, &mut stats)?;
        std::fs::write(&bundle_out, serde_json::to_string_pretty(&value)?)?;
        RenderBundle::load(&bundle_out)?
    };
    // `build_bundle` resolves `auto` and validates the mode; dispatch on the
    // resolved mode it wrote into the bundle.
    let mode = bundle.mode.clone();
    match mode.as_str() {
        "overlay" => {
            stages::overlay::run_overlay(&bundle)?;
        }
        "dual" => {
            stages::dual::run_dual(&bundle)?;
        }
        "typst" | "typst_visual" => {
            let cleaned_bg = stages::background::run_background(&bundle)?;
            stats.record_hit(sub::BACKGROUND);
            let compiled_pdf = stages::typst::run_typst(&bundle, &cleaned_bg)?;
            stages::save::run_save(&bundle, &compiled_pdf)?;
        }
        other => anyhow::bail!("render mode {other:?} not supported by render_rs (C1 supports typst/typst_visual/overlay/dual)"),
    }
    // Every mode compiles typst (overlay/dual via `run_overlay_compile`).
    stats.record_hit(sub::TYPST);

    let elapsed_seconds = started.elapsed().as_secs_f64();
    let summary_path = summary::write_pipeline_summary(
        &spec,
        &bundle.output_pdf,
        &bundle.source_pdf,
        &mode,
        bundle.page_map.source_page_indices.len(),
        elapsed_seconds,
    )?;
    let stats_path = native_stats::write_native_stats(&spec, &stats)?;

    Ok(RenderOutcome {
        output_pdf: bundle.output_pdf,
        source_pdf: bundle.source_pdf,
        translations_dir: spec.inputs.translations_dir,
        summary_path,
        stats_path,
        mode,
        page_count: bundle.page_map.source_page_indices.len(),
        elapsed_seconds,
    })
}
