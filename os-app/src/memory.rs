//! Memory integration via Horizons Voyager.
//!
//! Provides context retrieval before assistant responses and observation storage after.

use horizons_core::memory_traits::{HorizonsMemory, MemoryItem, MemoryType, RetrievalQuery, Scope};
use horizons_core::OrgId;

const AGENT_ID: &str = "os.assistant";

/// Retrieve relevant memories for a user message.
#[allow(dead_code)]
pub async fn retrieve_context(
    memory: &dyn HorizonsMemory,
    org_id: OrgId,
    user_message: &str,
) -> anyhow::Result<Vec<String>> {
    let query = RetrievalQuery::new(user_message, 5);

    let items = memory.retrieve(org_id, AGENT_ID, query).await?;
    Ok(items
        .into_iter()
        .map(|item| item.content_as_text())
        .collect())
}

/// Append an observation from the conversation.
#[allow(dead_code)]
pub async fn append_observation(
    memory: &dyn HorizonsMemory,
    org_id: OrgId,
    content: &str,
) -> anyhow::Result<()> {
    let scope = Scope::new(org_id.to_string(), AGENT_ID.to_string());
    let item = MemoryItem::new(
        &scope,
        MemoryType::observation(),
        serde_json::json!(content),
        chrono::Utc::now(),
    );

    memory.append_item(org_id, item).await?;
    Ok(())
}
