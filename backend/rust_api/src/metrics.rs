//! Prometheus text-format metrics for the api server.
//!
//! Counters are hydrated lazily from the SQLite jobs table plus each finished
//! render job's `pipeline_summary.json` / `native_stats.json` artifact files,
//! then cached for a short TTL so the endpoint stays cheap. Missing artifact
//! files and render jobs without a recorded renderer (pre-D5 history) count as
//! zero / `unknown` rather than errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::db::Db;
use crate::job_runner::process_contract::WorkerContract;

pub const PIPELINE_SUMMARY_FILE_NAME: &str = "pipeline_summary.json";
pub const NATIVE_STATS_FILE_NAME: &str = "native_stats.json";

/// How long a hydrated snapshot is reused before the next request reloads it.
const CACHE_TTL: Duration = Duration::from_secs(10);

/// Aggregated counters backing both `/metrics` and the /health aggregation.
#[derive(Debug, Clone, Default)]
pub struct MetricsSnapshot {
    pub jobs_by_status: BTreeMap<String, u64>,
    /// Keyed `(renderer, status)` for succeeded/failed render jobs.
    pub render_jobs_by_renderer_status: BTreeMap<(String, String), u64>,
    pub render_elapsed_count: u64,
    pub render_elapsed_sum_seconds: f64,
    pub native_hits_by_subsystem: BTreeMap<String, u64>,
    /// Keyed `(subsystem, reason)`.
    pub native_fallbacks_by_subsystem_reason: BTreeMap<(String, String), u64>,
}

