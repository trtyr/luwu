//! Skill listing and detail endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;

use crate::app::AppState;
use crate::error::ApiError;

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
) -> Result<axum::response::Response, ApiError> {
    let skill = state
        .skills
        .get(&name)
        .ok_or_else(|| ApiError::NotFound(format!("Skill '{name}' not found")))?;
    let files = state.skills.skill_files(&name);
    Ok(Json(serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "instructions": skill.instructions,
        "base_path": skill.base_path.to_string_lossy(),
        "files": files,
    }))
    .into_response())
}
