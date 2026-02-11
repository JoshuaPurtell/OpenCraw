//! Pairing lifecycle enforcement.
//!
//! See: docs/channels/pairing.md

use crate::config::{OpenShellConfig, SenderAccessMode};
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

const PAIRING_CODE_LEN: usize = 8;
const PAIRING_CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const DEFAULT_PENDING_REQUEST_CAP: usize = 3;
const DEFAULT_REQUEST_TTL_MINUTES: i64 = 60;
const MAX_IDENTITY_LEN: usize = 256;
const MAX_RESOLUTION_NOTE_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingRequestStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRequest {
    pub channel_id: String,
    pub sender_id: String,
    pub code: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: PairingRequestStatus,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_note: Option<String>,
}

impl PairingRequest {
    fn new(
        channel_id: String,
        sender_id: String,
        code: String,
        requested_at: DateTime<Utc>,
        request_ttl: Duration,
    ) -> Self {
        Self {
            channel_id,
            sender_id,
            code,
            requested_at,
            expires_at: requested_at + request_ttl,
            status: PairingRequestStatus::Pending,
            resolved_at: None,
            resolution_note: None,
        }
    }

    fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingDenialReason {
    PendingApprovalCreated,
    PendingApprovalRequired,
    PendingCapReached,
    InvalidIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingDenial {
    pub reason: PairingDenialReason,
    pub request: Option<PairingRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingDecision {
    Allowed,
    Denied(PairingDenial),
}

impl PairingDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    InvalidChannelId,
    InvalidSenderId,
    InvalidCode,
    InvalidResolutionNote,
    RequestNotFound,
    RequestExpired,
    RequestAlreadyResolved(PairingRequestStatus),
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChannelId => write!(f, "invalid channel id"),
            Self::InvalidSenderId => write!(f, "invalid sender id"),
            Self::InvalidCode => write!(f, "invalid pairing code"),
            Self::InvalidResolutionNote => write!(f, "invalid resolution note"),
            Self::RequestNotFound => write!(f, "pairing request not found"),
            Self::RequestExpired => write!(f, "pairing request expired"),
            Self::RequestAlreadyResolved(status) => {
                write!(f, "pairing request already resolved with status {status:?}")
            }
        }
    }
}

impl std::error::Error for PairingError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PairingPolicy {
    pending_cap: usize,
    request_ttl: Duration,
}

impl Default for PairingPolicy {
    fn default() -> Self {
        Self {
            pending_cap: DEFAULT_PENDING_REQUEST_CAP,
            request_ttl: Duration::minutes(DEFAULT_REQUEST_TTL_MINUTES),
        }
    }
}

#[derive(Debug, Default)]
struct ChannelPairingState {
    approved_senders: HashSet<String>,
    requests: Vec<PairingRequest>,
}

#[derive(Debug, Default)]
struct PairingRuntime {
    channels: HashMap<String, ChannelPairingState>,
    policy: PairingPolicy,
}

impl PairingRuntime {
    fn evaluate_sender(
        &mut self,
        cfg: &OpenShellConfig,
        channel_id: &str,
        sender_id: &str,
        now: DateTime<Utc>,
    ) -> PairingDecision {
        // WebChat is local/dev and remains open by default.
        if channel_id == "webchat" {
            return PairingDecision::Allowed;
        }

        let channel_id = match normalize_channel_id(channel_id) {
            Ok(value) => value,
            Err(_) => {
                return PairingDecision::Denied(PairingDenial {
                    reason: PairingDenialReason::InvalidIdentity,
                    request: None,
                });
            }
        };
        let sender_id = match normalize_sender_id(sender_id) {
            Ok(value) => value,
            Err(_) => {
                return PairingDecision::Denied(PairingDenial {
                    reason: PairingDenialReason::InvalidIdentity,
                    request: None,
                });
            }
        };

        if is_allowlisted(cfg, &channel_id, &sender_id) {
            return PairingDecision::Allowed;
        }

        let state = self.channels.entry(channel_id.clone()).or_default();
        expire_pending_requests(state, now);

        if state.approved_senders.contains(&sender_id) {
            return PairingDecision::Allowed;
        }

        if let Some(existing) = state
            .requests
            .iter()
            .find(|request| {
                request.status == PairingRequestStatus::Pending && request.sender_id == sender_id
            })
            .cloned()
        {
            return PairingDecision::Denied(PairingDenial {
                reason: PairingDenialReason::PendingApprovalRequired,
                request: Some(existing),
            });
        }

        let pending_count = state
            .requests
            .iter()
            .filter(|request| request.status == PairingRequestStatus::Pending)
            .count();
        if pending_count >= self.policy.pending_cap {
            return PairingDecision::Denied(PairingDenial {
                reason: PairingDenialReason::PendingCapReached,
                request: None,
            });
        }

        let request = PairingRequest::new(
            channel_id,
            sender_id,
            generate_unique_pairing_code(&state.requests),
            now,
            self.policy.request_ttl,
        );
        state.requests.push(request.clone());
        PairingDecision::Denied(PairingDenial {
            reason: PairingDenialReason::PendingApprovalCreated,
            request: Some(request),
        })
    }

