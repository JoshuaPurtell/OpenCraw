//! Session multiplexer: all channel adapters feed into a single inbound queue.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::assistant::AssistantAgent;
use crate::commands;
use crate::config::OpenShellConfig;
use crate::pairing;
use crate::session::SessionManager;
use anyhow::Result;
use os_channels::{ChannelAdapter, InboundMessage, InboundMessageKind, OutboundMessage};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Gateway {
    cfg: OpenShellConfig,
    started_at: Instant,
    sessions: Arc<SessionManager>,
    assistant: Arc<AssistantAgent>,
    channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    inbound_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<InboundMessage>>>,
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
        Self {
            cfg,
            started_at,
            sessions,
            assistant,
            channels,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
        }
    }

    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            if let Err(e) = self.run_loop().await {
                tracing::error!(%e, "gateway loop exited");
            }
        });
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn run_loop(&self) -> Result<()> {
        loop {
            let msg = {
                let mut rx = self.inbound_rx.lock().await;
                rx.recv().await
            };
            let Some(inbound) = msg else {
                return Ok(());
            };

            if let Err(e) = self.handle_inbound(inbound).await {
                tracing::warn!(%e, "handle_inbound failed");
            }
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn handle_inbound(&self, inbound: InboundMessage) -> Result<()> {
        if !pairing::is_allowed(&self.cfg, &inbound.channel_id, &inbound.sender_id) {
            return Ok(());
        }

        if inbound.kind == InboundMessageKind::Reaction {
            self.assistant.on_reaction(&inbound).await?;
            return Ok(());
        }

        let channel = self
            .channels
            .get(&inbound.channel_id)
            .ok_or_else(|| anyhow::anyhow!("unknown channel: {}", inbound.channel_id))?
            .clone();

        let mut active_channels: Vec<String> = self.channels.keys().cloned().collect();
        active_channels.sort();

        let uptime = self.started_at.elapsed();
        let mut session = self
            .sessions
            .get_or_create_mut(&inbound.channel_id, &inbound.sender_id);

        if let Some(reply) = commands::handle_command(
            &self.cfg,
            &mut session,
            &inbound.content,
            uptime,
            &active_channels,
        ) {
            channel
                .send(
                    inbound.thread_id.as_deref().unwrap_or(&inbound.sender_id),
                    OutboundMessage {
                        content: reply,
                        reply_to_message_id: Some(inbound.message_id),
                        attachments: vec![],
                    },
                )
                .await?;
            return Ok(());
        }

        session.last_user_message_id = Some(inbound.message_id.clone());
        session.last_active = chrono::Utc::now();

        let response = match self
            .assistant
            .run(
                &inbound.channel_id,
                &inbound.sender_id,
                &mut session,
                &inbound.content,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(%e, "assistant.run failed");
                format!("Error: {e}")
            }
        };

        channel
            .send(
                inbound.thread_id.as_deref().unwrap_or(&inbound.sender_id),
                OutboundMessage {
                    content: response,
                    reply_to_message_id: Some(inbound.message_id),
                    attachments: vec![],
                },
            )
            .await?;

        Ok(())
    }
}
