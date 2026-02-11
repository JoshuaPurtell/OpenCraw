use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;

pub struct ClipboardTool;

impl ClipboardTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ClipboardTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "clipboard".to_string(),
            description: "Read or write the system clipboard.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set"] },
                    "content": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::Low,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;

        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        match action.as_str() {
            "get" => {
                let text = clipboard
                    .get_text()
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                Ok(serde_json::json!({ "content": text }))
            }
            "set" => {
                let content = require_string(&arguments, "content")?;
                clipboard
                    .set_text(content)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                Ok(serde_json::json!({ "status": "ok" }))
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}
