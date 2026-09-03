use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::metrics::MetricsSnapshot;
use crate::models::api::ApiResponse;
use crate::models::domain::{now_iso, JobStatusKind};
use crate::ocr_provider::supported_provider_keys;
use crate::routes::common::{build_health_route_deps, HealthRouteDeps};
use crate::AppState;

#[derive(Serialize)]
pub struct HealthView {
    pub status: &'static str,
    pub db: &'static str,
    pub queue_depth: i64,
    pub running_jobs: i64,
    pub provider_backends: Vec<String>,
    /// Render jobs by worker flavor (`render_rs`/`python`/`unknown`), for
    /// non-render jobs or jobs without the field `unknown`.
    pub render_jobs_by_renderer: BTreeMap<String, u64>,
    /// Native routing hit ratio (hits / hits+fallbacks) per subsystem.
    pub native_hit_ratio: BTreeMap<String, f64>,
    /// Native routing fallbacks by (subsystem, reason) — the reason dimension
    /// backing the per-subsystem ratio (native_not_built / forced_off /
    /// in_memory_page / ...).
    pub native_fallbacks: BTreeMap<String, BTreeMap<String, u64>>,
    pub time: String,
}

fn build_health_view(deps: HealthRouteDeps<'_>) -> HealthView {
    let db_ok = deps.db.ping().is_ok();
    let queued = deps
        .db
        .count_jobs_with_status(&JobStatusKind::Queued)
        .unwrap_or(0);
    let running = deps
        .db
        .count_jobs_with_status(&JobStatusKind::Running)
        .unwrap_or(0);
    let snapshot = deps
        .metrics
        .snapshot(deps.db, deps.data_root)
        .unwrap_or_default();
    HealthView {
        status: if db_ok { "up" } else { "degraded" },
        db: if db_ok { "ok" } else { "error" },
        queue_depth: queued,
        running_jobs: running,
        provider_backends: supported_provider_keys(),
        render_jobs_by_renderer: render_jobs_by_renderer(&snapshot),
        native_hit_ratio: native_hit_ratio(&snapshot),
        native_fallbacks: native_fallbacks(&snapshot),
        time: now_iso(),
    }
}

fn render_jobs_by_renderer(snapshot: &MetricsSnapshot) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for ((renderer, _), count) in &snapshot.render_jobs_by_renderer_status {
        *out.entry(renderer.clone()).or_insert(0) += count;
    }
    out
}

fn native_hit_ratio(snapshot: &MetricsSnapshot) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for subsystem in snapshot.native_hits_by_subsystem.keys() {
        out.insert(subsystem.clone(), snapshot.native_hit_ratio(subsystem));
    }
    for (subsystem, _) in snapshot.native_fallbacks_by_subsystem_reason.keys() {
        out.entry(subsystem.clone())
            .or_insert_with(|| snapshot.native_hit_ratio(subsystem));
    }
    out
}

fn native_fallbacks(snapshot: &MetricsSnapshot) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for ((subsystem, reason), count) in &snapshot.native_fallbacks_by_subsystem_reason {
        *out.entry(subsystem.clone())
            .or_default()
            .entry(reason.clone())
            .or_insert(0) += count;
    }
    out
}

pub async fn health(State(state): State<AppState>) -> Json<ApiResponse<HealthView>> {
    Json(ApiResponse::ok(build_health_view(build_health_route_deps(
        &state,
    ))))
}
