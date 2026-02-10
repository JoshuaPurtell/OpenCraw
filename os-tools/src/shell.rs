use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use horizons_core::engine::local_adapter::{
    DockerLocalAdapter, DockerLocalAdapterConfig, LocalAction, TokioCommandRunner,
};
use horizons_core::engine::models::{EnvSpec, EnvTemplate};
use horizons_core::engine::traits::RhodesAdapter;
use horizons_core::models::AgentIdentity;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

const LOG_BYTES_MAX: usize = 32_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecutionMode {
    Sandbox,
    Elevated,
}

impl ShellExecutionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Elevated => "elevated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSandboxBackend {
    HostConstrained,
    HorizonsDocker,
}

impl ShellSandboxBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::HostConstrained => "host_constrained",
            Self::HorizonsDocker => "horizons_docker",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellPolicy {
    pub default_mode: ShellExecutionMode,
    pub allow_elevated: bool,
    pub sandbox_root: PathBuf,
    pub sandbox_backend: ShellSandboxBackend,
    pub sandbox_image: Option<String>,
    pub max_background_processes: usize,
}

impl Default for ShellPolicy {
    fn default() -> Self {
        let sandbox_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            default_mode: ShellExecutionMode::Sandbox,
            allow_elevated: false,
            sandbox_root,
            sandbox_backend: ShellSandboxBackend::HostConstrained,
            sandbox_image: None,
            max_background_processes: 8,
        }
    }
}

struct BackgroundProcess {
    child: Child,
    command: String,
    mode: ShellExecutionMode,
    working_directory: PathBuf,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    started_at: Instant,
}

pub struct ShellTool {
    timeout: Duration,
    policy: ShellPolicy,
    background: Mutex<HashMap<String, BackgroundProcess>>,
    next_process_id: AtomicU64,
}

impl ShellTool {
    pub fn new(timeout: Duration, policy: ShellPolicy) -> Self {
        Self {
            timeout,
            policy,
            background: Mutex::new(HashMap::new()),
            next_process_id: AtomicU64::new(1),
        }
    }

    fn requested_mode(&self, arguments: &serde_json::Value) -> Result<ShellExecutionMode> {
        let Some(raw) = optional_string(arguments, "sandbox_permissions")? else {
            return Ok(self.policy.default_mode);
        };
        match raw.trim().to_ascii_lowercase().as_str() {
            "sandbox" => Ok(ShellExecutionMode::Sandbox),
            "require_elevated" | "elevated" => Ok(ShellExecutionMode::Elevated),
            other => Err(ToolError::InvalidArguments(format!(
                "sandbox_permissions must be 'sandbox' or 'require_elevated', got {other:?}"
            ))),
        }
    }

    fn enforce_mode_policy(&self, mode: ShellExecutionMode) -> Result<()> {
        if mode == ShellExecutionMode::Elevated && !self.policy.allow_elevated {
            return Err(ToolError::Unauthorized(
                "elevated execution is disabled by tools.shell_policy.allow_elevated".to_string(),
            ));
        }
        Ok(())
    }

    fn resolve_working_directory(
        &self,
        mode: ShellExecutionMode,
        requested: Option<&str>,
    ) -> Result<PathBuf> {
        match mode {
            ShellExecutionMode::Elevated => resolve_elevated_working_directory(requested),
            ShellExecutionMode::Sandbox => match self.policy.sandbox_backend {
                ShellSandboxBackend::HostConstrained => {
                    resolve_sandbox_working_directory(&self.policy.sandbox_root, requested)
                }
                ShellSandboxBackend::HorizonsDocker => {
                    validate_relative_working_directory(requested)
                }
            },
        }
    }

