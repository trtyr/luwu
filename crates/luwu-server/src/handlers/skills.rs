//! Skill listing and detail endpoints.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::app::AppState;

/// GET /v1/skills — list all loaded skills.
pub async fn list_skills(State(state): State<Arc<AppState>>) -> axum::response::Response {
    let skills = state.skills.list();
    let summary: Vec<serde_json::Value> = skills
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "skills": summary })).into_response()
}

/// GET /v1/skills/{name} — get skill details.
pub async fn get_skill_detail(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let Some(skill) = state.skills.get(&name) else {
        return (axum::http::StatusCode::NOT_FOUND, "Skill not found").into_response();
    };
    let files = state.skills.skill_files(&name);
    Json(serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "instructions": skill.instructions,
        "base_path": skill.base_path.to_string_lossy(),
        "files": files,
    }))
    .into_response()
}
