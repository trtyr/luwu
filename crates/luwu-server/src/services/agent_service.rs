//! Agent service — orchestrates agent turns with cycle management and memory workers.
//!
//! This is the application layer: it knows about domain concepts (TurnEngine,
//! CycleState, MemoryStore, workers) but NOT about HTTP/SSE/axum.
//!
//! Construct via [`AgentService::builder`], then call [`run`](Self::run)
//! to get a receiver of [`AgentEvent`]s.
//!
//! # Builder pattern
//!
//! The handler is responsible for HTTP-gate concerns (try_set_running,
//! RunningGuard, session lookup, config resolution). Everything else —
//! the `MemoryStore`, `FileHistory`, `ToolRegistry`, `EventBus`,
//! `TurnEngine` — is constructed by the builder. This makes the service
//! testable in isolation (inject mocks via the builder) and keeps the
//! handler thin.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;

use luwu_core::{
    CancelToken, CycleAction, CycleState, EventBus, LlmProvider, Message, TurnEngine, TurnEvent,
};
use luwu_memory::{CorrectionDetector, CorrectionPattern, MemoryStore, compile_summary};

use crate::app::AppState;
use crate::config::ResolvedConfig;
use crate::handlers::workers::{
    run_consolidation_writer, run_observer_worker, run_reflector_worker,
};

// ---------------------------------------------------------------------------
// SessionLog — lightweight per-session file logger
// ---------------------------------------------------------------------------

/// Writes structured log lines to `~/.luwu/sessions/{id}/agent.log`.
///
/// This is separate from the global tracing infrastructure (which handles
/// daemon-level operational logs). Each session gets its own log file so
/// you can look up what happened in a specific session by id.
pub struct SessionLog {
    path: PathBuf,
}

impl SessionLog {
    pub fn new(session_dir: &Path) -> Self {
        let path = session_dir.join("agent.log");
        Self { path }
    }

    /// Append a log line. Best-effort — silently ignores IO errors.
    pub fn log(&self, level: &str, msg: &str) {
        let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
        let line = format!("{ts} {level} {msg}\n");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    pub fn info(&self, msg: &str) {
        self.log("INFO", msg);
    }

    pub fn warn(&self, msg: &str) {
        self.log("WARN", msg);
    }

    pub fn error(&self, msg: &str) {
        self.log("ERROR", msg);
    }
}

// ---------------------------------------------------------------------------
// AgentEvent — enriched events emitted by the service
// ---------------------------------------------------------------------------

/// Events emitted by [`AgentService::run`].
#[derive(Debug)]
pub enum AgentEvent {
    /// Raw turn event from the engine — serialized for SSE passthrough.
    Turn(TurnEvent),
    /// Checkpoint triggered by tool-call accumulation.
    ToolCheckpoint { count: usize },
    /// Checkpoint triggered by context-window usage.
    CycleCheckpoint { cycle: usize, usage_pct: u8 },
    /// Memory consolidation triggered (files exceeded threshold).
    Consolidation { files: Vec<String> },
    /// Cycle rebuild triggered (context window pressure).
    Rebuild { cycle: usize },
}

// ---------------------------------------------------------------------------
// AgentService
// ---------------------------------------------------------------------------

/// Orchestrates a single agent turn: correction detection, engine execution,
/// cycle management, memory worker dispatch, and message persistence.
///
/// Construct via [`AgentService::builder`], then call [`run`](Self::run).
pub struct AgentService {
    state: Arc<AppState>,
    engine: TurnEngine,
    memory: Arc<MemoryStore>,
    provider: Arc<dyn LlmProvider>,
    resolved: ResolvedConfig,
    session_id: String,
    working_dir: PathBuf,
    file_history: Arc<tokio::sync::Mutex<luwu_core::file_history::FileHistory>>,
    session_log: SessionLog,
}

impl AgentService {
    /// Start building an [`AgentService`].
    ///
    /// Required inputs are passed to builder methods; everything else
    /// (MemoryStore, FileHistory, ToolRegistry, TurnEngine, EventBus) is
    /// constructed inside [`AgentServiceBuilder::build`].
    pub fn builder() -> AgentServiceBuilder {
        AgentServiceBuilder::new()
    }

