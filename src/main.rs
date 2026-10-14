use std::net::SocketAddr;
use std::process::ExitCode;

use autoreverse_server::auth::bootstrap::ensure_first_admin;
use autoreverse_server::{AppState, Config, app, db};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("autoreverse_server=info,tower_http=warn")),
        )
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;
    ensure_first_admin(
        &pool,
        config.admin_user.as_deref(),
        config.admin_password.as_deref(),
    )
    .await?;

    let addr = SocketAddr::new(config.host, config.port);
    let state = AppState::new(pool, config);
    autoreverse_server::podcasts::job::start(state.clone());
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Autoreverse läuft auf http://{addr}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
