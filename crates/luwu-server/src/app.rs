//! Application state, router wiring, and shared infrastructure.
//!
//! Extracted from `api.rs` to separate infrastructure concerns from
//! request handling logic.

use std::sync::Arc;

use axum::Router;
use tokio::task::JoinSet;
use tower_http::cors::CorsLayer;

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
    /// Shared HTTP client with connection pool — all providers and workers use this.
    pub http_client: reqwest::Client,
    /// Tracked worker tasks — aborted on shutdown.
    pub worker_tasks: tokio::sync::Mutex<JoinSet<()>>,
}

impl AppState {
    /// Spawn a tracked worker task. All workers are aborted on shutdown.
    pub fn spawn_worker(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        self.worker_tasks
            .try_lock()
            .expect("worker_tasks lock poisoned")
            .spawn(task);
    }
}

/// Build the default tool registry from `luwu_tools`.
pub fn builtin_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool in luwu_tools::all_builtin_tools() {
        registry.register(tool);
    }
    registry
}

/// Provider factory — selects the correct LLM provider based on config.
///
/// Matching is by provider name (the config key):
/// - `"anthropic"` → AnthropicProvider (Claude Messages API)
/// - Everything else → OpenAiProvider (OpenAI-compatible: OpenAI, MiniMax, DeepSeek, etc.)
pub fn create_provider(resolved: &ResolvedConfig, http_client: reqwest::Client) -> Arc<dyn LlmProvider> {
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

/// Build the axum router with all routes registered.
pub fn router(state: AppState) -> Router {
    Router::new()
        // Health & models.
        .route("/health", axum::routing::get(handlers::health))
        .route("/v1/models", axum::routing::get(handlers::list_models))
        // OpenAI-compatible chat completions.
        .route(
            "/v1/chat/completions",
            axum::routing::post(handlers::chat_completions),
        )
        // Session management.
        .route("/v1/sessions", axum::routing::get(handlers::list_sessions))
        .route(
            "/v1/sessions",
            axum::routing::post(handlers::create_session),
        )
        .route(
            "/v1/sessions/{id}",
            axum::routing::get(handlers::get_session).delete(handlers::delete_session),
        )
        // Agent event stream.
        .route(
            "/v1/sessions/{id}/chat",
            axum::routing::post(handlers::agent_chat),
        )
        // Cancel.
        .route(
            "/v1/sessions/{id}/cancel",
            axum::routing::post(handlers::cancel_turn),
        )
        // Memory endpoints.
        .route(
            "/v1/sessions/{id}/checkpoint",
            axum::routing::get(handlers::get_checkpoint),
        )
        .route(
            "/v1/sessions/{id}/history",
            axum::routing::get(handlers::search_history),
        )
        // Skill endpoints.
        .route("/v1/skills", axum::routing::get(handlers::list_skills))
        .route(
            "/v1/skills/{name}",
            axum::routing::get(handlers::get_skill_detail),
        )
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state))
}
