use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use chrono::Utc;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

const TELEGRAM_CHANNEL_ID: &str = "telegram";
const TELEGRAM_NON_TEXT_PLACEHOLDER: &str = "[telegram non-text message]";
const TELEGRAM_CALLBACK_APPROVE_PREFIX: &str = "oc:approve:";
const TELEGRAM_CALLBACK_DENY_PREFIX: &str = "oc:deny:";
const TELEGRAM_REPLY_MARKUP_KEY: &str = "telegram_reply_markup";
const TELEGRAM_EDIT_MESSAGE_ID_KEY: &str = "telegram_edit_message_id";
const TELEGRAM_CLEAR_REPLY_MARKUP_KEY: &str = "telegram_clear_reply_markup";
const TELEGRAM_MAX_MESSAGE_CHARS_HARD_LIMIT: usize = 4096;

#[derive(Clone)]
pub struct TelegramAdapter {
    http: reqwest::Client,
    bot_token: String,
    long_poll_timeout_seconds: u64,
    allowed_updates: Vec<String>,
    retry_base_ms: u64,
    retry_max_ms: u64,
    non_transient_delay: Duration,
    parse_mode: Option<String>,
    disable_link_previews: bool,
    max_message_chars: usize,
    bot_commands: Vec<TelegramBotCommand>,
}

