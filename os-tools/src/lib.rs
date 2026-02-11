//! Local tool bridge for OpenShell.
//!
//! Tools are invoked by the assistant agent, gated by Horizons CoreAgents policies.
//! See: specifications/openshell/implementation_v0_1_0.md

mod apply_patch;
mod browser;
mod clipboard;
mod email;
mod error;
mod filesystem;
mod imessage;
mod linear;
mod shell;
mod traits;

pub use apply_patch::ApplyPatchTool;
pub use browser::BrowserTool;
pub use clipboard::ClipboardTool;
pub use email::{EmailActionToggles, EmailTool};
pub use error::{Result, ToolError};
pub use filesystem::FilesystemTool;
pub use imessage::{ImessageActionToggles, ImessageTool};
pub use linear::{LinearActionToggles, LinearLimits, LinearTool, LinearToolConfig};
pub use shell::{ShellExecutionMode, ShellPolicy, ShellSandboxBackend, ShellTool};
pub use traits::{Tool, ToolSpec, to_llm_tool_def};
