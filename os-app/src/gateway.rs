//! Session multiplexer: all channel adapters feed into a single inbound queue.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::assistant::{ActionDecision, AssistantAgent};
use crate::commands;
use crate::config::{OpenShellConfig, QueueMode};
use crate::pairing;
use crate::session::SessionManager;
use anyhow::{Error, Result};
use os_channels::{ChannelAdapter, InboundMessage, InboundMessageKind, OutboundMessage};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const TELEGRAM_EDIT_MESSAGE_ID_KEY: &str = "telegram_edit_message_id";
const TELEGRAM_CLEAR_REPLY_MARKUP_KEY: &str = "telegram_clear_reply_markup";

#[derive(Clone)]
struct LaneHandle {
    tx: mpsc::Sender<InboundMessage>,
    interrupt_tx: watch::Sender<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleInboundOutcome {
    Completed,
    Nuked,
    Interrupted,
}

#[derive(Clone)]
pub struct Gateway {
    cfg: OpenShellConfig,
    started_at: Instant,
    sessions: Arc<SessionManager>,
    assistant: Arc<AssistantAgent>,
    channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    inbound_rx: Arc<Mutex<mpsc::Receiver<InboundMessage>>>,
    lane_queues: Arc<Mutex<HashMap<String, LaneHandle>>>,
    worker_budget: Arc<Semaphore>,
}

impl Gateway {
    pub fn new(
        cfg: OpenShellConfig,
        started_at: Instant,
        sessions: Arc<SessionManager>,
        assistant: Arc<AssistantAgent>,
        channels: HashMap<String, Arc<dyn ChannelAdapter>>,
        inbound_rx: mpsc::Receiver<InboundMessage>,
    ) -> Self {
        let max_concurrency = cfg.queue.max_concurrency;
        Self {
            cfg,
            started_at,
            sessions,
            assistant,
            channels,
            inbound_rx: Arc::new(Mutex::new(inbound_rx)),
            lane_queues: Arc::new(Mutex::new(HashMap::new())),
            worker_budget: Arc::new(Semaphore::new(max_concurrency)),
        }
    }

    pub fn start(self: Arc<Self>, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            tracing::info!(
                queue_mode = ?self.cfg.queue.mode,
                max_concurrency = self.cfg.queue.max_concurrency,
                lane_buffer = self.cfg.queue.lane_buffer,
                debounce_ms = self.cfg.queue.debounce_ms,
                "gateway task spawned"
            );
            if let Err(e) = self.run_loop(shutdown).await {
                tracing::error!(%e, "gateway loop exited");
            }
        })
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn run_loop(self: Arc<Self>, shutdown: CancellationToken) -> Result<()> {
        tracing::info!("gateway loop started");
        loop {
            let msg = {
                let mut rx = self.inbound_rx.lock().await;
                tokio::select! {
                    _ = shutdown.cancelled() => None,
                    msg = rx.recv() => msg,
                }
            };
            let Some(inbound) = msg else {
                if shutdown.is_cancelled() {
                    tracing::info!("gateway shutdown signal received; stopping run loop");
                } else {
                    tracing::warn!("inbound queue closed; gateway loop stopping");
                }
                self.lane_queues.lock().await.clear();
                return Ok(());
            };

            tracing::info!(
                channel_id = %inbound.channel_id,
                sender_id = %inbound.sender_id,
                message_id = %inbound.message_id,
                inbound_kind = ?inbound.kind,
                "inbound message received"
            );
            if self.try_handle_action_decision(&inbound).await? {
                continue;
            }
            self.dispatch_inbound(inbound, &shutdown).await?;
        }
    }