impl MetricsSnapshot {
    pub fn native_hit_ratio(&self, subsystem: &str) -> f64 {
        let hits = self
            .native_hits_by_subsystem
            .get(subsystem)
            .copied()
            .unwrap_or(0);
        let fallbacks: u64 = self
            .native_fallbacks_by_subsystem_reason
            .iter()
            .filter(|((sub, _), _)| sub == subsystem)
            .map(|(_, count)| *count)
            .sum();
        let total = hits + fallbacks;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

struct CachedMetrics {
    snapshot: MetricsSnapshot,
    loaded_at: Instant,
}

/// Process-shared registry; cheap to clone (shares the cache mutex).
#[derive(Clone, Default)]
pub struct MetricsRegistry {
    cache: Arc<Mutex<Option<CachedMetrics>>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current snapshot, reloading at most once per TTL.
    pub fn snapshot(&self, db: &Db, data_root: &Path) -> Result<MetricsSnapshot> {
        if let Some(cached) = self.cached_fresh() {
            return Ok(cached);
        }
        let snapshot = load_snapshot(db, data_root)?;
        let mut guard = self.cache.lock().expect("metrics cache mutex poisoned");
        *guard = Some(CachedMetrics {
            snapshot: snapshot.clone(),
            loaded_at: Instant::now(),
        });
        Ok(snapshot)
    }

    fn cached_fresh(&self) -> Option<MetricsSnapshot> {
        let guard = self.cache.lock().expect("metrics cache mutex poisoned");
        guard
            .as_ref()
            .filter(|cached| cached.loaded_at.elapsed() < CACHE_TTL)
            .map(|cached| cached.snapshot.clone())
    }
}

fn load_snapshot(db: &Db, data_root: &Path) -> Result<MetricsSnapshot> {
    let mut snapshot = MetricsSnapshot::default();
    for row in db.list_job_metric_rows()? {
        *snapshot
            .jobs_by_status
            .entry(row.status.clone())
            .or_insert(0) += 1;
        if !is_render_job(&row.command) {
            continue;
        }
        let renderer = row
            .renderer
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if matches!(row.status.as_str(), "succeeded" | "failed") {
            *snapshot
                .render_jobs_by_renderer_status
                .entry((renderer, row.status))
                .or_insert(0) += 1;
        }
        hydrate_render_artifacts(&mut snapshot, data_root, &row.job_id);
    }
    Ok(snapshot)
}

fn is_render_job(command: &[String]) -> bool {
    WorkerContract::from_command(command) == WorkerContract::Render
}

fn hydrate_render_artifacts(snapshot: &mut MetricsSnapshot, data_root: &Path, job_id: &str) {
    let artifacts_dir = data_root.join("jobs").join(job_id).join("artifacts");
    if let Some(elapsed) = read_render_elapsed(&artifacts_dir.join(PIPELINE_SUMMARY_FILE_NAME)) {
        snapshot.render_elapsed_count += 1;
        snapshot.render_elapsed_sum_seconds += elapsed;
    }
    read_native_stats(&artifacts_dir.join(NATIVE_STATS_FILE_NAME))
        .into_iter()
        .for_each(|(hits, fallbacks)| {
            for (subsystem, count) in hits {
                *snapshot
                    .native_hits_by_subsystem
                    .entry(subsystem)
                    .or_insert(0) += count;
            }
            for ((subsystem, reason), count) in fallbacks {
                *snapshot
                    .native_fallbacks_by_subsystem_reason
                    .entry((subsystem, reason))
                    .or_insert(0) += count;
            }
        });
}

fn read_render_elapsed(path: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("render_elapsed").and_then(|item| item.as_f64())
}

/// native_stats.json 解析结果: (subsystem -> 命中次数, (subsystem, reason) -> 回退次数)。
type NativeStats = (BTreeMap<String, u64>, BTreeMap<(String, String), u64>);

/// Reads `native_stats.json` into `(hits: subsystem -> count, fallbacks:
/// (subsystem, reason) -> count)`. Any malformed entry is skipped.
fn read_native_stats(path: &Path) -> Option<NativeStats> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut hits = BTreeMap::new();
    let mut fallbacks = BTreeMap::new();
    if let Some(map) = value.get("hits").and_then(|item| item.as_object()) {
        for (subsystem, count) in map {
            if let Some(n) = count.as_u64() {
                *hits.entry(subsystem.clone()).or_insert(0) += n;
            }
        }
    }
    if let Some(map) = value.get("fallbacks").and_then(|item| item.as_object()) {
        for (subsystem, reasons) in map {
            let Some(reasons) = reasons.as_object() else {
                continue;
            };
            for (reason, count) in reasons {
                if let Some(n) = count.as_u64() {
                    *fallbacks
                        .entry((subsystem.clone(), reason.clone()))
                        .or_insert(0) += n;
                }
            }
        }
    }
    Some((hits, fallbacks))
}

/// Renders the snapshot as Prometheus text exposition (content type
/// `text/plain; version=0.0.4`). Pure and deterministic for testability.
pub fn render_prometheus(snapshot: &MetricsSnapshot) -> String {
    let mut out = String::new();
    for (status, count) in &snapshot.jobs_by_status {
        out.push_str(&format!(
            "retainpdf_jobs_total{{status=\"{status}\"}} {count}\n"
        ));
    }
    for ((renderer, status), count) in &snapshot.render_jobs_by_renderer_status {
        out.push_str(&format!(
            "retainpdf_render_jobs_total{{renderer=\"{renderer}\",status=\"{status}\"}} {count}\n"
        ));
    }
    out.push_str(&format!(
        "retainpdf_render_elapsed_seconds_count {}\n",
        snapshot.render_elapsed_count
    ));
    out.push_str(&format!(
        "retainpdf_render_elapsed_seconds_sum {:.3}\n",
        snapshot.render_elapsed_sum_seconds
    ));
    for (subsystem, count) in &snapshot.native_hits_by_subsystem {
        out.push_str(&format!(
            "retainpdf_native_routing_hits{{subsystem=\"{subsystem}\"}} {count}\n"
        ));
    }
    for ((subsystem, reason), count) in &snapshot.native_fallbacks_by_subsystem_reason {
        out.push_str(&format!(
            "retainpdf_native_routing_fallbacks{{subsystem=\"{subsystem}\",reason=\"{reason}\"}} {count}\n"
        ));
    }
    for subsystem in native_subsystems(snapshot) {
        out.push_str(&format!(
            "retainpdf_native_hit_ratio{{subsystem=\"{subsystem}\"}} {:.3}\n",
            snapshot.native_hit_ratio(&subsystem)
        ));
    }
    out
}

fn native_subsystems(snapshot: &MetricsSnapshot) -> BTreeSet<String> {
    let mut subsystems: BTreeSet<String> =
        snapshot.native_hits_by_subsystem.keys().cloned().collect();
    for (subsystem, _) in snapshot.native_fallbacks_by_subsystem_reason.keys() {
        subsystems.insert(subsystem.clone());
    }
    subsystems
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::domain::{JobSnapshot, JobStatusKind};
    use crate::models::request::CreateJobInput;

    struct TestMetricsFs {
        root: PathBuf,
        data_root: PathBuf,
        db: Db,
    }

    impl TestMetricsFs {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rust-api-metrics-{name}-{}-{}",
                std::process::id(),
                fastrand::u64(..)
            ));
            let data_root = root.join("data");
            let db = Db::new(data_root.join("db").join("jobs.db"), data_root.clone());
            db.init().expect("init db");
            Self {
                root,
                data_root,
                db,
            }
        }

        fn save_job(&self, command: Vec<String>, status: JobStatusKind, renderer: Option<&str>) {
            let mut job = JobSnapshot::new(
                format!("job-{}", fastrand::u64(..)),
                CreateJobInput::default(),
                command,
            );
            job.status = status;
            if let Some(renderer) = renderer {
                job.runtime.get_or_insert_with(Default::default).renderer =
                    Some(renderer.to_string());
            }
            job.sync_runtime_state();
            self.db.save_job(&job).expect("save job");
        }

        fn artifacts_dir(&self, job_id: &str) -> PathBuf {
            self.data_root.join("jobs").join(job_id).join("artifacts")
        }

        fn write_artifact(&self, job_id: &str, name: &str, contents: &str) {
            let dir = self.artifacts_dir(job_id);
            fs::create_dir_all(&dir).expect("artifacts dir");
            fs::write(dir.join(name), contents).expect("write artifact");
        }
    }

    impl Drop for TestMetricsFs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn render_command() -> Vec<String> {
        vec![
            "/opt/bin/render_rs".to_string(),
            "--spec".to_string(),
            "/tmp/spec.json".to_string(),
        ]
    }

    #[test]
    fn snapshot_aggregates_render_jobs_by_renderer_and_status() {
        let fs = TestMetricsFs::new("render");
        fs.save_job(
            render_command(),
            JobStatusKind::Succeeded,
            Some("render_rs"),
        );
        fs.save_job(render_command(), JobStatusKind::Succeeded, Some("python"));
        fs.save_job(render_command(), JobStatusKind::Failed, Some("render_rs"));
        fs.save_job(render_command(), JobStatusKind::Running, Some("render_rs"));
        // Non-render job must not appear in render_jobs_total.
        fs.save_job(
            vec![
                "python".to_string(),
                "run_translate_only.py".to_string(),
                "--spec".to_string(),
                "spec.json".to_string(),
            ],
            JobStatusKind::Succeeded,
            None,
        );

        let snapshot = load_snapshot(&fs.db, &fs.data_root).expect("load snapshot");
        assert_eq!(snapshot.jobs_by_status.get("succeeded"), Some(&3));
        assert_eq!(snapshot.jobs_by_status.get("running"), Some(&1));
        assert_eq!(
            snapshot
                .render_jobs_by_renderer_status
                .get(&("render_rs".to_string(), "succeeded".to_string())),
            Some(&1)
        );
        assert_eq!(
            snapshot
                .render_jobs_by_renderer_status
                .get(&("python".to_string(), "succeeded".to_string())),
            Some(&1)
        );
        assert_eq!(
            snapshot
                .render_jobs_by_renderer_status
                .get(&("render_rs".to_string(), "failed".to_string())),
            Some(&1)
        );
        // Running render jobs are excluded from the succeeded/failed metric.
        assert_eq!(snapshot.render_jobs_by_renderer_status.len(), 3);
    }

    #[test]
    fn snapshot_counts_render_jobs_without_renderer_as_unknown() {
        let fs = TestMetricsFs::new("unknown-renderer");
        fs.save_job(render_command(), JobStatusKind::Succeeded, None);

        let snapshot = load_snapshot(&fs.db, &fs.data_root).expect("load snapshot");
        assert_eq!(
            snapshot
                .render_jobs_by_renderer_status
                .get(&("unknown".to_string(), "succeeded".to_string())),
            Some(&1)
        );
    }

    #[test]
    fn snapshot_hydrates_render_elapsed_and_native_stats_from_artifacts() {
        let fs = TestMetricsFs::new("artifacts");
        let job_id = "job-artifacts";
        fs.write_artifact(
            job_id,
            PIPELINE_SUMMARY_FILE_NAME,
            r#"{"render_elapsed": 12.5, "renderer": "render_rs"}"#,
        );
        fs.write_artifact(
            job_id,
            NATIVE_STATS_FILE_NAME,
            r#"{"hits": {"source": 4, "typst": 2}, "fallbacks": {"background": {"strategy_not_ported": 1}}}"#,
        );
        let mut job = JobSnapshot::new(
            job_id.to_string(),
            CreateJobInput::default(),
            render_command(),
        );
        job.status = JobStatusKind::Succeeded;
        job.runtime.get_or_insert_with(Default::default).renderer = Some("render_rs".to_string());
        job.sync_runtime_state();
        fs.db.save_job(&job).expect("save render job");

        let snapshot = load_snapshot(&fs.db, &fs.data_root).expect("load snapshot");
        assert_eq!(snapshot.render_elapsed_count, 1);
        assert!((snapshot.render_elapsed_sum_seconds - 12.5).abs() < 1e-9);
        assert_eq!(snapshot.native_hits_by_subsystem.get("source"), Some(&4));
        assert_eq!(snapshot.native_hits_by_subsystem.get("typst"), Some(&2));
        assert_eq!(
            snapshot
                .native_fallbacks_by_subsystem_reason
                .get(&("background".to_string(), "strategy_not_ported".to_string())),
            Some(&1)
        );
    }

    #[test]
    fn render_prometheus_emits_counters_and_hit_ratios() {
        let mut snapshot = MetricsSnapshot::default();
        snapshot.jobs_by_status.insert("succeeded".to_string(), 2);
        snapshot
            .render_jobs_by_renderer_status
            .insert(("render_rs".to_string(), "succeeded".to_string()), 2);
        snapshot.render_elapsed_count = 1;
        snapshot.render_elapsed_sum_seconds = 3.0;
        snapshot
            .native_hits_by_subsystem
            .insert("source".to_string(), 3);
        snapshot
            .native_fallbacks_by_subsystem_reason
            .insert(("source".to_string(), "in_memory_page".to_string()), 1);

        let text = render_prometheus(&snapshot);
        assert!(text.contains("retainpdf_jobs_total{status=\"succeeded\"} 2"));
        assert!(text.contains(
            "retainpdf_render_jobs_total{renderer=\"render_rs\",status=\"succeeded\"} 2"
        ));
        assert!(text.contains("retainpdf_render_elapsed_seconds_count 1"));
        assert!(text.contains("retainpdf_render_elapsed_seconds_sum 3.000"));
        assert!(text.contains("retainpdf_native_routing_hits{subsystem=\"source\"} 3"));
        assert!(text.contains(
            "retainpdf_native_routing_fallbacks{subsystem=\"source\",reason=\"in_memory_page\"} 1"
        ));
        assert!(text.contains("retainpdf_native_hit_ratio{subsystem=\"source\"} 0.750"));
    }

    #[test]
    fn registry_caches_snapshot_within_ttl() {
        let fs = TestMetricsFs::new("cache");
        fs.save_job(
            render_command(),
            JobStatusKind::Succeeded,
            Some("render_rs"),
        );
        let registry = MetricsRegistry::new();
        let first = registry
            .snapshot(&fs.db, &fs.data_root)
            .expect("first snapshot");
        assert_eq!(
            first
                .render_jobs_by_renderer_status
                .get(&("render_rs".to_string(), "succeeded".to_string())),
            Some(&1)
        );
        // Add a job after hydration; a cached read must not see it.
        fs.save_job(render_command(), JobStatusKind::Failed, Some("python"));
        let second = registry
            .snapshot(&fs.db, &fs.data_root)
            .expect("cached snapshot");
        assert_eq!(second.render_jobs_by_renderer_status.len(), 1);
    }

    #[test]
    fn native_hit_ratio_zero_when_no_activity() {
        let snapshot = MetricsSnapshot::default();
        assert_eq!(snapshot.native_hit_ratio("source"), 0.0);
    }
}
