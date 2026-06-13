//! Luwu server — HTTP API for the luwu agent.

mod app;
mod config;
mod error;
mod types;
mod handlers;

use std::net::SocketAddr;

use app::AppState;
use config::Config;
use luwu_core::SessionManager;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Init tracing.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    println!("陆吾 v{} — 昆仑山的管家 (server)", env!("CARGO_PKG_VERSION"));
    println!();

    // Load config.
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            eprintln!("Config file: {}", config::config_path().display());
            std::process::exit(1);
        }
    };

    // Verify default provider is configured.
    if let Err(e) = config.resolve(None) {
        eprintln!("Config error: {e}");
        std::process::exit(1);
    }

    let resolved = config.resolve(None).unwrap();
    println!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
    println!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
    println!("\x1b[2mconfig:   {}\x1b[0m", config::config_path().display());
    println!();

    // Set up luwu home directory.
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    // Initialize session manager with file persistence.
    let sessions_dir = luwu_home.join("sessions");
    let sessions = match SessionManager::with_persistence(&sessions_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to initialize sessions directory {}: {e}", sessions_dir.display());
            std::process::exit(1);
        }
    };

    // Recover persisted sessions from disk.
    let recovered = sessions.load_from_disk().await;
    println!("\x1b[2msessions: {} recovered\x1b[0m", recovered);

    // Discover skills.
    let skills = luwu_core::SkillRegistry::discover(&luwu_home, &working_dir)
        .unwrap_or_else(|e| {
            tracing::warn!("Skill discovery failed: {e}");
            luwu_core::SkillRegistry::new()
        });
    println!("\x1b[2mskills:   {} loaded\x1b[0m", skills.len());

    // Build shared HTTP client.
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .expect("failed to build shared HTTP client");

    // Build app state.
    let state = AppState {
        config,
        sessions,
        working_dir: working_dir.clone(),
        skills,
        http_client,
        worker_tasks: tokio::sync::Mutex::new(tokio::task::JoinSet::new()),
    };

    let app = crate::app::router(state);
    println!("  GET    /v1/skills             List skills");
    println!("  GET    /v1/skills/{{name}}     Get skill detail");

    // Start server.
    let addr = SocketAddr::from(([127, 0, 0, 1], 51740));
    println!("Listening on http://{}", addr);
    println!();
    println!("Endpoints:");
    println!("  GET    /health                Health check");
    println!("  GET    /v1/models             List available models");
    println!("  POST   /v1/chat/completions   Chat (OpenAI-compatible, real SSE streaming)");
    println!("  GET    /v1/sessions           List sessions");
    println!("  POST   /v1/sessions           Create session");
    println!("  GET    /v1/sessions/{{id}}     Get session info");
    println!("  DELETE /v1/sessions/{{id}}     Delete session");
    println!("  POST   /v1/sessions/{{id}}/chat   Agent chat (full event stream)");
    println!("  POST   /v1/sessions/{{id}}/cancel Cancel running turn");
    println!();

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            eprintln!("Is another luwu instance running on port 51740?");
            std::process::exit(1);
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl-C, initiating graceful shutdown...");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown...");
        }
    }
}
