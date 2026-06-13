//! LLM provider implementations for luwu.
//!
//! This crate provides concrete [`LlmProvider`](luwu_core::LlmProvider) implementations
//! for various LLM APIs:
//!
//! - **OpenAI** — GPT-4o, GPT-4.1, o3, etc. (Responses API with streaming)
//! - **Anthropic** — Claude Sonnet, Opus, Haiku (Messages API with streaming)
//! - Any OpenAI-compatible endpoint (Ollama, vLLM, etc.)
//!
//! # Streaming
//!
//! Both providers stream responses via Server-Sent Events (SSE).
//! The shared [`sse`] module handles parsing.

pub mod anthropic;
pub mod openai;
mod sse;
pub mod error;
