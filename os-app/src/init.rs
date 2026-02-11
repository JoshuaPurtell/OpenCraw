//! Configuration scaffolding for `opencraw init`.
//!
//! Initializes `~/.opencraw/` from repository templates without overwriting
//! existing local files.

use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct InitReport {
    pub root: PathBuf,
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
struct TemplateFile {
    relative_path: &'static str,
    contents: &'static str,
}

const TEMPLATE_FILES: &[TemplateFile] = &[
    TemplateFile {
        relative_path: "config.toml",
        contents: include_str!("../../config-templates/config.toml"),
    },
    TemplateFile {
        relative_path: "configs/keys.toml",
        contents: include_str!("../../config-templates/configs/keys.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-telegram.toml",
        contents: include_str!("../../config-templates/configs/channel-telegram.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-discord.toml",
        contents: include_str!("../../config-templates/configs/channel-discord.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-slack.toml",
        contents: include_str!("../../config-templates/configs/channel-slack.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-matrix.toml",
        contents: include_str!("../../config-templates/configs/channel-matrix.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-signal.toml",
        contents: include_str!("../../config-templates/configs/channel-signal.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-whatsapp.toml",
        contents: include_str!("../../config-templates/configs/channel-whatsapp.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-imessage.toml",
        contents: include_str!("../../config-templates/configs/channel-imessage.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-email.toml",
        contents: include_str!("../../config-templates/configs/channel-email.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-linear.toml",
        contents: include_str!("../../config-templates/configs/channel-linear.toml"),
    },
    TemplateFile {
        relative_path: "configs/channel-external-plugins.toml",
        contents: include_str!("../../config-templates/configs/channel-external-plugins.toml"),
    },
];

pub async fn initialize_default() -> Result<InitReport> {
    let config_path = crate::config::default_config_path()?;
    let root = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid default config path: {}", config_path.display()))?
        .to_path_buf();
    initialize_at_root(&root).await
}

pub async fn initialize_at_root(root: &Path) -> Result<InitReport> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|e| anyhow::anyhow!("create config root {}: {e}", root.display()))?;

    let mut report = InitReport {
        root: root.to_path_buf(),
        created: Vec::new(),
        skipped: Vec::new(),
    };

    for template in TEMPLATE_FILES {
        let target = root.join(template.relative_path);
        match tokio::fs::metadata(&target).await {
            Ok(_) => {
                report.skipped.push(target);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(|e| {
                        anyhow::anyhow!("create config dir {}: {e}", parent.display())
                    })?;
                }
                tokio::fs::write(&target, template.contents)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("write config template {}: {e}", target.display())
                    })?;
                report.created.push(target);
            }
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "inspect config path {}: {err}",
                    target.display()
                ));
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{TEMPLATE_FILES, initialize_at_root};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("opencraw-init-{name}-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn init_creates_all_templates_when_missing() {
        let root = temp_root("create");
        let report = initialize_at_root(&root).await.expect("init succeeds");

        assert_eq!(report.created.len(), TEMPLATE_FILES.len());
        assert!(report.skipped.is_empty());
        for template in TEMPLATE_FILES {
            let target = root.join(template.relative_path);
            assert!(target.exists(), "missing template {}", target.display());
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn init_is_idempotent_and_never_overwrites() {
        let root = temp_root("idempotent");
        let first = initialize_at_root(&root)
            .await
            .expect("first init succeeds");
        assert_eq!(first.created.len(), TEMPLATE_FILES.len());

        let second = initialize_at_root(&root)
            .await
            .expect("second init succeeds");
        assert!(
            second.created.is_empty(),
            "second run should not create files"
        );
        assert_eq!(second.skipped.len(), TEMPLATE_FILES.len());

        let _ = std::fs::remove_dir_all(root);
    }
}
