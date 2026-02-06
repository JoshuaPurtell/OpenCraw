use crate::error::{Result, ToolError};
use crate::traits::{optional_string, require_string, Tool, ToolSpec};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use regex::Regex;
use std::path::{Component, Path, PathBuf};

pub struct FilesystemTool {
    root_dir: PathBuf,
    search_results_max: usize,
    file_bytes_max: usize,
}

impl FilesystemTool {
    pub fn new(root_dir: impl AsRef<Path>) -> Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        if root_dir.as_os_str().is_empty() {
            return Err(ToolError::InvalidArguments(
                "root_dir is required".to_string(),
            ));
        }
        Ok(Self {
            root_dir,
            search_results_max: 200,
            file_bytes_max: 1_000_000,
        })
    }

    fn resolve_path(&self, user_path: &str) -> Result<PathBuf> {
        let rel = Path::new(user_path);
        if rel.is_absolute() {
            return Err(ToolError::Unauthorized(
                "absolute paths are not allowed".to_string(),
            ));
        }

        for component in rel.components() {
            match component {
                Component::ParentDir => {
                    return Err(ToolError::Unauthorized(
                        "path traversal is not allowed".to_string(),
                    ));
                }
                Component::CurDir | Component::Normal(_) => {}
                Component::RootDir | Component::Prefix(_) => {
                    return Err(ToolError::Unauthorized("invalid path".to_string()));
                }
            }
        }

        Ok(self.root_dir.join(rel))
    }

    async fn read_file(&self, path: &Path) -> Result<String> {
        let bytes = tokio::fs::read(path).await?;
        if bytes.len() > self.file_bytes_max {
            return Err(ToolError::ExecutionFailed(format!(
                "file too large: {} bytes (max {})",
                bytes.len(),
                self.file_bytes_max
            )));
        }
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    async fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        if content.as_bytes().len() > self.file_bytes_max {
            return Err(ToolError::ExecutionFailed(format!(
                "content too large: {} bytes (max {})",
                content.as_bytes().len(),
                self.file_bytes_max
            )));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut rd = tokio::fs::read_dir(path).await?;
        while let Some(entry) = rd.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            out.push(name);
            if out.len() >= self.search_results_max {
                break;
            }
        }
        out.sort();
        Ok(out)
    }

    async fn search_files(&self, path: &Path, pattern: &str) -> Result<Vec<String>> {
        let regex = Regex::new(pattern)
            .map_err(|e| ToolError::InvalidArguments(format!("invalid regex: {e}")))?;

        let mut stack = vec![path.to_path_buf()];
        let mut out = Vec::new();
        let mut steps = 0usize;
        let steps_max = 50_000usize;

        while let Some(dir) = stack.pop() {
            steps += 1;
            if steps >= steps_max {
                break;
            }

            let mut rd = match tokio::fs::read_dir(&dir).await {
                Ok(v) => v,
                Err(_) => continue,
            };

            while let Some(entry) = rd.next_entry().await? {
                let p = entry.path();
                let meta = match entry.metadata().await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if meta.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !meta.is_file() {
                    continue;
                }

                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if regex.is_match(name) {
                    if let Ok(rel) = p.strip_prefix(&self.root_dir) {
                        out.push(rel.to_string_lossy().to_string());
                    }
                    if out.len() >= self.search_results_max {
                        return Ok(out);
                    }
                }
            }
        }

        Ok(out)
    }
}

#[async_trait]
impl Tool for FilesystemTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "filesystem".to_string(),
            description: "Read and write files within a configured root directory.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["read_file", "write_file", "list_dir", "search_files"] },
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "pattern": { "type": "string" }
                },
                "required": ["action", "path"]
            }),
            risk_level: RiskLevel::Medium,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        let path = require_string(&arguments, "path")?;
        let resolved = self.resolve_path(&path)?;

        match action.as_str() {
            "read_file" => {
                let content = self.read_file(&resolved).await?;
                Ok(serde_json::json!({ "content": content }))
            }
            "write_file" => {
                let content = require_string(&arguments, "content")?;
                self.write_file(&resolved, &content).await?;
                Ok(serde_json::json!({ "status": "ok" }))
            }
            "list_dir" => {
                let entries = self.list_dir(&resolved).await?;
                Ok(serde_json::json!({ "entries": entries }))
            }
            "search_files" => {
                let pattern =
                    optional_string(&arguments, "pattern")?.unwrap_or_else(|| ".*".to_string());
                let matches = self.search_files(&resolved, &pattern).await?;
                Ok(serde_json::json!({ "matches": matches }))
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn filesystem_prevents_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = FilesystemTool::new(tmp.path()).unwrap();
        let err = tool
            .execute(serde_json::json!({
                "action": "read_file",
                "path": "../secrets.txt"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("traversal"));
    }
}
