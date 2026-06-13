//! Health check and model listing endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use crate::app::AppState;
use crate::types::{ModelInfo, ModelsResponse};

pub async fn health() -> &'static str {
    "ok"
}

pub async fn list_models(State(state): State<Arc<AppState>>) -> Json<ModelsResponse> {
    let mut models = Vec::new();

    if let Some(default_model) = &state.config.default.model {
        models.push(ModelInfo {
            id: default_model.clone(),
            object: "model".to_string(),
            created: 0,
            owned_by: state
                .config
                .default
                .provider
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        });
    }

    for (name, provider) in &state.config.providers {
        if let Some(model) = &provider.model {
            models.push(ModelInfo {
                id: model.clone(),
                object: "model".to_string(),
                created: 0,
                owned_by: name.clone(),
            });
        }
    }

    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
}