    async fn run_foreground(
        &self,
        command: &str,
        mode: ShellExecutionMode,
        working_directory: &Path,
    ) -> Result<serde_json::Value> {
        if mode == ShellExecutionMode::Sandbox
            && self.policy.sandbox_backend == ShellSandboxBackend::HorizonsDocker
        {
            return self
                .run_foreground_horizons_docker(command, working_directory)
                .await;
        }

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc")
            .arg(command)
            .current_dir(working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if mode == ShellExecutionMode::Sandbox {
            cmd.env("OPENCRAW_EXECUTION_MODE", "sandbox");
        } else {
            cmd.env("OPENCRAW_EXECUTION_MODE", "elevated");
        }

        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .map_err(|_| ToolError::ExecutionFailed("shell command timed out".to_string()))?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
            "execution_mode": mode.as_str(),
            "working_directory": working_directory.display().to_string(),
            "sandbox_backend": self.policy.sandbox_backend.as_str(),
        }))
    }

    async fn run_foreground_horizons_docker(
        &self,
        command: &str,
        working_directory: &Path,
    ) -> Result<serde_json::Value> {
        let adapter = build_docker_local_adapter()?;
        let image = self
            .policy
            .sandbox_image
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "ubuntu:24.04".to_string());
        let env_spec = EnvSpec::new(
            EnvTemplate::HarborCoding,
            serde_json::json!({ "image": image }),
            None,
        )
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let identity = AgentIdentity::System {
            name: "opencraw.shell".to_string(),
        };
        let handle = adapter
            .provision(env_spec, &identity)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox provision failed: {e}")))?;

        let wrapped_command = wrap_command_for_working_directory(command, working_directory);
        let action = LocalAction::Exec {
            cmd: vec!["sh".to_string(), "-lc".to_string(), wrapped_command],
            timeout_ms: Some(self.timeout.as_millis() as u64),
            env: None,
            workdir: None,
        };
        let action_value =
            serde_json::to_value(action).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let run_result = adapter
            .run_step(&handle, action_value)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox exec failed: {e}")));
        let release_result = adapter.release(&handle).await;
        if let Err(e) = release_result {
            tracing::warn!(error = %e, "failed to release sandbox container after shell execution");
        }
        let observation = run_result?;

        let status = observation
            .output
            .get("status")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let stdout = observation
            .output
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let stderr = observation
            .output
            .get("stderr")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": status,
            "execution_mode": ShellExecutionMode::Sandbox.as_str(),
            "working_directory": working_directory.display().to_string(),
            "sandbox_backend": ShellSandboxBackend::HorizonsDocker.as_str(),
            "sandbox_image": self.policy.sandbox_image.clone().unwrap_or_else(|| "ubuntu:24.04".to_string()),
        }))
    }

    fn background_lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, BackgroundProcess>>> {
        self.background
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("background process lock poisoned".to_string()))
    }

    async fn start_background(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let command = require_string(arguments, "command")?;
        let requested_dir = optional_string(arguments, "working_directory")?;
        let mode = self.requested_mode(arguments)?;
        self.enforce_mode_policy(mode)?;
        if mode == ShellExecutionMode::Sandbox
            && self.policy.sandbox_backend == ShellSandboxBackend::HorizonsDocker
        {
            return Err(ToolError::ExecutionFailed(
                "background shell processes are not yet supported with sandbox_backend=horizons_docker".to_string(),
            ));
        }
        let working_directory = self.resolve_working_directory(mode, requested_dir.as_deref())?;

        let mut background = self.background_lock()?;
        if background.len() >= self.policy.max_background_processes {
            return Err(ToolError::ExecutionFailed(format!(
                "background process limit reached ({})",
                self.policy.max_background_processes
            )));
        }

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc")
            .arg(command.clone())
            .current_dir(&working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if mode == ShellExecutionMode::Sandbox {
            cmd.env("OPENCRAW_EXECUTION_MODE", "sandbox");
        } else {
            cmd.env("OPENCRAW_EXECUTION_MODE", "elevated");
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(pipe) = child.stdout.take() {
            spawn_log_collector(pipe, stdout.clone());
        }
        if let Some(pipe) = child.stderr.take() {
            spawn_log_collector(pipe, stderr.clone());
        }

        let process_id = format!(
            "bg-{}",
            self.next_process_id.fetch_add(1, Ordering::Relaxed)
        );
        background.insert(
            process_id.clone(),
            BackgroundProcess {
                child,
                command,
                mode,
                working_directory: working_directory.clone(),
                stdout,
                stderr,
                started_at: Instant::now(),
            },
        );

        Ok(serde_json::json!({
            "status": "started",
            "process_id": process_id,
            "execution_mode": mode.as_str(),
            "working_directory": working_directory.display().to_string(),
            "sandbox_backend": self.policy.sandbox_backend.as_str(),
        }))
    }

    fn list_background(&self) -> Result<serde_json::Value> {
        let mut background = self.background_lock()?;
        let mut finished = Vec::new();
        let mut processes = Vec::with_capacity(background.len());

        for (process_id, process) in background.iter_mut() {
            let status = process
                .child
                .try_wait()
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            let running = status.is_none();
            let exit_code = status.map(|s| s.code().unwrap_or(-1));
            if !running {
                finished.push(process_id.clone());
            }
            processes.push(serde_json::json!({
                "process_id": process_id,
                "command": process.command,
                "execution_mode": process.mode.as_str(),
                "working_directory": process.working_directory.display().to_string(),
                "sandbox_backend": self.policy.sandbox_backend.as_str(),
                "running": running,
                "exit_code": exit_code,
                "uptime_ms": process.started_at.elapsed().as_millis() as u64,
            }));
        }

        for process_id in finished {
            background.remove(&process_id);
        }

        Ok(serde_json::json!({ "processes": processes }))
    }

    fn poll_background(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let process_id = require_string(arguments, "process_id")?;
        let mut background = self.background_lock()?;

        let mut remove_after = false;
        let response = {
            let process = background.get_mut(&process_id).ok_or_else(|| {
                ToolError::InvalidArguments(format!("unknown process_id: {process_id}"))
            })?;

            let status = process
                .child
                .try_wait()
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            let running = status.is_none();
            let exit_code = status.map(|s| s.code().unwrap_or(-1));
            if !running {
                remove_after = true;
            }

            serde_json::json!({
                "process_id": process_id,
                "command": process.command,
                "execution_mode": process.mode.as_str(),
                "working_directory": process.working_directory.display().to_string(),
                "sandbox_backend": self.policy.sandbox_backend.as_str(),
                "running": running,
                "exit_code": exit_code,
                "stdout": read_log_buffer(&process.stdout),
                "stderr": read_log_buffer(&process.stderr),
                "uptime_ms": process.started_at.elapsed().as_millis() as u64,
            })
        };

        if remove_after {
            background.remove(&process_id);
        }

        Ok(response)
    }

    async fn stop_background(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let process_id = require_string(arguments, "process_id")?;
        let mut process = {
            let mut background = self.background_lock()?;
            background.remove(&process_id).ok_or_else(|| {
                ToolError::InvalidArguments(format!("unknown process_id: {process_id}"))
            })?
        };

        let status = process
            .child
            .try_wait()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let exit_code = if let Some(code) = status {
            code.code().unwrap_or(-1)
        } else {
            process
                .child
                .kill()
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            let waited = process
                .child
                .wait()
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            waited.code().unwrap_or(-1)
        };

        Ok(serde_json::json!({
            "status": "stopped",
            "process_id": process_id,
            "command": process.command,
            "execution_mode": process.mode.as_str(),
            "working_directory": process.working_directory.display().to_string(),
            "sandbox_backend": self.policy.sandbox_backend.as_str(),
            "exit_code": exit_code,
            "stdout": read_log_buffer(&process.stdout),
            "stderr": read_log_buffer(&process.stderr),
        }))
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell_execute".to_string(),
            description: "Execute shell commands in sandbox or elevated mode, including background process control.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["exec", "start_background", "list_background", "poll_background", "stop_background"]
                    },
                    "command": { "type": "string" },
                    "working_directory": { "type": "string" },
                    "sandbox_permissions": { "type": "string", "enum": ["sandbox", "require_elevated"] },
                    "process_id": { "type": "string" }
                }
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = optional_string(&arguments, "action")?.unwrap_or_else(|| "exec".to_string());
        match action.as_str() {
            "exec" => {
                let command = require_string(&arguments, "command")?;
                let requested_dir = optional_string(&arguments, "working_directory")?;
                let mode = self.requested_mode(&arguments)?;
                self.enforce_mode_policy(mode)?;
                let working_directory =
                    self.resolve_working_directory(mode, requested_dir.as_deref())?;
                self.run_foreground(&command, mode, &working_directory)
                    .await
            }
            "start_background" => self.start_background(&arguments).await,
            "list_background" => self.list_background(),
            "poll_background" => self.poll_background(&arguments),
            "stop_background" => self.stop_background(&arguments).await,
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

fn resolve_elevated_working_directory(requested: Option<&str>) -> Result<PathBuf> {
    let Some(raw) = requested else {
        return std::env::current_dir().map_err(|e| ToolError::ExecutionFailed(e.to_string()));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ToolError::InvalidArguments(
            "working_directory must not be empty".to_string(),
        ));
    }
    Ok(PathBuf::from(trimmed))
}

fn validate_relative_working_directory(requested: Option<&str>) -> Result<PathBuf> {
    let rel = Path::new(requested.unwrap_or("."));
    if rel.is_absolute() {
        return Err(ToolError::Unauthorized(
            "sandbox mode requires relative working_directory".to_string(),
        ));
    }
    for component in rel.components() {
        match component {
            Component::ParentDir => {
                return Err(ToolError::Unauthorized(
                    "path traversal is not allowed in sandbox mode".to_string(),
                ));
            }
            Component::CurDir | Component::Normal(_) => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolError::Unauthorized(
                    "invalid working_directory for sandbox mode".to_string(),
                ));
            }
        }
    }
    Ok(rel.to_path_buf())
}

