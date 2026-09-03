use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::metrics::render_prometheus;
use crate::AppState;

/// Prometheus text exposition, mounted unauthenticated next to `/health`.
pub async fn metrics(State(state): State<AppState>) -> Response {
    match state
        .metrics
        .snapshot(state.db.as_ref(), &state.config.data_root)
    {
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
