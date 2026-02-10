use crate::config::{DiscoveryMode, RuntimeNetworkPolicy};
use anyhow::Context;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use serde_json::json;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_MULTICAST_PORT: u16 = 5353;
const MDNS_SERVICE_LABELS: [&str; 3] = ["_opencraw", "_tcp", "local"];
const MDNS_PROBE_INTERVAL_SECONDS: u64 = 30;
const TAILNET_PROBE_INTERVAL_SECONDS: u64 = 30;
const DISCOVERY_STALE_HEARTBEAT_MULTIPLIER: u64 = 3;
const DISCOVERY_UNHEALTHY_FAILURE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryHealth {
    Disabled,
    Starting,
    Healthy,
    Degraded,
    Unhealthy,
}

pub fn discovery_health_status_label(health: DiscoveryHealth) -> &'static str {
    match health {
        DiscoveryHealth::Disabled | DiscoveryHealth::Starting | DiscoveryHealth::Healthy => "ok",
        DiscoveryHealth::Degraded | DiscoveryHealth::Unhealthy => "degraded",
    }
}

pub fn discovery_health_ready(health: DiscoveryHealth) -> bool {
    !matches!(health, DiscoveryHealth::Unhealthy)
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryStatus {
    pub policy: RuntimeNetworkPolicy,
    pub active: bool,
    pub started_at: DateTime<Utc>,
    pub probe_interval_seconds: Option<u64>,
    pub consecutive_failures: u32,
    pub total_successes: u64,
    pub total_failures: u64,
    pub health: DiscoveryHealth,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_probe: Option<serde_json::Value>,
}

pub struct DiscoveryRuntime {
    policy: RuntimeNetworkPolicy,
    state: Arc<RwLock<DiscoveryStatus>>,
    shutdown: CancellationToken,
    task_handle: Mutex<Option<JoinHandle<()>>>,
}

impl DiscoveryRuntime {
    pub fn new(policy: RuntimeNetworkPolicy) -> Self {
        let mut status = DiscoveryStatus {
            active: !matches!(policy.discovery_mode, DiscoveryMode::Disabled),
            started_at: Utc::now(),
            probe_interval_seconds: probe_interval_seconds_for_mode(policy.discovery_mode),
            consecutive_failures: 0,
            total_successes: 0,
            total_failures: 0,
            health: DiscoveryHealth::Starting,
            last_heartbeat_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error: None,
            last_probe: None,
            policy: policy.clone(),
        };
        status.health = evaluate_discovery_health(&status, Utc::now());
        Self {
            policy,
            state: Arc::new(RwLock::new(status)),
            shutdown: CancellationToken::new(),
            task_handle: Mutex::new(None),
        }
    }

    pub async fn start(&self) {
        if matches!(self.policy.discovery_mode, DiscoveryMode::Disabled) {
            return;
        }

        let mut task_handle = self.task_handle.lock().await;
        if task_handle.is_some() {
            return;
        }

        {
            let mut guard = self.state.write().await;
            guard.active = true;
            guard.started_at = Utc::now();
            guard.probe_interval_seconds =
                probe_interval_seconds_for_mode(self.policy.discovery_mode);
            guard.health = evaluate_discovery_health(&guard, Utc::now());
        }

        let state = Arc::clone(&self.state);
        let policy = self.policy.clone();
        let shutdown = self.shutdown.clone();
        *task_handle = Some(tokio::spawn(async move {
            run_probe_loop(policy, state, shutdown).await;
        }));
    }

    pub async fn status_snapshot(&self) -> DiscoveryStatus {
        let mut snapshot = self.state.read().await.clone();
        snapshot.health = evaluate_discovery_health(&snapshot, Utc::now());
        snapshot
    }

    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let handle = self.task_handle.lock().await.take();
        if let Some(handle) = handle {
            if let Err(error) = handle.await {
                tracing::warn!(?error, "discovery runtime shutdown join failed");
            }
        }

        let mut guard = self.state.write().await;
        guard.active = false;
        guard.health = evaluate_discovery_health(&guard, Utc::now());
    }
}