    fn approve_request(
        &mut self,
        channel_id: &str,
        code: &str,
        now: DateTime<Utc>,
    ) -> Result<PairingRequest, PairingError> {
        let channel_id = normalize_channel_id(channel_id)?;
        let code = normalize_code(code)?;

        let state = self
            .channels
            .get_mut(&channel_id)
            .ok_or(PairingError::RequestNotFound)?;
        expire_pending_requests(state, now);

        let index = state
            .requests
            .iter()
            .position(|request| request.code == code)
            .ok_or(PairingError::RequestNotFound)?;

        match state.requests[index].status {
            PairingRequestStatus::Pending => {
                if state.requests[index].is_expired_at(now) {
                    mark_request_expired(&mut state.requests[index]);
                    return Err(PairingError::RequestExpired);
                }
                state.requests[index].status = PairingRequestStatus::Approved;
                state.requests[index].resolved_at = Some(now);
                state.requests[index].resolution_note = None;
                state
                    .approved_senders
                    .insert(state.requests[index].sender_id.clone());
                Ok(state.requests[index].clone())
            }
            PairingRequestStatus::Expired => Err(PairingError::RequestExpired),
            status => Err(PairingError::RequestAlreadyResolved(status)),
        }
    }

    fn reject_request(
        &mut self,
        channel_id: &str,
        code: &str,
        resolution_note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<PairingRequest, PairingError> {
        let channel_id = normalize_channel_id(channel_id)?;
        let code = normalize_code(code)?;
        let resolution_note = normalize_resolution_note(resolution_note)?;

        let state = self
            .channels
            .get_mut(&channel_id)
            .ok_or(PairingError::RequestNotFound)?;
        expire_pending_requests(state, now);

        let index = state
            .requests
            .iter()
            .position(|request| request.code == code)
            .ok_or(PairingError::RequestNotFound)?;

        match state.requests[index].status {
            PairingRequestStatus::Pending => {
                if state.requests[index].is_expired_at(now) {
                    mark_request_expired(&mut state.requests[index]);
                    return Err(PairingError::RequestExpired);
                }
                state.requests[index].status = PairingRequestStatus::Rejected;
                state.requests[index].resolved_at = Some(now);
                state.requests[index].resolution_note = resolution_note;
                Ok(state.requests[index].clone())
            }
            PairingRequestStatus::Expired => Err(PairingError::RequestExpired),
            status => Err(PairingError::RequestAlreadyResolved(status)),
        }
    }

    fn list_requests(
        &mut self,
        channel_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<PairingRequest>, PairingError> {
        let channel_id = normalize_channel_id(channel_id)?;
        let Some(state) = self.channels.get_mut(&channel_id) else {
            return Ok(Vec::new());
        };
        expire_pending_requests(state, now);
        let mut requests = state.requests.clone();
        requests.sort_by(|a, b| b.requested_at.cmp(&a.requested_at));
        Ok(requests)
    }
}

static PAIRING_RUNTIME: OnceLock<Mutex<PairingRuntime>> = OnceLock::new();

fn with_runtime<R>(f: impl FnOnce(&mut PairingRuntime) -> R) -> R {
    let runtime = PAIRING_RUNTIME.get_or_init(|| Mutex::new(PairingRuntime::default()));
    let mut guard = runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

pub fn evaluate_sender(
    cfg: &OpenShellConfig,
    channel_id: &str,
    sender_id: &str,
) -> PairingDecision {
    with_runtime(|runtime| runtime.evaluate_sender(cfg, channel_id, sender_id, Utc::now()))
}

pub fn is_allowed(cfg: &OpenShellConfig, channel_id: &str, sender_id: &str) -> bool {
    evaluate_sender(cfg, channel_id, sender_id).is_allowed()
}

pub fn approve_pairing_request(
    channel_id: &str,
    code: &str,
) -> Result<PairingRequest, PairingError> {
    with_runtime(|runtime| runtime.approve_request(channel_id, code, Utc::now()))
}

pub fn reject_pairing_request(
    channel_id: &str,
    code: &str,
    resolution_note: Option<&str>,
) -> Result<PairingRequest, PairingError> {
    with_runtime(|runtime| runtime.reject_request(channel_id, code, resolution_note, Utc::now()))
}

pub fn list_pairing_requests(channel_id: &str) -> Result<Vec<PairingRequest>, PairingError> {
    with_runtime(|runtime| runtime.list_requests(channel_id, Utc::now()))
}

pub fn list_pending_pairing_requests(
    channel_id: &str,
) -> Result<Vec<PairingRequest>, PairingError> {
    Ok(list_pairing_requests(channel_id)?
        .into_iter()
        .filter(|request| request.status == PairingRequestStatus::Pending)
        .collect())
}

fn is_allowlisted(cfg: &OpenShellConfig, channel_id: &str, sender_id: &str) -> bool {
    match cfg.channel_access_mode(channel_id) {
        SenderAccessMode::Open => true,
        SenderAccessMode::Allowlist => cfg.channel_is_sender_allowlisted(channel_id, sender_id),
        SenderAccessMode::Pairing => false,
    }
}

fn normalize_channel_id(value: &str) -> Result<String, PairingError> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized.len() > MAX_IDENTITY_LEN {
        return Err(PairingError::InvalidChannelId);
    }
    if normalized
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(PairingError::InvalidChannelId);
    }
    Ok(normalized)
}

fn normalize_sender_id(value: &str) -> Result<String, PairingError> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() || normalized.len() > MAX_IDENTITY_LEN {
        return Err(PairingError::InvalidSenderId);
    }
    if normalized.chars().any(char::is_control) {
        return Err(PairingError::InvalidSenderId);
    }
    Ok(normalized)
}

