use crate::config::OpenShellConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshot {
    pub path: String,
    pub base_hash: String,
    pub updated_at: DateTime<Utc>,
    pub config: OpenShellConfig,
}

#[derive(Clone)]
pub struct ConfigControl {
    path: PathBuf,
    state: Arc<Mutex<ConfigState>>,
}

#[derive(Clone)]
struct ConfigState {
    config: OpenShellConfig,
    base_hash: String,
    updated_at: DateTime<Utc>,
}

impl ConfigControl {
    pub fn new(path: PathBuf, config: OpenShellConfig) -> Result<Self> {
        let base_hash = hash_config(&config)?;
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(ConfigState {
                config,
                base_hash,
                updated_at: Utc::now(),
            })),
        })
    }

    pub async fn snapshot(&self) -> ConfigSnapshot {
        let state = self.state.lock().await;
        ConfigSnapshot {
            path: self.path.display().to_string(),
            base_hash: state.base_hash.clone(),
            updated_at: state.updated_at,
            config: state.config.clone(),
        }
    }

    pub async fn apply(&self, config: OpenShellConfig) -> Result<ConfigSnapshot> {
        config.validate()?;
        let mut state = self.state.lock().await;
        write_config_file(&self.path, &config).await?;
        state.base_hash = hash_config(&config)?;
        state.updated_at = Utc::now();
        state.config = config;

        Ok(ConfigSnapshot {
            path: self.path.display().to_string(),
            base_hash: state.base_hash.clone(),
            updated_at: state.updated_at,
            config: state.config.clone(),
        })
    }

    pub async fn patch(
        &self,
        base_hash: Option<&str>,
        patch: serde_json::Value,
    ) -> Result<ConfigSnapshot> {
        let mut state = self.state.lock().await;
        if let Some(hash) = base_hash {
            if hash != state.base_hash {
                return Err(anyhow::anyhow!("base_hash mismatch"));
            }
        }

        let mut current_json = serde_json::to_value(state.config.clone())?;
        merge_json_value(&mut current_json, patch);
        let next: OpenShellConfig = serde_json::from_value(current_json)?;
        next.validate()?;
        write_config_file(&self.path, &next).await?;

        state.base_hash = hash_config(&next)?;
        state.updated_at = Utc::now();
        state.config = next;

        Ok(ConfigSnapshot {
            path: self.path.display().to_string(),
            base_hash: state.base_hash.clone(),
            updated_at: state.updated_at,
            config: state.config.clone(),
        })
    }
}

fn hash_config(config: &OpenShellConfig) -> Result<String> {
    let bytes = serde_json::to_vec(config)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn merge_json_value(target: &mut serde_json::Value, patch: serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(&key);
                    continue;
                }
                let entry = target_map.entry(key).or_insert(serde_json::Value::Null);
                merge_json_value(entry, value);
            }
        }
        (target_value, patch_value) => {
            *target_value = patch_value;
        }
    }
}

async fn write_config_file(path: &Path, config: &OpenShellConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = toml::to_string_pretty(config)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_config() -> OpenShellConfig {
        toml::from_str(
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test prompt"

[keys]
openai_api_key = "test-key"

[channels.webchat]
enabled = true
port = 3000
"#,
        )
        .expect("parse test config")
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("opencraw-{name}-{}.toml", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn patch_rejects_stale_base_hash() {
        let path = temp_path("cfg-stale");
        let control = ConfigControl::new(path.clone(), test_config()).expect("new config control");

        let snap = control.snapshot().await;
        let err = control
            .patch(
                Some("deadbeef"),
                serde_json::json!({ "llm": { "profiles": { "default": { "model": "gpt-4.1-mini" } } } }),
            )
            .await
            .expect_err("stale hash should fail");

        assert!(err.to_string().contains("base_hash mismatch"));
        let after = control.snapshot().await;
        assert_eq!(after.base_hash, snap.base_hash);

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn patch_updates_config_and_base_hash() {
        let path = temp_path("cfg-patch");
        let control = ConfigControl::new(path.clone(), test_config()).expect("new config control");

        let before = control.snapshot().await;
        let after = control
            .patch(
                Some(&before.base_hash),
                serde_json::json!({ "llm": { "profiles": { "default": { "model": "gpt-4.1-mini" } } } }),
            )
            .await
            .expect("patch should succeed");

        assert_ne!(before.base_hash, after.base_hash);
        assert_eq!(
            after
                .config
                .llm
                .profiles
                .get("default")
                .expect("default profile exists")
                .model,
            "gpt-4.1-mini"
        );

        let on_disk = tokio::fs::read_to_string(&path)
            .await
            .expect("read config file");
        assert!(on_disk.contains("gpt-4.1-mini"));

        let _ = std::fs::remove_file(path);
    }
}
