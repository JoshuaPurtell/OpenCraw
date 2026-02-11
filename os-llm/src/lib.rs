//! BYO-key LLM client for OpenShell.
//!
//! Pure HTTP client, no Horizons dependency.
//! See: specifications/openshell/implementation_v0_1_0.md

mod anthropic;
mod client;
mod error;
mod openai;
mod types;

pub use client::{LlmClient, Provider, validate_tool_name_all_providers};
pub use error::{LlmError, Result};
pub use types::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, ToolDefinition, Usage};
