//! `native_stats.json` writer for the D5 runtime-metrics layer.
//!
//! The rust_api `/metrics` endpoint hydrates per-subsystem native hit ratios
//! from each finished render job's `native_stats.json` (`{"hits": {subsystem:
//! n}, "fallbacks": {subsystem: {reason: n}}}`). render_rs is fully native —
//! no routing fallbacks exist — so `record_hit` is exercised once per subsystem
//! at its execution surface and `record_fallback` exists as a regression guard
//! with no current call sites. File location and shape mirror `summary.rs` so
//! the metrics reader (`rust_api/src/metrics.rs::read_native_stats`) sees the
//! same artifact layout.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use crate::spec::RenderStageSpec;

pub const NATIVE_STATS_FILE_NAME: &str = "native_stats.json";

/// Subsystem names emitted as `hits` keys — the D5 subsystems
/// (`d5_baseline.json::native_hit_ratio.subsystems`).
pub mod subsystem {
    pub const SOURCE: &str = "source";
    pub const BACKGROUND: &str = "background";
    pub const TYPST: &str = "typst";
    pub const LAYOUT: &str = "layout";
    pub const LAYOUT_PAYLOAD: &str = "layout_payload";
    pub const VISUAL_PROFILE: &str = "visual_profile";
    pub const PDF_STRUCTURE_PROFILE: &str = "pdf_structure_profile";
    pub const ANALYSIS: &str = "analysis";
    pub const SOURCE_CLEANUP_PLANNING: &str = "source_cleanup_planning";
}

/// Process-local native routing counters for one render job.
#[derive(Debug, Default)]
pub struct NativeStats {
    hits: BTreeMap<&'static str, u64>,
    fallbacks: BTreeMap<&'static str, BTreeMap<&'static str, u64>>,
}

impl NativeStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one native execution of `subsystem`.
    pub fn record_hit(&mut self, subsystem: &'static str) {
        *self.hits.entry(subsystem).or_insert(0) += 1;
    }

    /// Record one Python fallback of `subsystem` for `reason`. No call sites in
    /// render_rs (fully native); kept as a regression guard mirroring the
    /// retired Python `_routing.py` shape.
    pub fn record_fallback(&mut self, subsystem: &'static str, reason: &'static str) {
        *self
            .fallbacks
            .entry(subsystem)
            .or_default()
            .entry(reason)
            .or_insert(0) += 1;
    }
}

/// Write `{hits, fallbacks}` to `{job_root}/artifacts/native_stats.json` (the
/// path rust_api metrics hydrates). Mirrors `summary::write_pipeline_summary`.
pub fn write_native_stats(
    spec: &RenderStageSpec,
    stats: &NativeStats,
) -> anyhow::Result<PathBuf> {
    let stats_path = spec.job.job_root.join("artifacts").join(NATIVE_STATS_FILE_NAME);
    if let Some(parent) = stats_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let value = json!({
        "hits": &stats.hits,
        "fallbacks": &stats.fallbacks,
    });
    std::fs::write(&stats_path, serde_json::to_string_pretty(&value)?)?;
    Ok(stats_path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::spec::{RenderStageInputs, RenderStageParams, RenderStageSpec, StageJobRef};

    fn unique_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("render-rs-native-stats-{}-{nanos}", std::process::id()))
    }

    fn spec(job_root: PathBuf) -> RenderStageSpec {
        RenderStageSpec {
            schema_version: crate::spec::RENDER_STAGE_SCHEMA_VERSION.to_string(),
            stage: "render".to_string(),
            job: StageJobRef {
                job_id: "job-test".to_string(),
                job_root,
                workflow: "book".to_string(),
            },
            inputs: RenderStageInputs {
                source_pdf: PathBuf::from("/tmp/source/in.pdf"),
                translations_dir: PathBuf::from("/tmp/translated"),
                translation_manifest: Some(PathBuf::from("/tmp/translated/translation_manifest.json")),
            },
            params: RenderStageParams::default(),
        }
    }

    #[test]
    fn writes_native_stats_with_hits_and_fallbacks() {
        let root = unique_root();
        let job_root = root.join("jobs").join("job-test");
        let spec = spec(job_root.clone());
        let mut stats = NativeStats::new();
        stats.record_hit(subsystem::SOURCE);
        stats.record_hit(subsystem::TYPST);
        stats.record_fallback(subsystem::BACKGROUND, "strategy_not_ported");

        let path = write_native_stats(&spec, &stats).expect("write native stats");

        assert_eq!(path, job_root.join("artifacts").join(NATIVE_STATS_FILE_NAME));
        assert!(path.is_file());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read native stats"))
                .expect("parse native stats");
        assert_eq!(value["hits"]["source"], 1);
        assert_eq!(value["hits"]["typst"], 1);
        assert_eq!(value["fallbacks"]["background"]["strategy_not_ported"], 1);
        assert_eq!(value["hits"]["analysis"], serde_json::Value::Null);
        let _ = std::fs::remove_dir_all(&root);
    }
}
