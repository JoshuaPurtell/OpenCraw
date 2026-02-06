use crate::error::{Result, ToolError};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;

pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
    pub risk_level: RiskLevel,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value>;
}

pub fn to_llm_tool_def(tool: &dyn Tool) -> os_llm::ToolDefinition {
    let spec = tool.spec();
    os_llm::ToolDefinition {
        name: spec.name,
        description: spec.description,
        parameters: spec.parameters_schema,
    }
}

pub(crate) fn require_string(args: &serde_json::Value, key: &str) -> Result<String> {
    let Some(v) = args.get(key) else {
        return Err(ToolError::InvalidArguments(format!("missing key: {key}")));
    };
    match v {
        serde_json::Value::String(s) => Ok(s.clone()),
        other => Err(ToolError::InvalidArguments(format!(
            "key {key} must be string, got {other:?}"
        ))),
    }
}

pub(crate) fn optional_string(args: &serde_json::Value, key: &str) -> Result<Option<String>> {
    let Some(v) = args.get(key) else {
        return Ok(None);
    };
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(s.clone())),
        other => Err(ToolError::InvalidArguments(format!(
            "key {key} must be string, got {other:?}"
        ))),
    }
}
