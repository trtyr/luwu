//! Memory worker functions — consolidation, checkpoint, observer, reflector.
//!
//! These currently use raw HTTP calls with a hardcoded model name.
//! TODO (Phase A2): route through LlmProvider::complete() instead.

use luwu_core::message::{ContentPart, Role};
use luwu_core::{writer_system_prompt, Message};
use luwu_memory::{
    apply_consolidation, consolidation_prompt,
    observer_prompt, reflector_prompt, ConsolidationNeeded, MemoryFileType,
    MemoryStore, Observation, Priority, Reflection,
};

/// Run the consolidation Writer — an LLM call that merges memory entries.
pub(crate) async fn run_consolidation_writer(
    api_key: &str,
    base_url: &str,
    content: &str,
    file_path: &std::path::Path,
    file_type: MemoryFileType,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use serde_json::json;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = json!({
        "model": "MiniMax-M3",
        "temperature": 0.1,
        "max_tokens": 4096,
        "messages": [
            {"role": "system", "content": consolidation_prompt()},
            {"role": "user", "content": content}
        ]
    });

    let url = format!("{}/chat/completions", base_url);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let consolidated = data
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or(content);

    let needed = ConsolidationNeeded {
        file_type,
        current_size: content.chars().count(),
        threshold: 8000,
        path: file_path.to_path_buf(),
    };
    apply_consolidation(&needed, consolidated);
    tracing::info!("Consolidated {} memory file", file_type.label());
    Ok(())
}

/// Run the checkpoint Writer — an independent LLM call that extracts
/// structured state from conversation history.
pub(crate) async fn run_checkpoint_writer(
    api_key: &str,
    base_url: &str,
    _messages: &[Message],
    memory: &MemoryStore,
) -> Result<(), String> {
    let client: reqwest::Client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let system_prompt = writer_system_prompt();

    let current_checkpoint = memory.read_checkpoint_raw();
    let user_content = if current_checkpoint.is_empty() {
        "（新会话，尚无历史记录）".to_string()
    } else {
        format!("以下是当前 checkpoint，请增量更新：\n\n{}", current_checkpoint)
    };

    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_content}
        ],
        "temperature": 0.1,
        "max_tokens": 4096
    });

    let resp = client
        .post(format!("{}/chat/completions", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Writer request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Writer parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    if !output.is_empty() {
        memory
            .write_checkpoint_raw(output)
            .map_err(|e| format!("Writer write failed: {e}"))?;
    }

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
                ContentPart::ToolResult { content, is_error, .. } => {
                    let prefix = if *is_error { "Tool Error" } else { "Tool Result" };
                    out.push_str(&format!("{prefix}: {content}\n\n"));
                }
            }
            if out.len() > max_chars {
                out.truncate(max_chars);
                out.push_str("...[truncated]");
                return out;
            }
        }
    }
    out
}

/// Run the Observer worker — extracts timestamped observations from conversation.
pub(crate) async fn run_observer_worker(
    api_key: &str,
    base_url: &str,
    messages: &[Message],
    memory: &MemoryStore,
) -> Result<usize, String> {
    let transcript = messages_to_transcript(messages, 30_000);
    if transcript.is_empty() {
        return Ok(0);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": observer_prompt()},
            {"role": "user", "content": transcript}
        ],
        "temperature": 0.1,
        "max_tokens": 2048
    });

    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Observer request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Observer parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

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
            let category = v.get("category").and_then(|c| c.as_str()).unwrap_or("event").to_string();
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            if !content.is_empty() {
                let obs = Observation::new(priority, category, content);
                if let Err(e) = memory.append_observation(&obs) { tracing::warn!(%e, "Failed to append observation"); }
                count += 1;
            }
        }
    }

    tracing::info!("Observer extracted {} observations", count);
    Ok(count)
}

/// Run the Reflector worker — synthesizes durable reflections from observations.
pub(crate) async fn run_reflector_worker(
    api_key: &str,
    base_url: &str,
    memory: &MemoryStore,
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": reflector_prompt()},
            {"role": "user", "content": obs_text}
        ],
        "temperature": 0.1,
        "max_tokens": 2048
    });

    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Reflector request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Reflector parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    let mut count = 0;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
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
                if let Err(e) = memory.append_reflection(&refl) { tracing::warn!(%e, "Failed to append reflection"); }
                count += 1;
            }
        }
    }

    tracing::info!("Reflector synthesized {} reflections", count);
    Ok(count)
}
