//! Luwu — terminal-native AI agent server.

use luwu_core::SessionManager;
use luwu_server::app::AppState;
use luwu_server::config::{Config, LoggingConfig};
use std::net::SocketAddr;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Init structured tracing from `[logging]` config section.
fn init_tracing(log: &LoggingConfig) {
    let filter = EnvFilter::new(&log.level);
    let is_json = log.format.eq_ignore_ascii_case("json");
    let registry = tracing_subscriber::registry().with(filter);

    if let Some(path) = &log.file {
        // File layer: always JSON for machine-readable logs, with daily rotation
        let file_appender = tracing_appender::rolling::daily(".", path);
        let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
        std::mem::forget(guard); // keep writing until process exits

        let file_layer = fmt::layer().json().with_writer(file_writer);
        let console_layer = if is_json {
            fmt::layer().json().with_writer(std::io::stderr).boxed()
        } else {
            fmt::layer().with_writer(std::io::stderr).boxed()
        };
        registry.with(file_layer).with(console_layer).init();
    } else if is_json {
        registry
            .with(fmt::layer().json().with_writer(std::io::stderr))
            .init();
    } else {
        registry
            .with(fmt::layer().with_writer(std::io::stderr))
            .init();
    }
}

#[tokio::main]
async fn main() {
    // Load config first (before logging — use eprintln for config errors).
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            eprintln!(
                "Config file: {}",
                luwu_server::config::config_path().display()
            );
            std::process::exit(1);
        }
    };

    // Init structured logging from [logging] section.
    init_tracing(&config.logging);

    println!(
        "\x1b[2m陆吾 v{} — 昆仑山的管家\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );

    // Verify default provider is configured.
    if let Err(e) = config.resolve(None) {
        eprintln!("Config error: {e}");
        std::process::exit(1);
    }

    let resolved = config.resolve(None).unwrap();
    println!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
    println!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
    println!(
        "\x1b[2mlogging:  {} {}, {}\x1b[0m",
        config.logging.level,
        config.logging.format,
        config.logging.file.as_deref().unwrap_or("stderr")
    );
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
            eprintln!(
                "Failed to initialize sessions directory {}: {e}",
                sessions_dir.display()
            );
            std::process::exit(1);
        }
    };

    // Recover persisted sessions from disk.
    let recovered = sessions.load_from_disk().await;
    println!("\x1b[2msessions: {} recovered\x1b[0m", recovered);

    // Discover skills.
    let skills = luwu_core::SkillRegistry::discover(&luwu_home, &working_dir).unwrap_or_else(|e| {
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

    let app = luwu_server::app::router(state);

    // Start server.
    let addr = SocketAddr::from(([127, 0, 0, 1], 51740));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            eprintln!("Is another luwu instance running on port 51740?");
            std::process::exit(1);
        }
    };

    println!("Listening on http://{addr}");
    println!();
    println!("Endpoints:");
    println!("  GET    /health                Health check");
    println!("  GET    /v1/models             List available models");
    println!("  POST   /v1/chat/completions   Chat (OpenAI-compatible SSE)");
    println!("  GET    /v1/sessions           List sessions");
    println!("  POST   /v1/sessions           Create session");
    println!("  GET    /v1/sessions/{{id}}     Get session info");
    println!("  DELETE /v1/sessions/{{id}}     Delete session");
    println!("  POST   /v1/sessions/{{id}}/chat   Agent chat (event stream)");
    println!("  POST   /v1/sessions/{{id}}/cancel Cancel running turn");
    println!("  GET    /v1/skills             List skills");
    println!("  GET    /v1/skills/{{name}}     Get skill detail");
    println!("  GET    /v1/stats              Runtime stats");
    println!();
    println!("\x1b[2mCtrl+C to stop.\x1b[0m");

    // Graceful shutdown on Ctrl-C / SIGTERM.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });

    println!("\x1b[2m再见 👋\x1b[0m");
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install TERM signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
