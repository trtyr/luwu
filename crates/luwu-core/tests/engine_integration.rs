//! Engine integration tests.
//!
//! These tests cover the two safety valves that live inside `run()` and
//! `run_stream()` and therefore can't be tested via the per-module
//! unit tests in `stuckness.rs` / `engine.rs`:
//!
//! 1. **StucknessGuard** — verify that 3 identical tool calls actually
//!    break the loop with `Err(LuwuError::Llm(...))`, and that diverse
//!    calls do NOT trigger.
//! 2. **Token Budget** — verify that exceeding the soft cap actually
//!    appends the wrap-up hint to `system_prompt` for the next LLM call.
//!
//! Both tests run against a `MockLlmProvider` that returns pre-canned
//! event sequences, so they don't hit the network.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use luwu_core::engine::TurnEngine;
use luwu_core::error::{LuwuError, Result};
use luwu_core::event::EventBus;
use luwu_core::llm::{LlmEvent, LlmProvider, LlmRequest, LlmUsage};
use luwu_core::session::SessionData;
use luwu_core::skill::SkillRegistry;
use luwu_core::tool::{Tool, ToolContext, ToolOutput};
use luwu_core::tool_registry::ToolRegistry;

// ─── MockLlmProvider ──────────────────────────────────────────────
//
// Returns pre-canned event sequences. Each call to `stream()` pops the
// next sequence. This lets us drive the engine through a deterministic
// script of LLM responses (e.g. "tool call, tool call, tool call,
// done") without any real network IO.

struct MockLlmProvider {
    responses: Mutex<VecDeque<mpsc::Receiver<Result<LlmEvent>>>>,
}

impl MockLlmProvider {
    fn from_sequences(sequences: Vec<Vec<LlmEvent>>) -> Self {
        let mut responses = VecDeque::new();
        for events in sequences {
            let (tx, rx) = mpsc::channel(16);
            for event in events {
                let _ = tx.try_send(Ok(event));
            }
            responses.push_back(rx);
        }
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["mock-model".to_string()])
    }

    async fn stream(
        &self,
        _req: LlmRequest,
    ) -> Result<mpsc::Receiver<Result<LlmEvent>>> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| LuwuError::Llm("MockLlmProvider: no more responses".to_string()))
    }
}

// ─── EchoTool ─────────────────────────────────────────────────────
//
// A minimal tool for testing. Just echoes its input back. Used for
// stuckness tests where the tool name + args are what matter, not
// the tool's behavior.

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes the input back as a string"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            }
        })
    }

    async fn execute(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        Ok(ToolOutput {
            content: "ok".to_string(),
            is_error: false,
        })
    }
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Build a tool call event sequence for the LLM to emit.
///
/// Real LLM streams emit `ToolCallBegin` (name) + several
/// `ToolCallDelta` (incremental args) + finally `Done(usage)`. The
/// engine collects them into finalized tool calls and executes them.
fn tool_call_events(call_id: &str, tool_name: &str, args: Value, usage: LlmUsage) -> Vec<LlmEvent> {
    vec![
        LlmEvent::ToolCallBegin {
            id: call_id.to_string(),
            name: tool_name.to_string(),
        },
        LlmEvent::ToolCallDelta {
            id: call_id.to_string(),
            delta: args.to_string(),
        },
        LlmEvent::Done(usage),
    ]
}

/// A "done, here's a final text" event sequence.
fn text_done_events(text: &str, usage: LlmUsage) -> Vec<LlmEvent> {
    vec![
        LlmEvent::TextDelta(text.to_string()),
        LlmEvent::Done(usage),
    ]
}

fn default_usage() -> LlmUsage {
    LlmUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        prompt_cache_hit_tokens: 0,
        prompt_cache_miss_tokens: 0,
    }
}

fn build_engine_with_provider(provider: Arc<dyn LlmProvider>) -> TurnEngine {
    let tools = ToolRegistry::builder()
        .register(Box::new(EchoTool))
        .build();
    let events = EventBus::new(64);
    let skills = SkillRegistry::default();
    TurnEngine::new(provider, tools, skills, events, PathBuf::from("/tmp"))
}

