//! Memory worker functions — consolidation, observer, reflector.
//!
//! All workers route LLM calls through the LlmProvider trait.
//! No raw HTTP — no hardcoded model names.

use std::sync::Arc;

use luwu_core::message::{ContentPart, Role};
use luwu_core::{LlmProvider, LlmRequest, Message};
use luwu_memory::{
    ConsolidationNeeded, MemoryFileType, MemoryStore, Observation, Priority, Reflection,
    apply_consolidation, consolidation_prompt, observer_prompt, reflector_prompt,
};

/// Run the consolidation Writer — merges memory entries when files exceed threshold.
pub(crate) async fn run_consolidation_writer(
    provider: Arc<dyn LlmProvider>,
    model: String,
    content: String,
    file_path: std::path::PathBuf,
    file_type: MemoryFileType,
) -> Result<(), String> {
    let request = LlmRequest {
        model,
        messages: vec![Message::user(&content)],
        tools: vec![],
        system_prompt: Some(consolidation_prompt().to_string()),
        temperature: Some(0.1),
        max_tokens: Some(4096),
        stop_sequences: vec![],
        extra_body: None,
    };
    let consolidated = provider
        .complete(request)
        .await
        .map_err(|e| format!("Consolidation LLM call failed: {e}"))?;

    let needed = ConsolidationNeeded {
        file_type,
        current_size: content.chars().count(),
        threshold: 8000,
        path: file_path,
    };
    apply_consolidation(&needed, &consolidated);
    tracing::info!("Consolidated {} memory file", file_type.label());
    Ok(())
}

/// Convert messages into a readable transcript for LLM workers.
pub(crate) fn messages_to_transcript(messages: &[Message], max_chars: usize) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            _ => "System",
        };
        for part in &msg.content {
            match part {
                ContentPart::Text { text } => {
                    out.push_str(&format!("{role}: {text}\n\n"));
                }
                ContentPart::ToolCall { name, .. } => {
                    out.push_str(&format!("{role}: [called tool: {name}]\n\n"));
                }
                ContentPart::ToolResult {
                    content, is_error, ..
                } => {
                    let prefix = if *is_error {
                        "Tool Error"
                    } else {
                        "Tool Result"
                    };
                    out.push_str(&format!("{prefix}: {content}\n\n"));
                }
                // Reasoning/thinking content is rendered as plain text
                // for the transcript view — the TUI displays it separately
                // in ReasoningBlock, but for the human-readable transcript
                // a flat string is fine.
                ContentPart::Reasoning { text } => {
                    out.push_str(&format!("{role}: {text}\n\n"));
                }
            }
            if out.len() > max_chars {
                let end = out.floor_char_boundary(max_chars);
                out.truncate(end);
                out.push_str("...[truncated]");
                return out;
            }
        }
    }
    out
}

/// Run the Observer worker — extracts timestamped observations from conversation.
pub(crate) async fn run_observer_worker(
    provider: Arc<dyn LlmProvider>,
    model: String,
    messages: Vec<Message>,
    memory: Arc<MemoryStore>,
) -> Result<usize, String> {
    let transcript = messages_to_transcript(&messages, 30_000);
    if transcript.is_empty() {
        return Ok(0);
    }

    let request = LlmRequest {
        model,
        messages: vec![Message::user(&transcript)],
        tools: vec![],
        system_prompt: Some(observer_prompt().to_string()),
        temperature: Some(0.1),
        max_tokens: Some(2048),
        stop_sequences: vec![],
        extra_body: None,
    };
    let output = provider
        .complete(request)
        .await
        .map_err(|e| format!("Observer LLM call failed: {e}"))?;

    let mut count = 0;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let priority = match v.get("priority").and_then(|p| p.as_str()) {
                Some("high") => Priority::High,
                Some("low") => Priority::Low,
                _ => Priority::Medium,
            };
            let category = v
                .get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("event")
                .to_string();
            let content = v
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            if !content.is_empty() {
                let obs = Observation::new(priority, category, content);
                if let Err(e) = memory.append_observation(&obs) {
                    tracing::warn!(%e, "Failed to append observation");
                }
                count += 1;
            }
        }
    }

    tracing::info!("Observer extracted {} observations", count);
    Ok(count)
}

/// Run the Reflector worker — synthesizes durable reflections from observations.
pub(crate) async fn run_reflector_worker(
    provider: Arc<dyn LlmProvider>,
    model: String,
    memory: Arc<MemoryStore>,
) -> Result<usize, String> {
    let observations = memory.read_observations();
    if observations.is_empty() {
        return Ok(0);
    }

    let obs_text: String = observations
        .iter()
        .map(|o| format!("[{}] {} ({}): {}", o.id, o.timestamp, o.priority, o.content))
        .collect::<Vec<_>>()
        .join("\n");

    let request = LlmRequest {
        model,
        messages: vec![Message::user(&obs_text)],
        tools: vec![],
        system_prompt: Some(reflector_prompt().to_string()),
        temperature: Some(0.1),
        max_tokens: Some(2048),
        stop_sequences: vec![],
        extra_body: None,
    };
    let output = provider
        .complete(request)
        .await
        .map_err(|e| format!("Reflector LLM call failed: {e}"))?;

    let mut count = 0;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let content = v
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let source_ids: Vec<String> = v
                .get("source_ids")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if !content.is_empty() {
                let refl = Reflection::new(content, source_ids);
                if let Err(e) = memory.append_reflection(&refl) {
                    tracing::warn!(%e, "Failed to append reflection");
                }
                count += 1;
            }
        }
    }

    tracing::info!("Reflector synthesized {} reflections", count);
    Ok(count)
}
