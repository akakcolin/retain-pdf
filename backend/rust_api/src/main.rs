use rust_api::config::AppConfig;
use rust_api::run_servers_with_shutdown;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_api=info,tower_http=info".into()),
        )
        .init();

    run_servers_with_shutdown(AppConfig::from_env()?, shutdown_signal()).await
}

/// Route Ctrl-C into the graceful shutdown path so in-flight requests drain
/// before the process exits instead of being cut off mid-response.
///
/// This does not reap running worker process groups: workers are spawned in
/// their own process group (`setpgid`) and are only terminated on timeout,
/// explicit cancel, or boot-time reconciliation.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
