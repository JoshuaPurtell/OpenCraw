use crate::config::SkillsConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use horizons_core::models::{OrgId, ProjectDbHandle};
use horizons_core::onboard::traits::{ProjectDb, ProjectDbParam, ProjectDbValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillPolicyDecision {
    Approve,
    Warn,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub source: Option<String>,
    pub content: Option<String>,
    pub signature: Option<String>,
    pub digest_sha256: String,
    pub decision: SkillPolicyDecision,
    pub policy_reasons: Vec<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub approved_by_operator: bool,
    #[serde(default)]
    pub scan_count: u64,
    #[serde(default)]
    pub last_scan_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillScanRecord {
    pub scan_id: String,
    pub skill_id: String,
    pub scanner: String,
    pub decision: SkillPolicyDecision,
    pub reasons: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallSkillInput {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
}

pub struct SkillsRuntime {
    skills: Arc<RwLock<HashMap<String, SkillRecord>>>,
    project_db: Arc<dyn ProjectDb>,
    org_id: OrgId,
    project_db_handle: ProjectDbHandle,
    policy: SkillPolicyConfig,
}

#[derive(Debug, Clone)]
pub struct SkillPolicyConfig {
    pub require_source_provenance: bool,
    pub require_https_source: bool,
    pub require_trusted_source: bool,
    pub trusted_source_prefixes: Vec<String>,
    pub require_sha256_signature: bool,
}

impl Default for SkillPolicyConfig {
    fn default() -> Self {
        Self {
            require_source_provenance: false,
            require_https_source: true,
            require_trusted_source: false,
            trusted_source_prefixes: Vec::new(),
            require_sha256_signature: false,
        }
    }
}

impl From<SkillsConfig> for SkillPolicyConfig {
    fn from(value: SkillsConfig) -> Self {
        Self {
            require_source_provenance: value.require_source_provenance,
            require_https_source: value.require_https_source,
            require_trusted_source: value.require_trusted_source,
            trusted_source_prefixes: value
                .trusted_source_prefixes
                .into_iter()
                .filter_map(|prefix| {
                    let trimmed = prefix.trim().to_ascii_lowercase();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                })
                .collect(),
            require_sha256_signature: value.require_sha256_signature,
        }
    }
}

impl SkillPolicyConfig {
    fn source_matches_trusted_prefixes(&self, source: &str) -> bool {
        let normalized = source.trim().to_ascii_lowercase();
        self.trusted_source_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
    }
}

impl SkillsRuntime {
    pub async fn load_or_new(
        project_db: Arc<dyn ProjectDb>,
        org_id: OrgId,
        project_db_handle: ProjectDbHandle,
        policy: SkillsConfig,
    ) -> Result<Self> {
        let runtime = Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            project_db,
            org_id,
            project_db_handle,
            policy: policy.into(),
        };
        runtime.ensure_schema().await?;
        runtime.load_skills().await?;
        Ok(runtime)
    }

    pub async fn list(&self) -> Vec<SkillRecord> {
        let skills = self.skills.read().await;
        let mut out = skills.values().cloned().collect::<Vec<_>>();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub async fn search(&self, query: &str) -> Vec<SkillRecord> {
        let lowered = query.trim().to_ascii_lowercase();
        if lowered.is_empty() {
            return self.list().await;
        }
        let skills = self.skills.read().await;
        let mut out = skills
            .values()
            .filter(|record| {
                let mut haystack = String::new();
                haystack.push_str(&record.name.to_ascii_lowercase());
                haystack.push('\n');
                haystack.push_str(&record.description.to_ascii_lowercase());
                if let Some(source) = &record.source {
                    haystack.push('\n');
                    haystack.push_str(&source.to_ascii_lowercase());
                }
                if let Some(content) = &record.content {
                    haystack.push('\n');
                    haystack.push_str(&content.to_ascii_lowercase());
                }
                haystack.contains(&lowered)
            })
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub async fn get(&self, skill_id: &str) -> Option<SkillRecord> {
        self.skills.read().await.get(skill_id).cloned()
    }

    pub async fn install(&self, input: InstallSkillInput) -> Result<SkillRecord> {
        let normalized = normalize_install_input(input)?;
        let digest_sha256 = digest_install_input(&normalized)?;
        let skill_id = format!("skill-{}", &digest_sha256[..16]);
        let (decision, policy_reasons) = evaluate_policy(&normalized, &digest_sha256, &self.policy);
        let now = Utc::now();

        let record = {
            let mut skills = self.skills.write().await;
            let prior = skills.get(&skill_id).cloned();
            let created_at = prior
                .as_ref()
                .map(|record| record.created_at)
                .unwrap_or(now);
            let approved_by_operator = if decision == SkillPolicyDecision::Block {
                false
            } else {
                prior
                    .as_ref()
                    .map(|record| record.approved_by_operator)
                    .unwrap_or(false)
            };
            let scan_count = prior
                .as_ref()
                .map(|record| record.scan_count)
                .unwrap_or(0)
                .saturating_add(1);
            let record = SkillRecord {
                skill_id: skill_id.clone(),
                name: normalized.name,
                description: normalized.description,
                source: normalized.source,
                content: normalized.content,
                signature: normalized.signature,
                digest_sha256,
                decision,
                policy_reasons: policy_reasons.clone(),
                active: skill_is_active(decision, approved_by_operator),
                approved_by_operator,
                scan_count,
                last_scan_at: Some(now),
                created_at,
                updated_at: now,
            };
            skills.insert(skill_id, record.clone());
            record
        };

        self.persist_record(&record).await?;
        self.persist_scan(&SkillScanRecord {
            scan_id: Uuid::new_v4().to_string(),
            skill_id: record.skill_id.clone(),
            scanner: "policy.install".to_string(),
            decision: record.decision,
            reasons: policy_reasons,
            created_at: now,
        })
        .await?;
        Ok(record)
    }

    pub async fn approve(&self, skill_id: &str, note: Option<String>) -> Result<SkillRecord> {
        let now = Utc::now();
        let (updated, audit_reasons) = {
            let mut skills = self.skills.write().await;
            let record = skills
                .get_mut(skill_id)
                .ok_or_else(|| anyhow::anyhow!("skill not found: {skill_id}"))?;
            if record.decision == SkillPolicyDecision::Block {
                return Err(anyhow::anyhow!(
                    "blocked skill cannot be approved: {skill_id}"
                ));
            }
            record.approved_by_operator = true;
            record.active = true;
            record.updated_at = now;

            let mut reasons = vec!["operator approval".to_string()];
            if let Some(note) = note.and_then(|v| normalize_optional_string(Some(v))) {
                reasons.push(format!("note: {note}"));
            }
            (record.clone(), reasons)
        };
        self.persist_record(&updated).await?;
        self.persist_scan(&SkillScanRecord {
            scan_id: Uuid::new_v4().to_string(),
            skill_id: updated.skill_id.clone(),
            scanner: "operator.approve".to_string(),
            decision: updated.decision,
            reasons: audit_reasons,
            created_at: now,
        })
        .await?;
        Ok(updated)
    }

    pub async fn revoke(&self, skill_id: &str, note: Option<String>) -> Result<SkillRecord> {
        let now = Utc::now();
        let (updated, audit_reasons) = {
            let mut skills = self.skills.write().await;
            let record = skills
                .get_mut(skill_id)
                .ok_or_else(|| anyhow::anyhow!("skill not found: {skill_id}"))?;
            record.active = false;
            record.approved_by_operator = false;
            record.updated_at = now;

            let mut reasons = vec!["operator revocation".to_string()];
            if let Some(note) = note.and_then(|v| normalize_optional_string(Some(v))) {
                reasons.push(format!("note: {note}"));
            }
            (record.clone(), reasons)
        };
        self.persist_record(&updated).await?;
        self.persist_scan(&SkillScanRecord {
            scan_id: Uuid::new_v4().to_string(),
            skill_id: updated.skill_id.clone(),
            scanner: "operator.revoke".to_string(),
            decision: updated.decision,
            reasons: audit_reasons,
            created_at: now,
        })
        .await?;
        Ok(updated)
    }

    pub async fn rescan(&self, skill_id: &str) -> Result<SkillRecord> {
        let now = Utc::now();
        let (updated, scan_reasons) = {
            let mut skills = self.skills.write().await;
            let record = skills
                .get_mut(skill_id)
                .ok_or_else(|| anyhow::anyhow!("skill not found: {skill_id}"))?;

            let input = InstallSkillInput {
                name: record.name.clone(),
                description: record.description.clone(),
                source: record.source.clone(),
                content: record.content.clone(),
                signature: record.signature.clone(),
            };
            let normalized = normalize_install_input(input)?;
            let (decision, reasons) =
                evaluate_policy(&normalized, &record.digest_sha256, &self.policy);
            record.decision = decision;
            record.policy_reasons = reasons.clone();
            if decision == SkillPolicyDecision::Block {
                record.approved_by_operator = false;
            }
            record.active = skill_is_active(decision, record.approved_by_operator);
            record.scan_count = record.scan_count.saturating_add(1);
            record.last_scan_at = Some(now);
            record.updated_at = now;
            (record.clone(), reasons)
        };
        self.persist_record(&updated).await?;
        self.persist_scan(&SkillScanRecord {
            scan_id: Uuid::new_v4().to_string(),
            skill_id: updated.skill_id.clone(),
            scanner: "policy.rescan".to_string(),
            decision: updated.decision,
            reasons: scan_reasons,
            created_at: now,
        })
        .await?;
        Ok(updated)
    }

    pub async fn list_scans(&self, skill_id: &str, limit: usize) -> Result<Vec<SkillScanRecord>> {
        let trimmed = skill_id.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("skill_id must not be empty"));
        }
        let rows = self
            .project_db
            .query(
                self.org_id,
                &self.project_db_handle,
                "SELECT scan_json FROM opencraw_skill_scans WHERE skill_id = ?1 ORDER BY created_at DESC",
                &[ProjectDbParam::String(trimmed.to_string())],
            )
            .await?;
        let mut scans = Vec::with_capacity(rows.len());
        for row in rows {
            let scan_json = row_required_string(&row, "scan_json")?;
            let record: SkillScanRecord = serde_json::from_str(&scan_json)?;
            scans.push(record);
        }
        let applied_limit = limit.clamp(1, 200);
        scans.truncate(applied_limit);
        Ok(scans)
    }

    async fn ensure_schema(&self) -> Result<()> {
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE TABLE IF NOT EXISTS opencraw_skills (
    skill_id TEXT PRIMARY KEY,
    skill_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)
"#,
                &[],
            )
            .await?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE TABLE IF NOT EXISTS opencraw_skill_scans (
    scan_id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    scan_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)
"#,
                &[],
            )
            .await?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE INDEX IF NOT EXISTS idx_opencraw_skill_scans_skill_created
ON opencraw_skill_scans (skill_id, created_at DESC)
"#,
                &[],
            )
            .await?;
        Ok(())
    }

    async fn load_skills(&self) -> Result<()> {
        let rows = self
            .project_db
            .query(
                self.org_id,
                &self.project_db_handle,
                "SELECT skill_id, skill_json FROM opencraw_skills",
                &[],
            )
            .await?;
        let mut loaded = HashMap::new();
        for row in rows {
            let skill_id = row_required_string(&row, "skill_id")?;
            let skill_json = row_required_string(&row, "skill_json")?;
            let record: SkillRecord = serde_json::from_str(&skill_json)?;
            loaded.insert(skill_id, record);
        }
        *self.skills.write().await = loaded;
        Ok(())
    }

    async fn persist_record(&self, record: &SkillRecord) -> Result<()> {
        let skill_json = serde_json::to_string(record)?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
INSERT INTO opencraw_skills (skill_id, skill_json, updated_at)
VALUES (?1, ?2, CURRENT_TIMESTAMP)
ON CONFLICT(skill_id) DO UPDATE
SET skill_json = excluded.skill_json,
    updated_at = CURRENT_TIMESTAMP
"#,
                &[
                    ProjectDbParam::String(record.skill_id.clone()),
                    ProjectDbParam::String(skill_json),
                ],
            )
            .await?;
        Ok(())
    }

    async fn persist_scan(&self, scan: &SkillScanRecord) -> Result<()> {
        let scan_json = serde_json::to_string(scan)?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
INSERT INTO opencraw_skill_scans (scan_id, skill_id, scan_json, created_at)
VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
"#,
                &[
                    ProjectDbParam::String(scan.scan_id.clone()),
                    ProjectDbParam::String(scan.skill_id.clone()),
                    ProjectDbParam::String(scan_json),
                ],
            )
            .await?;
        Ok(())
    }
}

