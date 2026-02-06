//! Startup wiring for OpenShell.
//!
//! In v0.1.0, OpenShell primarily runs in-process (gateway + channels). We still
//! register `os.*` subscriptions for parity with Horizons apps.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use anyhow::Result;
use horizons_core::events::models::{
    EventDirection, Subscription, SubscriptionConfig, SubscriptionHandler,
};
use horizons_core::events::traits::EventBus;

pub async fn register_subscriptions(bus: &dyn EventBus, org_id: &str) -> Result<()> {
    let sub = Subscription::new(
        org_id,
        "os.chat.message.received",
        EventDirection::Inbound,
        SubscriptionHandler::Callback {
            handler_id: "agent:os.assistant".to_string(),
        },
        SubscriptionConfig::default(),
        None,
    )?;
    let _ = bus.subscribe(sub).await?;

    let sub = Subscription::new(
        org_id,
        "os.chat.response.*",
        EventDirection::Outbound,
        SubscriptionHandler::Callback {
            handler_id: "os:route_to_channel".to_string(),
        },
        SubscriptionConfig::default(),
        None,
    )?;
    let _ = bus.subscribe(sub).await?;

    let sub = Subscription::new(
        org_id,
        "os.feedback.*",
        EventDirection::Inbound,
        SubscriptionHandler::Callback {
            handler_id: "os:record_feedback".to_string(),
        },
        SubscriptionConfig::default(),
        None,
    )?;
    let _ = bus.subscribe(sub).await?;

    Ok(())
}
