//! Luwu — terminal-native AI agent.
//!
//! Architecture: independent server daemon + TUI clients.
//!
//! `cargo run` with no server running:
//!   → spawn detached --daemon process (server)
//!   → poll until server is ready
//!   → start TUI (connects to daemon)
//!
//! `cargo run` with server already running:
//!   → start TUI (connects to existing daemon)
//!
//! The daemon auto-shuts down when no TUI has made a request for 30s.
//! No manual shutdown needed — just close all TUI windows.

use luwu_core::SessionManager;
use luwu_server::app::AppState;
use luwu_server::config::{Config, LoggingConfig};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing_subscriber::{fmt, prelude::*};

#[cfg(tui_embedded)]
const TUI_BINARY: &[u8] = include_bytes!("../../../ui/dist/luwu-tui");

static MODE: OnceLock<RunMode> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Debug)]
enum RunMode {
    Daemon,
    Headless,
    Tui,
}

const LUWU_PORT: u16 = 51740;
const DAEMON_READY_TIMEOUT_MS: u64 = 15000;
const DAEMON_POLL_INTERVAL_MS: u64 = 100;
/// Daemon auto-shuts down if no TUI request arrives in this window.
/// Must be >> heartbeat interval (10s) to survive event loop congestion.
const IDLE_SHUTDOWN_SECS: u64 = 120;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless" || a == "--server");
    let daemon = args.iter().any(|a| a == "--daemon");

    let addr = SocketAddr::from(([127, 0, 0, 1], LUWU_PORT));

    // ── Mode selection ──
    let mode = if daemon {
        RunMode::Daemon
    } else if headless {
        RunMode::Headless
    } else {
        #[cfg(tui_embedded)]
        {
            ensure_server_running(addr).await;
            RunMode::Tui
        }
        #[cfg(not(tui_embedded))]
        {
            RunMode::Headless
        }
    };

    let _ = MODE.set(mode);

    // ── Config ──
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

    // ── TUI mode: just launch TUI, no server in this process ──
    if mode == RunMode::Tui {
        #[cfg(tui_embedded)]
        {
            run_tui().await;
        }
        return;
    }

    // ── Daemon / Headless: start the server ──
    let resolved = match config.resolve(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let sessions_dir = luwu_home.join("sessions");
    let sessions = match SessionManager::with_persistence(&sessions_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Failed to init sessions dir {}: {e}",
                sessions_dir.display()
            );
            std::process::exit(1);
        }
    };

    let recovered = sessions.load_from_disk().await;

    let skills = luwu_core::SkillRegistry::discover(&luwu_home, &working_dir).unwrap_or_else(|e| {
        tracing::warn!("Skill discovery failed: {e}");
        luwu_core::SkillRegistry::new()
    });

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .expect("failed to build shared HTTP client");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let last_request = Arc::new(AtomicU64::new(now_ms));
    let sessions_for_shutdown = sessions.clone();

    let state = AppState {
        config,
        sessions,
        working_dir: working_dir.clone(),
        skills,
        http_client,
        worker_tasks: tokio::sync::Mutex::new(tokio::task::JoinSet::new()),
        last_request: last_request.clone(),
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

    match mode {
        RunMode::Daemon => {
            let pid_file = luwu_home.join("luwu.pid");
            let _ = std::fs::create_dir_all(&luwu_home);
            let _ = std::fs::write(&pid_file, std::process::id().to_string());
            tracing::info!("Daemon PID {} → {}", std::process::id(), pid_file.display());

            // Clean up PID file on exit
            let pid_file_clone = pid_file.clone();
            tokio::spawn(async move {
                shutdown_signal().await;
                let _ = std::fs::remove_file(&pid_file_clone);
                tracing::info!("Daemon shutting down (signal)");
                std::process::exit(0);
            });

            // ── Auto-shutdown: kill daemon when idle ──
            // Two conditions must BOTH be true to shut down:
            //   1. No HTTP request for IDLE_SHUTDOWN_SECS (no TUI heartbeat)
            //   2. No session is_running (no active agent turn in progress)
            // This prevents killing the daemon mid-stream when the TUI's
            // setInterval heartbeat gets delayed by React render congestion.
            let lr = last_request.clone();
            let auto_sessions = sessions_for_shutdown;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let last = lr.load(Ordering::Relaxed);
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if now - last > IDLE_SHUTDOWN_SECS * 1000 {
                        // Double-check: is any session actively running?
                        let has_running = auto_sessions
                            .list()
                            .await
                            .iter()
                            .any(|s| s.is_running);
                        if has_running {
                            tracing::debug!("Idle for {}s but session still running — keeping alive", IDLE_SHUTDOWN_SECS);
                            continue;
                        }
                        tracing::info!("No TUI activity for {IDLE_SHUTDOWN_SECS}s — auto-shutdown");
                        let pf = dirs::home_dir()
                            .map(|h| h.join(".luwu").join("luwu.pid"))
                            .unwrap_or_default();
                        let _ = std::fs::remove_file(&pf);
                        std::process::exit(0);
                    }
                }
            });

            tracing::info!(
                "Daemon listening on 127.0.0.1:{LUWU_PORT} (model: {}, auto-shutdown: {IDLE_SHUTDOWN_SECS}s idle)",
                resolved.model
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Server error: {e}");
                    std::process::exit(1);
                });
        }
        RunMode::Headless => {
            eprintln!(
                "\x1b[2m陆吾 v{} — 昆仑山的管家\x1b[0m",
                env!("CARGO_PKG_VERSION")
            );
            eprintln!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
            eprintln!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
            eprintln!("\x1b[2msessions: {} recovered\x1b[0m", recovered);
            eprintln!();
            eprintln!("Listening on http://127.0.0.1:{LUWU_PORT}");
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
        RunMode::Tui => unreachable!(),
    }
}