fn normalize_resolution_note(value: Option<&str>) -> Result<Option<String>, PairingError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Ok(None);
    }
    if normalized.len() > MAX_RESOLUTION_NOTE_LEN || normalized.chars().any(char::is_control) {
        return Err(PairingError::InvalidResolutionNote);
    }
    Ok(Some(normalized))
}

fn normalize_code(value: &str) -> Result<String, PairingError> {
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.len() != PAIRING_CODE_LEN || !normalized.chars().all(is_valid_pairing_code_char) {
        return Err(PairingError::InvalidCode);
    }
    Ok(normalized)
}

fn is_valid_pairing_code_char(ch: char) -> bool {
    PAIRING_CODE_ALPHABET
        .iter()
        .copied()
        .map(char::from)
        .any(|allowed| allowed == ch)
}

fn generate_pairing_code() -> String {
    let seed = Uuid::new_v4().into_bytes();
    let mut code = String::with_capacity(PAIRING_CODE_LEN);
    for byte in seed.iter().take(PAIRING_CODE_LEN) {
        let idx = usize::from(*byte) % PAIRING_CODE_ALPHABET.len();
        code.push(char::from(PAIRING_CODE_ALPHABET[idx]));
    }
    code
}

fn generate_unique_pairing_code(existing_requests: &[PairingRequest]) -> String {
    loop {
        let candidate = generate_pairing_code();
        if existing_requests
            .iter()
            .all(|request| request.code != candidate)
        {
            return candidate;
        }
    }
}