fn resolve_sandbox_working_directory(
    sandbox_root: &Path,
    requested: Option<&str>,
) -> Result<PathBuf> {
    let rel = validate_relative_working_directory(requested)?;
    Ok(sandbox_root.join(rel))
}

fn build_docker_local_adapter() -> Result<DockerLocalAdapter> {
    let cfg = DockerLocalAdapterConfig::default();
    let runner = Arc::new(TokioCommandRunner);
    DockerLocalAdapter::new(cfg, runner).map_err(|e| ToolError::ExecutionFailed(e.to_string()))
}

fn wrap_command_for_working_directory(command: &str, working_directory: &Path) -> String {
    let wd = working_directory.display().to_string();
    if wd == "." || wd.is_empty() {
        return command.to_string();
    }
    let quoted = sh_single_quote(&wd);
    format!("mkdir -p -- {quoted} && cd -- {quoted} && {command}")
}

fn sh_single_quote(raw: &str) -> String {
    let escaped = raw.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn spawn_log_collector<R>(reader: R, buffer: Arc<Mutex<Vec<u8>>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    append_log(&buffer, line.as_bytes());
                    append_log(&buffer, b"\n");
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });
}

fn append_log(buffer: &Arc<Mutex<Vec<u8>>>, bytes: &[u8]) {
    if let Ok(mut guard) = buffer.lock() {
        guard.extend_from_slice(bytes);
        if guard.len() > LOG_BYTES_MAX {
            let drop_len = guard.len() - LOG_BYTES_MAX;
            guard.drain(0..drop_len);
        }
    }
}