    async fn try_handle_action_decision(&self, inbound: &InboundMessage) -> Result<bool> {
        let Some(decision) = parse_action_decision_command(&inbound.content) else {
            return Ok(false);
        };

        if inbound.kind != InboundMessageKind::Message {
            return Ok(false);
        }

        let Some(channel) = self.channels.get(inbound.channel_id.as_str()) else {
            return Ok(false);
        };

        if !pairing::is_allowed(
            &self.cfg,
            inbound.channel_id.as_str(),
            inbound.sender_id.as_str(),
        ) {
            tracing::warn!(
                channel_id = %inbound.channel_id,
                sender_id = %inbound.sender_id,
                "approval decision rejected by pairing policy"
            );
            return Ok(true);
        }

        let reply = self
            .assistant
            .resolve_action_decision(
                inbound.channel_id.as_str(),
                inbound.sender_id.as_str(),
                inbound.thread_id.as_ref().map(|id| id.as_str()),
                decision.decision,
                decision.action_id,
                decision.reason.as_deref(),
            )
            .await?;

        if let Some(recipient) = inbound.thread_id.as_ref() {
            if inbound.channel_id.as_str().eq_ignore_ascii_case("telegram") {
                if let Some(callback_message_id) =
                    parse_telegram_callback_message_id(&inbound.metadata)
                {
                    let resolution =
                        render_telegram_approval_resolution(decision.decision, decision.action_id);
                    let metadata = serde_json::json!({
                        TELEGRAM_EDIT_MESSAGE_ID_KEY: callback_message_id,
                        TELEGRAM_CLEAR_REPLY_MARKUP_KEY: true,
                    });
                    if let Err(error) = channel
                        .send(
                            recipient.as_str(),
                            OutboundMessage {
                                content: resolution,
                                reply_to_message_id: None,
                                attachments: vec![],
                                metadata,
                            },
                        )
                        .await
                    {
                        tracing::warn!(
                            %error,
                            channel_id = %inbound.channel_id,
                            sender_id = %inbound.sender_id,
                            callback_message_id,
                            "failed to update telegram approval prompt state"
                        );
                    }
                }
            }

            let reply_to_message_id =
                if inbound.channel_id.as_str().eq_ignore_ascii_case("telegram") {
                    parse_telegram_callback_message_id(&inbound.metadata)
                        .map(|value| value.to_string().into())
                        .or_else(|| Some(inbound.message_id.clone()))
                } else {
                    Some(inbound.message_id.clone())
                };
            channel
                .send(
                    recipient.as_str(),
                    OutboundMessage {
                        content: reply,
                        reply_to_message_id,
                        attachments: vec![],
                        metadata: serde_json::Value::Null,
                    },
                )
                .await?;
        }

        tracing::info!(
            channel_id = %inbound.channel_id,
            sender_id = %inbound.sender_id,
            action_id = ?decision.action_id,
            decision = ?decision.decision,
            "action decision handled out-of-band"
        );
        Ok(true)
    }

    async fn dispatch_inbound(
        self: &Arc<Self>,
        inbound: InboundMessage,
        shutdown: &CancellationToken,
    ) -> Result<()> {
        let lane_key = format!("{}::{}", inbound.channel_id, inbound.sender_id);
        let mut created_lane = false;
        let lane_handle = {
            let mut queues = self.lane_queues.lock().await;
            if let Some(existing) = queues.get(&lane_key) {
                existing.clone()
            } else {
                let (tx, rx) = mpsc::channel(self.cfg.queue.lane_buffer);
                let (interrupt_tx, interrupt_rx) = watch::channel(0_u64);
                let handle = LaneHandle {
                    tx: tx.clone(),
                    interrupt_tx,
                };
                queues.insert(lane_key.clone(), handle.clone());
                created_lane = true;
                let gateway = self.clone();
                let lane_key_for_worker = lane_key.clone();
                let lane_shutdown = shutdown.child_token();
                tokio::spawn(async move {
                    if let Err(e) = gateway
                        .run_lane_loop(lane_key_for_worker.clone(), rx, interrupt_rx, lane_shutdown)
                        .await
                    {
                        tracing::error!(lane = %lane_key_for_worker, %e, "lane worker exited");
                    }
                });
                handle
            }
        };

        if created_lane {
            tracing::info!(lane = %lane_key, "lane created");
        } else {
            tracing::debug!(lane = %lane_key, "lane reused");
        }

        if self.cfg.queue.mode == QueueMode::Interrupt
            && inbound.kind == InboundMessageKind::Message
        {
            let next_interrupt_seq = (*lane_handle.interrupt_tx.borrow()).saturating_add(1);
            lane_handle
                .interrupt_tx
                .send(next_interrupt_seq)
                .map_err(|e| anyhow::anyhow!("interrupt signal dispatch failed: {e}"))?;
            tracing::debug!(
                lane = %lane_key,
                interrupt_seq = next_interrupt_seq,
                "lane interrupt signal emitted"
            );
        }

        lane_handle
            .tx
            .send(inbound)
            .await
            .map_err(|e| anyhow::anyhow!("lane dispatch failed: {e}"))?;
        tracing::debug!(lane = %lane_key, "inbound message enqueued to lane");
        Ok(())
    }

