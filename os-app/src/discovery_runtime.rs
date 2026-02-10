use crate::config::{DiscoveryMode, RuntimeNetworkPolicy};
use anyhow::Context;
use chrono::{DateTime, Utc};
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

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryStatus {
    pub policy: RuntimeNetworkPolicy,
    pub active: bool,
    pub started_at: DateTime<Utc>,
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
        let status = DiscoveryStatus {
            active: !matches!(policy.discovery_mode, DiscoveryMode::Disabled),
            started_at: Utc::now(),
            last_heartbeat_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error: None,
            last_probe: None,
            policy: policy.clone(),
        };
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

        let state = Arc::clone(&self.state);
        let policy = self.policy.clone();
        let shutdown = self.shutdown.clone();
        *task_handle = Some(tokio::spawn(async move {
            run_probe_loop(policy, state, shutdown).await;
        }));
    }

    pub async fn status_snapshot(&self) -> DiscoveryStatus {
        self.state.read().await.clone()
    }

    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let handle = self.task_handle.lock().await.take();
        if let Some(handle) = handle {
            if let Err(error) = handle.await {
                tracing::warn!(?error, "discovery runtime shutdown join failed");
            }
        }
    }
}

async fn run_probe_loop(
    policy: RuntimeNetworkPolicy,
    state: Arc<RwLock<DiscoveryStatus>>,
    shutdown: CancellationToken,
) {
    let interval_seconds = match policy.discovery_mode {
        DiscoveryMode::Mdns => MDNS_PROBE_INTERVAL_SECONDS,
        DiscoveryMode::TailnetServe | DiscoveryMode::TailnetFunnel => {
            TAILNET_PROBE_INTERVAL_SECONDS
        }
        DiscoveryMode::Disabled => return,
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
    guard.last_heartbeat_at = Some(Utc::now());
}

async fn update_probe_success(state: &Arc<RwLock<DiscoveryStatus>>, probe: serde_json::Value) {
    let mut guard = state.write().await;
    let now = Utc::now();
    guard.last_success_at = Some(now);
    guard.last_error = None;
    guard.last_error_at = None;
    guard.last_probe = Some(probe);
}

async fn update_probe_error(state: &Arc<RwLock<DiscoveryStatus>>, error: anyhow::Error) {
    let mut guard = state.write().await;
    let now = Utc::now();
    guard.last_error_at = Some(now);
    guard.last_error = Some(error.to_string());
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
    use super::build_mdns_query;

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
}
