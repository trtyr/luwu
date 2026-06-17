//! Application state, router wiring, and shared infrastructure.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::State;
use axum::middleware::{self, Next};
use axum::response::Response;
use tokio::task::JoinSet;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use luwu_core::{LlmProvider, SessionManager, ToolRegistry};
use luwu_llm::anthropic::AnthropicProvider;
use luwu_llm::openai::OpenAiProvider;

use crate::config::{Config, ResolvedConfig};
use crate::handlers;

/// Shared application state accessible to all request handlers.
pub struct AppState {
    pub config: Config,
    pub sessions: SessionManager,
    pub working_dir: std::path::PathBuf,
    pub skills: luwu_core::SkillRegistry,
    pub http_client: reqwest::Client,
    pub worker_tasks: tokio::sync::Mutex<JoinSet<()>>,
    /// Epoch millis of the last request from any TUI client.
    /// Daemon auto-shuts down when this goes stale.
    pub last_request: Arc<AtomicU64>,
}

impl AppState {
    pub fn spawn_worker(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        self.worker_tasks
            .try_lock()
            .expect("worker_tasks lock poisoned")
            .spawn(task);
    }
}

pub fn builtin_tool_registry() -> ToolRegistry {
    let mut builder = ToolRegistry::builder();
    for tool in luwu_tools::all_builtin_tools() {
        builder = builder.register(tool);
    }
    builder.build()
}

pub fn create_provider(
    resolved: &ResolvedConfig,
    http_client: reqwest::Client,
) -> Arc<dyn LlmProvider> {
    match resolved.provider_name.as_str() {
        "anthropic" => Arc::new(AnthropicProvider::with_client(
            &resolved.api_key,
            &resolved.base_url,
            http_client,
        )),
        _ => Arc::new(OpenAiProvider::with_client(
            &resolved.api_key,
            &resolved.base_url,
            http_client,
        )),
    }
}

/// Middleware: stamp last_request on every HTTP request.
/// This lets the daemon know when the last TUI client was active.
async fn heartbeat_mw(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state.last_request.store(now, Ordering::Relaxed);
    next.run(request).await
}

pub fn router(state: AppState) -> Router {
    let shared = Arc::new(state);

    Router::new()
        .route("/health", axum::routing::get(handlers::health))
        .route("/v1/models", axum::routing::get(handlers::list_models))
        .route(
            "/v1/chat/completions",
            axum::routing::post(handlers::chat_completions),
        )
        .route("/v1/sessions", axum::routing::get(handlers::list_sessions))
        .route(
            "/v1/sessions",
            axum::routing::post(handlers::create_session),
        )
        .route(
            "/v1/sessions/{id}",
            axum::routing::get(handlers::get_session).delete(handlers::delete_session),
        )
        .route(
            "/v1/sessions/{id}/chat",
            axum::routing::post(handlers::agent_chat),
        )
        .route(
            "/v1/sessions/{id}/cancel",
            axum::routing::post(handlers::cancel_turn),
        )
        .route(
            "/v1/sessions/{id}/checkpoint",
            axum::routing::get(handlers::get_checkpoint),
        )
        .route(
            "/v1/sessions/{id}/history",
            axum::routing::get(handlers::search_history),
        )
        .route(
            "/v1/sessions/{id}/tasks",
            axum::routing::get(handlers::list_tasks),
        )
        .route(
            "/v1/sessions/{id}/rewind/messages",
            axum::routing::get(handlers::list_rewind_messages),
        )
        .route(
            "/v1/sessions/{id}/rewind",
            axum::routing::post(handlers::rewind_session),
        )
        .route(
            "/v1/sessions/{id}/summarize",
            axum::routing::post(handlers::summarize_session),
        )
        .route("/v1/stats", axum::routing::get(handlers::stats))
        .route("/v1/skills", axum::routing::get(handlers::list_skills))
        .route(
            "/v1/skills/{name}",
            axum::routing::get(handlers::get_skill_detail),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(shared.clone(), heartbeat_mw))
        .with_state(shared)
}