    async fn run_lane_loop(
        self: Arc<Self>,
        lane_key: String,
        mut lane_rx: mpsc::Receiver<InboundMessage>,
        mut interrupt_rx: watch::Receiver<u64>,
        shutdown: CancellationToken,
    ) -> Result<()> {
        tracing::info!(lane = %lane_key, queue_mode = ?self.cfg.queue.mode, "lane worker started");
        let mut pending = VecDeque::<InboundMessage>::new();
        loop {
            let next_message = tokio::select! {
                _ = shutdown.cancelled() => None,
                msg = next_lane_message(&mut lane_rx, &mut pending) => msg,
            };
            let Some(first) = next_message else {
                break;
            };
            let (first, debounced_count) = debounce_lane_messages(
                self.cfg.queue.debounce_ms,
                first,
                &mut lane_rx,
                &mut pending,
            )
            .await;
            let (inbound, reshaped_count) =
                prepare_lane_message(self.cfg.queue.mode, first, &mut lane_rx, &mut pending);
            let permit = tokio::select! {
                _ = shutdown.cancelled() => None,
                permit = self.worker_budget.acquire() => permit.ok(),
            };
            let Some(_permit) = permit else {
                tracing::warn!(lane = %lane_key, "worker budget closed or lane shutdown triggered");
                break;
            };
            tracing::debug!(
                lane = %lane_key,
                message_id = %inbound.message_id,
                queue_mode = ?self.cfg.queue.mode,
                debounced_count,
                reshaped_count,
                "lane dequeued inbound message"
            );
            let outcome = self.handle_inbound(inbound, &mut interrupt_rx).await?;
            if outcome == HandleInboundOutcome::Nuked {
                drain_lane_receiver(&mut lane_rx, &mut pending);
                pending.clear();
                tracing::info!(lane = %lane_key, "lane backlog purged by /nuke command");
            }
            if outcome == HandleInboundOutcome::Interrupted {
                tracing::warn!(lane = %lane_key, "assistant run interrupted by newer lane message");
            }
        }
        if shutdown.is_cancelled() {
            tracing::info!(lane = %lane_key, "lane worker stopped by shutdown signal");
        }
        tracing::info!(lane = %lane_key, "lane worker exited");
        Ok(())
    }

