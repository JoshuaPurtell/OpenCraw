use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc;

const LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";
const QUERY_VIEWER_ASSIGNED_ISSUES: &str = r#"
query ViewerAssignedIssues($first: Int!) {
  viewer {
    assignedIssues(first: $first, orderBy: updatedAt) {
      nodes {
        id
        identifier
        title
        url
        team {
          id
          key
          name
        }
        comments(last: 10) {
          nodes {
            id
            body
            createdAt
            user {
              id
              name
            }
          }
        }
      }
    }
  }
}
"#;

const MUTATION_CREATE_COMMENT: &str = r#"
mutation CreateComment($issueId: String!, $body: String!) {
  commentCreate(input: { issueId: $issueId, body: $body }) {
    success
  }
}
"#;

#[derive(Clone)]
pub struct LinearAdapter {
    http: reqwest::Client,
    api_key: String,
    poll_interval: Duration,
    team_ids: Vec<String>,
    start_from_latest: bool,
    max_issues: usize,
}

impl LinearAdapter {
    pub fn new(api_key: &str) -> Result<Self> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(anyhow!("linear api key is required"));
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            http,
            api_key: api_key.to_string(),
            poll_interval: Duration::from_millis(3000),
            team_ids: Vec::new(),
            start_from_latest: true,
            max_issues: 50,
        })
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_team_ids(mut self, team_ids: Vec<String>) -> Self {
        self.team_ids = team_ids
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_max_issues(mut self, max_issues: usize) -> Self {
        self.max_issues = max_issues.max(1);
        self
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for LinearAdapter {
    fn channel_id(&self) -> &str {
        "linear"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.run_poll_loop(tx).await {
                tracing::error!(%e, "linear poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let issue_id = recipient_id.trim();
        if issue_id.is_empty() {
            return Err(anyhow!("recipient_id (Linear issue id) is required"));
        }

        let body = message.content.trim();
        if body.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let data = self
            .graphql(
                MUTATION_CREATE_COMMENT,
                serde_json::json!({
                    "issueId": issue_id,
                    "body": body,
                }),
            )
            .await?;

        let success = data
            .get("commentCreate")
            .and_then(|v| v.get("success"))
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow!("linear mutation response missing commentCreate.success"))?;

        if !success {
            return Err(anyhow!("linear commentCreate returned success=false"));
        }

        tracing::info!(issue_id = %issue_id, "linear comment posted");
        Ok(())
    }
}

impl LinearAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut seen_comment_ids = HashSet::<String>::new();

        if self.start_from_latest {
            let initial = self.fetch_issue_comments().await?;
            for comment in initial {
                seen_comment_ids.insert(comment.comment_id);
            }
            tracing::info!(
                seeded_count = seen_comment_ids.len(),
                "linear adapter seeded initial cursor"
            );
        }

        loop {
            let pulled = self.fetch_issue_comments().await?;
            let mut emitted = 0usize;

            for comment in pulled {
                if seen_comment_ids.contains(&comment.comment_id) {
                    continue;
                }

                let inbound = InboundMessage {
                    kind: InboundMessageKind::Message,
                    message_id: comment.comment_id.clone(),
                    channel_id: "linear".to_string(),
                    sender_id: comment.sender_id,
                    thread_id: Some(comment.issue_id),
                    is_group: true,
                    content: comment.content,
                    metadata: comment.metadata,
                    received_at: Utc::now(),
                };

                tx.send(inbound)
                    .await
                    .map_err(|e| anyhow!("linear inbound queue closed: {e}"))?;

                seen_comment_ids.insert(comment.comment_id);
                emitted += 1;
            }

            tracing::info!(
                emitted,
                seen_count = seen_comment_ids.len(),
                "linear poll cycle complete"
            );

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn fetch_issue_comments(&self) -> Result<Vec<LinearCommentEvent>> {
        let data = self
            .graphql(
                QUERY_VIEWER_ASSIGNED_ISSUES,
                serde_json::json!({
                    "first": self.max_issues,
                }),
            )
            .await?;
        let payload: LinearAssignedIssuesData = serde_json::from_value(data)?;

        let team_filter: HashSet<String> = self
            .team_ids
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();

        let mut out = Vec::new();

        for issue in payload.viewer.assigned_issues.nodes {
            if !team_filter.is_empty() && !matches_team(&issue, &team_filter) {
                continue;
            }

            let mut comments = issue.comments.nodes;
            comments.sort_by(|a, b| {
                let a_ts = parse_rfc3339(&a.created_at).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
                let b_ts = parse_rfc3339(&b.created_at).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
                a_ts.cmp(&b_ts).then_with(|| a.id.cmp(&b.id))
            });

            for comment in comments {
                let user = comment
                    .user
                    .ok_or_else(|| anyhow!("linear comment missing user"))?;
                let body = comment.body.trim();
                if body.is_empty() {
                    return Err(anyhow!("linear comment {} has empty body", comment.id));
                }

                let content = format!(
                    "[{}] {}\nFrom: {}\n\n{}",
                    issue.identifier,
                    issue.title,
                    if user.name.trim().is_empty() {
                        user.id.as_str()
                    } else {
                        user.name.trim()
                    },
                    body
                );
                let comment_id = comment.id.clone();
                let sender_id = user.id.clone();

                out.push(LinearCommentEvent {
                    comment_id,
                    sender_id,
                    issue_id: issue.id.clone(),
                    content,
                    metadata: serde_json::json!({
                        "issue": {
                            "id": issue.id,
                            "identifier": issue.identifier,
                            "title": issue.title,
                            "url": issue.url,
                            "team": issue.team,
                        },
                        "comment": {
                            "id": comment.id,
                            "created_at": comment.created_at,
                        },
                        "author": {
                            "id": user.id,
                            "name": user.name,
                        },
                    }),
                });
            }
        }

        Ok(out)
    }

    async fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let payload = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let resp = self
            .http
            .post(LINEAR_GRAPHQL_URL)
            .header("Authorization", self.auth_header())
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!(
                "linear graphql failed: status={status} body={text}"
            ));
        }

        let parsed: GraphqlResponse = resp.json().await?;
        if !parsed.errors.is_empty() {
            let detail = parsed
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!("linear graphql returned errors: {detail}"));
        }

        parsed
            .data
            .ok_or_else(|| anyhow!("linear graphql response missing data"))
    }

    fn auth_header(&self) -> String {
        format!("Linear {}", self.api_key)
    }
}

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    let dt = DateTime::parse_from_rfc3339(raw)
        .map_err(|e| anyhow!("invalid RFC3339 datetime {raw:?}: {e}"))?;
    Ok(dt.with_timezone(&Utc))
}