impl TelegramAdapter {
    pub fn new(bot_token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            bot_token: bot_token.to_string(),
            long_poll_timeout_seconds: 30,
            allowed_updates: vec![
                "message".to_string(),
                "message_reaction".to_string(),
                "callback_query".to_string(),
            ],
            retry_base_ms: 250,
            retry_max_ms: 30_000,
            non_transient_delay: Duration::from_secs(10),
            parse_mode: Some("Markdown".to_string()),
            disable_link_previews: true,
            max_message_chars: 4000,
            bot_commands: Vec::new(),
        })
    }

    fn api_url(&self, method: &str) -> Result<Url> {
        Ok(Url::parse(&format!(
            "https://api.telegram.org/bot{}/{}",
            self.bot_token, method
        ))?)
    }

    pub fn with_long_poll_timeout_seconds(mut self, timeout_seconds: u64) -> Self {
        self.long_poll_timeout_seconds = timeout_seconds.max(1);
        self
    }

    pub fn with_allowed_updates(mut self, allowed_updates: Vec<String>) -> Self {
        let mut updates = Vec::new();
        for update in allowed_updates {
            let update = update.trim().to_string();
            if update.is_empty() {
                continue;
            }
            if !updates.iter().any(|existing| existing == &update) {
                updates.push(update);
            }
        }
        if !updates.is_empty() {
            self.allowed_updates = updates;
        }
        self
    }

    pub fn with_retry_backoff(mut self, retry_base_ms: u64, retry_max_ms: u64) -> Self {
        self.retry_base_ms = retry_base_ms.max(1);
        self.retry_max_ms = retry_max_ms.max(self.retry_base_ms);
        self
    }

    pub fn with_non_transient_delay(mut self, delay: Duration) -> Self {
        self.non_transient_delay = delay.max(Duration::from_millis(1));
        self
    }

    pub fn with_parse_mode(mut self, parse_mode: Option<&str>) -> Self {
        self.parse_mode = parse_mode
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        self
    }

    pub fn with_disable_link_previews(mut self, disable_link_previews: bool) -> Self {
        self.disable_link_previews = disable_link_previews;
        self
    }

    pub fn with_max_message_chars(mut self, max_message_chars: usize) -> Self {
        self.max_message_chars = max_message_chars.clamp(1, TELEGRAM_MAX_MESSAGE_CHARS_HARD_LIMIT);
        self
    }

    pub fn with_bot_commands(mut self, commands: Vec<(String, String)>) -> Self {
        let mut normalized = Vec::new();
        for (command, description) in commands {
            let command = command.trim().trim_start_matches('/').to_ascii_lowercase();
            let description = description.trim();
            if command.is_empty() || description.is_empty() {
                continue;
            }
            if !command
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
            {
                continue;
            }
            if command.len() > 32 {
                continue;
            }
            if normalized
                .iter()
                .any(|existing: &TelegramBotCommand| existing.command == command)
            {
                continue;
            }
            normalized.push(TelegramBotCommand {
                command,
                description: description.to_string(),
            });
        }
        self.bot_commands = normalized;
        self
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
        let long_poll_timeout_seconds = self.long_poll_timeout_seconds;
        let allowed_updates = self.allowed_updates.clone();
        let retry_base_ms = self.retry_base_ms;
        let retry_max_ms = self.retry_max_ms;
        let non_transient_delay = self.non_transient_delay;
        let parse_mode = self.parse_mode.clone();
        let disable_link_previews = self.disable_link_previews;
        let max_message_chars = self.max_message_chars;
        let bot_commands = self.bot_commands.clone();
        tokio::spawn(async move {
            let adapter = TelegramAdapter {
                http,
                bot_token: token,
                long_poll_timeout_seconds,
                allowed_updates,
                retry_base_ms,
                retry_max_ms,
                non_transient_delay,
                parse_mode,
                disable_link_previews,
                max_message_chars,
                bot_commands,
            };
            if let Err(e) = adapter.run_poll_loop(tx).await {
                tracing::error!(%e, "telegram poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let chat_id = recipient_id.trim();
        if chat_id.is_empty() {
            return Err(anyhow::anyhow!(
                "recipient_id (telegram chat id) is required"
            ));
        }
        let text = message.content.trim();
        if text.is_empty() {
            return Err(anyhow::anyhow!("message content is empty"));
        }

        if let Some(edit_message_id) = telegram_edit_message_id(&message.metadata) {
            match self
                .edit_message(chat_id, edit_message_id, text, &message.metadata)
                .await
            {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        chat_id,
                        edit_message_id,
                        "telegram editMessageText failed; falling back to sendMessage"
                    );
                }
            }
        }

        let chunks = chunk_for_telegram(text, self.max_message_chars);
        for (idx, chunk) in chunks.iter().enumerate() {
            let url = self.api_url("sendMessage")?;
            let mut body = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "disable_web_page_preview": self.disable_link_previews,
            });

            if idx == 0 {
                if let Some(reply_markup) = message.metadata.get(TELEGRAM_REPLY_MARKUP_KEY) {
                    if !reply_markup.is_null() {
                        body["reply_markup"] = reply_markup.clone();
                    }
                }

                if let Some(reply_to_message_id) = message.reply_to_message_id.as_ref() {
                    if let Ok(reply_id) = reply_to_message_id.parse::<i64>() {
                        body["reply_to_message_id"] = serde_json::Value::Number(reply_id.into());
                    }
                }
            }

            if let Some(parse_mode) = self.parse_mode.as_deref() {
                body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
            }

            let mut resp = self.http.post(url.clone()).json(&body).send().await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await?;
                let parse_error = status == StatusCode::BAD_REQUEST
                    && text.to_ascii_lowercase().contains("can't parse entities");
                if parse_error && self.parse_mode.is_some() {
                    let mut fallback_body = body;
                    if let Some(obj) = fallback_body.as_object_mut() {
                        obj.remove("parse_mode");
                    }
                    resp = self.http.post(url).json(&fallback_body).send().await?;
                    if !resp.status().is_success() {
                        let fallback_status = resp.status();
                        let fallback_text = resp.text().await?;
                        return Err(anyhow::anyhow!(
                            "telegram send failed: status={fallback_status} body={fallback_text}"
                        ));
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "telegram send failed: status={status} body={text}"
                    ));
                }
            }
        }
        Ok(())
    }

    async fn send_typing(&self, recipient_id: &str, active: bool) -> Result<()> {
        if !active {
            return Ok(());
        }
        let chat_id = recipient_id.trim();
        if chat_id.is_empty() {
            return Err(anyhow::anyhow!(
                "recipient_id (telegram chat id) is required"
            ));
        }

        let url = self.api_url("sendChatAction")?;
        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });
        let resp = self.http.post(url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow::anyhow!(
                "telegram sendChatAction failed: status={status} body={text}"
            ));
        }
        Ok(())
    }

    fn supports_typing_events(&self) -> bool {
        true
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl TelegramAdapter {
    async fn edit_message(
        &self,
        chat_id: &str,
        message_id: i64,
        text: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let url = self.api_url("editMessageText")?;
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "disable_web_page_preview": self.disable_link_previews,
        });

        if let Some(parse_mode) = self.parse_mode.as_deref() {
            body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
        }

        if metadata
            .get(TELEGRAM_CLEAR_REPLY_MARKUP_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            body["reply_markup"] = serde_json::json!({ "inline_keyboard": [] });
        } else if let Some(reply_markup) = metadata.get(TELEGRAM_REPLY_MARKUP_KEY) {
            if !reply_markup.is_null() {
                body["reply_markup"] = reply_markup.clone();
            }
        }

        let mut resp = self.http.post(url.clone()).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            let normalized = text.to_ascii_lowercase();
            if status == StatusCode::BAD_REQUEST && normalized.contains("message is not modified") {
                return Ok(());
            }
            let parse_error =
                status == StatusCode::BAD_REQUEST && normalized.contains("can't parse entities");
            if parse_error && self.parse_mode.is_some() {
                let mut fallback_body = body;
                if let Some(obj) = fallback_body.as_object_mut() {
                    obj.remove("parse_mode");
                }
                resp = self.http.post(url).json(&fallback_body).send().await?;
                if !resp.status().is_success() {
                    let fallback_status = resp.status();
                    let fallback_text = resp.text().await?;
                    return Err(anyhow::anyhow!(
                        "telegram edit failed: status={fallback_status} body={fallback_text}"
                    ));
                }
            } else {
                return Err(anyhow::anyhow!(
                    "telegram edit failed: status={status} body={text}"
                ));
            }
        }

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut offset: i64 = 0;
        let mut consecutive_failures: u32 = 0;
        let long_poll_timeout = self.long_poll_timeout_seconds.to_string();
        let allowed_updates = serde_json::to_string(&self.allowed_updates)
            .map_err(|e| anyhow::anyhow!("serialize telegram allowed_updates: {e}"))?;

        if !self.bot_commands.is_empty() {
            if let Err(error) = self.register_bot_commands().await {
                tracing::warn!(%error, "telegram setMyCommands failed; slash suggestions may be missing");
            }
        }

        loop {
            let url = self.api_url("getUpdates")?;
            let response = match self
                .http
                .get(url)
                .query(&[
                    ("timeout", long_poll_timeout.as_str()),
                    ("offset", &offset.to_string()),
                    ("allowed_updates", allowed_updates.as_str()),
                ])
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    consecutive_failures += 1;
                    let delay = transient_retry_delay(
                        consecutive_failures,
                        self.retry_base_ms,
                        self.retry_max_ms,
                    );
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
                    let delay = transient_retry_delay(
                        consecutive_failures,
                        self.retry_base_ms,
                        self.retry_max_ms,
                    );
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
                        ?self.non_transient_delay,
                        "telegram getUpdates non-transient failure; keeping poll loop alive"
                    );
                    tokio::time::sleep(self.non_transient_delay).await;
                }
                continue;
            }

            let parsed = match response.json::<TelegramGetUpdatesResponse>().await {
                Ok(parsed) => parsed,
                Err(error) => {
                    consecutive_failures += 1;
                    let delay = transient_retry_delay(
                        consecutive_failures,
                        self.retry_base_ms,
                        self.retry_max_ms,
                    );
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
                let callback_query_id = update.callback_query.as_ref().map(|cq| cq.id.clone());

                for inbound in build_inbound_messages(&update) {
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow::anyhow!("telegram inbound queue closed: {e}"))?;
                }

                if let Some(callback_query_id) = callback_query_id {
                    if let Err(error) = self.answer_callback_query(&callback_query_id, None).await {
                        tracing::warn!(
                            %error,
                            callback_query_id = %callback_query_id,
                            "telegram answerCallbackQuery failed"
                        );
                    }
                }
            }
        }
    }

    async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let url = self.api_url("answerCallbackQuery")?;
        let mut body = serde_json::json!({
            "callback_query_id": callback_query_id,
        });
        if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
            body["text"] = serde_json::Value::String(text.to_string());
        }
        let response = self.http.post(url).json(&body).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let payload = response
                .text()
                .await
                .unwrap_or_else(|error| format!("<failed to read telegram error body: {error}>"));
            return Err(anyhow::anyhow!(
                "telegram answerCallbackQuery failed: status={status} body={payload}"
            ));
        }
        Ok(())
    }

    async fn register_bot_commands(&self) -> Result<()> {
        let url = self.api_url("setMyCommands")?;
        let body = serde_json::json!({
            "commands": self.bot_commands,
        });
        let response = self.http.post(url).json(&body).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let payload = response
                .text()
                .await
                .unwrap_or_else(|error| format!("<failed to read telegram error body: {error}>"));
            return Err(anyhow::anyhow!(
                "telegram setMyCommands failed: status={status} body={payload}"
            ));
        }
        Ok(())
    }
}