    fn is_interrupt_mode(&self) -> bool {
        self.cfg.queue.mode == QueueMode::Interrupt
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            channel_id = %inbound.channel_id,
            sender_id = %inbound.sender_id,
            message_id = %inbound.message_id,
            inbound_kind = ?inbound.kind
        )
    )]
    async fn handle_inbound(
        &self,
        inbound: InboundMessage,
        interrupt_rx: &mut watch::Receiver<u64>,
    ) -> Result<HandleInboundOutcome> {
        if !pairing::is_allowed(
            &self.cfg,
            inbound.channel_id.as_str(),
            inbound.sender_id.as_str(),
        ) {
            tracing::warn!("inbound rejected by pairing policy");
            return Ok(HandleInboundOutcome::Completed);
        }

        if inbound.kind == InboundMessageKind::Reaction {
            tracing::info!("forwarding reaction to assistant feedback handler");
            self.assistant.on_reaction(&inbound).await?;
            return Ok(HandleInboundOutcome::Completed);
        }

        let channel = self
            .channels
            .get(inbound.channel_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("unknown channel: {}", inbound.channel_id))?
            .clone();

        let mut active_channels: Vec<String> = self.channels.keys().cloned().collect();
        active_channels.sort();
        let recipient = inbound
            .thread_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("inbound message missing thread_id"))?;

        let uptime = self.started_at.elapsed();
        let mut session = self
            .sessions
            .get_or_create_mut(inbound.channel_id.as_str(), inbound.sender_id.as_str());
        tracing::debug!(
            uptime_seconds = uptime.as_secs(),
            active_channels = ?active_channels,
            "session loaded for inbound message"
        );

        if let Some(reply) = commands::handle_command(
            &self.cfg,
            &mut session,
            &inbound.content,
            uptime,
            &active_channels,
        ) {
            let is_nuke = commands::is_nuke_command(&inbound.content);
            drop(session);
            let mut reply = reply;
            if is_nuke {
                let removed_session = self
                    .sessions
                    .clear_scope(inbound.channel_id.as_str(), inbound.sender_id.as_str())
                    .await?;
                let nuked_actions = self
                    .assistant
                    .nuke_pending_actions_for_sender(
                        inbound.channel_id.as_str(),
                        inbound.sender_id.as_str(),
                    )
                    .await?;
                reply = format!(
                    "Context nuked. session_removed={removed_session} pending_actions_denied={nuked_actions} lane_backlog_purged=true"
                );
            } else {
                self.sessions.persist().await?;
            }
            tracing::info!(
                is_nuke,
                reply_len = reply.len(),
                "command handled inbound message without assistant run"
            );
            channel
                .send(
                    recipient.as_str(),
                    OutboundMessage {
                        content: reply,
                        reply_to_message_id: Some(inbound.message_id.clone()),
                        attachments: vec![],
                        metadata: serde_json::Value::Null,
                    },
                )
                .await?;
            return Ok(if is_nuke {
                HandleInboundOutcome::Nuked
            } else {
                HandleInboundOutcome::Completed
            });
        }

        session.last_user_message_id = Some(inbound.message_id.to_string());
        session.last_active = chrono::Utc::now();
        let typing_heartbeat = if channel.supports_typing_events() {
            tracing::debug!("typing indicator enabled");
            if let Err(error) = channel.send_typing(recipient.as_str(), true).await {
                tracing::warn!(
                    %error,
                    channel_id = %inbound.channel_id,
                    "failed to send initial typing indicator"
                );
            }

            let cancel = CancellationToken::new();
            let cancel_child = cancel.child_token();
            let channel_typing = channel.clone();
            let recipient_typing = recipient.clone();
            let channel_id = inbound.channel_id.to_string();
            let heartbeat = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(4));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await;
                loop {
                    tokio::select! {
                        _ = cancel_child.cancelled() => break,
                        _ = ticker.tick() => {
                            if let Err(error) = channel_typing.send_typing(recipient_typing.as_str(), true).await {
                                tracing::warn!(
                                    %error,
                                    channel_id = %channel_id,
                                    "typing heartbeat send failed"
                                );
                            }
                        }
                    }
                }
            });
            Some((cancel, heartbeat))
        } else {
            None
        };

        let supports_streaming = channel.supports_streaming_deltas();
        tracing::info!(supports_streaming, "channel capability evaluated");
        let (stream_tx, stream_task) = if supports_streaming {
            let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
            let channel_stream = channel.clone();
            let recipient_stream = recipient.clone();
            let handle = tokio::spawn(async move {
                while let Some(delta) = delta_rx.recv().await {
                    channel_stream
                        .send_delta(recipient_stream.as_str(), &delta)
                        .await?;
                }
                Ok::<(), anyhow::Error>(())
            });
            (Some(delta_tx), Some(handle))
        } else {
            (None, None)
        };

        let assistant_started = Instant::now();
        tracing::info!(
            interrupt_mode = self.is_interrupt_mode(),
            "assistant run started"
        );
        let response = if self.is_interrupt_mode() {
            let observed_interrupt_seq = *interrupt_rx.borrow_and_update();
            let run_fut = self.assistant.run(
                inbound.channel_id.as_str(),
                inbound.sender_id.as_str(),
                Some(recipient.as_str()),
                &mut session,
                &inbound.content,
                stream_tx,
            );
            tokio::pin!(run_fut);
            tokio::select! {
                run_result = &mut run_fut => AssistantRunOutcome::Completed(run_result),
                interrupt_result = wait_for_interrupt_signal(interrupt_rx, observed_interrupt_seq) => {
                    interrupt_result?;
                    AssistantRunOutcome::Interrupted
                },
            }
        } else {
            AssistantRunOutcome::Completed(
                self.assistant
                    .run(
                        inbound.channel_id.as_str(),
                        inbound.sender_id.as_str(),
                        Some(recipient.as_str()),
                        &mut session,
                        &inbound.content,
                        stream_tx,
                    )
                    .await,
            )
        };
        drop(session);
        self.sessions.persist().await?;
        tracing::info!(
            latency_ms = assistant_started.elapsed().as_millis() as u64,
            assistant_outcome = ?response,
            "assistant run completed"
        );
        if let Some(stream_task) = stream_task {
            stream_task
                .await
                .map_err(|e| anyhow::anyhow!("delta stream task join failed: {e}"))??;
            tracing::debug!("streaming delta task drained");
        }
        if let Some((typing_cancel, typing_task)) = typing_heartbeat {
            typing_cancel.cancel();
            if let Err(error) = typing_task.await {
                tracing::warn!(
                    %error,
                    channel_id = %inbound.channel_id,
                    "typing heartbeat join failed"
                );
            }
            tracing::debug!("typing indicator disabled");
            if let Err(error) = channel.send_typing(recipient.as_str(), false).await {
                tracing::warn!(
                    %error,
                    channel_id = %inbound.channel_id,
                    "failed to disable typing indicator"
                );
            }
        }
        let response = match response {
            AssistantRunOutcome::Completed(Ok(response)) => response,
            AssistantRunOutcome::Completed(Err(error)) => {
                let fallback = user_visible_assistant_error(&error);
                tracing::error!(
                    %error,
                    channel_id = %inbound.channel_id,
                    sender_id = %inbound.sender_id,
                    "assistant run failed; returning fallback error reply"
                );
                if let Err(send_error) = channel
                    .send(
                        recipient.as_str(),
                        OutboundMessage {
                            content: fallback,
                            reply_to_message_id: Some(inbound.message_id.clone()),
                            attachments: vec![],
                            metadata: serde_json::Value::Null,
                        },
                    )
                    .await
                {
                    tracing::error!(
                        %send_error,
                        channel_id = %inbound.channel_id,
                        sender_id = %inbound.sender_id,
                        "failed to deliver assistant error reply"
                    );
                }
                return Ok(HandleInboundOutcome::Completed);
            }
            AssistantRunOutcome::Interrupted => {
                tracing::warn!("assistant run interrupted by queue mode");
                return Ok(HandleInboundOutcome::Interrupted);
            }
        };
        let response = if response.trim().is_empty() {
            tracing::warn!(
                channel_id = %inbound.channel_id,
                sender_id = %inbound.sender_id,
                "assistant returned empty response; sending fallback message"
            );
            "I couldn't produce a response message for that request. Please retry. If this follows a tool approval prompt, approve/deny it first and resend your request.".to_string()
        } else {
            response
        };
        tracing::info!(response_len = response.len(), "sending assistant response");

        if let Err(error) = channel
            .send(
                recipient.as_str(),
                OutboundMessage {
                    content: response,
                    reply_to_message_id: Some(inbound.message_id.clone()),
                    attachments: vec![],
                    metadata: serde_json::Value::Null,
                },
            )
            .await
        {
            tracing::error!(
                %error,
                channel_id = %inbound.channel_id,
                sender_id = %inbound.sender_id,
                "failed to deliver assistant response"
            );
            return Ok(HandleInboundOutcome::Completed);
        }
        tracing::info!("assistant response delivered");

        Ok(HandleInboundOutcome::Completed)
    }
}

