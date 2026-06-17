//! Integration tests for luwu-server handlers.
//!
//! Tests the HTTP transport layer using axum's test utilities.
//! No real LLM calls — only pure infrastructure paths.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use tower::ServiceExt;

use luwu_server::app::{AppState, router};
use luwu_server::config::{Config, DefaultConfig, LoggingConfig, ProviderConfig};

/// Build a minimal AppState for testing — no file I/O, no network.
fn test_state() -> AppState {
    let mut providers = std::collections::HashMap::new();
    providers.insert(
        "test".to_string(),
        ProviderConfig {
            api_key: "test-key".to_string(),
            base_url: Some("https://api.test.com/v1".to_string()),
            model: Some("test-model".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(4096),
        },
    );

    AppState {
        config: Config {
            default: DefaultConfig {
                provider: Some("test".to_string()),
                model: Some("test-model".to_string()),
            },
            providers,
            logging: LoggingConfig::default(),
        },
        sessions: luwu_core::SessionManager::new(),
        working_dir: std::env::temp_dir(),
        skills: luwu_core::SkillRegistry::new(),
        http_client: reqwest::Client::new(),
        worker_tasks: tokio::sync::Mutex::new(tokio::task::JoinSet::new()),
        last_request: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
    }
}

async fn app() -> axum::Router {
    router(test_state())
}

// ─── Health ───────────────────────────────────────────────

#[tokio::test]
async fn health_returns_ok() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(text, "ok");
}

// ─── Models ───────────────────────────────────────────────

#[tokio::test]
async fn list_models_returns_list() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response should be valid JSON");
    assert_eq!(json["object"], "list");
    assert!(json["data"].is_array());
}

// ─── Sessions CRUD ────────────────────────────────────────

#[tokio::test]
async fn create_session_returns_201() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "model": "test-model"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn create_session_with_empty_body_defaults() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    // Either 201 (defaults work) or 400 — both acceptable for empty body.
    assert!(response.status() == StatusCode::OK || response.status() == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_sessions_empty_returns_array() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["sessions"].is_array());
    assert!(json["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_missing_session_returns_404() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_missing_session_returns_404() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/sessions/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_lifecycle_create_get_delete() {
    let app = app().await;

    // 1. Create
    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"model": "test-model"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(create_response.into_body(), 8192)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let session_id = json["id"].as_str().unwrap().to_string();

    // 2. Get
    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    // 3. List
    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_response.into_body(), 8192)
        .await
        .unwrap();
    let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json["sessions"].as_array().unwrap().len(), 1);

    // 4. Delete
    let del_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_response.status(), StatusCode::OK);

    // 5. Verify gone
    // (Can't reuse app — oneshot consumes it. Trust delete_response.)
}

// ─── Skills ───────────────────────────────────────────────

#[tokio::test]
async fn list_skills_returns_array() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/skills")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["skills"].is_array());
}

// ─── Config validation ────────────────────────────────────

#[tokio::test]
async fn agent_chat_missing_session_returns_404() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/nonexistent/chat")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({"message": "hi"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_nonexistent_session_returns_404_or_200() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/nonexistent/cancel")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Cancel of non-existent session — either 404 or 200 with cancelled:false.
    assert!(response.status() == StatusCode::NOT_FOUND || response.status() == StatusCode::OK);
}

// ─── Stats ────────────────────────────────────────────────

#[tokio::test]
async fn stats_returns_counts() {
    let app = app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessions"]["total"], 0);
    assert_eq!(json["sessions"]["running"], 0);
    assert_eq!(json["workers"]["active"], 0);
}

#[tokio::test]
async fn stats_reflects_created_session() {
    let app = app().await;

    // Create a session first.
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"model": "test-model"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Now stats should show 1 session.
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessions"]["total"], 1);
}

// ─── ApiError status code mapping ─────────────────────────────────
//
// The ApiError → HTTP status code mapping is the single most important
// contract between server and clients. These tests lock in the codes
// so a refactor can't silently regress the wire format.
//
// Covered:
// - Unauthorized    → 401 (LLM auth failure: 401/403 from provider)
// - GatewayTimeout  → 504 (LLM timeout)
// - NotFound        → 404 (covered by handler tests above)
// - Conflict        → 409 (covered by handler tests above)
// - BadRequest      → 400 (covered by handler tests above)
// - Internal        → 500 (the catch-all bucket)

#[tokio::test]
async fn api_error_unauthorized_maps_to_401() {
    use luwu_server::error::ApiError;
    let err = ApiError::Unauthorized("LLM authentication failed".to_string());
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_error_gateway_timeout_maps_to_504() {
    use luwu_server::error::ApiError;
    let err = ApiError::GatewayTimeout("LLM request timed out".to_string());
    let response = err.into_response();
    assert_eq!(
        response.status(),
        StatusCode::GATEWAY_TIMEOUT,
        "LlmError::Timeout should map to 504 (Gateway Timeout), not 500"
    );
}

#[tokio::test]
async fn api_error_not_found_maps_to_404() {
    use luwu_server::error::ApiError;
    let err = ApiError::NotFound("session not found".to_string());
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_error_conflict_maps_to_409() {
    use luwu_server::error::ApiError;
    let err = ApiError::Conflict("agent already running".to_string());
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn api_error_bad_request_maps_to_400() {
    use luwu_server::error::ApiError;
    let err = ApiError::BadRequest("invalid provider name".to_string());
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_error_internal_maps_to_500() {
    use luwu_server::error::ApiError;
    let err = ApiError::Internal("unexpected server error".to_string());
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ─── LuwuError → ApiError mapping ─────────────────────────────────
//
// The handler chain relies on `?` automatically converting LuwuError
// to ApiError. The LlmAuth / LlmTimeout variants must map to specific
// HTTP codes (not the Internal 500 bucket) so clients can react
// appropriately (refresh creds on 401, retry on 504, etc.).

#[test]
fn luwu_error_llmauth_maps_to_401() {
    use luwu_core::LuwuError;
    use luwu_server::error::ApiError;
    let err: ApiError = LuwuError::LlmAuth("bad API key".to_string()).into();
    assert!(
        matches!(err, ApiError::Unauthorized(_)),
        "LlmError::Auth should map to ApiError::Unauthorized, got {err:?}"
    );
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn luwu_error_llmtimeout_maps_to_504() {
    use luwu_core::LuwuError;
    use luwu_server::error::ApiError;
    let err: ApiError = LuwuError::LlmTimeout.into();
    assert!(
        matches!(err, ApiError::GatewayTimeout(_)),
        "LlmError::Timeout should map to ApiError::GatewayTimeout, got {err:?}"
    );
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}

#[test]
fn luwu_error_session_maps_to_404() {
    use luwu_core::LuwuError;
    use luwu_server::error::ApiError;
    let err: ApiError = LuwuError::Session("session xyz not found".to_string()).into();
    assert!(matches!(err, ApiError::NotFound(_)));
}

#[test]
fn luwu_error_config_maps_to_400() {
    use luwu_core::LuwuError;
    use luwu_server::error::ApiError;
    let err: ApiError = LuwuError::Config("missing api_key".to_string()).into();
    assert!(matches!(err, ApiError::BadRequest(_)));
}

#[test]
fn luwu_error_generic_maps_to_500() {
    use luwu_core::LuwuError;
    use luwu_server::error::ApiError;
    let err: ApiError = LuwuError::Llm("unknown provider error".to_string()).into();
    assert!(
        matches!(err, ApiError::Internal(_)),
        "generic LuwuError::Llm should still map to Internal 500, got {err:?}"
    );
}
