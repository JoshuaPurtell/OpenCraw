use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use chrono::Utc;
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

const TELEGRAM_CHANNEL_ID: &str = "telegram";
const TELEGRAM_LONG_POLL_TIMEOUT_SECS: &str = "30";
const TELEGRAM_ALLOWED_UPDATES: &str = r#"[\"message\",\"message_reaction\"]"#;
const TELEGRAM_NON_TEXT_PLACEHOLDER: &str = "[telegram non-text message]";
const TELEGRAM_NON_TRANSIENT_DELAY: Duration = Duration::from_secs(10);
const TELEGRAM_RETRY_BASE_MS: u64 = 250;
const TELEGRAM_RETRY_MAX_MS: u64 = 30_000;

#[derive(Clone)]
pub struct TelegramAdapter {
    http: reqwest::Client,
    bot_token: String,
}

impl TelegramAdapter {
    pub fn new(bot_token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
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
        let mut consecutive_failures: u32 = 0;

        loop {
            let url = self.api_url("getUpdates")?;
            let response = match self
                .http
                .get(url)
                .query(&[
                    ("timeout", TELEGRAM_LONG_POLL_TIMEOUT_SECS),
                    ("offset", &offset.to_string()),
                    ("allowed_updates", TELEGRAM_ALLOWED_UPDATES),
                ])
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    consecutive_failures += 1;
                    let delay = transient_retry_delay(consecutive_failures);
                    tracing::warn!(
                        %error,
                        attempt = consecutive_failures,
                        ?delay,
                        "telegram getUpdates request failed; retrying with backoff"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_else(|error| {
                    format!("<failed to read telegram error body: {error}>")
                });
                if is_transient_status(status) {
                    consecutive_failures += 1;
                    let delay = transient_retry_delay(consecutive_failures);
                    tracing::warn!(
                        %status,
                        %body,
                        attempt = consecutive_failures,
                        ?delay,
                        "telegram getUpdates transient failure; retrying with backoff"
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    consecutive_failures = 0;
                    tracing::error!(
                        %status,
                        %body,
                        ?TELEGRAM_NON_TRANSIENT_DELAY,
                        "telegram getUpdates non-transient failure; keeping poll loop alive"
                    );
                    tokio::time::sleep(TELEGRAM_NON_TRANSIENT_DELAY).await;
                }
                continue;
            }

            let parsed = match response.json::<TelegramGetUpdatesResponse>().await {
                Ok(parsed) => parsed,
                Err(error) => {
                    consecutive_failures += 1;
                    let delay = transient_retry_delay(consecutive_failures);
                    tracing::warn!(
                        %error,
                        attempt = consecutive_failures,
                        ?delay,
                        "telegram getUpdates payload parse failed; retrying with backoff"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };

            consecutive_failures = 0;

            let mut updates = parsed.result;
            updates.sort_by_key(|update| update.update_id);
            for update in updates {
                // Advance offset before conversion to avoid poison-update replay loops.
                if update.update_id < offset {
                    continue;
                }
                offset = update.update_id.saturating_add(1);

                for inbound in build_inbound_messages(&update) {
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow::anyhow!("telegram inbound queue closed: {e}"))?;
                }
            }
        }
    }
}

fn transient_retry_delay(attempt: u32) -> Duration {
    let multiplier = 1_u64 << attempt.saturating_sub(1).min(10);
    Duration::from_millis((TELEGRAM_RETRY_BASE_MS * multiplier).min(TELEGRAM_RETRY_MAX_MS))
}

fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
}

fn build_inbound_messages(update: &TelegramUpdate) -> Vec<InboundMessage> {
    let mut inbound = Vec::new();
    if let Some(message) = build_message_inbound(update.update_id, update.message.as_ref()) {
        inbound.push(message);
    }
    if let Some(reaction) =
        build_reaction_inbound(update.update_id, update.message_reaction.as_ref())
    {
        inbound.push(reaction);
    }
    inbound
}

fn build_message_inbound(
    update_id: i64,
    message: Option<&TelegramMessage>,
) -> Option<InboundMessage> {
    let message = message?;
    let chat = message.chat.as_ref()?;
    let content = extract_message_content(message)?;
    let sender_id = message
        .from
        .as_ref()
        .map(|user| user.id.to_string())
        .unwrap_or_else(|| format!("chat:{}", chat.id));
    let message_id = message
        .message_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("update:{update_id}:message"));