#[derive(Debug)]
enum AssistantRunOutcome {
    Completed(Result<String>),
    Interrupted,
}

#[derive(Debug, Clone)]
struct ParsedActionDecision {
    decision: ActionDecision,
    action_id: Option<Uuid>,
    reason: Option<String>,
}

fn parse_telegram_callback_message_id(metadata: &serde_json::Value) -> Option<i64> {
    metadata
        .get("message")
        .and_then(|value| value.get("message_id"))
        .and_then(|value| {
            value.as_i64().or_else(|| {
                value
                    .as_str()
                    .and_then(|raw| raw.trim().parse::<i64>().ok())
            })
        })
}

fn render_telegram_approval_resolution(
    decision: ActionDecision,
    action_id: Option<Uuid>,
) -> String {
    let decision_label = match decision {
        ActionDecision::Approve => "APPROVED",
        ActionDecision::Deny => "DENIED",
    };
    let action_label = action_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "Approval {decision_label}\nAction ID: `{action_label}`\nStatus: closed (this prompt is no longer interactive)."
    )
}

fn parse_action_decision_command(content: &str) -> Option<ParsedActionDecision> {
    let trimmed = content.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let command = parts.next()?.to_ascii_lowercase();
    let decision = match command.as_str() {
        "/approve" | "/approve-action" => ActionDecision::Approve,
        "/deny" | "/deny-action" => ActionDecision::Deny,
        _ => return None,
    };

    let remaining: Vec<&str> = parts.collect();
    let requires_action_id = matches!(command.as_str(), "/approve-action" | "/deny-action");
    let action_id = if requires_action_id {
        Some(Uuid::parse_str(remaining.first()?).ok()?)
    } else {
        remaining
            .first()
            .and_then(|candidate| Uuid::parse_str(candidate).ok())
    };
    let reason = match decision {
        ActionDecision::Approve => None,
        ActionDecision::Deny => {
            let start_idx = usize::from(action_id.is_some());
            let text = remaining
                .iter()
                .skip(start_idx)
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if text.is_empty() { None } else { Some(text) }
        }
    };

    Some(ParsedActionDecision {
        decision,
        action_id,
        reason,
    })
}

