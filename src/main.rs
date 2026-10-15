use std::net::SocketAddr;
use std::process::ExitCode;

use autoreverse_server::auth::bootstrap::ensure_first_admin;
use autoreverse_server::cli::{self, Command};
use autoreverse_server::{AppState, Config, app, config, db};
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
        // No colour codes in `docker logs` or a journal, only on a terminal.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(command) => command,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::SetPassword { name, admin } => return set_password(&name, admin).await,
        Command::Healthcheck => {
            // Same PORT rule as `Config` (default 3000); the probe always goes to loopback.
            let port = std::env::var("PORT")
                .ok()
                .and_then(|p| p.trim().parse().ok())
                .unwrap_or(3000);
            return if cli::healthcheck(port) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Command::Serve => {}
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::from_env()?;
    let explicit = std::env::var_os("AUTOREVERSE_STATIC")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from);
    let exe = std::env::current_exe().ok();
    config.static_dir = config::find_static_dir(explicit.as_deref(), exe.as_deref())?;
    tracing::info!("Statische Dateien aus {}", config.static_dir.display());
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
    autoreverse_server::library::thumbs::spawn_render_ahead(
        state.db.clone(),
        state.config.thumbs_dir.clone(),
    );
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Autoreverse läuft auf http://{addr}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// `autoreverse-server set-password <name> [--admin]`: exit 0 on success, 2 for a bad password,
/// 1 if the database is unreachable.
async fn set_password(name: &str, admin: bool) -> ExitCode {
    let password = match cli::read_new_password() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let pool = match connect().await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match cli::set_password(&pool, name, &password, admin).await {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

async fn connect() -> Result<sqlx::PgPool, Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;
    Ok(pool)
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