fn transient_retry_delay(attempt: u32, retry_base_ms: u64, retry_max_ms: u64) -> Duration {
    let multiplier = 1_u64 << attempt.saturating_sub(1).min(10);
    Duration::from_millis((retry_base_ms * multiplier).min(retry_max_ms))
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
    if let Some(callback) = build_callback_inbound(update.update_id, update.callback_query.as_ref())
    {
        inbound.push(callback);
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

fn build_callback_inbound(
    update_id: i64,
    callback_query: Option<&TelegramCallbackQuery>,
) -> Option<InboundMessage> {
    let callback_query = callback_query?;
    let callback_message = callback_query.message.as_ref()?;
    let chat = callback_message.chat.as_ref()?;
    let content = callback_data_to_command(callback_query.data.as_deref()?)?;
    let sender_id = callback_query.from.id.to_string();
    let message_id = callback_message
        .message_id
        .map(|id| format!("callback:{id}:{update_id}"))
        .unwrap_or_else(|| format!("callback:update:{update_id}"));

    Some(InboundMessage {
        kind: InboundMessageKind::Message,
        message_id: message_id.into(),
        channel_id: TELEGRAM_CHANNEL_ID.into(),
        sender_id: sender_id.into(),
        thread_id: Some(chat.id.to_string().into()),
        is_group: chat.r#type != "private",
        content,
        metadata: serde_json::to_value(callback_query).unwrap_or(serde_json::Value::Null),
        received_at: Utc::now(),
    })
}

fn callback_data_to_command(data: &str) -> Option<String> {
    let data = data.trim();
    if let Some(action_id) = data.strip_prefix(TELEGRAM_CALLBACK_APPROVE_PREFIX) {
        let action_id = action_id.trim();
        if !action_id.is_empty() {
            return Some(format!("/approve-action {action_id}"));
        }
    }
    if let Some(action_id) = data.strip_prefix(TELEGRAM_CALLBACK_DENY_PREFIX) {
        let action_id = action_id.trim();
        if !action_id.is_empty() {
            return Some(format!("/deny-action {action_id}"));
        }
    }
    None
}

fn telegram_edit_message_id(metadata: &serde_json::Value) -> Option<i64> {
    let raw = metadata.get(TELEGRAM_EDIT_MESSAGE_ID_KEY)?;
    raw.as_i64().or_else(|| {
        raw.as_str()
            .and_then(|value| value.trim().parse::<i64>().ok())
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

fn chunk_for_telegram(content: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.clamp(1, TELEGRAM_MAX_MESSAGE_CHARS_HARD_LIMIT);
    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= max_chars {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        chunks.push(chunk);
        start = end;
    }
    chunks
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
    #[serde(default)]
    callback_query: Option<TelegramCallbackQuery>,
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
struct TelegramCallbackQuery {
    id: String,
    from: TelegramUser,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    message: Option<TelegramCallbackMessage>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramCallbackMessage {
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(default)]
    chat: Option<TelegramChat>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct TelegramReaction {
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    custom_emoji_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct TelegramBotCommand {
    command: String,
    description: String,
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
        TELEGRAM_NON_TEXT_PLACEHOLDER, TelegramAdapter, TelegramChat, TelegramMessage,
        TelegramMessageReaction, TelegramReaction, TelegramUpdate, build_inbound_messages,
        callback_data_to_command, chunk_for_telegram, extract_message_content, reaction_message_id,
        telegram_edit_message_id, transient_retry_delay,
    };

    #[test]
    fn retry_delay_grows_exponentially_and_caps() {
        assert_eq!(transient_retry_delay(1, 250, 30_000).as_millis(), 250);
        assert_eq!(transient_retry_delay(2, 250, 30_000).as_millis(), 500);
        assert_eq!(transient_retry_delay(3, 250, 30_000).as_millis(), 1000);
        assert_eq!(transient_retry_delay(20, 250, 30_000).as_millis(), 30_000);
    }

    #[test]
    fn telegram_chunking_respects_character_limit() {
        let text = "a".repeat(10_000);
        let chunks = chunk_for_telegram(&text, 4000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 4000);
        assert_eq!(chunks[1].chars().count(), 4000);
        assert_eq!(chunks[2].chars().count(), 2000);
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
            callback_query: None,
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

    #[test]
    fn callback_data_maps_to_approval_commands() {
        assert_eq!(
            callback_data_to_command("oc:approve:1234").as_deref(),
            Some("/approve-action 1234")
        );
        assert_eq!(
            callback_data_to_command("oc:deny:5678").as_deref(),
            Some("/deny-action 5678")
        );
        assert_eq!(callback_data_to_command("unknown"), None);
    }

    #[test]
    fn telegram_edit_message_id_accepts_number_and_string() {
        let numeric = serde_json::json!({ "telegram_edit_message_id": 42 });
        assert_eq!(telegram_edit_message_id(&numeric), Some(42));

        let string = serde_json::json!({ "telegram_edit_message_id": "84" });
        assert_eq!(telegram_edit_message_id(&string), Some(84));
    }

    #[test]
    fn bot_commands_are_normalized_and_invalid_entries_dropped() {
        let adapter = TelegramAdapter::new("token")
            .expect("adapter")
            .with_bot_commands(vec![
                ("/nuke".to_string(), "Reset context".to_string()),
                ("HELP".to_string(), "Show help".to_string()),
                ("bad-cmd".to_string(), "Invalid chars".to_string()),
                ("".to_string(), "Missing command".to_string()),
                ("nuke".to_string(), "Duplicate".to_string()),
            ]);
        assert_eq!(adapter.bot_commands.len(), 2);
        assert_eq!(adapter.bot_commands[0].command, "nuke");
        assert_eq!(adapter.bot_commands[1].command, "help");
    }
}