fn read_log_buffer(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    match buffer.lock() {
        Ok(guard) => String::from_utf8_lossy(&guard).to_string(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shell_exec_echo_works() {
        let sandbox_root = tempfile::tempdir().unwrap();
        let tool = ShellTool::new(
            Duration::from_secs(5),
            ShellPolicy {
                sandbox_root: sandbox_root.path().to_path_buf(),
                ..ShellPolicy::default()
            },
        );
        let out = tool
            .execute(serde_json::json!({ "command": "echo hello" }))
            .await
            .unwrap();
        assert_eq!(out["exit_code"].as_i64().unwrap(), 0);
        assert!(out["stdout"].as_str().unwrap().contains("hello"));
        assert_eq!(out["execution_mode"].as_str().unwrap(), "sandbox");
    }

    #[tokio::test]
    async fn shell_elevated_requires_policy_enablement() {
        let tool = ShellTool::new(Duration::from_secs(5), ShellPolicy::default());
        let err = tool
            .execute(serde_json::json!({
                "command": "echo denied",
                "sandbox_permissions": "require_elevated"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("elevated execution is disabled"));
    }

    #[tokio::test]
    async fn shell_sandbox_rejects_parent_traversal_workdir() {
        let tool = ShellTool::new(Duration::from_secs(5), ShellPolicy::default());
        let err = tool
            .execute(serde_json::json!({
                "command": "pwd",
                "working_directory": "../outside"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    #[tokio::test]
    async fn shell_background_lifecycle_works() {
        let tool = ShellTool::new(Duration::from_secs(10), ShellPolicy::default());
        let start = tool
            .execute(serde_json::json!({
                "action": "start_background",
                "command": "sleep 30"
            }))
            .await
            .unwrap();
        let process_id = start["process_id"].as_str().unwrap().to_string();

        let list = tool
            .execute(serde_json::json!({
                "action": "list_background"
            }))
            .await
            .unwrap();
        assert!(
            list["processes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["process_id"] == process_id)
        );

        let stop = tool
            .execute(serde_json::json!({
                "action": "stop_background",
                "process_id": process_id
            }))
            .await
            .unwrap();
        assert_eq!(stop["status"].as_str().unwrap(), "stopped");
    }

    #[tokio::test]
    async fn shell_docker_sandbox_blocks_background_mode() {
        let tool = ShellTool::new(
            Duration::from_secs(5),
            ShellPolicy {
                sandbox_backend: ShellSandboxBackend::HorizonsDocker,
                ..ShellPolicy::default()
            },
        );
        let err = tool
            .execute(serde_json::json!({
                "action": "start_background",
                "command": "sleep 5"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not yet supported"));
    }

    #[test]
    fn shell_quote_wraps_single_quotes() {
        let quoted = sh_single_quote("a'b");
        assert_eq!(quoted, "'a'\"'\"'b'");
    }
}