    /// Run the agent turn and return a receiver of enriched events.
    ///
    /// 1. Detects corrections in the user message and saves them.
    /// 2. Starts the engine stream.
    /// 3. Spawns a background task that consumes engine events, manages cycles,
    ///    dispatches memory workers, persists messages, and forwards
    ///    [`AgentEvent`]s through the channel.
    /// 4. Resets `is_running` on completion (safety net alongside `RunningGuard`).
    #[tracing::instrument(skip(self))]
    pub async fn run(
        self,
        user_message: String,
        messages: Vec<Message>,
        model: String,
        cancel_token: Option<CancelToken>,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel::<AgentEvent>(128);

        let msg_preview: String = user_message.chars().take(80).collect();
        self.session_log
            .info(&format!("Turn started — msg=\"{msg_preview}\""));

        // ── Correction detection ──
        {
            let mut detector = CorrectionDetector::new();
            detector.advance_turn();
            if let Some(correction) = detector.detect(&user_message) {
                let label = match correction.pattern_type {
                    CorrectionPattern::Strong => "纠错",
                    CorrectionPattern::Weak => "疑似纠错",
                };
                let entry = format!("[{}] {}", label, correction.full_message);
                self.session_log
                    .warn(&format!("Correction detected: {label}"));
                let mem = self.memory.clone();
                self.state.spawn_worker(async move {
                    if let Err(e) = mem.append_correction(&entry) {
                        tracing::warn!(%e, "Failed to save correction");
                    }
                });
                tracing::info!("Correction detected and saved");
            }
        }

        let messages_for_workers = messages.clone();

        // ── File history: snapshot before this turn starts ──
        let msg_ref = user_message.chars().take(100).collect::<String>();
        if let Err(e) = self.file_history.lock().await.make_snapshot(&msg_ref) {
            tracing::warn!(%e, "File history make_snapshot failed");
        }

        let event_rx = self
            .engine
            .run_stream(
                luwu_core::SessionId(self.session_id.clone()),
                model,
                messages,
                user_message.clone(),
                cancel_token,
            )
            .await;

        // ── Spawn orchestration task ──
        let state = self.state;
        let memory = self.memory;
        let provider = self.provider;
        let resolved = self.resolved;
        let sessions = state.sessions.clone();
        let session_id = self.session_id;
        let working_dir = self.working_dir;
        let user_msg_for_session = user_message;
        let session_log = self.session_log;

        tokio::spawn(async move {
            let mut rx = event_rx;
            let mut cycle = CycleState::default();

            while let Some(event) = rx.recv().await {
                // Forward the raw event to the handler.
                let _ = tx.send(AgentEvent::Turn(event.clone())).await;

                match &event {
                    TurnEvent::Done {
                        assistant_text,
                        usage,
                        ..
                    } => {
                        cycle.add_tokens(usage.total_tokens as usize);

                        session_log.info(&format!(
                            "Turn done — tokens={}/{}/{} text_len={}",
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens,
                            assistant_text.len()
                        ));

                        // Persist messages to session for multi-turn.
                        let sessions_c = sessions.clone();
                        let id_c = session_id.clone();
                        let user_msg = user_msg_for_session.clone();
                        let asst_text = assistant_text.clone();
                        state.spawn_worker(async move {
                            let mut msgs = vec![Message::user(&user_msg)];
                            if !asst_text.is_empty() {
                                msgs.push(Message::assistant(&asst_text));
                            }
                            sessions_c.append_messages(&id_c, msgs).await;
                        });

                        // Check if any memory files need consolidation.
                        let needed = memory.check_consolidation();
                        if !needed.is_empty() {
                            for n in &needed {
                                let content = std::fs::read_to_string(&n.path).unwrap_or_default();
                                let np = n.path.clone();
                                let ft = n.file_type;
                                let prov = provider.clone();
                                let mdl = resolved.model.clone();
                                state.spawn_worker(async move {
                                    if let Err(e) =
                                        run_consolidation_writer(prov, mdl, content, np, ft).await
                                    {
                                        tracing::warn!(%e, "Consolidation writer failed");
                                    }
                                });
                            }
                            let files = needed
                                .iter()
                                .map(|n| n.file_type.label().to_string())
                                .collect::<Vec<_>>();
                            session_log
                                .info(&format!("Consolidation triggered — files={:?}", files));
                            let _ = tx.send(AgentEvent::Consolidation { files }).await;
                        }
                        break;
                    }
                    TurnEvent::ToolCompleted {
                        call_id, output, ..
                    } => {
                        let preview: String = output.chars().take(100).collect();
                        session_log.info(&format!(
                            "Tool completed — call_id={call_id} result=\"{preview}\""
                        ));

                        if let CycleAction::Checkpoint = cycle.add_tool_call() {
                            cycle.mark_tool_call_checkpoint();

                            session_log.info(&format!(
                                "Tool checkpoint — tool_calls={}",
                                cycle.tool_usage()
                            ));

                            // Deterministic compaction — zero LLM cost.
                            let det_summary = compile_summary(&messages_for_workers, &working_dir);
                            if let Err(e) = memory.write_checkpoint_raw(&det_summary.to_markdown())
                            {
                                tracing::warn!(%e, "Failed to write checkpoint");
                            }

                            // Spawn Observer worker.
                            let wm = memory.clone();
                            let prov = provider.clone();
                            let mdl = resolved.model.clone();
                            let obs_msgs = messages_for_workers.clone();
                            state.spawn_worker(async move {
                                if let Err(e) = run_observer_worker(prov, mdl, obs_msgs, wm).await {
                                    tracing::warn!(%e, "Observer worker failed");
                                }
                            });

                            let _ = tx
                                .send(AgentEvent::ToolCheckpoint {
                                    count: cycle.tool_usage(),
                                })
                                .await;
                        }
                    }
                    TurnEvent::Cancelled => {
                        session_log.warn("Turn cancelled");
                        break;
                    }
                    TurnEvent::Error { message } => {
                        session_log.error(&format!("Turn error: {message}"));
                        break;
                    }
                    _ => {}
                }

                // Cycle checkpoint/rebuild after each LLM call.
                match cycle.check() {
                    CycleAction::Checkpoint => {
                        let pct = cycle.usage_pct();
                        cycle.mark_checkpoint(pct);

                        session_log.info(&format!(
                            "Cycle checkpoint — cycle={} usage={pct}%",
                            cycle.cycle_index
                        ));

                        let wm = memory.clone();
                        let prov = provider.clone();
                        let mdl = resolved.model.clone();
                        let obs_msgs = messages_for_workers.clone();

                        // Deterministic compaction.
                        let det_summary = compile_summary(&messages_for_workers, &working_dir);
                        if let Err(e) = memory.write_checkpoint_raw(&det_summary.to_markdown()) {
                            tracing::warn!(%e, "Failed to write checkpoint");
                        }

                        // Spawn Observer worker.
                        state.spawn_worker(async move {
                            if let Err(e) = run_observer_worker(prov, mdl, obs_msgs, wm).await {
                                tracing::warn!(%e, "Observer worker failed");
                            }
                        });

                        let _ = tx
                            .send(AgentEvent::CycleCheckpoint {
                                cycle: cycle.cycle_index,
                                usage_pct: pct,
                            })
                            .await;
                    }
                    CycleAction::Rebuild => {
                        session_log.info(&format!("Cycle rebuild — cycle={}", cycle.cycle_index));

                        let refl_memory = memory.clone();
                        let prov = provider.clone();
                        let mdl = resolved.model.clone();
                        state.spawn_worker(async move {
                            if let Err(e) = run_reflector_worker(prov, mdl, refl_memory).await {
                                tracing::warn!(%e, "Reflector worker failed");
                            }
                        });

                        let _ = tx
                            .send(AgentEvent::Rebuild {
                                cycle: cycle.cycle_index,
                            })
                            .await;
                        cycle.reset_cycle();
                    }
                    CycleAction::Continue => {}
                }
            }

            // Cleanup — safety net alongside RunningGuard.
            let _ = sessions.set_running(&session_id, false).await;
        });

        rx
    }
}

// ---------------------------------------------------------------------------
// AgentServiceBuilder — chainable input for AgentService
// ---------------------------------------------------------------------------

/// Chainable builder for [`AgentService`].
///
/// All inputs are required; call [`build`](Self::build) to construct the
/// service. Internal components (MemoryStore, FileHistory, ToolRegistry,
/// TurnEngine, EventBus) are constructed inside `build` so the handler
/// doesn't have to know about them.
pub struct AgentServiceBuilder {
    state: Option<Arc<AppState>>,
    provider: Option<Arc<dyn LlmProvider>>,
    resolved: Option<ResolvedConfig>,
    session_id: Option<String>,
    working_dir: Option<PathBuf>,
    /// Override the default tool registry (for tests).
    tools: Option<luwu_core::ToolRegistry>,
}

impl AgentServiceBuilder {
    /// Create an empty builder. All required fields must be set before
    /// calling [`build`](Self::build).
    pub fn new() -> Self {
        Self {
            state: None,
            provider: None,
            resolved: None,
            session_id: None,
            working_dir: None,
            tools: None,
        }
    }

