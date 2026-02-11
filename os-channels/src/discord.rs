use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio_tungstenite::tungstenite::Message;

const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const DISCORD_DEFAULT_INTENTS: u64 = (1 << 9) | (1 << 15);

#[derive(Clone)]
pub struct DiscordAdapter {
    http: reqwest::Client,
    bot_token: String,
    gateway_intents: u64,
    require_mention_in_group_chats: bool,
}

impl DiscordAdapter {
    pub fn new(bot_token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            bot_token: bot_token.to_string(),
            gateway_intents: DISCORD_DEFAULT_INTENTS,
            require_mention_in_group_chats: true,
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("https://discord.com/api/v10{path}")
    }

    pub fn with_gateway_intents(mut self, gateway_intents: u64) -> Self {
        self.gateway_intents = gateway_intents;
        self
    }

    pub fn with_require_mention_in_group_chats(
        mut self,
        require_mention_in_group_chats: bool,
    ) -> Self {
        self.require_mention_in_group_chats = require_mention_in_group_chats;
        self
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
        let gateway_intents = self.gateway_intents;
        let require_mention_in_group_chats = self.require_mention_in_group_chats;
        tokio::spawn(async move {
            let adapter = DiscordAdapter {
                http,
                bot_token: token,
                gateway_intents,
                require_mention_in_group_chats,
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
            let text = resp.text().await?;
            return Err(anyhow::anyhow!(
                "discord send failed: status={status} body={text}"
            ));
        }
        Ok(())
    }
}

impl DiscordAdapter {
    async fn run_gateway_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        self.run_gateway_once(tx).await
    }

    async fn run_gateway_once(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let (ws, _) = tokio_tungstenite::connect_async(DISCORD_GATEWAY_URL).await?;
        let (write, mut read) = ws.split();
        let write = Arc::new(Mutex::new(write));

        // HELLO.
        let heartbeat_interval_ms: u64 = if let Some(msg) = read.next().await {
            let msg = msg?;
            let v: serde_json::Value = serde_json::from_str(msg.to_text()?)?;
            v.get("d")
                .and_then(|d| d.get("heartbeat_interval"))
                .and_then(|x| x.as_u64())
                .ok_or_else(|| anyhow::anyhow!("discord HELLO missing heartbeat_interval"))?
        } else {
            return Err(anyhow::anyhow!("discord gateway closed before HELLO"));
        };

        // IDENTIFY.
        let identify = serde_json::json!({
            "op": 2,
            "d": {
                "token": format!("Bot {}", self.bot_token),
                "intents": self.gateway_intents,
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

            let op = v
                .get("op")
                .and_then(|o| o.as_i64())
                .ok_or_else(|| anyhow::anyhow!("discord payload missing op"))?;
            if op == 11 {
                continue;
            }

            let t = v.get("t").and_then(|t| t.as_str());
            match t {
                Some("READY") => {
                    let id = v
                        .get("d")
                        .and_then(|d| d.get("user"))
                        .and_then(|u| u.get("id"))
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string());
                    *bot_user_id.write().await = id;
                }
                Some("MESSAGE_CREATE") => {
                    let event_payload = v
                        .get("d")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("discord MESSAGE_CREATE missing payload"))?;
                    let event: DiscordMessageCreate = serde_json::from_value(event_payload)?;
                    if event.author.bot {
                        continue;
                    }

                    let is_group = event.guild_id.is_some();
                    if is_group && self.require_mention_in_group_chats {
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

                    let metadata = serde_json::to_value(&event)?;
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Message,
                        message_id: event.id.into(),
                        channel_id: "discord".into(),
                        sender_id: event.author.id.into(),
                        thread_id: Some(event.channel_id.into()),
                        is_group,
                        content: event.content,
                        metadata,
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow::anyhow!("discord inbound queue closed: {e}"))?;
                }
                Some(_) | None => {}
            }
        }

        Err(anyhow::anyhow!("discord gateway stream ended unexpectedly"))
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
    bot: bool,
}
