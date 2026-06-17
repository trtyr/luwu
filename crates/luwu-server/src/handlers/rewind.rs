//! Rewind handler — conversation rewind, code restore, and summarize.
//!
// - GET /v1/sessions/{id}/rewind/messages — list user messages for selection
// - POST /v1/sessions/{id}/rewind — rewind conversation and/or restore code
// - POST /v1/sessions/{id}/summarize — compress messages from a point

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use luwu_core::file_history::{DiffStats, FileHistory};

use crate::app::{AppState, create_provider};
use crate::config::Config;
use crate::error::ApiError;

// ── Response types ──

#[derive(Debug, Serialize)]
pub struct RewindMessage {
    pub index: usize,
    pub text: String,
    pub diff_stats: Option<DiffStats>,
}

#[derive(Debug, Serialize)]
pub struct RewindMessagesResponse {
    pub messages: Vec<RewindMessage>,
}

#[derive(Debug, Deserialize)]
pub struct RewindRequest {
    pub message_index: usize,
    #[serde(default)]
    pub restore_code: bool,
    #[serde(default)]
    pub restore_conversation: bool,
}

#[derive(Debug, Serialize)]
pub struct RewindResponse {
    pub restored_text: String,
    pub files_changed: Vec<String>,
    pub remaining_messages: usize,
}

#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub message_index: usize,
    #[serde(default = "default_direction")]
    pub direction: String,
    pub feedback: Option<String>,
}

fn default_direction() -> String {
    "from".to_string()
}

#[derive(Debug, Serialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub messages_removed: usize,
}

// ── Helpers ──

fn extract_text(msg: &luwu_core::Message) -> String {
    msg.content
        .iter()
        .filter_map(|p| {
            if let luwu_core::ContentPart::Text { text } = p {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn luwu_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".luwu")
}

// ── Handlers ──

/// GET /v1/sessions/{id}/rewind/messages
pub async fn list_rewind_messages(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<RewindMessagesResponse>, ApiError> {
    let session = state
        .sessions
        .get(&session_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Session '{session_id}' not found")))?;

    let session_dir = luwu_home().join("sessions").join(&session_id);
    let fh = FileHistory::new(&session_dir, &state.working_dir);

    let mut messages = Vec::new();
    let mut user_msg_index = 0usize;

    for msg in &session.data.messages {
        if msg.role == luwu_core::Role::User {
            let text = extract_text(msg);
            let display_text = if text.len() > 200 {
                let boundary = text.floor_char_boundary(200);
                format!("{}…", &text[..boundary])
            } else {
                text.clone()
            };

            let msg_ref = text.chars().take(100).collect::<String>();
            let diff_stats = fh.diff_stats_for(&msg_ref);

            messages.push(RewindMessage {
                index: user_msg_index,
                text: display_text,
                diff_stats,
            });
            user_msg_index += 1;
        }
    }

    Ok(Json(RewindMessagesResponse { messages }))
}

/// POST /v1/sessions/{id}/rewind
pub async fn rewind_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<RewindRequest>,
) -> Result<Json<RewindResponse>, ApiError> {
    let session = state
        .sessions
        .get(&session_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Session '{session_id}' not found")))?;

    // Find the user message at the given index
    let mut user_msg_count = 0usize;
    let mut target_abs_index = None;
    let mut target_text = String::new();

    for (i, msg) in session.data.messages.iter().enumerate() {
        if msg.role == luwu_core::Role::User {
            if user_msg_count == req.message_index {
                target_abs_index = Some(i);
                target_text = extract_text(msg);
                break;
            }
            user_msg_count += 1;
        }
    }

    let target_index = target_abs_index
        .ok_or_else(|| ApiError::BadRequest("Message index out of range".to_string()))?;

    let mut files_changed = Vec::new();

    // ── Restore code ──
    if req.restore_code {
        let session_dir = luwu_home().join("sessions").join(&session_id);
        let fh = FileHistory::new(&session_dir, &state.working_dir);
        let msg_ref = target_text.chars().take(100).collect::<String>();
        match fh.rewind_to(&msg_ref) {
            Ok(changed) => {
                files_changed = changed;
                tracing::info!(files = files_changed.len(), "Code restore completed");
            }
            Err(e) => {
                tracing::warn!(%e, "Code restore failed — no snapshot found");
            }
        }
    }

    // ── Restore conversation ──
    let remaining = if req.restore_conversation {
        let truncated: Vec<_> = session.data.messages[..target_index].to_vec();
        let count = truncated.len();
        state.sessions.update_messages(&session_id, truncated).await;
        count
    } else {
        session.data.messages.len()
    };

    Ok(Json(RewindResponse {
        restored_text: target_text,
        files_changed,
        remaining_messages: remaining,
    }))
}

/// POST /v1/sessions/{id}/summarize
pub async fn summarize_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ApiError> {
    let session = state
        .sessions
        .get(&session_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Session '{session_id}' not found")))?;

    // Find the absolute index of the user message at message_index
    let mut user_msg_count = 0usize;
    let mut target_abs_index = None;

    for (i, msg) in session.data.messages.iter().enumerate() {
        if msg.role == luwu_core::Role::User {
            if user_msg_count == req.message_index {
                target_abs_index = Some(i);
                break;
            }
            user_msg_count += 1;
        }
    }

    let target_index = target_abs_index
        .ok_or_else(|| ApiError::BadRequest("Message index out of range".to_string()))?;

    let all_messages = &session.data.messages;
    let (to_summarize, to_keep): (Vec<luwu_core::Message>, Vec<luwu_core::Message>) =
        if req.direction == "up_to" {
            let (sum, keep) = all_messages.split_at(target_index);
            (sum.to_vec(), keep.to_vec())
        } else {
            let (keep, sum) = all_messages.split_at(target_index + 1);
            (sum.to_vec(), keep.to_vec())
        };

    if to_summarize.is_empty() {
        return Ok(Json(SummarizeResponse {
            summary: "No messages to summarize.".to_string(),
            messages_removed: 0,
        }));
    }

    // Build conversation text
    let conv_text: String = to_summarize
        .iter()
        .map(|m| format!("{}: {}", &format!("{:?}", m.role), extract_text(m)))
        .collect::<Vec<_>>()
        .join("\n\n");

    let config = Config::load().map_err(|e| ApiError::Internal(e.to_string()))?;
    let resolved = config
        .resolve(None)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let provider = create_provider(&resolved, state.http_client.clone());

    let prompt = format!(
        "Summarize the following conversation concisely. Capture key decisions, actions taken, and important context.\n\n{}\n\nSummary:",
        conv_text.chars().take(8000).collect::<String>()
    );

    let request = luwu_core::LlmRequest {
        model: resolved.model.clone(),
        messages: vec![],
        tools: vec![],
        system_prompt: Some(prompt),
        temperature: Some(0.1),
        max_tokens: Some(2000),
        stop_sequences: vec![],
        extra_body: None,
    };

    let summary = provider
        .complete(request)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Update session: keep messages + summary
    let mut new_messages = to_keep.clone();
    new_messages.push(luwu_core::Message::assistant(format!(
        "📋 **Conversation summarized**\n\n{summary}"
    )));

    let removed = to_summarize.len();
    state
        .sessions
        .update_messages(&session_id, new_messages)
        .await;

    Ok(Json(SummarizeResponse {
        summary,
        messages_removed: removed,
    }))
}
