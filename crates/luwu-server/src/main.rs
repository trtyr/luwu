//! Luwu — terminal-native AI agent.
//! Single binary: starts HTTP server + Ink TUI.

use luwu_core::SessionManager;
use luwu_server::app::AppState;
use luwu_server::config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Init tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("info".parse().unwrap()),
        )
        .init();

    // Parse args.
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless" || a == "--server");

    println!(
        "\x1b[2m陆吾 v{} — 昆仑山的管家\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );

    // Load config.
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

    // Verify default provider is configured.
    if let Err(e) = config.resolve(None) {
        eprintln!("Config error: {e}");
        std::process::exit(1);
    }

    let resolved = config.resolve(None).unwrap();
    println!("\x1b[2mprovider: {}\x1b[0m", resolved.provider_name);
    println!("\x1b[2mmodel:    {}\x1b[0m", resolved.model);
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

    // Shutdown coordination: server stops when TUI exits OR Ctrl-C.
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn server in background.
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                // Wait for shutdown signal from TUI exit or Ctrl-C.
                shutdown_rx.changed().await.ok();
            })
            .await
            .unwrap_or_else(|e| {
                eprintln!("Server error: {e}");
                std::process::exit(1);
            });
    });

    // Give the server a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    if headless {
        println!("Listening on http://{}", addr);
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
        println!();
        println!("\x1b[2mHeadless mode — Ctrl+C to stop.\x1b[0m");

        // Wait for Ctrl-C.
        tokio::signal::ctrl_c().await.ok();
        let _ = shutdown_tx.send(true);
    } else {
        // TUI mode: find UI directory and spawn bun.
        match find_ui_dir() {
            Some(ui_dir) => {
                // Check bun is available.
                if which_bun().is_none() {
                    eprintln!("\x1b[33m⚠ bun not found in PATH. Install: curl -fsSL https://bun.sh/install | bash\x1b[0m");
                    eprintln!("\x1b[2mFalling back to headless mode. Use --headless to suppress this message.\x1b[0m");
                    // Fall through to headless-like wait.
                    tokio::signal::ctrl_c().await.ok();
                    let _ = shutdown_tx.send(true);
                    return;
                }

                // Spawn the TUI as a child process.
                let mut child = match tokio::process::Command::new("bun")
                    .arg("run")
                    .arg("src/index.tsx")
                    .current_dir(&ui_dir)
                    .stdin(std::process::Stdio::inherit())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("\x1b[31m✗ Failed to spawn TUI: {e}\x1b[0m");
                        eprintln!("\x1b[2mUI directory: {}\x1b[0m", ui_dir.display());
                        tokio::signal::ctrl_c().await.ok();
                        let _ = shutdown_tx.send(true);
                        return;
                    }
                };

                // Wait for TUI to exit (user pressed Ctrl+C in the TUI).
                let status = child.wait().await;
                if let Err(e) = &status {
                    tracing::warn!("TUI process error: {e}");
                }

                // Shut down the server.
                let _ = shutdown_tx.send(true);
            }
            None => {
                eprintln!("\x1b[33m⚠ UI directory not found. Run from project root or install ui/.\x1b[0m");
                eprintln!("\x1b[2mFalling back to headless mode.\x1b[0m");
                tokio::signal::ctrl_c().await.ok();
                let _ = shutdown_tx.send(true);
            }
        }
    }

    // Wait for server to finish shutting down.
    let _ = server_task.await;
    println!("\x1b[2m再见 👋\x1b[0m");
}

/// Find the UI directory relative to CWD or executable location.
fn find_ui_dir() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = [
        // Relative to CWD (e.g. running `cargo run` from project root)
        std::env::current_dir().ok().map(|d| d.join("ui")),
        // Relative to executable (installed binary layout)
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("ui"))),
        // Two levels up from executable (target/release → project root → ui)
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent()?.parent().map(|p| p.join("ui"))),
        // Three levels up (target/release/deep nesting)
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent()?.parent()?.parent().map(|p| p.join("ui"))),
    ]
    .into_iter()
    .flatten()
    .collect();

    for candidate in &candidates {
        if candidate.join("src/index.tsx").exists() {
            return Some(candidate.clone());
        }
    }
    None
}

/// Check if bun is available in PATH.
fn which_bun() -> Option<String> {
    let result = std::process::Command::new("which")
        .arg("bun")
        .output()
        .ok()?;
    if result.status.success() {
        Some(String::from_utf8_lossy(&result.stdout).trim().to_string())
    } else {
        None
    }
}