fn normalize_install_input(input: InstallSkillInput) -> Result<InstallSkillInput> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow::anyhow!("skill name must not be empty"));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(anyhow::anyhow!(
            "skill name may only contain alphanumeric, '-', '_', or '.'"
        ));
    }
    let description = input.description.trim().to_string();
    if description.is_empty() {
        return Err(anyhow::anyhow!("skill description must not be empty"));
    }

    let source = normalize_optional_string(input.source);
    let content = normalize_optional_string(input.content);
    let signature = normalize_optional_string(input.signature);

    Ok(InstallSkillInput {
        name,
        description,
        source,
        content,
        signature,
    })
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn digest_install_input(input: &InstallSkillInput) -> Result<String> {
    let canonical = serde_json::json!({
        "name": input.name,
        "description": input.description,
        "source": input.source,
        "content": input.content,
        "signature": input.signature,
    });
    let bytes = serde_json::to_vec(&canonical)?;
    let digest = Sha256::digest(bytes);
    Ok(bytes_to_hex(&digest))
}

fn evaluate_policy(
    input: &InstallSkillInput,
    digest_sha256: &str,
    policy: &SkillPolicyConfig,
) -> (SkillPolicyDecision, Vec<String>) {
    let source = input.source.as_deref().map(str::trim).unwrap_or_default();
    let signature = input
        .signature
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();

    let mut warnings = Vec::new();
    let mut blocks = Vec::new();
    if source.is_empty() {
        if policy.require_source_provenance || policy.require_trusted_source {
            blocks.push("missing source provenance".to_string());
        } else {
            warnings.push("missing source provenance".to_string());
        }
    } else if !source.to_ascii_lowercase().starts_with("https://") {
        if policy.require_https_source || policy.require_trusted_source {
            blocks.push("source URL must use https://".to_string());
        } else {
            warnings.push("source URL is not HTTPS".to_string());
        }
    } else if policy.require_trusted_source && !policy.source_matches_trusted_prefixes(source) {
        blocks.push("source URL is not in trusted source roots".to_string());
    } else if !policy.trusted_source_prefixes.is_empty()
        && !policy.source_matches_trusted_prefixes(source)
    {
        warnings.push("source URL is outside configured trusted source roots".to_string());
    }
    if signature.is_empty() {
        if policy.require_sha256_signature {
            blocks.push("missing artifact signature".to_string());
        } else {
            warnings.push("missing artifact signature".to_string());
        }
    } else if let Some(expected_digest) = signature.strip_prefix("sha256:") {
        let expected = expected_digest.trim().to_ascii_lowercase();
        if expected.is_empty() {
            if policy.require_sha256_signature {
                blocks.push("empty sha256 signature payload".to_string());
            } else {
                warnings.push("empty sha256 signature payload".to_string());
            }
        } else if expected != digest_sha256.to_ascii_lowercase() {
            blocks.push("sha256 signature does not match artifact digest".to_string());
        }
    } else if policy.require_sha256_signature {
        blocks.push("signature must use sha256:<digest> format".to_string());
    }

    let mut corpus = String::new();
    corpus.push_str(&input.name.to_ascii_lowercase());
    corpus.push('\n');
    corpus.push_str(&input.description.to_ascii_lowercase());
    if let Some(content) = input.content.as_deref() {
        corpus.push('\n');
        corpus.push_str(&content.to_ascii_lowercase());
    }

    for (pattern, reason) in [
        (
            "rm -rf /",
            "contains destructive filesystem command pattern",
        ),
        ("curl | sh", "contains pipe-to-shell install pattern"),
        (
            "powershell -enc",
            "contains obfuscated PowerShell execution pattern",
        ),
        ("drop table", "contains destructive SQL pattern"),
    ] {
        if corpus.contains(pattern) {
            blocks.push(reason.to_string());
        }
    }

    if !blocks.is_empty() {
        return (SkillPolicyDecision::Block, blocks);
    }
    if !warnings.is_empty() {
        return (SkillPolicyDecision::Warn, warnings);
    }
    (SkillPolicyDecision::Approve, Vec::new())
}

