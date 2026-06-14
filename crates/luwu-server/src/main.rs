//! Luwu — terminal-native AI agent.
//! Single binary: axum server (background) + Ink TUI (foreground).
//! Supports concurrent instances: first instance starts the server,
//! subsequent instances connect to the existing server and only run TUI.
//! Use --headless for server-only mode.

use luwu_core::SessionManager;
use luwu_server::app::AppState;
use luwu_server::config::{Config, LoggingConfig};
use std::net::SocketAddr;
use std::sync::OnceLock;
use tracing_subscriber::{fmt, prelude::*};

#[cfg(tui_embedded)]
const TUI_BINARY: &[u8] = include_bytes!("../../../ui/dist/luwu-tui");

/// Which mode are we in? Determines where logs go.
/// Integrated → file only (terminal belongs to TUI).
/// Headless   → stderr (terminal belongs to server).
/// TuiOnly    → no server in this process, just a TUI client.
static MODE: OnceLock<RunMode> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Debug)]
enum RunMode {
    Integrated,
    Headless,
    TuiOnly,
}

const LUWU_PORT: u16 = 51740;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless" || a == "--server");

    // ── Config (errors go to stderr, safe in all modes) ──
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

    // ── Decide mode BEFORE init_tracing ──
    let addr = SocketAddr::from(([127, 0, 0, 1], LUWU_PORT));

    #[cfg(tui_embedded)]
    let mode = if headless {
        RunMode::Headless
    } else {
        // ── Concurrent instance support ──
        // Check if a server is already running on LUWU_PORT.
        // If yes → TuiOnly (just launch TUI, connect to existing server).
        // If no  → Integrated (start server + TUI).
        let server_running = tokio::net::TcpStream::connect(addr).await.is_ok();
        if server_running {
            tracing::info!("Server already running on {addr}, TUI-only mode");
            RunMode::TuiOnly
        } else {
            RunMode::Integrated
        }
    };
    #[cfg(not(tui_embedded))]
    let mode = RunMode::Headless;

    let _ = MODE.set(mode);

    init_tracing(&config.logging);

    // ── TUI-only: skip server entirely ──
    if mode == RunMode::TuiOnly {
        #[cfg(tui_embedded)]
        {
            run_tui_only().await;
            return;
        }
        #[cfg(not(tui_embedded))]
        {
            eprintln!("Server already running, but TUI not embedded. Use --headless.");
            return;
        }
    }

    // ── Resolve config ──
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
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            eprintln!("Is another luwu instance running on port {LUWU_PORT}?");
            std::process::exit(1);
        }
    };

    // ── Branch ──
    match mode {
        RunMode::Integrated => {
            #[cfg(tui_embedded)]
            {
                run_integrated(app, listener).await;
                return;
            }
            #[cfg(not(tui_embedded))]
            {
                eprintln!("TUI not embedded. Running headless.");
            }
        }
        RunMode::Headless => {}
        RunMode::TuiOnly => unreachable!(),
    }

    run_headless(app, listener, addr, &resolved, recovered).await;
}

/// TUI-only mode: server is already running in another process.
/// Just extract and spawn the TUI binary, then exit when it exits.
#[cfg(tui_embedded)]
async fn run_tui_only() {
    tracing::info!("Starting in TUI-only mode (connecting to existing server)");

    let temp_dir = std::env::temp_dir();
    let tui_path = temp_dir.join(format!("luwu-tui-{}", std::process::id()));

    if let Err(e) = std::fs::write(&tui_path, TUI_BINARY) {
        tracing::error!("Failed to write TUI binary: {e}");
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tui_path)
            .map(|m| m.permissions())
            .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o755));
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&tui_path, perms);
    }

    tracing::debug!("TUI binary extracted to {}", tui_path.display());

    let mut child = match tokio::process::Command::new(&tui_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn TUI: {e}");
            let _ = std::fs::remove_file(&tui_path);
            return;
        }
    };

    let _ = child.wait().await;
    let _ = std::fs::remove_file(&tui_path);
}

