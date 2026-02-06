use crate::error::{Result, ToolError};
use crate::traits::{require_string, Tool, ToolSpec};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;

/// Browser automation tool backed by Chrome DevTools Protocol.
///
/// v0.1.0 keeps this as a compile-time placeholder.
pub struct BrowserTool;

impl BrowserTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "browser".to_string(),
            description: "Control a local Chrome/Chromium instance via CDP.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["navigate", "screenshot"] },
                    "url": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::Medium,
        }
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        match action.as_str() {
            "navigate" => {
                let url = require_string(&arguments, "url")?;
                Ok(serde_json::json!({ "result": format!("navigate not implemented (url={url})") }))
            }
            "screenshot" => Ok(serde_json::json!({ "result": "screenshot not implemented" })),
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}