    Some(InboundMessage {
        kind: InboundMessageKind::Message,
        message_id: message_id.into(),
        channel_id: TELEGRAM_CHANNEL_ID.into(),
        sender_id: sender_id.into(),
        thread_id: Some(chat.id.to_string().into()),
        is_group: chat.r#type != "private",
        content,
        metadata: serde_json::to_value(message).unwrap_or(serde_json::Value::Null),
        received_at: Utc::now(),
    })
}

fn build_reaction_inbound(
    update_id: i64,
    reaction: Option<&TelegramMessageReaction>,
) -> Option<InboundMessage> {
    let reaction = reaction?;
    let chat = reaction.chat.as_ref()?;
    let content = extract_reaction_content(reaction)?;
    let sender_id = reaction
        .user
        .as_ref()
        .map(|user| user.id.to_string())
        .or_else(|| {
            reaction
                .actor_chat
                .as_ref()
                .map(|actor_chat| format!("chat:{}", actor_chat.id))
        })
        .unwrap_or_else(|| format!("chat:{}", chat.id));

    Some(InboundMessage {
        kind: InboundMessageKind::Reaction,
        message_id: reaction_message_id(update_id, chat.id, reaction).into(),
        channel_id: TELEGRAM_CHANNEL_ID.into(),
        sender_id: sender_id.into(),
        thread_id: Some(chat.id.to_string().into()),
        is_group: chat.r#type != "private",
        content,
        metadata: serde_json::to_value(reaction).unwrap_or(serde_json::Value::Null),
        received_at: Utc::now(),
    })
}

fn extract_message_content(message: &TelegramMessage) -> Option<String> {
    if let Some(text) = message.text.as_deref().map(str::trim) {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    if let Some(caption) = message.caption.as_deref().map(str::trim) {
        if !caption.is_empty() {
            return Some(caption.to_string());
        }
    }
    if message.has_non_text_payload() {
        return Some(TELEGRAM_NON_TEXT_PLACEHOLDER.to_string());
    }
    None
}

fn extract_reaction_content(reaction: &TelegramMessageReaction) -> Option<String> {
    reaction.new_reaction.iter().find_map(|reaction_entry| {
        reaction_entry
            .emoji
            .as_deref()
            .map(str::trim)
            .filter(|emoji| !emoji.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                reaction_entry
                    .custom_emoji_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("custom_emoji:{id}"))
            })
    })
}