fn probe_interval_seconds_for_mode(mode: DiscoveryMode) -> Option<u64> {
    match mode {
        DiscoveryMode::Mdns => Some(MDNS_PROBE_INTERVAL_SECONDS),
        DiscoveryMode::TailnetServe | DiscoveryMode::TailnetFunnel => {
            Some(TAILNET_PROBE_INTERVAL_SECONDS)
        }
        DiscoveryMode::Disabled => None,
    }
}

fn stale_heartbeat_threshold(status: &DiscoveryStatus) -> Option<ChronoDuration> {
    let interval_seconds = status
        .probe_interval_seconds
        .or_else(|| probe_interval_seconds_for_mode(status.policy.discovery_mode))?;
    let stale_seconds = interval_seconds
        .saturating_mul(DISCOVERY_STALE_HEARTBEAT_MULTIPLIER)
        .min(i64::MAX as u64) as i64;
    Some(ChronoDuration::seconds(stale_seconds))
}

pub fn evaluate_discovery_health(status: &DiscoveryStatus, now: DateTime<Utc>) -> DiscoveryHealth {
    if matches!(status.policy.discovery_mode, DiscoveryMode::Disabled) {
        return DiscoveryHealth::Disabled;
    }

    if !status.active {
        return DiscoveryHealth::Unhealthy;
    }

    if let Some(last_heartbeat_at) = status.last_heartbeat_at {
        if let Some(stale_after) = stale_heartbeat_threshold(status) {
            if now.signed_duration_since(last_heartbeat_at) > stale_after {
                return DiscoveryHealth::Unhealthy;
            }
        }
    }

    if status.consecutive_failures >= DISCOVERY_UNHEALTHY_FAILURE_THRESHOLD {
        return DiscoveryHealth::Unhealthy;
    }

    if let Some(last_error_at) = status.last_error_at {
        let recovered = status
            .last_success_at
            .map(|last_success_at| last_success_at >= last_error_at)
            .unwrap_or(false);
        if !recovered {
            return DiscoveryHealth::Degraded;
        }
    }

    if status.last_success_at.is_some() {
        return DiscoveryHealth::Healthy;
    }

    if status.total_failures > 0 {
        return DiscoveryHealth::Degraded;
    }

    DiscoveryHealth::Starting
}

async fn run_probe_loop(
    policy: RuntimeNetworkPolicy,
    state: Arc<RwLock<DiscoveryStatus>>,
    shutdown: CancellationToken,
) {
    let Some(interval_seconds) = probe_interval_seconds_for_mode(policy.discovery_mode) else {
        return;
    };
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                update_heartbeat(&state).await;
                let probe_result = match policy.discovery_mode {
                    DiscoveryMode::Mdns => probe_mdns(&policy),
                    DiscoveryMode::TailnetServe | DiscoveryMode::TailnetFunnel => {
                        probe_tailnet(&policy).await
                    }
                    DiscoveryMode::Disabled => Ok(json!({"mode": "disabled"})),
                };
                match probe_result {
                    Ok(probe) => update_probe_success(&state, probe).await,
                    Err(error) => update_probe_error(&state, error).await,
                }
            }
        }
    }
}

async fn update_heartbeat(state: &Arc<RwLock<DiscoveryStatus>>) {
    let mut guard = state.write().await;
    let now = Utc::now();
    guard.last_heartbeat_at = Some(now);
    guard.health = evaluate_discovery_health(&guard, now);
}

async fn update_probe_success(state: &Arc<RwLock<DiscoveryStatus>>, probe: serde_json::Value) {
    let mut guard = state.write().await;
    let now = Utc::now();
    guard.last_success_at = Some(now);
    guard.last_error = None;
    guard.last_error_at = None;
    guard.last_probe = Some(probe);
    guard.total_successes = guard.total_successes.saturating_add(1);
    guard.consecutive_failures = 0;
    guard.health = evaluate_discovery_health(&guard, now);
}