async fn wait_for_interrupt_signal(
    interrupt_rx: &mut watch::Receiver<u64>,
    observed_seq: u64,
) -> Result<()> {
    loop {
        interrupt_rx
            .changed()
            .await
            .map_err(|_| anyhow::anyhow!("lane interrupt channel closed"))?;
        let next_seq = *interrupt_rx.borrow_and_update();
        if next_seq > observed_seq {
            return Ok(());
        }
    }
}

async fn next_lane_message(
    lane_rx: &mut mpsc::Receiver<InboundMessage>,
    pending: &mut VecDeque<InboundMessage>,
) -> Option<InboundMessage> {
    if let Some(msg) = pending.pop_front() {
        return Some(msg);
    }
    lane_rx.recv().await
}

async fn debounce_lane_messages(
    debounce_ms: u64,
    mut first: InboundMessage,
    lane_rx: &mut mpsc::Receiver<InboundMessage>,
    pending: &mut VecDeque<InboundMessage>,
) -> (InboundMessage, usize) {
    if debounce_ms == 0 || first.kind != InboundMessageKind::Message {
        return (first, 0);
    }

    let debounce_window = Duration::from_millis(debounce_ms);
    let deadline = tokio::time::Instant::now() + debounce_window;
    let mut buffered = 0usize;

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        match tokio::time::timeout(remaining, lane_rx.recv()).await {
            Ok(Some(next)) => {
                if next.kind == InboundMessageKind::Message {
                    buffered = buffered.saturating_add(1);
                }
                pending.push_back(next);
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    if buffered > 0 {
        attach_queue_metadata(&mut first, "debounced_messages", buffered as u64 + 1);
    }
    (first, buffered)
}

fn prepare_lane_message(
    mode: QueueMode,
    first: InboundMessage,
    lane_rx: &mut mpsc::Receiver<InboundMessage>,
    pending: &mut VecDeque<InboundMessage>,
) -> (InboundMessage, usize) {
    drain_lane_receiver(lane_rx, pending);
    match mode {
        QueueMode::Followup => (first, 0),
        QueueMode::Collect => collect_lane_messages(first, pending),
        QueueMode::Steer | QueueMode::Interrupt => latest_message_wins(first, pending),
    }
}

fn drain_lane_receiver(
    lane_rx: &mut mpsc::Receiver<InboundMessage>,
    pending: &mut VecDeque<InboundMessage>,
) {
    loop {
        match lane_rx.try_recv() {
            Ok(msg) => pending.push_back(msg),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
}

fn collect_lane_messages(
    first: InboundMessage,
    pending: &mut VecDeque<InboundMessage>,
) -> (InboundMessage, usize) {
    if first.kind != InboundMessageKind::Message {
        return (first, 0);
    }

    let mut merged = first;
    let mut absorbed = 0usize;
    let mut retained = VecDeque::new();
    while let Some(next) = pending.pop_front() {
        if next.kind != InboundMessageKind::Message {
            retained.push_back(next);
            continue;
        }
        absorbed = absorbed.saturating_add(1);
        if !merged.content.is_empty() {
            merged.content.push('\n');
        }
        merged.content.push_str(next.content.trim());
        merged.message_id = next.message_id;
        merged.received_at = next.received_at;
        if next.thread_id.is_some() {
            merged.thread_id = next.thread_id;
        }
    }
    *pending = retained;
    if absorbed > 0 {
        attach_queue_metadata(&mut merged, "collected_messages", absorbed as u64 + 1);
    }
    (merged, absorbed)
}

fn latest_message_wins(
    first: InboundMessage,
    pending: &mut VecDeque<InboundMessage>,
) -> (InboundMessage, usize) {
    if first.kind != InboundMessageKind::Message {
        return (first, 0);
    }

    let mut latest = first;
    let mut dropped = 0usize;
    let mut retained = VecDeque::new();
    while let Some(next) = pending.pop_front() {
        if next.kind == InboundMessageKind::Message {
            latest = next;
            dropped = dropped.saturating_add(1);
        } else {
            retained.push_back(next);
        }
    }
    *pending = retained;
    if dropped > 0 {
        attach_queue_metadata(&mut latest, "dropped_messages", dropped as u64);
    }
    (latest, dropped)
}

fn attach_queue_metadata(message: &mut InboundMessage, key: &str, value: u64) {
    let mut map = match std::mem::take(&mut message.metadata) {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    map.insert(
        format!("queue_{key}"),
        serde_json::Value::Number(serde_json::Number::from(value)),
    );
    message.metadata = serde_json::Value::Object(map);
}

fn user_visible_assistant_error(error: &Error) -> String {
    let normalized = error.to_string().to_ascii_lowercase();
    if normalized.contains("429 too many requests") || normalized.contains("rate_limit") {
        return "I hit the AI provider rate limit for this request. Please retry in a few seconds. If it keeps happening, send /nuke to reset this chat context and reduce prompt size.".to_string();
    }

    if normalized.contains("timeout") || normalized.contains("timed out") {
        return "The request timed out before I could finish. Please retry once. If it keeps timing out, send /nuke to reset this chat context and try a shorter request.".to_string();
    }

    "I hit an internal error while processing that request. Please retry once. If it keeps failing, send /nuke to reset this chat context and try again.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn inbound(kind: InboundMessageKind, message_id: &str, content: &str) -> InboundMessage {
        InboundMessage {
            kind,
            message_id: message_id.into(),
            channel_id: "webchat".into(),
            sender_id: "user-1".into(),
            thread_id: Some("thread-1".into()),
            is_group: false,
            content: content.to_string(),
            metadata: serde_json::Value::Null,
            received_at: Utc::now(),
        }
    }

    #[test]
    fn collect_mode_merges_message_burst() {
        let first = inbound(InboundMessageKind::Message, "m1", "first");
        let mut pending = VecDeque::from(vec![
            inbound(InboundMessageKind::Message, "m2", "second"),
            inbound(InboundMessageKind::Reaction, "r1", "👍"),
            inbound(InboundMessageKind::Message, "m3", "third"),
        ]);
        let (prepared, absorbed) = collect_lane_messages(first, &mut pending);

        assert_eq!(absorbed, 2);
        assert_eq!(prepared.message_id.as_str(), "m3");
        assert_eq!(prepared.content, "first\nsecond\nthird");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, InboundMessageKind::Reaction);
        assert_eq!(
            prepared
                .metadata
                .get("queue_collected_messages")
                .and_then(|v| v.as_u64()),
            Some(3)
        );
    }

    #[test]
    fn latest_mode_keeps_only_latest_message() {
        let first = inbound(InboundMessageKind::Message, "m1", "first");
        let mut pending = VecDeque::from(vec![
            inbound(InboundMessageKind::Message, "m2", "second"),
            inbound(InboundMessageKind::Reaction, "r1", "❤️"),
            inbound(InboundMessageKind::Message, "m3", "third"),
        ]);
        let (prepared, dropped) = latest_message_wins(first, &mut pending);

        assert_eq!(dropped, 2);
        assert_eq!(prepared.message_id.as_str(), "m3");
        assert_eq!(prepared.content, "third");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, InboundMessageKind::Reaction);
        assert_eq!(
            prepared
                .metadata
                .get("queue_dropped_messages")
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn non_message_is_never_reshaped() {
        let first = inbound(InboundMessageKind::Reaction, "r1", "👍");
        let mut pending = VecDeque::from(vec![inbound(InboundMessageKind::Message, "m1", "hello")]);
        let (prepared_collect, absorbed) = collect_lane_messages(first.clone(), &mut pending);
        assert_eq!(prepared_collect.kind, InboundMessageKind::Reaction);
        assert_eq!(absorbed, 0);
        assert_eq!(pending.len(), 1);

        let mut pending = VecDeque::from(vec![inbound(InboundMessageKind::Message, "m1", "hello")]);
        let (prepared_latest, dropped) = latest_message_wins(first, &mut pending);
        assert_eq!(prepared_latest.kind, InboundMessageKind::Reaction);
        assert_eq!(dropped, 0);
        assert_eq!(pending.len(), 1);
    }

    #[tokio::test]
    async fn interrupt_signal_waits_for_newer_sequence() {
        let (interrupt_tx, interrupt_rx) = watch::channel(5_u64);
        let waiter = tokio::spawn(async move {
            let mut rx = interrupt_rx;
            wait_for_interrupt_signal(&mut rx, 5).await
        });
        interrupt_tx.send(6).expect("send interrupt signal");
        waiter
            .await
            .expect("join waiter")
            .expect("interrupt waiter result");
    }

    #[tokio::test]
    async fn debounce_buffers_followup_messages_within_window() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut pending = VecDeque::new();
        let first = inbound(InboundMessageKind::Message, "m1", "first");

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            tx.send(inbound(InboundMessageKind::Message, "m2", "second"))
                .await
                .expect("send second message");
        });

        let (debounced, buffered) = debounce_lane_messages(50, first, &mut rx, &mut pending).await;
        assert_eq!(buffered, 1);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message_id.as_str(), "m2");
        assert_eq!(
            debounced
                .metadata
                .get("queue_debounced_messages")
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn parse_action_decision_command_supports_explicit_ids() {
        let action_id = Uuid::new_v4();
        let parsed = parse_action_decision_command(&format!("/approve-action {action_id}"))
            .expect("parse approve-action");
        assert_eq!(parsed.decision, ActionDecision::Approve);
        assert_eq!(parsed.action_id, Some(action_id));
    }

    #[test]
    fn parse_action_decision_command_supports_latest_pending_shortcut() {
        let parsed = parse_action_decision_command("/deny this is unsafe")
            .expect("parse deny without explicit id");
        assert_eq!(parsed.decision, ActionDecision::Deny);
        assert_eq!(parsed.action_id, None);
        assert_eq!(parsed.reason.as_deref(), Some("this is unsafe"));
    }

    #[test]
    fn parse_telegram_callback_message_id_extracts_nested_message_id() {
        let metadata = serde_json::json!({
            "id": "callback-id",
            "message": {
                "message_id": 12345
            }
        });
        assert_eq!(parse_telegram_callback_message_id(&metadata), Some(12345));
    }

    #[test]
    fn render_telegram_approval_resolution_marks_prompt_closed() {
        let action_id = Uuid::new_v4();
        let rendered =
            render_telegram_approval_resolution(ActionDecision::Approve, Some(action_id));
        assert!(rendered.contains("APPROVED"));
        assert!(rendered.contains("closed"));
        assert!(rendered.contains(&action_id.to_string()));
    }

    #[test]
    fn user_visible_assistant_error_handles_rate_limit() {
        let err = anyhow::anyhow!("anthropic stream status=429 Too Many Requests rate_limit_error");
        let msg = user_visible_assistant_error(&err);
        assert!(msg.contains("rate limit"));
        assert!(msg.contains("/nuke"));
    }

    #[test]
    fn user_visible_assistant_error_handles_timeout() {
        let err = anyhow::anyhow!("request timed out while waiting for response");
        let msg = user_visible_assistant_error(&err);
        assert!(msg.contains("timed out"));
        assert!(msg.contains("/nuke"));
    }
}
