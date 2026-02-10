use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Clone)]
pub struct TelegramAdapter {
    http: reqwest::Client,
    bot_token: String,
}

impl TelegramAdapter {
    pub fn new(bot_token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            bot_token: bot_token.to_string(),
        })
    }

    fn api_url(&self, method: &str) -> Result<Url> {
        Ok(Url::parse(&format!(
            "https://api.telegram.org/bot{}/{}",
            self.bot_token, method
        ))?)
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn channel_id(&self) -> &str {
        "telegram"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let http = self.http.clone();
        let token = self.bot_token.clone();
        tokio::spawn(async move {
            let adapter = TelegramAdapter {
                http,
                bot_token: token,
            };
            if let Err(e) = adapter.run_poll_loop(tx).await {
                tracing::error!(%e, "telegram poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let url = self.api_url("sendMessage")?;
        let body = serde_json::json!({
            "chat_id": recipient_id,
            "text": message.content,
        });
        let resp = self.http.post(url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow::anyhow!(
                "telegram send failed: status={status} body={text}"
            ));
        }
        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl TelegramAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut offset: i64 = 0;

        loop {
            let url = self.api_url("getUpdates")?;
            let resp = self
                .http
                .get(url)
                .query(&[
                    ("timeout", "30"),
                    ("offset", &offset.to_string()),
                    ("allowed_updates", r#"[\"message\",\"message_reaction\"]"#),
                ])
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await?;
                return Err(anyhow::anyhow!(
                    "telegram getUpdates failed: status={status} body={text}"
                ));
            }

            let parsed: TelegramGetUpdatesResponse = resp.json().await?;
            for update in parsed.result {
                offset = update.update_id + 1;

                if let Some(m) = update.message {
                    let text = m
                        .text
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("telegram message missing text"))?;
                    let is_group = m.chat.r#type != "private";
                    let sender_id = m
                        .from
                        .as_ref()
                        .map(|f| f.id.to_string())
                        .ok_or_else(|| anyhow::anyhow!("telegram message missing sender"))?;
                    let metadata = serde_json::to_value(&m)?;
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Message,
                        message_id: m.message_id.to_string().into(),
                        channel_id: "telegram".into(),
                        sender_id: sender_id.into(),
                        thread_id: Some(m.chat.id.to_string().into()),
                        is_group,
                        content: text.to_string(),
                        metadata,
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow::anyhow!("telegram inbound queue closed: {e}"))?;
                }

                if let Some(r) = update.message_reaction {
                    let sender_id = r
                        .user
                        .as_ref()
                        .map(|u| u.id.to_string())
                        .ok_or_else(|| anyhow::anyhow!("telegram reaction missing sender"))?;
                    let emoji = r
                        .new_reaction
                        .first()
                        .and_then(|x| x.emoji.clone())
                        .ok_or_else(|| anyhow::anyhow!("telegram reaction missing emoji"))?;
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Reaction,
                        message_id: Uuid::new_v4().to_string().into(),
                        channel_id: "telegram".into(),
                        sender_id: sender_id.into(),
                        thread_id: Some(r.chat.id.to_string().into()),
                        is_group: r.chat.r#type != "private",
                        content: emoji,
                        metadata: serde_json::to_value(&r)?,
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow::anyhow!("telegram inbound queue closed: {e}"))?;
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct TelegramGetUpdatesResponse {
    #[serde(default)]
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
    #[serde(default)]
    message_reaction: Option<TelegramMessageReaction>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramMessage {
    message_id: i64,
    #[serde(default)]
    from: Option<TelegramUser>,
    chat: TelegramChat,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramMessageReaction {
    chat: TelegramChat,
    #[serde(default)]
    user: Option<TelegramUser>,
    #[serde(default)]
    new_reaction: Vec<TelegramReaction>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramReaction {
    #[serde(default)]
    emoji: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramUser {
    id: i64,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    r#type: String,
}