// ─── Tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn stuckness_breaks_loop_on_repeated_identical_calls() {
    // Three identical (tool, args) calls in a row → the engine should
    // detect stuckness and return Err. This is the core safety
    // guarantee.
    let tool_args = json!({"message": "hello"});
    let sequences = vec![
        tool_call_events("call-1", "echo", tool_args.clone(), default_usage()),
        tool_call_events("call-2", "echo", tool_args.clone(), default_usage()),
        tool_call_events("call-3", "echo", tool_args.clone(), default_usage()),
    ];
    let provider = Arc::new(MockLlmProvider::from_sequences(sequences));
    let engine = build_engine_with_provider(provider);

    let mut session = SessionData::new("test-model");
    let result = engine.run(&mut session, "test message".to_string()).await;

    assert!(result.is_err(), "expected stuckness to break the loop");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Stuckness") || err_msg.contains("stuck"),
        "expected stuckness error, got: {err_msg}"
    );
}

#[tokio::test]
async fn stuckness_does_not_trigger_on_diverse_calls() {
    // Three tool calls with DIFFERENT args → no stuckness. The engine
    // should consume all 3 mock responses without triggering the
    // stuckness safety valve.
    let sequences = vec![
        tool_call_events("c1", "echo", json!({"message": "a"}), default_usage()),
        tool_call_events("c2", "echo", json!({"message": "b"}), default_usage()),
        tool_call_events("c3", "echo", json!({"message": "c"}), default_usage()),
        // Final text response (the engine will request one more LLM
        // call after the last tool to get the final answer).
        text_done_events("All done", default_usage()),
    ];
    let provider = Arc::new(MockLlmProvider::from_sequences(sequences));
    let engine = build_engine_with_provider(provider);

    let mut session = SessionData::new("test-model");
    let result = engine.run(&mut session, "diverse test".to_string()).await;

    // The result should succeed (or fail for a non-stuckness reason
    // like running out of mock responses), but NOT with a stuckness
    // error.
    if let Err(e) = &result {
        let msg = e.to_string();
        assert!(
            !msg.contains("Stuckness"),
            "diverse calls should NOT trigger stuckness: {msg}"
        );
    }
}

#[tokio::test]
async fn budget_injects_wrap_up_hint_after_soft_cap() {
    // Simulate high token usage (600k, well above 500k soft cap) on
    // the first LLM call. Verify the engine appends the wrap-up hint
    // to `system_prompt` for the SECOND LLM call.
    //
    // We use a recording mock that captures every LlmRequest it sees
    // so we can inspect the second request's system_prompt.

    struct RecordingProvider {
        requests: Mutex<Vec<LlmRequest>>,
        responses: Mutex<VecDeque<mpsc::Receiver<Result<LlmEvent>>>>,
    }

    #[async_trait]
    impl LlmProvider for RecordingProvider {
        fn name(&self) -> &str {
            "recording"
        }

        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec!["recording-model".to_string()])
        }

        async fn stream(
            &self,
            req: LlmRequest,
        ) -> Result<mpsc::Receiver<Result<LlmEvent>>> {
            self.requests.lock().unwrap().push(req);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| LuwuError::Llm("RecordingProvider: no more".to_string()))
        }
    }

    let high_usage = LlmUsage {
        prompt_tokens: 600_000, // > TOKEN_BUDGET_SOFT_CAP
        completion_tokens: 1000,
        total_tokens: 601_000,
        prompt_cache_hit_tokens: 0,
        prompt_cache_miss_tokens: 0,
    };
    let response_sequences = vec![
        // First call: high usage → should trigger budget warning +
        // inject hint into system_prompt.
        tool_call_events("c1", "echo", json!({"message": "x"}), high_usage),
        // Second call: should see the hint in system_prompt.
        text_done_events("Done", default_usage()),
    ];
    let mut q = VecDeque::new();
    for events in response_sequences {
        let (tx, rx) = mpsc::channel(16);
        for event in events {
            let _ = tx.try_send(Ok(event));
        }
        q.push_back(rx);
    }
    let provider = Arc::new(RecordingProvider {
        requests: Mutex::new(Vec::new()),
        responses: Mutex::new(q),
    });

    let engine = build_engine_with_provider(provider.clone());
    let mut session = SessionData::new("test-model");
    let _ = engine.run(&mut session, "budget test".to_string()).await;

    // Inspect captured requests. The second request should have the
    // wrap-up hint appended to system_prompt.
    let requests = provider.requests.lock().unwrap();
    assert!(
        requests.len() >= 2,
        "expected at least 2 LLM calls, got {}",
        requests.len()
    );

    let second_req = &requests[1];
    let sys_prompt = second_req
        .system_prompt
        .as_deref()
        .expect("second request should have a system_prompt");
    assert!(
        sys_prompt.contains("Token budget nearly exhausted"),
        "second request should contain budget hint, got system_prompt: {sys_prompt}"
    );
    assert!(
        sys_prompt.contains("Wrap up your current task"),
        "second request should contain wrap-up instruction"
    );
}
