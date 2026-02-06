use crate::error::{Result, ToolError};
use crate::traits::{optional_string, require_string, Tool, ToolSpec};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use tokio::process::Command;

pub struct ShellTool {
    timeout: std::time::Duration,
}

impl ShellTool {
    pub fn new(timeout: std::time::Duration) -> Self {
        Self { timeout }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell.execute".to_string(),
            description: "Execute a shell command on the host machine.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "command": { "type": "string" },
                    "working_directory": { "type": "string" }
                },
                "required": ["command"]
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let command = require_string(&arguments, "command")?;
        let working_directory = optional_string(&arguments, "working_directory")?;

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc").arg(command);
        if let Some(dir) = working_directory {
            cmd.current_dir(dir);
        }

        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .map_err(|_| ToolError::ExecutionFailed("shell command timed out".to_string()))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shell_exec_echo_works() {
        let tool = ShellTool::new(std::time::Duration::from_secs(5));
        let out = tool
            .execute(serde_json::json!({ "command": "echo hello" }))
            .await
            .unwrap();
        assert_eq!(out["exit_code"].as_i64().unwrap(), 0);
        assert!(out["stdout"].as_str().unwrap().contains("hello"));
    }
}
