//! Application service layer — business logic between transport and infrastructure.
//!
//! Services hold domain logic (engine orchestration, memory management, cycle control).
//! Handlers are thin: extract HTTP request → call service → format response.

pub mod agent_service;