async fn update_probe_error(state: &Arc<RwLock<DiscoveryStatus>>, error: anyhow::Error) {
    let mut guard = state.write().await;
    let now = Utc::now();
    let error_message = error.to_string();
    guard.last_error_at = Some(now);
    guard.last_error = Some(error_message.clone());
    guard.last_probe = Some(json!({
        "kind": "probe_error",
        "error": error_message,
        "at": now,
    }));
    guard.total_failures = guard.total_failures.saturating_add(1);
    guard.consecutive_failures = guard.consecutive_failures.saturating_add(1);
    guard.health = evaluate_discovery_health(&guard, now);
}

fn probe_mdns(policy: &RuntimeNetworkPolicy) -> anyhow::Result<serde_json::Value> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .context("bind ephemeral UDP socket for mDNS probe")?;
    socket
        .set_multicast_ttl_v4(255)
        .context("set mDNS multicast TTL")?;

    let payload = build_mdns_query(&MDNS_SERVICE_LABELS);
    let sent_bytes = socket
        .send_to(
            &payload,
            SocketAddrV4::new(MDNS_MULTICAST_ADDR, MDNS_MULTICAST_PORT),
        )
        .context("send mDNS probe query")?;

    Ok(json!({
        "kind": "mdns_probe",
        "query_name": "_opencraw._tcp.local",
        "sent_bytes": sent_bytes,
        "bind_addr": policy.bind_addr,
        "advertised_base_url": policy.advertised_base_url,
    }))
}

async fn probe_tailnet(policy: &RuntimeNetworkPolicy) -> anyhow::Result<serde_json::Value> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await
        .context("execute tailscale status --json")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow::anyhow!(
            "tailscale status --json failed with status {}: {}",
            output.status,
            stderr
        ));
    }

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parse tailscale status JSON")?;
    Ok(json!({
        "kind": "tailnet_status",
        "requested_mode": policy.discovery_mode,
        "backend_state": parsed.get("BackendState").and_then(|v| v.as_str()),
        "self_dns_name": parsed
            .get("Self")
            .and_then(|v| v.get("DNSName"))
            .and_then(|v| v.as_str()),
        "tailnet_name": parsed
            .get("CurrentTailnet")
            .and_then(|v| v.get("Name"))
            .and_then(|v| v.as_str()),
        "health": parsed.get("Health").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
    }))
}

