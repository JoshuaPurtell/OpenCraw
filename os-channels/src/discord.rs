use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;

const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

#[derive(Clone)]
pub struct DiscordAdapter {
    http: reqwest::Client,
    bot_token: String,
}

impl DiscordAdapter {
    pub fn new(bot_token: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        %e,
                        "reqwest client build failed; falling back to default client"
                    );
                    reqwest::Client::new()
                }),
            bot_token: bot_token.to_string(),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("https://discord.com/api/v10{path}")
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for DiscordAdapter {
    fn channel_id(&self) -> &str {
        "discord"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let http = self.http.clone();
        let token = self.bot_token.clone();
        tokio::spawn(async move {
            let adapter = DiscordAdapter {
                http,
                bot_token: token,
            };
            if let Err(e) = adapter.run_gateway_loop(tx).await {
                tracing::error!(%e, "discord gateway loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let url = self.api_url(&format!("/channels/{recipient_id}/messages"));
        let body = serde_json::json!({ "content": message.content });
        let resp = self
            .http
            .post(url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!(%status, %text, "discord send failed");
        }
        Ok(())
    }
}

impl DiscordAdapter {
    async fn run_gateway_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut reconnects: usize = 0;
        loop {
            reconnects += 1;
            if let Err(e) = self.run_gateway_once(tx.clone()).await {
                tracing::warn!(%e, reconnects, "discord gateway failed; retrying");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    async fn run_gateway_once(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let (ws, _) = tokio_tungstenite::connect_async(DISCORD_GATEWAY_URL).await?;
        let (write, mut read) = ws.split();
        let write = Arc::new(Mutex::new(write));

        // HELLO.
        let mut heartbeat_interval_ms: u64 = 41_250;
        if let Some(msg) = read.next().await {
            let msg = msg?;
            let v: serde_json::Value = serde_json::from_str(msg.to_text()?)?;
            heartbeat_interval_ms = v
                .get("d")
                .and_then(|d| d.get("heartbeat_interval"))
                .and_then(|x| x.as_u64())
                .unwrap_or(heartbeat_interval_ms);
        }

        // IDENTIFY.
        let identify = serde_json::json!({
            "op": 2,
            "d": {
                "token": format!("Bot {}", self.bot_token),
                "intents": (1 << 9) | (1 << 15),
                "properties": { "os": "linux", "browser": "opencraw", "device": "opencraw" }
            }
        });
        write
            .lock()
            .await
            .send(Message::Text(identify.to_string().into()))
            .await?;

        let seq: Arc<RwLock<Option<i64>>> = Arc::new(RwLock::new(None));
        let bot_user_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

        // Heartbeat loop.
        {
            let write = write.clone();
            let seq = seq.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(heartbeat_interval_ms));
                loop {
                    interval.tick().await;
                    let s = *seq.read().await;
                    let payload = serde_json::json!({ "op": 1, "d": s });
                    if write
                        .lock()
                        .await
                        .send(Message::Text(payload.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }

        while let Some(msg) = read.next().await {
            let msg = msg?;
            let txt = msg.to_text()?;
            let v: serde_json::Value = serde_json::from_str(txt)?;

            if let Some(s) = v.get("s").and_then(|s| s.as_i64()) {
                *seq.write().await = Some(s);
            }

            let op = v.get("op").and_then(|o| o.as_i64()).unwrap_or(-1);
            if op == 11 {
                continue;
            }

            let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
            match t {
                "READY" => {
                    let id = v
                        .get("d")
                        .and_then(|d| d.get("user"))
                        .and_then(|u| u.get("id"))
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string());
                    *bot_user_id.write().await = id;
                }
                "MESSAGE_CREATE" => {
                    let event: DiscordMessageCreate = serde_json::from_value(
                        v.get("d").cloned().unwrap_or_else(|| serde_json::json!({})),
                    )?;
                    if event.author.bot.unwrap_or(false) {
                        continue;
                    }

                    let is_group = event.guild_id.is_some();
                    if is_group {
                        if let Some(bot_id) = bot_user_id.read().await.clone() {
                            let mention1 = format!("<@{bot_id}>");
                            let mention2 = format!("<@!{bot_id}>");
                            if !event.content.contains(&mention1)
                                && !event.content.contains(&mention2)
                            {
                                continue;
                            }
                        }
                    }

                    let metadata =
                        serde_json::to_value(&event).unwrap_or_else(|_| serde_json::json!({}));
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Message,
                        message_id: event.id,
                        channel_id: "discord".to_string(),
                        sender_id: event.author.id,
                        thread_id: Some(event.channel_id),
                        is_group,
                        content: event.content,
                        metadata,
                        received_at: Utc::now(),
                    };
                    let _ = tx.send(inbound).await;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct DiscordMessageCreate {
    id: String,
    channel_id: String,
    #[serde(default)]
    guild_id: Option<String>,
    #[serde(default)]
    content: String,
    author: DiscordAuthor,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct DiscordAuthor {
    id: String,
    #[serde(default)]
    bot: Option<bool>,
}