fn skill_is_active(decision: SkillPolicyDecision, approved_by_operator: bool) -> bool {
    match decision {
        SkillPolicyDecision::Approve => true,
        SkillPolicyDecision::Warn => approved_by_operator,
        SkillPolicyDecision::Block => false,
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}

fn row_required_string(
    row: &std::collections::BTreeMap<String, ProjectDbValue>,
    key: &str,
) -> Result<String> {
    let value = row
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("skills row missing required key: {key}"))?;
    match value {
        ProjectDbValue::String(v) => Ok(v.clone()),
        other => Err(anyhow::anyhow!(
            "skills row key {key} expected string but received {other:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizons_core::models::ProjectId;
    use horizons_rs::dev_backends::DevProjectDb;
    use std::sync::Arc;
    use uuid::Uuid;

    async fn new_runtime() -> SkillsRuntime {
        new_runtime_with_policy(SkillsConfig::default()).await
    }

    async fn new_runtime_with_policy(policy: SkillsConfig) -> SkillsRuntime {
        let root = std::env::temp_dir().join(format!("opencraw-skills-{}", Uuid::new_v4()));
        let project_db = Arc::new(
            DevProjectDb::new(root.join("project_dbs"))
                .await
                .expect("new dev project db"),
        );
        let org_id = OrgId(Uuid::new_v4());
        let project_id = ProjectId(Uuid::new_v4());
        let handle = project_db
            .provision(org_id, project_id)
            .await
            .expect("provision project db");
        SkillsRuntime::load_or_new(project_db, org_id, handle, policy)
            .await
            .expect("load runtime")
    }

    #[test]
    fn policy_blocks_dangerous_payload() {
        let input = InstallSkillInput {
            name: "dangerous".to_string(),
            description: "installs via curl | sh".to_string(),
            source: Some("https://example.com/skill".to_string()),
            content: None,
            signature: Some("sig".to_string()),
        };
        let (decision, reasons) =
            evaluate_policy(&input, "deadbeef", &SkillPolicyConfig::default());
        assert_eq!(decision, SkillPolicyDecision::Block);
        assert!(!reasons.is_empty());
    }

    #[test]
    fn policy_blocks_sha256_signature_mismatch() {
        let input = InstallSkillInput {
            name: "signed_skill".to_string(),
            description: "signed skill payload".to_string(),
            source: Some("https://example.com/skill".to_string()),
            content: Some("echo hi".to_string()),
            signature: Some("sha256:0000".to_string()),
        };
        let (decision, reasons) =
            evaluate_policy(&input, "abcd1234", &SkillPolicyConfig::default());
        assert_eq!(decision, SkillPolicyDecision::Block);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("sha256 signature does not match"))
        );
    }

    #[test]
    fn policy_blocks_source_outside_required_trust_roots() {
        let input = InstallSkillInput {
            name: "source_gate".to_string(),
            description: "source trust root enforcement".to_string(),
            source: Some("https://evil.example.com/skill".to_string()),
            content: Some("echo hi".to_string()),
            signature: Some("sha256:abcd1234".to_string()),
        };
        let policy = SkillPolicyConfig {
            require_source_provenance: true,
            require_https_source: true,
            require_trusted_source: true,
            trusted_source_prefixes: vec!["https://docs.openclaw.ai/".to_string()],
            require_sha256_signature: true,
        };
        let (decision, reasons) = evaluate_policy(&input, "abcd1234", &policy);
        assert_eq!(decision, SkillPolicyDecision::Block);
        assert!(reasons.iter().any(|r| r.contains("trusted source roots")));
    }

    #[test]
    fn policy_blocks_non_sha256_signature_when_required() {
        let input = InstallSkillInput {
            name: "sig_gate".to_string(),
            description: "signature format gate".to_string(),
            source: Some("https://docs.openclaw.ai/skill".to_string()),
            content: Some("echo hi".to_string()),
            signature: Some("ed25519:abc".to_string()),
        };
        let policy = SkillPolicyConfig {
            require_source_provenance: true,
            require_https_source: true,
            require_trusted_source: false,
            trusted_source_prefixes: Vec::new(),
            require_sha256_signature: true,
        };
        let (decision, reasons) = evaluate_policy(&input, "abcd1234", &policy);
        assert_eq!(decision, SkillPolicyDecision::Block);
        assert!(reasons.iter().any(|r| r.contains("sha256:<digest> format")));
    }

    #[tokio::test]
    async fn install_persists_and_reloads() {
        let runtime = new_runtime().await;
        let record = runtime
            .install(InstallSkillInput {
                name: "safe_skill".to_string(),
                description: "A safe automation helper".to_string(),
                source: Some("https://example.com/skills/safe".to_string()),
                content: Some("echo hello".to_string()),
                signature: Some("sig-v1".to_string()),
            })
            .await
            .expect("install skill");
        assert_eq!(record.decision, SkillPolicyDecision::Approve);
        assert!(record.active);
        assert_eq!(record.scan_count, 1);

        let listed = runtime.list().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "safe_skill");
        assert!(listed[0].active);
        let scans = runtime
            .list_scans(&record.skill_id, 10)
            .await
            .expect("list scans");
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].scanner, "policy.install");
    }

    #[tokio::test]
    async fn warn_skill_requires_operator_approval_to_activate() {
        let runtime = new_runtime().await;
        let warned = runtime
            .install(InstallSkillInput {
                name: "warn_skill".to_string(),
                description: "No source and no signature".to_string(),
                source: None,
                content: Some("echo safe".to_string()),
                signature: None,
            })
            .await
            .expect("install warned skill");
        assert_eq!(warned.decision, SkillPolicyDecision::Warn);
        assert!(!warned.active);

        let approved = runtime
            .approve(&warned.skill_id, Some("manual review complete".to_string()))
            .await
            .expect("approve warned skill");
        assert!(approved.active);
        assert!(approved.approved_by_operator);

        let rescanned = runtime
            .rescan(&warned.skill_id)
            .await
            .expect("rescan warned skill");
        assert_eq!(rescanned.decision, SkillPolicyDecision::Warn);
        assert!(rescanned.active);
        assert!(rescanned.scan_count >= 2);

        let scans = runtime
            .list_scans(&warned.skill_id, 10)
            .await
            .expect("list scans");
        assert!(scans.len() >= 3);
    }

    #[tokio::test]
    async fn blocked_skill_cannot_be_operator_approved() {
        let runtime = new_runtime().await;
        let blocked = runtime
            .install(InstallSkillInput {
                name: "blocked_skill".to_string(),
                description: "installs with curl | sh".to_string(),
                source: Some("https://example.com/skills/blocked".to_string()),
                content: None,
                signature: Some("sig-v1".to_string()),
            })
            .await
            .expect("install blocked skill");
        assert_eq!(blocked.decision, SkillPolicyDecision::Block);
        assert!(!blocked.active);

        let err = runtime
            .approve(&blocked.skill_id, None)
            .await
            .expect_err("blocked skill approve should fail");
        assert!(err.to_string().contains("blocked skill cannot be approved"));
    }

    #[tokio::test]
    async fn revoke_deactivates_skill_and_records_scan() {
        let runtime = new_runtime().await;
        let record = runtime
            .install(InstallSkillInput {
                name: "revoke_skill".to_string(),
                description: "revocable skill".to_string(),
                source: Some("https://example.com/skills/revoke".to_string()),
                content: Some("echo revoke".to_string()),
                signature: Some("sig-v1".to_string()),
            })
            .await
            .expect("install skill");
        assert!(record.active);

        let revoked = runtime
            .revoke(&record.skill_id, Some("security incident".to_string()))
            .await
            .expect("revoke skill");
        assert!(!revoked.active);
        assert!(!revoked.approved_by_operator);

        let scans = runtime
            .list_scans(&record.skill_id, 10)
            .await
            .expect("list scans");
        assert!(scans.iter().any(|s| s.scanner == "operator.revoke"));
    }
}
