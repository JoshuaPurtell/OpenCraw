use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, OutboundMessage};
use anyhow::{Result, anyhow};
use reqwest::Url;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct WhatsAppCloudAdapter {
    http: reqwest::Client,
    access_token: String,
    phone_number_id: String,
}

impl WhatsAppCloudAdapter {
    pub fn new(access_token: &str, phone_number_id: &str) -> Result<Self> {
        let access_token = access_token.trim();
        if access_token.is_empty() {
            return Err(anyhow!("whatsapp access token is required"));
        }
        let phone_number_id = phone_number_id.trim();
        if phone_number_id.is_empty() {
            return Err(anyhow!("whatsapp phone number id is required"));
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            access_token: access_token.to_string(),
            phone_number_id: phone_number_id.to_string(),
        })
    }

    fn messages_url(&self) -> Result<Url> {
        Url::parse(&format!(
            "https://graph.facebook.com/v20.0/{}/messages",
            self.phone_number_id
        ))
        .map_err(|e| anyhow!("invalid whatsapp graph API URL: {e}"))
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for WhatsAppCloudAdapter {
    fn channel_id(&self) -> &str {
        "whatsapp"
    }

    async fn start(&self, _tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        // Inbound events are delivered via webhook route wiring in os-app.
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let to = recipient_id.trim();
        if to.is_empty() {
            return Err(anyhow!("recipient_id (E.164 phone number) is required"));
        }
        let text = message.content.trim();
        if text.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let url = self.messages_url()?;
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": {
                "preview_url": false,
                "body": text,
            }
        });

        let response = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "whatsapp send failed: status={} body={}",
                status,
                body
            ));
        }

        Ok(())
    }
}
