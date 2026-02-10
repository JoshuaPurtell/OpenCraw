use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct ApplyPatchTool {
    root_dir: PathBuf,
    apply_patch_bin: PathBuf,
    max_patch_bytes: usize,
}

impl ApplyPatchTool {
    pub fn new(root_dir: impl AsRef<Path>) -> Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        if root_dir.as_os_str().is_empty() {
            return Err(ToolError::InvalidArguments(
                "root_dir is required".to_string(),
            ));
        }

        let apply_patch_bin =
            resolve_apply_patch_binary().unwrap_or_else(|| PathBuf::from("apply_patch"));
        Ok(Self {
            root_dir,
            apply_patch_bin,
            max_patch_bytes: 256 * 1024,
        })
    }

    fn validate_patch_paths(&self, patch: &str) -> Result<Vec<String>> {
        if !patch.contains("*** Begin Patch") || !patch.contains("*** End Patch") {
            return Err(ToolError::InvalidArguments(
                "patch must include *** Begin Patch and *** End Patch".to_string(),
            ));
        }

        let mut touched_paths = Vec::new();
        for line in patch.lines() {
            let path = line
                .strip_prefix("*** Add File: ")
                .or_else(|| line.strip_prefix("*** Update File: "))
                .or_else(|| line.strip_prefix("*** Delete File: "))
                .or_else(|| line.strip_prefix("*** Move to: "));

            let Some(path) = path else { continue };
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return Err(ToolError::InvalidArguments(
                    "patch path must not be empty".to_string(),
                ));
            }
            validate_relative_path(trimmed)?;
            touched_paths.push(trimmed.to_string());
        }

        if touched_paths.is_empty() {
            return Err(ToolError::InvalidArguments(
                "patch did not reference any files".to_string(),
            ));
        }
        Ok(touched_paths)
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "apply_patch".to_string(),
            description: "Apply a structured multi-hunk patch to files under the configured root."
                .to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["apply"] },
                    "patch": { "type": "string" }
                },
                "required": ["patch"]
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = optional_string(&arguments, "action")?.unwrap_or_else(|| "apply".to_string());
        if action != "apply" {
            return Err(ToolError::InvalidArguments(format!(
                "unknown action: {action}"
            )));
        }

        let patch = require_string(&arguments, "patch")?;
        if patch.len() > self.max_patch_bytes {
            return Err(ToolError::InvalidArguments(format!(
                "patch exceeds max size of {} bytes",
                self.max_patch_bytes
            )));
        }
        let touched_paths = self.validate_patch_paths(&patch)?;

        let mut child = Command::new(&self.apply_patch_bin)
            .current_dir(&self.root_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(patch.as_bytes())
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "apply_patch failed: status={} stdout={} stderr={}",
                output.status.code().unwrap_or(-1),
                stdout.trim(),
                stderr.trim()
            )));
        }

        Ok(serde_json::json!({
            "status": "applied",
            "exit_code": output.status.code().unwrap_or(0),
            "touched_paths": touched_paths,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

fn validate_relative_path(path: &str) -> Result<()> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err(ToolError::Unauthorized(
            "absolute paths are not allowed".to_string(),
        ));
    }
    for component in p.components() {
        match component {
            Component::ParentDir => {
                return Err(ToolError::Unauthorized(
                    "path traversal is not allowed".to_string(),
                ));
            }
            Component::CurDir | Component::Normal(_) => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolError::Unauthorized("invalid patch path".to_string()));
            }
        }
    }
    Ok(())
}

fn resolve_apply_patch_binary() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("APPLY_PATCH_BIN") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Ok(raw) = std::env::var("PATH") {
        for entry in raw.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            let candidate = Path::new(entry).join("apply_patch");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apply_patch_updates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        tokio::fs::write(&path, "hello\n").await.unwrap();

        let tool = ApplyPatchTool::new(dir.path()).unwrap();
        let patch = r#"*** Begin Patch
*** Update File: hello.txt
@@
-hello
+world
*** End Patch
"#;

        tool.execute(serde_json::json!({ "patch": patch }))
            .await
            .unwrap();

        let updated = tokio::fs::read_to_string(path).await.unwrap();
        assert_eq!(updated, "world\n");
    }

    #[tokio::test]
    async fn apply_patch_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let tool = ApplyPatchTool::new(dir.path()).unwrap();
        let patch = r#"*** Begin Patch
*** Update File: ../outside.txt
@@
-a
+b
*** End Patch
"#;

        let err = tool
            .execute(serde_json::json!({ "patch": patch }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }
}
