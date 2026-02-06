//! Local tool bridge for OpenShell.
//!
//! Tools are invoked by the assistant agent, gated by Horizons CoreAgents policies.
//! See: specifications/openshell/implementation_v0_1_0.md

mod browser;
mod clipboard;
mod error;
mod filesystem;
mod shell;
mod traits;

pub use browser::BrowserTool;
pub use clipboard::ClipboardTool;
pub use error::{Result, ToolError};
pub use filesystem::FilesystemTool;
pub use shell::ShellTool;
pub use traits::{to_llm_tool_def, Tool, ToolSpec};
