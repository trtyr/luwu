//! Turn engine — the agent loop that drives conversations.
//!
//! The [`TurnEngine`] is the heart of luwu. It orchestrates the full cycle:
//!
//! ```text
//! User input → LLM → [tool calls → execute → feed results back → LLM] → Done
//! ```
//!
//! The engine is completely provider-agnostic — it works with any
//! [`LlmProvider`] and [`ToolRegistry`] you give it.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::error::{LuwuError, Result};
use crate::event::{Event, EventBus, TurnEvent, TurnId};
use crate::llm::{LlmEvent, LlmProvider, LlmRequest};
use crate::message::{ContentPart, Message};
use crate::prompt::system_prompt_with_tools_and_skills;
use crate::session::SessionData;
use crate::tool_registry::ToolRegistry;

// ---------------------------------------------------------------------------
// Turn result
// ---------------------------------------------------------------------------

/// The final result of a completed turn.
#[derive(Debug)]
pub struct TurnResult {
    /// All messages produced during this turn (assistant responses + tool results).
    pub messages: Vec<Message>,
    /// The accumulated text the assistant produced (concatenated from deltas).
    pub assistant_text: String,
    /// How many times the LLM was called (1 = no tool calls, 2+ = tool loops).
    pub llm_calls: u32,
    /// How many tool invocations happened.
    pub tool_calls: u32,
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// A token that can be used to cancel a running turn.
#[derive(Debug, Clone)]
pub struct CancelToken {
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl CancelToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { tx, rx }
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    /// Check if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Get a clone of the receiver for checking in async contexts.
    pub fn receiver(&self) -> watch::Receiver<bool> {
        self.rx.clone()
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Turn engine
// ---------------------------------------------------------------------------

/// The agent loop engine.
///
/// A `TurnEngine` is configured with an LLM provider, a tool registry,
/// and an event bus. You call [`TurnEngine::run`] to process a user
/// message through the full agent loop.
pub struct TurnEngine {
    provider: std::sync::Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    skills: crate::skill::SkillRegistry,
    events: EventBus,
    working_dir: PathBuf,
    /// Maximum number of LLM → tool → LLM iterations before forcing a stop.
    max_iterations: u32,
}

impl TurnEngine {
    /// Create a new turn engine.
    pub fn new(
        provider: std::sync::Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        skills: crate::skill::SkillRegistry,
        events: EventBus,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            provider,
            tools,
            skills,
            events,
            working_dir,
            max_iterations: 50,
        }
    }

    /// Set the maximum number of agentic iterations (LLM → tool → LLM loops).
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Run a single turn: send the user message through the full agent loop.
    /// Returns the complete result (non-streaming).
    pub async fn run(&self, session: &mut SessionData, user_message: String) -> Result<TurnResult> {
        let turn_id = TurnId::new();
        let session_id = session.id.clone();

        info!(
            session_id = %session_id,
            turn_id = %turn_id,
            user_message_len = user_message.len(),
            "Starting turn"
        );

        self.events.publish(Event::TurnStarted {
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
        });

        // Add the user message to the session.
        session.push_message(Message::user(&user_message));

        let mut result = TurnResult {
            messages: Vec::new(),
            assistant_text: String::new(),
            llm_calls: 0,
            tool_calls: 0,
        };

        // The agentic loop.
        let mut iteration = 0;
        let mut last_usage = None;

        loop {
            iteration += 1;
            if iteration > self.max_iterations {
                warn!(
                    iterations = iteration,
                    "Hit max iteration limit, stopping turn"
                );
                self.events.publish(Event::Error {
                    session_id: session_id.clone(),
                    turn_id: Some(turn_id.clone()),
                    message: format!("Turn exceeded max iterations ({})", self.max_iterations),
                });
                break;
            }

            result.llm_calls += 1;

            // Build the LLM request from the current session state.
            let request = self.build_request(session);

            // Call the LLM and collect its response.
            let (assistant_content, usage) = self.call_llm(request).await?;
            last_usage = usage;

            // Build the assistant message.
            let assistant_msg = Message {
                role: crate::message::Role::Assistant,
                content: assistant_content.clone(),
                name: None,
                tool_call_id: None,
            };

            // Extract text and tool calls from the response.
            let text_parts: String = assistant_content
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");

            let tool_call_parts: Vec<&ContentPart> = assistant_content
                .iter()
                .filter(|p| matches!(p, ContentPart::ToolCall { .. }))
                .collect();

            // Add assistant message to session.
            session.push_message(assistant_msg.clone());
            result.messages.push(assistant_msg);
            result.assistant_text.push_str(&text_parts);

            // If there are no tool calls, the turn is done.
            if tool_call_parts.is_empty() {
                debug!(iteration, "Turn complete (no tool calls)");
                break;
            }

            // Execute tool calls.
            for part in &tool_call_parts {
                if let ContentPart::ToolCall {
                    id,
                    name,
                    arguments,
                } = part
                {
                    result.tool_calls += 1;
                    let tool_name = name.clone();
                    let call_id = id.clone();

                    info!(tool = %tool_name, call_id = %call_id, "Executing tool");

                    self.events.publish(Event::ToolStarted {
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                    });

                    self.events.publish(Event::LlmToolCall {
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                    });

                    let output = match self
                        .tools
                        .execute(
                            name,
                            arguments.clone(),
                            self.working_dir.clone(),
                            session.id.clone(),
                        )
                        .await
                    {
                        Ok(output) => output,
                        Err(e) => crate::tool::ToolOutput {
                            content: e.to_string(),
                            is_error: true,
                        },
                    };

                    tracing::info!("Tool completed: {}", tool_name);
                    self.events.publish(Event::ToolCompleted {
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        call_id: call_id.clone(),
                        output: output.clone(),
                    });

                    // Add tool result to session.
                    let result_msg = Message::tool_result(call_id, output.content, output.is_error);
                    session.push_message(result_msg.clone());
                    result.messages.push(result_msg);
                }
            }

            // Loop back.
            debug!(
                iteration,
                tools_executed = tool_call_parts.len(),
                "Tool calls executed, continuing loop"
            );
        }

        tracing::info!("Agent turn completed, iterations={}", iteration);
        self.events.publish(Event::TurnCompleted {
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            usage: last_usage.unwrap_or_default(),
        });

        info!(
            llm_calls = result.llm_calls,
            tool_calls = result.tool_calls,
            text_len = result.assistant_text.len(),
            "Turn finished"
        );

        Ok(result)
    }

    /// Run a single turn in **streaming mode**.
    ///
    /// Returns an `mpsc::Receiver<TurnEvent>` that yields events in real-time:
    /// text deltas, tool calls, tool results, and a final `Done` event.
    ///
    /// Optionally accepts a `CancelToken` for user-initiated cancellation.
    #[tracing::instrument(skip(self))]
    pub async fn run_stream(
        &self,
        session_id: crate::event::SessionId,
        model: String,
        messages: Vec<Message>,
        user_message: String,
        cancel: Option<CancelToken>,
    ) -> mpsc::Receiver<TurnEvent> {
        let (tx, rx) = mpsc::channel(256);

        // Build initial messages with the user message appended.
        let request_messages = {
            let mut msgs = messages;
            msgs.push(Message::user(&user_message));
            msgs
        };

        let provider = self.provider.clone();
        let tools = self.tools.clone();
        let skills = self.skills.clone();
        let events = self.events.clone();
        let working_dir = self.working_dir.clone();
        let max_iterations = self.max_iterations;
        let system_prompt = system_prompt_with_tools_and_skills(&tools.tool_names(), &skills);

        tokio::spawn(async move {
            if let Some(cancel) = &cancel
                && cancel.is_cancelled()
            {
                let _ = tx.send(TurnEvent::Cancelled).await;
                return;
            }

            let turn_id = TurnId::new();
            let session_id_clone = session_id.clone();

            events.publish(Event::TurnStarted {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            });

            // Track all messages for the session.
            let mut all_messages = request_messages;
            let mut assistant_text = String::new();
            let mut llm_calls = 0u32;
            let mut tool_calls_count = 0u32;
            let mut iteration = 0u32;
            tracing::info!("Agent turn started, messages={}", all_messages.len());
            let _turn_start = std::time::Instant::now();
            let mut total_usage = crate::llm::LlmUsage::default();

            loop {
                // Check cancellation.
                if let Some(cancel) = &cancel
                    && cancel.is_cancelled()
                {
                    let _ = tx.send(TurnEvent::Cancelled).await;
                    break;
                }

                iteration += 1;
                tracing::debug!("Iteration {} started", iteration);
                if iteration > max_iterations {
                    let _ = tx
                        .send(TurnEvent::Error {
                            message: format!("Exceeded max iterations ({})", max_iterations),
                        })
                        .await;
                    break;
                }

                llm_calls += 1;

                // Open a new LLM stream for this iteration.
                let request = LlmRequest {
                    model: model.clone(),
                    messages: all_messages.clone(),
                    tools: tools.definitions(),
                    system_prompt: Some(system_prompt.clone()),
                    temperature: None,
                    max_tokens: None,
                    stop_sequences: Vec::new(),
                };

                let mut stream_rx = match provider.stream(request).await {
                    Ok(rx) => rx,
                    Err(e) => {
                        let _ = tx
                            .send(TurnEvent::Error {
                                message: format!("LLM stream failed: {}", e),
                            })
                            .await;
                        break;
                    }
                };

                // Consume the LLM stream, emitting text deltas as they arrive.
                let mut content_parts: Vec<ContentPart> = Vec::new();
                let mut current_text = String::new();
                let mut reasoning_text = String::new();
                let mut pending_tool_calls: HashMap<String, PendingToolCall> = HashMap::new();

                while let Some(event_result) = stream_rx.recv().await {
                    // Check cancellation during streaming.
                    if let Some(cancel) = &cancel
                        && cancel.is_cancelled()
                    {
                        let _ = tx.send(TurnEvent::Cancelled).await;
                        return;
                    }

                    let event = match event_result {
                        Ok(e) => e,
                        Err(e) => {
                            let _ = tx
                                .send(TurnEvent::Error {
                                    message: e.to_string(),
                                })
                                .await;
                            return;
                        }
                    };

                    match event {
                        LlmEvent::TextDelta(delta) => {
                            // Emit the delta immediately!
                            let _ = tx
                                .send(TurnEvent::TextDelta {
                                    delta: delta.clone(),
                                })
                                .await;
                            current_text.push_str(&delta);
                        }
                        LlmEvent::ReasoningDelta(reasoning) => {
                            let _ = tx
                                .send(TurnEvent::ReasoningDelta {
                                    delta: reasoning.clone(),
                                })
                                .await;
                            reasoning_text.push_str(&reasoning);
                        }

                        LlmEvent::ToolCallBegin { id, name } => {
                            if !current_text.is_empty() {
                                content_parts.push(ContentPart::Text {
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            pending_tool_calls.insert(
                                id.clone(),
                                PendingToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: String::new(),
                                },
                            );
                        }

                        LlmEvent::ToolCallDelta { id, delta } => {
                            if let Some(tc) = pending_tool_calls.get_mut(&id) {
                                tc.arguments.push_str(&delta);
                            }
                        }

                        LlmEvent::Done(usage) => {
                            // Accumulate usage across iterations.
                            total_usage.prompt_tokens += usage.prompt_tokens;
                            total_usage.completion_tokens += usage.completion_tokens;
                            total_usage.total_tokens += usage.total_tokens;
                        }

                        LlmEvent::Error(msg) => {
                            let _ = tx.send(TurnEvent::Error { message: msg }).await;
                            return;
                        }
                    }
                }

                // Flush remaining text. If only reasoning was produced (no text content),
                // emit it as a TextDelta so consumers always get text.
                if current_text.is_empty() && !reasoning_text.is_empty() {
                    let _ = tx
                        .send(TurnEvent::TextDelta {
                            delta: reasoning_text.clone(),
                        })
                        .await;
                    current_text = reasoning_text;
                }

                // Detect skill reference in assistant text and inject instructions.
                if let Some(skill_name) = skills.detect_skill_reference(&current_text)
                    && let Some(skill) = skills.get(&skill_name)
                {
                    tracing::info!("Skill activated: {}", skill_name);
                    let inject = format!(
                        "\n\n[Skill activated: {}]\n{}\n\nFollow these instructions for the current task.",
                        skill.name, skill.instructions
                    );
                    all_messages.push(crate::message::Message {
                        role: crate::message::Role::User,
                        content: vec![crate::message::ContentPart::Text { text: inject }],
                        name: None,
                        tool_call_id: None,
                    });
                }
                if !current_text.is_empty() {
                    content_parts.push(ContentPart::Text {
                        text: std::mem::take(&mut current_text),
                    });
                }

                // Finalize tool calls.
                let finalized_tool_calls: Vec<PendingToolCall> =
                    pending_tool_calls.into_values().collect();

                for tc in &finalized_tool_calls {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    content_parts.push(ContentPart::ToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: args.clone(),
                    });
                    assistant_text.push_str(&format!("[tool: {}]\n", tc.name));
                }

                // Build and add assistant message.
                let assistant_msg = Message {
                    role: crate::message::Role::Assistant,
                    content: content_parts.clone(),
                    name: None,
                    tool_call_id: None,
                };
                all_messages.push(assistant_msg);

                // If no tool calls, emit Done and finish.
                if finalized_tool_calls.is_empty() {
                    // Collect assistant text from content parts.
                    let full_text: String = content_parts
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    assistant_text = full_text;

                    let _ = tx
                        .send(TurnEvent::Done {
                            assistant_text: assistant_text.clone(),
                            llm_calls,
                            tool_calls: tool_calls_count,
                            usage: total_usage.clone(),
                        })
                        .await;
                    break;
                }

                // Execute tool calls.
                for tc in &finalized_tool_calls {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);

                    let _ = tx
                        .send(TurnEvent::ToolCall {
                            call_id: tc.id.clone(),
                            tool_name: tc.name.clone(),
                            arguments: args.clone(),
                        })
                        .await;

                    let _ = tx
                        .send(TurnEvent::ToolStarted {
                            call_id: tc.id.clone(),
                            tool_name: tc.name.clone(),
                        })
                        .await;

                    events.publish(Event::ToolStarted {
                        session_id: session_id_clone.clone(),
                        turn_id: turn_id.clone(),
                        call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                    });

                    let output = match tools
                        .execute(
                            &tc.name,
                            args,
                            working_dir.clone(),
                            session_id_clone.clone(),
                        )
                        .await
                    {
                        Ok(o) => o,
                        Err(e) => crate::tool::ToolOutput {
                            content: e.to_string(),
                            is_error: true,
                        },
                    };

                    let _ = tx
                        .send(TurnEvent::ToolCompleted {
                            call_id: tc.id.clone(),
                            tool_name: tc.name.clone(),
                            output: output.content.clone(),
                            is_error: output.is_error,
                        })
                        .await;

                    events.publish(Event::ToolCompleted {
                        session_id: session_id_clone.clone(),
                        turn_id: turn_id.clone(),
                        call_id: tc.id.clone(),
                        output: output.clone(),
                    });

                    // Add tool result to messages.
                    let result_msg =
                        Message::tool_result(tc.id.clone(), output.content, output.is_error);
                    all_messages.push(result_msg);
                    tool_calls_count += 1;
                }

                // Emit iteration end.
                let _ = tx
                    .send(TurnEvent::IterationEnd {
                        iteration,
                        tool_calls: finalized_tool_calls.len() as u32,
                    })
                    .await;

                // Loop back — the next iteration will open a new LLM stream
                // with the tool results appended to the message history.
            }

            events.publish(Event::TurnCompleted {
                session_id: session_id_clone,
                turn_id,
                usage: total_usage,
            });
        });

        rx
    }

    /// Build an [`LlmRequest`] from the current session state.
    fn build_request(&self, session: &SessionData) -> LlmRequest {
        LlmRequest {
            model: session.model.clone(),
            messages: session.messages.clone(),
            tools: self.tools.definitions(),
            system_prompt: Some(system_prompt_with_tools_and_skills(
                &self.tools.tool_names(),
                &self.skills,
            )),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    /// Call the LLM and collect the full response (all content parts + usage).
    #[tracing::instrument(skip(self))]
    async fn call_llm(
        &self,
        request: LlmRequest,
    ) -> Result<(Vec<ContentPart>, Option<crate::llm::LlmUsage>)> {
        let mut rx = self.provider.stream(request).await?;

        let mut content_parts: Vec<ContentPart> = Vec::new();
        let mut current_text = String::new();
        let mut reasoning_text = String::new();
        let mut tool_calls: HashMap<String, PendingToolCall> = HashMap::new();
        let mut final_usage = None;

        while let Some(event_result) = rx.recv().await {
            let event = event_result?;

            match event {
                LlmEvent::TextDelta(delta) => {
                    current_text.push_str(&delta);
                }

                LlmEvent::ReasoningDelta(reasoning) => {
                    reasoning_text.push_str(&reasoning);
                }

                LlmEvent::ToolCallBegin { id, name } => {
                    // Flush any accumulated text first.
                    if !current_text.is_empty() {
                        content_parts.push(ContentPart::Text {
                            text: std::mem::take(&mut current_text),
                        });
                    }

                    tool_calls.insert(
                        id.clone(),
                        PendingToolCall {
                            id,
                            name,
                            arguments: String::new(),
                        },
                    );
                }

                LlmEvent::ToolCallDelta { id, delta } => {
                    if let Some(tc) = tool_calls.get_mut(&id) {
                        tc.arguments.push_str(&delta);
                    }
                }

                LlmEvent::Done(usage) => {
                    final_usage = Some(usage);
                }

                LlmEvent::Error(msg) => {
                    return Err(LuwuError::Llm(msg));
                }
            }
        }

        // Flush remaining text. If only reasoning was produced (no text content),
        // treat the reasoning as the response content.
        if current_text.is_empty() && !reasoning_text.is_empty() {
            current_text = reasoning_text;
        }
        if !current_text.is_empty() {
            content_parts.push(ContentPart::Text { text: current_text });
        }

        // Add tool calls.
        for (_, tc) in tool_calls {
            let arguments: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
            content_parts.push(ContentPart::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments,
            });
        }

        Ok((content_parts, final_usage))
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A tool call being accumulated from streaming deltas.
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}