    pub fn state(mut self, state: Arc<AppState>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn resolved(mut self, resolved: ResolvedConfig) -> Self {
        self.resolved = Some(resolved);
        self
    }

    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    /// Inject a custom tool registry (mostly for tests).
    pub fn tools(mut self, tools: luwu_core::ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Build the [`AgentService`]. Panics if any required input is missing —
    /// this is a programmer error caught at construction time.
    #[tracing::instrument(skip_all)]
    pub fn build(self) -> AgentService {
        let state = self
            .state
            .expect("AgentServiceBuilder: state is required");
        let provider = self
            .provider
            .expect("AgentServiceBuilder: provider is required");
        let resolved = self
            .resolved
            .expect("AgentServiceBuilder: resolved is required");
        let session_id = self
            .session_id
            .expect("AgentServiceBuilder: session_id is required");
        let working_dir = self
            .working_dir
            .expect("AgentServiceBuilder: working_dir is required");

        let luwu_home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".luwu");
        let memory = Arc::new(MemoryStore::new(&luwu_home, &working_dir, &session_id));

        let session_dir = luwu_home.join("sessions").join(&session_id);
        let file_history = Arc::new(tokio::sync::Mutex::new(
            luwu_core::file_history::FileHistory::new(&session_dir, &working_dir),
        ));
        let session_log = SessionLog::new(&session_dir);

        // Build the tool registry: either the injected one (tests) or the
        // built-in one with file history attached.
        let tools = self
            .tools
            .unwrap_or_else(crate::app::builtin_tool_registry)
            .with_file_history(file_history.clone());

        let events = EventBus::new(256);
        let engine = TurnEngine::new(
            provider.clone(),
            tools,
            state.skills.clone(),
            events,
            working_dir.clone(),
        );

        session_log.info(&format!(
            "AgentService created — model={}, session={}",
            resolved.model, session_id
        ));

        AgentService {
            state,
            engine,
            memory,
            provider,
            resolved,
            session_id,
            working_dir,
            file_history,
            session_log,
        }
    }
}

impl Default for AgentServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_empty_fails() {
        let result = std::panic::catch_unwind(|| AgentService::builder().build());
        assert!(result.is_err());
    }
}