// ── Server detection + daemon spawning ──

async fn ensure_server_running(addr: SocketAddr) {
    if tokio::net::TcpStream::connect(addr).await.is_ok() {
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Cannot determine executable path: {e}");
            std::process::exit(1);
        }
    };

    tracing::debug!("Spawning daemon: {} --daemon", current_exe.display());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _child = match std::process::Command::new(&current_exe)
            .arg("--daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to spawn server daemon: {e}");
                eprintln!("Try 'cargo run --headless' to run the server manually.");
                std::process::exit(1);
            }
        };
    }

    #[cfg(not(unix))]
    {
        let _child = match std::process::Command::new(&current_exe)
            .arg("--daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to spawn server daemon: {e}");
                std::process::exit(1);
            }
        };
    }

    let deadline = std::time::Instant::now() + Duration::from_millis(DAEMON_READY_TIMEOUT_MS);
    loop {
        if std::time::Instant::now() >= deadline {
            eprintln!("Server daemon failed to start within {DAEMON_READY_TIMEOUT_MS}ms");
            eprintln!("Try 'cargo run --headless' to see server errors.");
            std::process::exit(1);
        }
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(DAEMON_POLL_INTERVAL_MS)).await;
    }
}

// ── TUI ──

#[cfg(tui_embedded)]
async fn run_tui() {
    tracing::debug!("Starting TUI client");

    let temp_dir = std::env::temp_dir();
    let tui_path = temp_dir.join(format!("luwu-tui-{}", std::process::id()));

    if let Err(e) = std::fs::write(&tui_path, TUI_BINARY) {
        eprintln!("Failed to extract TUI binary: {e}");
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tui_path, std::fs::Permissions::from_mode(0o755));
    }

    let mut child = match tokio::process::Command::new(&tui_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to launch TUI: {e}");
            let _ = std::fs::remove_file(&tui_path);
            return;
        }
    };

    let _ = child.wait().await;
    let _ = std::fs::remove_file(&tui_path);
}

// ── Tracing ──

fn init_tracing(log: &LoggingConfig) {
    let filter = tracing_subscriber::EnvFilter::new(&log.level);
    let is_json = log.format.eq_ignore_ascii_case("json");
    let registry = tracing_subscriber::registry().with(filter);

    let mode = MODE.get().copied().unwrap_or(RunMode::Headless);

    match mode {
        RunMode::Daemon | RunMode::Tui => {
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