fn build_mdns_query(labels: &[&str]) -> Vec<u8> {
    let mut packet = vec![0_u8; 12];
    // Header:
    // id=0x0000, flags=0x0000, qdcount=1, ancount=0, nscount=0, arcount=0
    packet[4] = 0;
    packet[5] = 1;

    for label in labels {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0); // name terminator

    // QTYPE PTR (12), QCLASS IN (1)
    packet.push(0);
    packet.push(12);
    packet.push(0);
    packet.push(1);

    packet
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryHealth, DiscoveryStatus, MDNS_PROBE_INTERVAL_SECONDS, build_mdns_query,
        discovery_health_ready, discovery_health_status_label, evaluate_discovery_health,
        update_probe_error, update_probe_success,
    };
    use crate::config::{BindMode, DiscoveryMode, RuntimeExposure, RuntimeNetworkPolicy};
    use chrono::{Duration as ChronoDuration, Utc};
    use serde_json::json;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn test_policy(mode: DiscoveryMode) -> RuntimeNetworkPolicy {
        RuntimeNetworkPolicy {
            bind_mode: BindMode::Loopback,
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
            discovery_mode: mode,
            exposure: RuntimeExposure::Loopback,
            public_ingress: false,
            control_api_auth_configured: true,
            allow_public_bind_without_auth: false,
            advertised_base_url: None,
        }
    }

    fn test_status(mode: DiscoveryMode) -> DiscoveryStatus {
        let policy = test_policy(mode);
        let mut status = DiscoveryStatus {
            policy,
            active: !matches!(mode, DiscoveryMode::Disabled),
            started_at: Utc::now(),
            probe_interval_seconds: Some(MDNS_PROBE_INTERVAL_SECONDS),
            consecutive_failures: 0,
            total_successes: 0,
            total_failures: 0,
            health: DiscoveryHealth::Starting,
            last_heartbeat_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error: None,
            last_probe: None,
        };
        status.health = evaluate_discovery_health(&status, Utc::now());
        status
    }

    #[test]
    fn mdns_query_builder_encodes_expected_suffix() {
        let packet = build_mdns_query(&["_opencraw", "_tcp", "local"]);
        assert!(packet.len() > 20);
        assert_eq!(&packet[0..6], &[0, 0, 0, 0, 0, 1]);
        assert_eq!(
            &packet[packet.len() - 4..],
            &[0, 12, 0, 1],
            "packet should end with PTR+IN question"
        );
    }

    #[test]
    fn discovery_health_label_maps_degraded_to_non_ready_status() {
        assert_eq!(
            discovery_health_status_label(DiscoveryHealth::Healthy),
            "ok"
        );
        assert_eq!(
            discovery_health_status_label(DiscoveryHealth::Starting),
            "ok"
        );
        assert_eq!(
            discovery_health_status_label(DiscoveryHealth::Disabled),
            "ok"
        );
        assert_eq!(
            discovery_health_status_label(DiscoveryHealth::Degraded),
            "degraded"
        );
        assert!(!discovery_health_ready(DiscoveryHealth::Unhealthy));
        assert!(discovery_health_ready(DiscoveryHealth::Degraded));
    }

    #[test]
    fn discovery_health_is_disabled_when_mode_is_disabled() {
        let status = test_status(DiscoveryMode::Disabled);
        assert_eq!(status.health, DiscoveryHealth::Disabled);
    }

    #[test]
    fn discovery_health_is_starting_before_any_probe() {
        let status = test_status(DiscoveryMode::Mdns);
        assert_eq!(status.health, DiscoveryHealth::Starting);
    }

    #[test]
    fn discovery_health_degrades_after_error_without_recovery() {
        let mut status = test_status(DiscoveryMode::Mdns);
        let now = Utc::now();
        status.last_error_at = Some(now);
        status.last_error = Some("probe failed".to_string());
        status.total_failures = 1;
        status.consecutive_failures = 1;
        assert_eq!(
            evaluate_discovery_health(&status, now),
            DiscoveryHealth::Degraded
        );
    }

    #[test]
    fn discovery_health_is_unhealthy_after_repeated_failures() {
        let mut status = test_status(DiscoveryMode::Mdns);
        let now = Utc::now();
        status.last_error_at = Some(now);
        status.last_error = Some("probe failed".to_string());
        status.total_failures = 3;
        status.consecutive_failures = 3;
        assert_eq!(
            evaluate_discovery_health(&status, now),
            DiscoveryHealth::Unhealthy
        );
    }

    #[test]
    fn discovery_health_is_unhealthy_when_heartbeat_is_stale() {
        let mut status = test_status(DiscoveryMode::Mdns);
        let now = Utc::now();
        status.last_success_at = Some(now - ChronoDuration::seconds(10));
        status.last_heartbeat_at =
            Some(now - ChronoDuration::seconds((MDNS_PROBE_INTERVAL_SECONDS * 3 + 1) as i64));
        assert_eq!(
            evaluate_discovery_health(&status, now),
            DiscoveryHealth::Unhealthy
        );
    }

    #[tokio::test]
    async fn discovery_probe_updates_track_failures_and_recovery() {
        let state = Arc::new(RwLock::new(test_status(DiscoveryMode::Mdns)));

        update_probe_error(&state, anyhow::anyhow!("tailscale unavailable")).await;
        let first = state.read().await.clone();
        assert_eq!(first.total_failures, 1);
        assert_eq!(first.consecutive_failures, 1);
        assert_eq!(first.health, DiscoveryHealth::Degraded);
        assert_eq!(
            first
                .last_probe
                .as_ref()
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str()),
            Some("probe_error")
        );

        update_probe_success(&state, json!({ "kind": "mdns_probe" })).await;
        let second = state.read().await.clone();
        assert_eq!(second.total_successes, 1);
        assert_eq!(second.consecutive_failures, 0);
        assert_eq!(second.health, DiscoveryHealth::Healthy);
        assert_eq!(
            second
                .last_probe
                .as_ref()
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str()),
            Some("mdns_probe")
        );
    }
}