/// Integrated mode: server in background, TUI in foreground.
/// ALL server output goes to ~/.luwu/logs/ — terminal is 100% TUI.
#[cfg(tui_embedded)]
async fn run_integrated(app: axum::Router, listener: tokio::net::TcpListener) {
    tracing::info!("Starting in integrated mode (server + TUI)");

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx.changed().await.ok();
                tracing::info!("Server shutting down (TUI exited)");
            })
            .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let temp_dir = std::env::temp_dir();
    let tui_path = temp_dir.join(format!("luwu-tui-{}", std::process::id()));

    if let Err(e) = std::fs::write(&tui_path, TUI_BINARY) {
        tracing::error!("Failed to write TUI binary: {e}");
        let _ = shutdown_tx.send(true);
        let _ = server_task.await;
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tui_path)
            .map(|m| m.permissions())
            .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o755));
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&tui_path, perms);
    }

    tracing::debug!("TUI binary extracted to {}", tui_path.display());

    // TUI gets full control of stdin/stdout/stderr
    let mut child = match tokio::process::Command::new(&tui_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn TUI: {e}");
            let _ = std::fs::remove_file(&tui_path);
            let _ = shutdown_tx.send(true);
            let _ = server_task.await;
            return;
        }
    };

    let _ = child.wait().await;
    let _ = std::fs::remove_file(&tui_path);

    // ── Shutdown decision ──
    // The server-starter's TUI has exited. But other TUI clients may still
    // be connected to the server. We keep the server alive briefly to let
    // them finish, then shut down. If no other clients exist, the server
    // shuts down immediately after the grace period.
    tracing::info!("TUI exited, keeping server alive for 3s grace period...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let _ = shutdown_tx.send(true);
    let _ = server_task.await;
}

/// Headless mode: server in foreground with startup banner to stderr.
async fn run_headless(
    app: axum::Router,
    listener: tokio::net::TcpListener,
    addr: SocketAddr,
    resolved: &luwu_server::config::ResolvedConfig,
    recovered: usize,
) {
    eprintln!(
        "\x1b[2m陆吾 v{} — 昆仑山的管家\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
    eprintln!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
    eprintln!("\x1b[2msessions: {} recovered\x1b[0m", recovered);
    eprintln!();
    eprintln!("Listening on http://{addr}");
    eprintln!();
    eprintln!("\x1b[2mCtrl+C to stop.\x1b[0m");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });

    eprintln!("\x1b[2m再见 👋\x1b[0m");
}

// ── Tracing ──

fn init_tracing(log: &LoggingConfig) {
    let filter = tracing_subscriber::EnvFilter::new(&log.level);
    let is_json = log.format.eq_ignore_ascii_case("json");
    let registry = tracing_subscriber::registry().with(filter);

    let mode = MODE.get().copied().unwrap_or(RunMode::Headless);

    match mode {
        RunMode::Integrated | RunMode::TuiOnly => {
            // ── Integrated/TuiOnly: file ONLY, zero terminal output ──
            // Logs go to ~/.luwu/logs/luwu.log (daily rotated)
            let log_dir = dirs::home_dir()
                .map(|h| h.join(".luwu").join("logs"))
                .unwrap_or_else(|| std::path::PathBuf::from("logs"));
            let _ = std::fs::create_dir_all(&log_dir);

            let file_appender = tracing_appender::rolling::daily(&log_dir, "luwu.log");
            let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
            std::mem::forget(guard);

            let file_layer = if is_json {
                fmt::layer().json().with_writer(file_writer).boxed()
            } else {
                fmt::layer().with_writer(file_writer).boxed()
            };
            registry.with(file_layer).init();

            tracing::info!("Logging to {} ({:?} mode)", log_dir.display(), mode);
        }
        RunMode::Headless => {
            // ── Headless: stderr + optional file ──
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
