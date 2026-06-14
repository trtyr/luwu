//! Luwu — terminal-native AI agent.
//! Single binary: axum server (background) + Ink TUI (foreground).
//! Use --headless for server-only mode.

use luwu_core::SessionManager;
use luwu_server::app::AppState;
use luwu_server::config::{Config, LoggingConfig};
use std::net::SocketAddr;
use tracing_subscriber::{fmt, prelude::*};

/// Embedded TUI binary (compiled via `bun build --compile` in build.rs).
/// This is ~60MB — the bun runtime + all UI code in one self-contained executable.
#[cfg(tui_embedded)]
const TUI_BINARY: &[u8] = include_bytes!("../../../ui/dist/luwu-tui");

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless" || a == "--server");

    // ── Config + tracing (errors go to stderr, safe in both modes) ──
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
    init_tracing(&config.logging);

    let resolved = match config.resolve(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    // ── Build AppState ──
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let working_dir =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let sessions_dir = luwu_home.join("sessions");
    let sessions = match SessionManager::with_persistence(&sessions_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to init sessions dir {}: {e}", sessions_dir.display());
            std::process::exit(1);
        }
    };

    let recovered = sessions.load_from_disk().await;

    let skills =
        luwu_core::SkillRegistry::discover(&luwu_home, &working_dir).unwrap_or_else(|e| {
            tracing::warn!("Skill discovery failed: {e}");
            luwu_core::SkillRegistry::new()
        });

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .expect("failed to build shared HTTP client");

    let state = AppState {
        config,
        sessions,
        working_dir: working_dir.clone(),
        skills,
        http_client,
        worker_tasks: tokio::sync::Mutex::new(tokio::task::JoinSet::new()),
    };

    let app = luwu_server::app::router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], 51740));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            eprintln!("Is another luwu instance running on port 51740?");
            std::process::exit(1);
        }
    };

    // ── Branch: integrated vs headless ──
    #[cfg(tui_embedded)]
    {
        if !headless {
            run_integrated(app, listener, addr).await;
            return;
        }
    }

    // Headless mode (or fallback when TUI not embedded)
    #[cfg(not(tui_embedded))]
    if !headless {
        eprintln!("TUI not embedded (bun not found at build time). Running headless.");
        eprintln!("To embed TUI: install bun, then rebuild.\n");
    }

    run_headless(app, listener, addr, &resolved, recovered).await;
}

/// Integrated mode: server in background, TUI in foreground.
/// TUI exit → server graceful shutdown.
#[cfg(tui_embedded)]
async fn run_integrated(
    app: axum::Router,
    listener: tokio::net::TcpListener,
    addr: SocketAddr,
) {
    tracing::info!("Starting in integrated mode (server + TUI)");

    // Server shutdown channel — triggered when TUI exits
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn server in background (silent — no stdout, logs to stderr only)
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx.changed().await.ok();
                tracing::info!("Server shutting down (TUI exited)");
            })
            .await
    });

    // Small delay for server to be ready (TUI has its own retry logic too)
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Extract embedded TUI binary to temp file
    let temp_dir = std::env::temp_dir();
    let tui_path = temp_dir.join(format!("luwu-tui-{}", std::process::id()));

    if let Err(e) = std::fs::write(&tui_path, TUI_BINARY) {
        eprintln!("Failed to write TUI binary: {e}");
        let _ = shutdown_tx.send(true);
        let _ = server_task.await;
        return;
    }

    // Make executable (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&tui_path)
            .map(|m| m.permissions())
            .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o755));
        let mut perms = perms;
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&tui_path, perms);
    }

    tracing::debug!("TUI binary extracted to {}", tui_path.display());

    // Spawn TUI with inherited stdio — it IS the user-facing process
    let mut child = match tokio::process::Command::new(&tui_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to spawn TUI: {e}");
            let _ = std::fs::remove_file(&tui_path);
            let _ = shutdown_tx.send(true);
            let _ = server_task.await;
            return;
        }
    };

    // Wait for TUI to exit
    let _ = child.wait().await;

    // Clean up temp file
    let _ = std::fs::remove_file(&tui_path);

    // Signal server to shut down
    let _ = shutdown_tx.send(true);
    let _ = server_task.await;

    eprintln!("\x1b[2m再见 👋\x1b[0m");
}

/// Headless mode: server in foreground with full startup banner.
async fn run_headless(
    app: axum::Router,
    listener: tokio::net::TcpListener,
    addr: SocketAddr,
    resolved: &luwu_server::config::ResolvedConfig,
    recovered: usize,
) {
    println!(
        "\x1b[2m陆吾 v{} — 昆仑山的管家\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );
    println!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
    println!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
    println!(
        "\x1b[2mlogging:  {} {}, {}\x1b[0m",
        resolved.provider_name,
        "info",
        "stderr"
    );
    println!("\x1b[2msessions: {} recovered\x1b[0m", recovered);
    println!();
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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });

    println!("\x1b[2m再见 👋\x1b[0m");
}

// ── Tracing ──

fn init_tracing(log: &LoggingConfig) {
    let filter = tracing_subscriber::EnvFilter::new(&log.level);
    let is_json = log.format.eq_ignore_ascii_case("json");
    let registry = tracing_subscriber::registry().with(filter);

    if let Some(path) = &log.file {
        let file_appender = tracing_appender::rolling::daily(".", path);
        let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
        std::mem::forget(guard);

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

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
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