fn reaction_message_id(update_id: i64, chat_id: i64, reaction: &TelegramMessageReaction) -> String {
    match reaction.message_id {
        Some(message_id) => format!("reaction:{chat_id}:{message_id}:{update_id}"),
        None => format!("reaction:update:{update_id}"),
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
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(default)]
    from: Option<TelegramUser>,
    #[serde(default)]
    chat: Option<TelegramChat>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    photo: Vec<serde_json::Value>,
    #[serde(default)]
    sticker: Option<serde_json::Value>,
    #[serde(default)]
    animation: Option<serde_json::Value>,
    #[serde(default)]
    audio: Option<serde_json::Value>,
    #[serde(default)]
    document: Option<serde_json::Value>,
    #[serde(default)]
    video: Option<serde_json::Value>,
    #[serde(default)]
    voice: Option<serde_json::Value>,
    #[serde(default)]
    video_note: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramMessageReaction {
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(default)]
    chat: Option<TelegramChat>,
    #[serde(default)]
    user: Option<TelegramUser>,
    #[serde(default)]
    actor_chat: Option<TelegramChat>,
    #[serde(default)]
    new_reaction: Vec<TelegramReaction>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramReaction {
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    custom_emoji_id: Option<String>,
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

impl TelegramMessage {
    fn has_non_text_payload(&self) -> bool {
        !self.photo.is_empty()
            || self.sticker.is_some()
            || self.animation.is_some()
            || self.audio.is_some()
            || self.document.is_some()
            || self.video.is_some()
            || self.voice.is_some()
            || self.video_note.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TELEGRAM_NON_TEXT_PLACEHOLDER, TelegramChat, TelegramMessage, TelegramMessageReaction,
        TelegramReaction, TelegramUpdate, build_inbound_messages, extract_message_content,
        reaction_message_id, transient_retry_delay,
    };

    #[test]
    fn retry_delay_grows_exponentially_and_caps() {
        assert_eq!(transient_retry_delay(1).as_millis(), 250);
        assert_eq!(transient_retry_delay(2).as_millis(), 500);
        assert_eq!(transient_retry_delay(3).as_millis(), 1000);
        assert_eq!(transient_retry_delay(20).as_millis(), 30000);
    }

    #[test]
    fn message_content_prefers_text_then_caption_then_non_text_placeholder() {
        let mut message = TelegramMessage {
            message_id: Some(5),
            from: None,
            chat: Some(TelegramChat {
                id: 10,
                r#type: "private".to_string(),
            }),
            text: Some(" hello ".to_string()),
            caption: Some("caption".to_string()),
            photo: vec![],
            sticker: None,
            animation: None,
            audio: None,
            document: None,
            video: None,
            voice: None,
            video_note: None,
        };
        assert_eq!(
            extract_message_content(&message).as_deref(),
            Some("hello"),
            "text should win when present"
        );

        message.text = None;
        assert_eq!(
            extract_message_content(&message).as_deref(),
            Some("caption"),
            "caption should be used when text is absent"
        );

        message.caption = None;
        message.photo = vec![serde_json::json!({"file_id":"abc"})];
        assert_eq!(
            extract_message_content(&message).as_deref(),
            Some(TELEGRAM_NON_TEXT_PLACEHOLDER)
        );
    }

    #[test]
    fn inbound_builders_handle_partial_payloads_without_panicking() {
        let update = TelegramUpdate {
            update_id: 100,
            message: Some(TelegramMessage {
                message_id: None,
                from: None,
                chat: Some(TelegramChat {
                    id: 777,
                    r#type: "group".to_string(),
                }),
                text: None,
                caption: None,
                photo: vec![serde_json::json!({"file_id":"p1"})],
                sticker: None,
                animation: None,
                audio: None,
                document: None,
                video: None,
                voice: None,
                video_note: None,
            }),
            message_reaction: Some(TelegramMessageReaction {
                message_id: Some(123),
                chat: Some(TelegramChat {
                    id: 777,
                    r#type: "group".to_string(),
                }),
                user: None,
                actor_chat: None,
                new_reaction: vec![TelegramReaction {
                    emoji: Some("🔥".to_string()),
                    custom_emoji_id: None,
                }],
            }),
        };

        let inbound = build_inbound_messages(&update);
        assert_eq!(inbound.len(), 2);
        assert_eq!(inbound[0].content, TELEGRAM_NON_TEXT_PLACEHOLDER);
        assert_eq!(inbound[0].sender_id.as_str(), "chat:777");
        assert_eq!(inbound[0].message_id.as_str(), "update:100:message");
        assert_eq!(inbound[1].sender_id.as_str(), "chat:777");
        assert_eq!(inbound[1].message_id.as_str(), "reaction:777:123:100");
    }

    #[test]
    fn reaction_message_id_is_deterministic() {
        let reaction = TelegramMessageReaction {
            message_id: Some(9),
            chat: Some(TelegramChat {
                id: 7,
                r#type: "group".to_string(),
            }),
            user: None,
            actor_chat: None,
            new_reaction: vec![],
        };
        assert_eq!(reaction_message_id(42, 7, &reaction), "reaction:7:9:42");
    }
}