fn expire_pending_requests(state: &mut ChannelPairingState, now: DateTime<Utc>) {
    for request in &mut state.requests {
        if request.status == PairingRequestStatus::Pending && request.is_expired_at(now) {
            mark_request_expired(request);
        }
    }
}

fn mark_request_expired(request: &mut PairingRequest) {
    request.status = PairingRequestStatus::Expired;
    request.resolved_at = Some(request.expires_at);
    request.resolution_note = Some("expired".to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ApprovalMode, AutomationConfig, ChannelsConfig, ContextConfig, DiscordConfig, EmailConfig,
        GeneralConfig, ImessageConfig, KeysConfig, LinearConfig, LlmConfig, LlmProfileConfig,
        LlmProvider, MatrixConfig, MemoryConfig, OpenShellConfig, OptimizationConfig, QueueConfig,
        RuntimeConfig, SecurityConfig, SenderAccessMode, SignalConfig, SkillsConfig, SlackConfig,
        TelegramConfig, ToolsConfig, WebChatConfig, WhatsAppConfig,
    };
    use chrono::TimeZone;

    fn base_cfg() -> OpenShellConfig {
        OpenShellConfig {
            llm: LlmConfig {
                active_profile: "default".to_string(),
                fallback_profiles: Vec::new(),
                failover_cooldown_base_seconds: 5,
                failover_cooldown_max_seconds: 300,
                profiles: std::collections::BTreeMap::from([(
                    "default".to_string(),
                    LlmProfileConfig {
                        provider: LlmProvider::Openai,
                        model: "gpt-4o-mini".to_string(),
                        fallback_models: Vec::new(),
                    },
                )]),
            },
            general: GeneralConfig {
                system_prompt: "x".to_string(),
            },
            keys: KeysConfig::default(),
            channels: ChannelsConfig {
                webchat: WebChatConfig {
                    enabled: true,
                    port: 3000,
                },
                telegram: TelegramConfig::default(),
                discord: DiscordConfig::default(),
                slack: SlackConfig::default(),
                matrix: MatrixConfig::default(),
                signal: SignalConfig::default(),
                whatsapp: WhatsAppConfig::default(),
                imessage: ImessageConfig::default(),
                email: EmailConfig::default(),
                linear: LinearConfig::default(),
                external_plugins: Vec::new(),
            },
            tools: ToolsConfig::default(),
            security: SecurityConfig {
                shell_approval: ApprovalMode::Human,
                browser_approval: ApprovalMode::Ai,
                filesystem_write_approval: ApprovalMode::Ai,
                human_approval_timeout_seconds: 300,
                control_api_key: None,
                control_api_keys: vec![],
                mutating_auth_exempt_prefixes: vec![
                    "/api/v1/os/automation/webhook/".to_string(),
                    "/api/v1/os/automation/poll/".to_string(),
                ],
            },
            runtime: RuntimeConfig::default(),
            queue: QueueConfig::default(),
            context: ContextConfig::default(),
            memory: MemoryConfig::default(),
            optimization: OptimizationConfig::default(),
            automation: AutomationConfig::default(),
            skills: SkillsConfig::default(),
        }
    }

    fn ts(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 10, hour, minute, 0)
            .single()
            .expect("valid timestamp")
    }

    fn expect_denied(decision: PairingDecision) -> PairingDenial {
        match decision {
            PairingDecision::Allowed => panic!("expected denied decision"),
            PairingDecision::Denied(denial) => denial,
        }
    }

    #[test]
    fn webchat_is_allowed_by_default() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        assert!(
            runtime
                .evaluate_sender(&cfg, "webchat", "any", ts(9, 0))
                .is_allowed()
        );
    }

    #[test]
    fn unknown_sender_creates_pending_pairing_request() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let denied = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now));
        assert_eq!(denied.reason, PairingDenialReason::PendingApprovalCreated);
        let request = denied.request.expect("created request");
        assert_eq!(request.status, PairingRequestStatus::Pending);
        assert_eq!(request.expires_at, now + Duration::minutes(60));
        assert_eq!(request.code.len(), PAIRING_CODE_LEN);
        assert!(request.code.chars().all(is_valid_pairing_code_char));
        assert!(!request.code.contains('0'));
        assert!(!request.code.contains('1'));
        assert!(!request.code.contains('I'));
        assert!(!request.code.contains('O'));
    }

    #[test]
    fn existing_pending_request_is_reused_for_same_sender() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);

        let first = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("first request");
        let second = expect_denied(runtime.evaluate_sender(
            &cfg,
            "telegram",
            "123",
            now + Duration::minutes(10),
        ));
        assert_eq!(second.reason, PairingDenialReason::PendingApprovalRequired);
        let reused = second.request.expect("reused request");
        assert_eq!(reused.code, first.code);

        let requests = runtime
            .list_requests("telegram", now + Duration::minutes(10))
            .expect("list requests");
        let pending_count = requests
            .iter()
            .filter(|request| request.status == PairingRequestStatus::Pending)
            .count();
        assert_eq!(pending_count, 1);
    }

    #[test]
    fn pending_cap_is_enforced_per_channel() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);

        for sender in ["u1", "u2", "u3"] {
            let denied = expect_denied(runtime.evaluate_sender(&cfg, "telegram", sender, now));
            assert_eq!(denied.reason, PairingDenialReason::PendingApprovalCreated);
        }

        let denied = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "u4", now));
        assert_eq!(denied.reason, PairingDenialReason::PendingCapReached);
        assert!(denied.request.is_none());
    }

    #[test]
    fn approve_request_allows_future_messages_for_sender() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let request = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("pending request");

        let approved = runtime
            .approve_request("telegram", &request.code, now + Duration::minutes(5))
            .expect("approved request");
        assert_eq!(approved.status, PairingRequestStatus::Approved);

        assert!(
            runtime
                .evaluate_sender(&cfg, "telegram", "123", now + Duration::minutes(6))
                .is_allowed()
        );
    }

    #[test]
    fn reject_request_keeps_sender_denied_and_allows_new_request_later() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let request = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("pending request");

        let rejected = runtime
            .reject_request(
                "telegram",
                &request.code,
                Some("operator rejected"),
                now + Duration::minutes(2),
            )
            .expect("rejected request");
        assert_eq!(rejected.status, PairingRequestStatus::Rejected);
        assert_eq!(
            rejected.resolution_note.as_deref(),
            Some("operator rejected")
        );

        let next = expect_denied(runtime.evaluate_sender(
            &cfg,
            "telegram",
            "123",
            now + Duration::minutes(3),
        ));
        assert_eq!(next.reason, PairingDenialReason::PendingApprovalCreated);
        let next_request = next.request.expect("new pending request");
        assert_ne!(next_request.code, request.code);
    }

    #[test]
    fn expired_request_rotates_on_next_inbound() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let first = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("first pending request");

        let next = expect_denied(runtime.evaluate_sender(
            &cfg,
            "telegram",
            "123",
            now + Duration::minutes(61),
        ));
        assert_eq!(next.reason, PairingDenialReason::PendingApprovalCreated);
        let second = next.request.expect("rotated request");
        assert_ne!(first.code, second.code);

        let requests = runtime
            .list_requests("telegram", now + Duration::minutes(61))
            .expect("list requests");
        assert!(
            requests.iter().any(|request| request.code == first.code
                && request.status == PairingRequestStatus::Expired)
        );
    }

    #[test]
    fn approve_validation_errors_are_strict() {
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);

        let err = runtime
            .approve_request("telegram", "BAD1CODE", now)
            .expect_err("invalid code should fail");
        assert_eq!(err, PairingError::InvalidCode);

        let err = runtime
            .approve_request(" telegram", "ABCDEFGH", now)
            .expect_err("unknown request should fail");
        assert_eq!(err, PairingError::RequestNotFound);
    }

    #[test]
    fn approving_expired_request_returns_expired_error() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let request = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("pending request");

        let err = runtime
            .approve_request("telegram", &request.code, now + Duration::minutes(61))
            .expect_err("expired request should fail");
        assert_eq!(err, PairingError::RequestExpired);
    }

    #[test]
    fn open_mode_allows_external_channels_without_pairing() {
        let mut cfg = base_cfg();
        cfg.channels.imessage.access.mode = SenderAccessMode::Open;
        let mut runtime = PairingRuntime::default();
        assert!(
            runtime
                .evaluate_sender(&cfg, "imessage", "+14155551212", ts(9, 0))
                .is_allowed()
        );
    }

    #[test]
    fn allowlist_mode_matches_sender_in_channel_config() {
        let mut cfg = base_cfg();
        cfg.channels.imessage.access.mode = SenderAccessMode::Allowlist;
        cfg.channels.imessage.access.allowed_senders = vec!["+14155551212".to_string()];
        let mut runtime = PairingRuntime::default();
        assert!(
            runtime
                .evaluate_sender(&cfg, "imessage", "+14155551212", ts(9, 0))
                .is_allowed()
        );

        let mut cfg = base_cfg();
        cfg.channels.telegram.access.mode = SenderAccessMode::Allowlist;
        cfg.channels.telegram.access.allowed_senders = vec!["123".to_string()];
        let mut runtime = PairingRuntime::default();
        assert!(
            !runtime
                .evaluate_sender(&cfg, "imessage", "+14155551212", ts(9, 0))
                .is_allowed()
        );
        assert!(
            runtime
                .evaluate_sender(&cfg, "telegram", "123", ts(9, 0))
                .is_allowed()
        );
    }

    #[test]
    fn invalid_sender_identity_is_denied_without_request() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();

        let denied = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "\n", ts(9, 0)));
        assert_eq!(denied.reason, PairingDenialReason::InvalidIdentity);
        assert!(denied.request.is_none());

        let requests = runtime
            .list_requests("telegram", ts(9, 0))
            .expect("list requests");
        assert!(requests.is_empty());
    }

    #[test]
    fn rejected_or_approved_requests_cannot_be_resolved_twice() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let request = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("pending request");

        runtime
            .approve_request("telegram", &request.code, now + Duration::minutes(1))
            .expect("approved");

        let err = runtime
            .approve_request("telegram", &request.code, now + Duration::minutes(2))
            .expect_err("second approval should fail");
        assert_eq!(
            err,
            PairingError::RequestAlreadyResolved(PairingRequestStatus::Approved)
        );

        let err = runtime
            .reject_request("telegram", &request.code, None, now + Duration::minutes(3))
            .expect_err("rejecting approved request should fail");
        assert_eq!(
            err,
            PairingError::RequestAlreadyResolved(PairingRequestStatus::Approved)
        );
    }

    #[test]
    fn reject_validation_errors_are_strict() {
        let cfg = base_cfg();
        let mut runtime = PairingRuntime::default();
        let now = ts(9, 0);
        let request = expect_denied(runtime.evaluate_sender(&cfg, "telegram", "123", now))
            .request
            .expect("pending request");

        let err = runtime
            .reject_request("telegram", &request.code, Some("bad\u{0007}note"), now)
            .expect_err("control chars should fail validation");
        assert_eq!(err, PairingError::InvalidResolutionNote);
    }

    #[test]
    fn global_api_wrappers_cover_integration_surface() {
        let cfg = base_cfg();
        let channel = format!("wrapper-{}", Uuid::new_v4());
        let sender = format!("sender-{}", Uuid::new_v4());

        let denied = expect_denied(evaluate_sender(&cfg, &channel, &sender));
        let request = denied.request.expect("pending request");

        let pending = list_pending_pairing_requests(&channel).expect("list pending requests");
        assert!(pending.iter().any(|entry| entry.code == request.code));

        let listed = list_pairing_requests(&channel).expect("list all requests");
        assert!(listed.iter().any(|entry| entry.code == request.code));

        let approved =
            approve_pairing_request(&channel, &request.code).expect("approve request via wrapper");
        assert_eq!(approved.status, PairingRequestStatus::Approved);
        assert!(is_allowed(&cfg, &channel, &sender));

        let err = reject_pairing_request(&channel, &request.code, Some("late reject"))
            .expect_err("rejecting approved request should fail");
        assert_eq!(
            err,
            PairingError::RequestAlreadyResolved(PairingRequestStatus::Approved)
        );
    }
}
