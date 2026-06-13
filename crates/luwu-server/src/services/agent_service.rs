//! Agent service — orchestrates agent turns with cycle management and memory workers.
//!
//! This is the application layer: it knows about domain concepts (TurnEngine,
//! CycleState, MemoryStore, workers) but NOT about HTTP/SSE/axum.
//!
//! The handler's job is to call [`AgentService::run`] and map [`AgentEvent`]s
//! to the wire format (SSE JSON, WebSocket frames, CLI output, etc.).

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use luwu_core::{
    CancelToken, CycleAction, CycleState, EventBus, LlmProvider, Message, TurnEngine, TurnEvent,
};
use luwu_memory::{CorrectionDetector, CorrectionPattern, MemoryStore, compile_summary};

use crate::app::{AppState, builtin_tool_registry};
use crate::config::ResolvedConfig;
use crate::handlers::workers::{
    run_consolidation_writer, run_observer_worker, run_reflector_worker,
};

// ---------------------------------------------------------------------------
// AgentEvent — enriched events emitted by the service
// ---------------------------------------------------------------------------

/// Events emitted by [`AgentService::run`].
///
/// Handlers map these to the appropriate transport format.
/// Every `TurnEvent` from the engine is forwarded as [`AgentEvent::Turn`];
/// side-effect notifications (checkpoints, consolidation, rebuild) are emitted
/// alongside the raw events so the client gets full visibility.
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
/// Construct this from the transport layer (handler), then call [`run`](Self::run)
/// to get a receiver of [`AgentEvent`]s.
pub struct AgentService {
    state: Arc<AppState>,
    engine: TurnEngine,
    memory: Arc<MemoryStore>,
    provider: Arc<dyn LlmProvider>,
    resolved: ResolvedConfig,
    session_id: String,
    working_dir: PathBuf,
}

impl AgentService {
    /// Build a service for the given session.
    ///
    /// The handler is responsible for `try_set_running`, `RunningGuard`, session
    /// lookup, and config resolution — those involve HTTP status-code decisions
    /// that don't belong in the service layer.
    #[tracing::instrument(skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: Arc<AppState>,
        provider: Arc<dyn LlmProvider>,
        resolved: ResolvedConfig,
        session_id: String,
        working_dir: PathBuf,
    ) -> Self {
        let luwu_home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".luwu");
        let memory = Arc::new(MemoryStore::new(&luwu_home, &working_dir, &session_id));

        let tools = builtin_tool_registry();
        let events = EventBus::new(256);
        let engine = TurnEngine::new(
            provider.clone(),
            tools,
            state.skills.clone(),
            events,
            working_dir.clone(),
        );

        Self {
            state,
            engine,
            memory,
            provider,
            resolved,
            session_id,
            working_dir,
        }
    }

    /// Run the agent turn and return a receiver of enriched events.
    ///
    /// This method:
    /// 1. Detects corrections in the user message and saves them.
    /// 2. Starts the engine stream.
    /// 3. Spawns a background task that consumes engine events, manages cycles,
    ///    dispatches memory workers, persists messages, and forwards
    ///    [`AgentEvent`]s through the channel.
    /// 4. Resets `is_running` on completion (safety net alongside `RunningGuard`).
    ///
    /// The channel closes when the turn completes, is cancelled, or errors.
    #[tracing::instrument(skip(self))]
    pub async fn run(
        self,
        user_message: String,
        messages: Vec<Message>,
        model: String,
        cancel_token: Option<CancelToken>,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel::<AgentEvent>(128);

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
                            let _ = tx.send(AgentEvent::Consolidation { files }).await;
                        }
                        break;
                    }
                    TurnEvent::ToolCompleted { .. } => {
                        if let CycleAction::Checkpoint = cycle.add_tool_call() {
                            cycle.mark_tool_call_checkpoint();

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
                    TurnEvent::Cancelled | TurnEvent::Error { .. } => break,
                    _ => {}
                }

                // Cycle checkpoint/rebuild after each LLM call.
                match cycle.check() {
                    CycleAction::Checkpoint => {
                        let pct = cycle.usage_pct();
                        cycle.mark_checkpoint(pct);

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
