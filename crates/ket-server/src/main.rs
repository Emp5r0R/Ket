use anyhow::{Context, Result};
use ket_server::{AccessService, AppState, ServerConfig, build_router};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    initialize_logging();
    let config = ServerConfig::from_env()?;
    if let Some(hysteria) = &config.hysteria {
        ket_server::hysteria::write_runtime_config(hysteria)
            .await
            .context("failed to render Hysteria2 configuration")?;
    }
    let access = AccessService::load_from_file(
        config.state_path.clone(),
        config.session_ttl,
        config.max_sessions,
    )
    .await
    .context("failed to initialize persistent state")?;
    let bind_address = config.bind_address;
    let public_url = config.public_url.clone();
    let state = AppState::new(config, access).context("failed to initialize data-plane control")?;
    state.start_background_tasks();
    let app = build_router(state);
    let listener = TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("failed to bind {bind_address}"))?;

    tracing::info!(%bind_address, %public_url, "Ket control plane is ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server stopped unexpectedly")
}

fn initialize_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("ket_server=info,tower_http=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl-C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to install SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
