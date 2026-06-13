//! Luwu server — HTTP API for the luwu agent.

mod api;
mod config;

use std::net::SocketAddr;

use api::AppState;
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

    // Build app state.
    let state = AppState {
        config,
        sessions: SessionManager::new(),
        working_dir: std::path::PathBuf::from("."),
    };
    let app = api::router(state);

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

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