fn matches_team(issue: &LinearIssueNode, team_filter: &HashSet<String>) -> bool {
    let Some(team) = issue.team.as_ref() else {
        return false;
    };

    team_filter.contains(&team.id.to_ascii_lowercase())
        || team_filter.contains(&team.key.to_ascii_lowercase())
        || team_filter.contains(&team.name.to_ascii_lowercase())
}

#[derive(Debug)]
struct LinearCommentEvent {
    comment_id: String,
    sender_id: String,
    issue_id: String,
    content: String,
    metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GraphqlResponse {
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct LinearAssignedIssuesData {
    viewer: LinearViewer,
}

#[derive(Debug, Deserialize)]
struct LinearViewer {
    #[serde(rename = "assignedIssues")]
    assigned_issues: LinearIssueConnection,
}

#[derive(Debug, Deserialize)]
struct LinearIssueConnection {
    #[serde(default)]
    nodes: Vec<LinearIssueNode>,
}

#[derive(Debug, Deserialize)]
struct LinearIssueNode {
    id: String,
    identifier: String,
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    team: Option<LinearTeam>,
    comments: LinearCommentConnection,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearTeam {
    id: String,
    key: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct LinearCommentConnection {
    #[serde(default)]
    nodes: Vec<LinearCommentNode>,
}

#[derive(Debug, Deserialize)]
struct LinearCommentNode {
    id: String,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(default)]
    user: Option<LinearUser>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearUser {
    id: String,
    #[serde(default)]
    name: String,
}

#[cfg(test)]
mod tests {
    use super::{matches_team, parse_rfc3339, LinearIssueNode, LinearTeam};

    #[test]
    fn parses_rfc3339_timestamp() {
        let dt = parse_rfc3339("2026-02-07T18:00:00.000Z").expect("parse timestamp");
        assert_eq!(dt.timestamp(), 1_770_487_200);
    }

    #[test]
    fn team_matching_accepts_id_key_or_name_case_insensitive() {
        let issue = LinearIssueNode {
            id: "issue_1".to_string(),
            identifier: "OPS-1".to_string(),
            title: "Example".to_string(),
            url: None,
            team: Some(LinearTeam {
                id: "team_123".to_string(),
                key: "OPS".to_string(),
                name: "Operations".to_string(),
            }),
            comments: super::LinearCommentConnection { nodes: Vec::new() },
        };

        let filter = ["ops".to_string()].into_iter().collect();
        assert!(matches_team(&issue, &filter));
    }
}
