//! Orchestration sequence + mode dispatch.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use serde_json::json;

use crate::bundle::RenderBundle;
use crate::bundle_builder;
use crate::events::{PipelineEventWriter, PIPELINE_EVENTS_FILE_NAME};
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
    pub events_jsonl: PathBuf,
    pub mode: String,
    pub page_count: usize,
    pub elapsed_seconds: f64,
}

pub fn run(spec_path: &Path) -> Result<RenderOutcome> {
    let started = Instant::now();
    let spec = RenderStageSpec::load(spec_path)?;
    let bundle_out = spec.job.job_root.join("render-bundle.json");
    let mut stats = NativeStats::new();
    let mut events = PipelineEventWriter::new(&spec.job.job_id, &spec.job.job_root.join("logs"));
    events.emit_transition("startup", "", "render_rs worker 已启动")?;
    events.emit_transition("render_prepare", "render_prepare", "开始准备纯渲染阶段")?;

    // C3-N11: build the bundle natively (in-process mirror of the retired
    // `run_render_delegate.py` prepare/page-specs segment); the delegate and its
    // `RETAINPDF_RENDER_BUNDLE_NATIVE` gate were retired with it.
    let (bundle, cover_fallback_diagnostics): (RenderBundle, _) = {
        let outcome = bundle_builder::build_bundle(&spec, &mut stats)?;
        std::fs::write(&bundle_out, serde_json::to_string_pretty(&outcome.bundle)?)?;
        (RenderBundle::load(&bundle_out)?, outcome.cover_fallback_diagnostics)
    };
    events.emit_progress(
        "render_preprocess",
        "render_preprocess",
        "渲染源准备完成",
        1,
        1,
        "step",
        &json!({"user_stage": "render", "progress_unit": "step"}),
    )?;

    // `build_bundle` resolves `auto` and validates the mode; dispatch on the
    // resolved mode it wrote into the bundle.
    let mode = bundle.mode.clone();
    let save_elapsed_seconds;
    match mode.as_str() {
        "overlay" => {
            let (_output, save_elapsed) = stages::overlay::run_overlay(&bundle)?;
            save_elapsed_seconds = save_elapsed;
        }
        "dual" => {
            let (_output, save_elapsed) = stages::dual::run_dual(&bundle)?;
            save_elapsed_seconds = save_elapsed;
        }
        "typst" | "typst_visual" => {
            let cleaned_bg = stages::background::run_background(&bundle)?;
            stats.record_hit(sub::BACKGROUND);
            let compiled_pdf = stages::typst::run_typst(&bundle, &cleaned_bg)?;
            let (_output, save_elapsed) = stages::save::run_save(&bundle, &compiled_pdf)?;
            save_elapsed_seconds = save_elapsed;
        }
        other => anyhow::bail!("render mode {other:?} not supported by render_rs (C1 supports typst/typst_visual/overlay/dual)"),
    }
    // Every mode compiles typst (overlay/dual via `run_overlay_compile`).
    stats.record_hit(sub::TYPST);

    let page_count = bundle.page_map.source_page_indices.len();
    events.emit_progress(
        "rendering",
        "render_pages",
        &format!("渲染完成 {page_count} 页"),
        page_count as i64,
        page_count as i64,
        "page",
        &json!({"user_stage": "render", "progress_unit": "page"}),
    )?;
    events.emit_progress(
        "compile",
        "render_compile",
        "typst 编译完成",
        1,
        1,
        "step",
        &json!({"user_stage": "render", "progress_unit": "step"}),
    )?;
    events.emit_transition("finished", "", "render_rs 阶段完成")?;

    let render_diagnostics = {
        let mut diagnostics = cover_fallback_diagnostics;
        if let Some(map) = diagnostics.as_object_mut() {
            map.insert("save_elapsed_seconds".to_string(), json!(save_elapsed_seconds));
        }
        diagnostics
    };
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let summary_path = summary::write_pipeline_summary(
        &spec,
        &bundle.output_pdf,
        &bundle.source_pdf,
        &mode,
        page_count,
        elapsed_seconds,
        &render_diagnostics,
        &events.path(),
    )?;
    let stats_path = native_stats::write_native_stats(&spec, &stats)?;

    Ok(RenderOutcome {
        output_pdf: bundle.output_pdf,
        source_pdf: bundle.source_pdf,
        translations_dir: spec.inputs.translations_dir,
        summary_path,
        stats_path,
        events_jsonl: spec.job.job_root.join("logs").join(PIPELINE_EVENTS_FILE_NAME),
        mode,
        page_count,
        elapsed_seconds,
    })
}
