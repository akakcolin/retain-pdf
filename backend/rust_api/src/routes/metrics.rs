use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::metrics::render_prometheus;
use crate::routes::common::build_metrics_route_deps;
use crate::AppState;

/// Prometheus text exposition, mounted unauthenticated next to `/health`.
pub async fn metrics(State(state): State<AppState>) -> Response {
    let deps = build_metrics_route_deps(&state);
    match deps.metrics.snapshot(deps.db, deps.data_root) {
        Ok(snapshot) => (
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            render_prometheus(&snapshot),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("metrics unavailable: {error:#}"),
        )
            .into_response(),
    }
}
