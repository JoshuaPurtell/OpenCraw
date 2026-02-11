use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";
const DEFAULT_MAX_RESULTS: usize = 20;
const MAX_RESULTS_CAP: usize = 100;

const QUERY_VIEWER: &str = r#"
query Viewer {
  viewer {
    id
    name
    email
  }
}
"#;

const QUERY_LIST_ASSIGNED: &str = r#"
query ViewerAssignedIssues($first: Int!) {
  viewer {
    assignedIssues(first: $first, orderBy: updatedAt) {
      nodes {
        id
        identifier
        title
        url
        priority
        team {
          id
          key
          name
        }
        assignee {
          id
          name
          email
        }
      }
    }
  }
}
"#;

const QUERY_LIST_USERS: &str = r#"
query Users($first: Int!) {
  users(first: $first) {
    nodes {
      id
      name
      email
      active
    }
  }
}
"#;

const QUERY_LIST_TEAMS: &str = r#"
query Teams($first: Int!) {
  teams(first: $first) {
    nodes {
      id
      key
      name
    }
  }
}
"#;

const QUERY_LIST_PROJECTS: &str = r#"
query Projects($first: Int!) {
  projects(first: $first, orderBy: updatedAt) {
    nodes {
      id
      name
      url
    }
  }
}
"#;

const MUTATION_ISSUE_CREATE: &str = r#"
mutation IssueCreate($input: IssueCreateInput!) {
  issueCreate(input: $input) {
    success
    issue {
      id
      identifier
      title
      url
    }
  }
}
"#;

const MUTATION_PROJECT_CREATE: &str = r#"
mutation ProjectCreate($input: ProjectCreateInput!) {
  projectCreate(input: $input) {
    success
    project {
      id
      name
      url
    }
  }
}
"#;

const MUTATION_ISSUE_UPDATE_ASSIGN: &str = r#"
mutation IssueAssign($id: String!, $assigneeId: String!) {
  issueUpdate(
    id: $id
    input: {
      assigneeId: $assigneeId
    }
  ) {
    success
    issue {
      id
      identifier
      title
      url
      assignee {
        id
        name
        email
      }
    }
  }
}
"#;

const MUTATION_ISSUE_UPDATE_FIELDS: &str = r#"
mutation IssueUpdate($id: String!, $input: IssueUpdateInput!) {
  issueUpdate(id: $id, input: $input) {
    success
    issue {
      id
      identifier
      title
      url
      priority
      assignee {
        id
        name
        email
      }
    }
  }
}
"#;

const MUTATION_COMMENT_CREATE: &str = r#"
mutation CreateComment($issueId: String!, $body: String!) {
  commentCreate(input: { issueId: $issueId, body: $body }) {
    success
  }
}
"#;

#[derive(Clone, Copy, Debug)]
pub struct LinearActionToggles {
    pub whoami: bool,
    pub list_assigned: bool,
    pub list_users: bool,
    pub list_teams: bool,
    pub list_projects: bool,
    pub create_issue: bool,
    pub create_project: bool,
    pub update_issue: bool,
    pub assign_issue: bool,
    pub comment_issue: bool,
}

impl LinearActionToggles {
    pub fn all_enabled() -> Self {
        Self {
            whoami: true,
            list_assigned: true,
            list_users: true,
            list_teams: true,
            list_projects: true,
            create_issue: true,
            create_project: true,
            update_issue: true,
            assign_issue: true,
            comment_issue: true,
        }
    }

    fn allows(self, action: &str) -> bool {
        match action {
            "whoami" => self.whoami,
            "list_assigned" => self.list_assigned,
            "list_users" => self.list_users,
            "list_teams" => self.list_teams,
            "list_projects" => self.list_projects,
            "create_issue" => self.create_issue,
            "create_project" => self.create_project,
            "update_issue" => self.update_issue,
            "assign_issue" => self.assign_issue,
            "comment_issue" => self.comment_issue,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct LinearTool {
    http: reqwest::Client,
    api_key: String,
    default_team_id: Option<String>,
    action_toggles: LinearActionToggles,
}

impl LinearTool {
    pub fn new(
        api_key: &str,
        default_team_id: Option<String>,
        action_toggles: LinearActionToggles,
    ) -> Result<Self> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ToolError::InvalidArguments(
                "linear api key is required".to_string(),
            ));
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let default_team_id = default_team_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Ok(Self {
            http,
            api_key: api_key.to_string(),
            default_team_id,
            action_toggles,
        })
    }

    fn ensure_action_enabled(&self, action: &str) -> Result<()> {
        if self.action_toggles.allows(action) {
            return Ok(());
        }
        Err(ToolError::Unauthorized(format!(
            "linear action {action:?} is disabled by channels.linear.actions.{action}"
        )))
    }

    async fn whoami(&self) -> Result<serde_json::Value> {
        let data = self.graphql(QUERY_VIEWER, json!({})).await?;
        let viewer: LinearViewerOnlyResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(json!({
            "viewer": viewer.viewer
        }))
    }

    async fn list_assigned(&self, max_results: usize) -> Result<serde_json::Value> {
        let data = self
            .graphql(
                QUERY_LIST_ASSIGNED,
                json!({
                    "first": max_results,
                }),
            )
            .await?;
        let parsed: LinearAssignedIssuesData =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(json!({
            "count": parsed.viewer.assigned_issues.nodes.len(),
            "issues": parsed.viewer.assigned_issues.nodes,
        }))
    }

    async fn list_users(
        &self,
        query: Option<&str>,
        max_results: usize,
    ) -> Result<serde_json::Value> {
        let data = self
            .graphql(
                QUERY_LIST_USERS,
                json!({
                    "first": max_results,
                }),
            )
            .await?;
        let parsed: LinearUsersResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let query = query.map(|value| value.trim().to_ascii_lowercase());
        let users: Vec<LinearUser> = parsed
            .users
            .nodes
            .into_iter()
            .filter(|user| user.active)
            .filter(|user| {
                let Some(query) = query.as_deref() else {
                    return true;
                };
                user.id.to_ascii_lowercase().contains(query)
                    || user.name.to_ascii_lowercase().contains(query)
                    || user
                        .email
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(query)
            })
            .collect();

        Ok(json!({
            "count": users.len(),
            "users": users,
        }))
    }

    async fn list_teams(
        &self,
        query: Option<&str>,
        max_results: usize,
    ) -> Result<serde_json::Value> {
        let data = self
            .graphql(
                QUERY_LIST_TEAMS,
                json!({
                    "first": max_results,
                }),
            )
            .await?;
        let parsed: LinearTeamsResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let query = query.map(|value| value.trim().to_ascii_lowercase());
        let teams: Vec<LinearTeam> = parsed
            .teams
            .nodes
            .into_iter()
            .filter(|team| {
                let Some(query) = query.as_deref() else {
                    return true;
                };
                team.id.to_ascii_lowercase().contains(query)
                    || team.key.to_ascii_lowercase().contains(query)
                    || team.name.to_ascii_lowercase().contains(query)
            })
            .collect();

        Ok(json!({
            "count": teams.len(),
            "teams": teams,
        }))
    }

    async fn list_projects(
        &self,
        query: Option<&str>,
        max_results: usize,
    ) -> Result<serde_json::Value> {
        let data = self
            .graphql(
                QUERY_LIST_PROJECTS,
                json!({
                    "first": max_results,
                }),
            )
            .await?;
        let parsed: LinearProjectsResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let query = query.map(|value| value.trim().to_ascii_lowercase());
        let projects: Vec<LinearProject> = parsed
            .projects
            .nodes
            .into_iter()
            .filter(|project| {
                let Some(query) = query.as_deref() else {
                    return true;
                };
                project.id.to_ascii_lowercase().contains(query)
                    || project.name.to_ascii_lowercase().contains(query)
            })
            .collect();

        Ok(json!({
            "count": projects.len(),
            "projects": projects,
        }))
    }

    async fn create_issue(
        &self,
        title: &str,
        description: Option<&str>,
        team_id: Option<&str>,
        assignee_id: Option<&str>,
        priority: Option<i64>,
    ) -> Result<serde_json::Value> {
        let title = title.trim();
        if title.is_empty() {
            return Err(ToolError::InvalidArguments(
                "title must not be empty".to_string(),
            ));
        }

        let team_id = team_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.default_team_id.clone())
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "team_id is required (or set channels.linear.default_team_id)".to_string(),
                )
            })?;

        let assignee_id = self.resolve_assignee_reference(assignee_id).await?;
        let description = description.map(str::trim).filter(|value| !value.is_empty());
        let priority = match priority {
            None => None,
            Some(value) if (0..=4).contains(&value) => Some(value),
            Some(value) => {
                return Err(ToolError::InvalidArguments(format!(
                    "priority must be between 0 and 4, got {value}"
                )));
            }
        };
        let mut input = serde_json::Map::new();
        input.insert("title".to_string(), json!(title));
        input.insert("teamId".to_string(), json!(team_id));
        if let Some(value) = description {
            input.insert("description".to_string(), json!(value));
        }
        if let Some(value) = assignee_id {
            input.insert("assigneeId".to_string(), json!(value));
        }
        if let Some(value) = priority {
            input.insert("priority".to_string(), json!(value));
        }

        let data = self
            .graphql(
                MUTATION_ISSUE_CREATE,
                json!({
                    "input": input,
                }),
            )
            .await?;
        let payload = data
            .get("issueCreate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing issueCreate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear issueCreate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear issueCreate returned success=false".to_string(),
            ));
        }
        let issue = payload.get("issue").cloned().ok_or_else(|| {
            ToolError::ExecutionFailed("linear issueCreate missing issue".to_string())
        })?;

        Ok(json!({
            "status": "created",
            "issue": issue,
        }))
    }

    async fn create_project(
        &self,
        name: &str,
        description: Option<&str>,
        team_ids: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ToolError::InvalidArguments(
                "name must not be empty".to_string(),
            ));
        }
        let description = description.map(str::trim).filter(|value| !value.is_empty());

        let mut team_ids = team_ids.unwrap_or_default();
        if team_ids.is_empty() {
            if let Some(default_team_id) = self
                .default_team_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                team_ids.push(default_team_id.to_string());
            }
        }
        if team_ids.is_empty() {
            return Err(ToolError::InvalidArguments(
                "team_ids must include at least one team id (or set channels.linear.default_team_id)"
                    .to_string(),
            ));
        }
        let mut input = serde_json::Map::new();
        input.insert("name".to_string(), json!(name));
        input.insert("teamIds".to_string(), json!(team_ids));
        if let Some(value) = description {
            input.insert("description".to_string(), json!(value));
        }

        let data = self
            .graphql(
                MUTATION_PROJECT_CREATE,
                json!({
                    "input": input,
                }),
            )
            .await?;
        let payload = data
            .get("projectCreate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing projectCreate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear projectCreate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear projectCreate returned success=false".to_string(),
            ));
        }
        let project = payload.get("project").cloned().ok_or_else(|| {
            ToolError::ExecutionFailed("linear projectCreate missing project".to_string())
        })?;

        Ok(json!({
            "status": "created",
            "project": project,
        }))
    }

    async fn assign_issue(&self, issue_id: &str, assignee_id: &str) -> Result<serde_json::Value> {
        let issue_id = self.resolve_issue_reference(issue_id).await?;
        let assignee_id = self
            .resolve_assignee_reference(Some(assignee_id))
            .await?
            .ok_or_else(|| {
                ToolError::InvalidArguments("assignee_id must not be empty".to_string())
            })?;

        let data = self
            .graphql(
                MUTATION_ISSUE_UPDATE_ASSIGN,
                json!({
                    "id": issue_id,
                    "assigneeId": assignee_id,
                }),
            )
            .await?;
        let payload = data
            .get("issueUpdate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing issueUpdate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear issueUpdate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear issueUpdate returned success=false".to_string(),
            ));
        }

        Ok(json!({
            "status": "assigned",
            "issue": payload.get("issue").cloned().unwrap_or(serde_json::Value::Null),
        }))
    }

    async fn update_issue(
        &self,
        issue_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        assignee_id: Option<&str>,
        priority: Option<i64>,
        state_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let issue_id = self.resolve_issue_reference(issue_id).await?;
        let assignee_id = self.resolve_assignee_reference(assignee_id).await?;
        let title = title.map(str::trim).filter(|value| !value.is_empty());
        let description = description.map(str::trim).filter(|value| !value.is_empty());
        let state_id = state_id.map(str::trim).filter(|value| !value.is_empty());
        let project_id = project_id.map(str::trim).filter(|value| !value.is_empty());
        let priority = match priority {
            None => None,
            Some(value) if (0..=4).contains(&value) => Some(value),
            Some(value) => {
                return Err(ToolError::InvalidArguments(format!(
                    "priority must be between 0 and 4, got {value}"
                )));
            }
        };

        let mut input = serde_json::Map::new();
        if let Some(value) = title {
            input.insert("title".to_string(), json!(value));
        }
        if let Some(value) = description {
            input.insert("description".to_string(), json!(value));
        }
        if let Some(value) = assignee_id {
            input.insert("assigneeId".to_string(), json!(value));
        }
        if let Some(value) = priority {
            input.insert("priority".to_string(), json!(value));
        }
        if let Some(value) = state_id {
            input.insert("stateId".to_string(), json!(value));
        }
        if let Some(value) = project_id {
            input.insert("projectId".to_string(), json!(value));
        }
        if input.is_empty() {
            return Err(ToolError::InvalidArguments(
                "update_issue requires at least one field: title, description, assignee_id, priority, state_id, project_id".to_string(),
            ));
        }

        let data = self
            .graphql(
                MUTATION_ISSUE_UPDATE_FIELDS,
                json!({
                    "id": issue_id,
                    "input": input,
                }),
            )
            .await?;
        let payload = data
            .get("issueUpdate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing issueUpdate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear issueUpdate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear issueUpdate returned success=false".to_string(),
            ));
        }

        Ok(json!({
            "status": "updated",
            "issue": payload.get("issue").cloned().unwrap_or(serde_json::Value::Null),
        }))
    }

    async fn comment_issue(&self, issue_id: &str, body: &str) -> Result<serde_json::Value> {
        let issue_id = self.resolve_issue_reference(issue_id).await?;
        let body = body.trim();
        if body.is_empty() {
            return Err(ToolError::InvalidArguments(
                "body must not be empty".to_string(),
            ));
        }

        let data = self
            .graphql(
                MUTATION_COMMENT_CREATE,
                json!({
                    "issueId": issue_id,
                    "body": body,
                }),
            )
            .await?;
        let payload = data
            .get("commentCreate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing commentCreate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear commentCreate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear commentCreate returned success=false".to_string(),
            ));
        }

        Ok(json!({
            "status": "commented",
            "issue_id": issue_id,
        }))
    }

    async fn resolve_assignee_reference(
        &self,
        assignee_id: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(assignee_id) = assignee_id else {
            return Ok(None);
        };
        let assignee_id = assignee_id.trim();
        if assignee_id.is_empty() {
            return Ok(None);
        }
        if assignee_id.eq_ignore_ascii_case("me") {
            let data = self.graphql(QUERY_VIEWER, json!({})).await?;
            let viewer: LinearViewerOnlyResponse = serde_json::from_value(data)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            return Ok(Some(viewer.viewer.id));
        }
        Ok(Some(assignee_id.to_string()))
    }

    async fn resolve_issue_reference(&self, issue_ref: &str) -> Result<String> {
        let issue_ref = issue_ref.trim();
        if issue_ref.is_empty() {
            return Err(ToolError::InvalidArguments(
                "issue_id must not be empty".to_string(),
            ));
        }
        if looks_like_uuid(issue_ref) {
            return Ok(issue_ref.to_string());
        }

        let data = self
            .graphql(
                QUERY_LIST_ASSIGNED,
                json!({
                    "first": 100,
                }),
            )
            .await?;
        let parsed: LinearAssignedIssuesData =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let issue_id = parsed
            .viewer
            .assigned_issues
            .nodes
            .into_iter()
            .find(|issue| issue.identifier.eq_ignore_ascii_case(issue_ref))
            .map(|issue| issue.id)
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!(
                    "issue reference {issue_ref:?} was not found in viewer-assigned issues; pass a UUID issue_id"
                ))
            })?;
        Ok(issue_id)
    }

    async fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let payload = json!({
            "query": query,
            "variables": variables,
        });

        let url = Url::parse(LINEAR_GRAPHQL_URL)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let response = self
            .http
            .post(url)
            .header("Authorization", self.api_key.clone())
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_string());
            return Err(ToolError::ExecutionFailed(format!(
                "linear graphql failed: status={status} body={body}"
            )));
        }

        let payload: GraphqlResponse = response
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if !payload.errors.is_empty() {
            let detail = serde_json::to_string(&payload.errors)
                .unwrap_or_else(|_| "<failed to serialize graphql errors>".to_string());
            return Err(ToolError::ExecutionFailed(format!(
                "linear graphql returned errors: {detail}"
            )));
        }

        payload.data.ok_or_else(|| {
            ToolError::ExecutionFailed("linear graphql response missing data".to_string())
        })
    }
}

#[async_trait]
impl Tool for LinearTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "linear".to_string(),
            description: "Manage Linear via API: read identity/users/teams/projects, create projects/issues, update/assign/comment on issues."
                .to_string(),
            parameters_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "whoami",
                            "list_assigned",
                            "list_users",
                            "list_teams",
                            "list_projects",
                            "create_issue",
                            "create_project",
                            "update_issue",
                            "assign_issue",
                            "comment_issue"
                        ]
                    },
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "title": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "team_id": { "type": "string" },
                    "team_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Preferred for create_project; team_id is accepted as a single-id alias."
                    },
                    "assignee_id": { "type": "string", "description": "Linear user id or 'me'" },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 4 },
                    "issue_id": { "type": "string" },
                    "project_id": { "type": "string" },
                    "state_id": { "type": "string" },
                    "body": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        self.ensure_action_enabled(action.as_str())?;
        match action.as_str() {
            "whoami" => self.whoami().await,
            "list_assigned" => {
                let max_results = parse_max_results(&arguments)?;
                self.list_assigned(max_results).await
            }
            "list_users" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments)?;
                self.list_users(query.as_deref(), max_results).await
            }
            "list_teams" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments)?;
                self.list_teams(query.as_deref(), max_results).await
            }
            "list_projects" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments)?;
                self.list_projects(query.as_deref(), max_results).await
            }
            "create_issue" => {
                let title = require_string(&arguments, "title")?;
                let description = optional_string(&arguments, "description")?;
                let team_id = optional_string(&arguments, "team_id")?;
                let assignee_id = optional_string(&arguments, "assignee_id")?;
                let priority = parse_optional_priority(&arguments)?;
                self.create_issue(
                    &title,
                    description.as_deref(),
                    team_id.as_deref(),
                    assignee_id.as_deref(),
                    priority,
                )
                .await
            }
            "create_project" => {
                let name = require_string(&arguments, "name")?;
                let description = optional_string(&arguments, "description")?;
                let team_ids = resolve_project_team_ids(&arguments)?;
                self.create_project(&name, description.as_deref(), team_ids)
                    .await
            }
            "update_issue" => {
                let issue_id = require_string(&arguments, "issue_id")?;
                let title = optional_string(&arguments, "title")?;
                let description = optional_string(&arguments, "description")?;
                let assignee_id = optional_string(&arguments, "assignee_id")?;
                let priority = parse_optional_priority(&arguments)?;
                let state_id = optional_string(&arguments, "state_id")?;
                let project_id = optional_string(&arguments, "project_id")?;
                self.update_issue(
                    &issue_id,
                    title.as_deref(),
                    description.as_deref(),
                    assignee_id.as_deref(),
                    priority,
                    state_id.as_deref(),
                    project_id.as_deref(),
                )
                .await
            }
            "assign_issue" => {
                let issue_id = require_string(&arguments, "issue_id")?;
                let assignee_id = require_string(&arguments, "assignee_id")?;
                self.assign_issue(&issue_id, &assignee_id).await
            }
            "comment_issue" => {
                let issue_id = require_string(&arguments, "issue_id")?;
                let body = require_string(&arguments, "body")?;
                self.comment_issue(&issue_id, &body).await
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

fn parse_max_results(arguments: &serde_json::Value) -> Result<usize> {
    match arguments.get("max_results") {
        None => Ok(DEFAULT_MAX_RESULTS),
        Some(value) => {
            let value = value.as_u64().ok_or_else(|| {
                ToolError::InvalidArguments("max_results must be an integer".to_string())
            })?;
            let value = usize::try_from(value).map_err(|_| {
                ToolError::InvalidArguments("max_results is out of range".to_string())
            })?;
            if !(1..=MAX_RESULTS_CAP).contains(&value) {
                return Err(ToolError::InvalidArguments(format!(
                    "max_results must be between 1 and {MAX_RESULTS_CAP}"
                )));
            }
            Ok(value)
        }
    }
}

fn parse_optional_priority(arguments: &serde_json::Value) -> Result<Option<i64>> {
    let Some(value) = arguments.get("priority") else {
        return Ok(None);
    };
    let value = value
        .as_i64()
        .ok_or_else(|| ToolError::InvalidArguments("priority must be an integer".to_string()))?;
    Ok(Some(value))
}

fn optional_string_list(arguments: &serde_json::Value, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolError::InvalidArguments(format!("key {key} must be an array of strings"))
    })?;
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        let Some(item) = value.as_str() else {
            return Err(ToolError::InvalidArguments(format!(
                "key {key} must be an array of strings"
            )));
        };
        let item = item.trim();
        if item.is_empty() {
            return Err(ToolError::InvalidArguments(format!(
                "key {key} cannot contain empty strings"
            )));
        }
        items.push(item.to_string());
    }
    Ok(Some(items))
}

fn resolve_project_team_ids(arguments: &serde_json::Value) -> Result<Option<Vec<String>>> {
    let team_ids = optional_string_list(arguments, "team_ids")?;
    let team_id_alias = optional_string(arguments, "team_id")?;
    let mut combined = Vec::new();

    if let Some(values) = team_ids {
        combined.extend(values);
    }
    if let Some(value) = team_id_alias {
        let value = value.trim();
        if !value.is_empty() {
            combined.push(value.to_string());
        }
    }

    if combined.is_empty() {
        return Ok(None);
    }

    combined.sort();
    combined.dedup();
    Ok(Some(combined))
}

fn looks_like_uuid(value: &str) -> bool {
    let mut parts = value.split('-');
    let expected = [8, 4, 4, 4, 12];
    for expected_len in expected {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != expected_len || !part.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return false;
        }
    }
    parts.next().is_none()
}

#[derive(Debug, Deserialize)]
struct GraphqlResponse {
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct GraphqlError {
    message: String,
    #[serde(default)]
    path: Vec<serde_json::Value>,
    #[serde(default)]
    extensions: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LinearViewerOnlyResponse {
    viewer: LinearViewer,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearViewer {
    id: String,
    name: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinearAssignedIssuesData {
    viewer: LinearAssignedIssuesViewer,
}

#[derive(Debug, Deserialize)]
struct LinearAssignedIssuesViewer {
    #[serde(rename = "assignedIssues")]
    assigned_issues: LinearIssueConnection,
}

#[derive(Debug, Deserialize)]
struct LinearIssueConnection {
    #[serde(default)]
    nodes: Vec<LinearIssue>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearIssue {
    id: String,
    identifier: String,
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    team: Option<LinearTeam>,
    #[serde(default)]
    assignee: Option<LinearUser>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearTeam {
    id: String,
    key: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct LinearUsersResponse {
    users: LinearUsersConnection,
}

#[derive(Debug, Deserialize)]
struct LinearTeamsResponse {
    teams: LinearTeamsConnection,
}

#[derive(Debug, Deserialize)]
struct LinearTeamsConnection {
    #[serde(default)]
    nodes: Vec<LinearTeam>,
}

#[derive(Debug, Deserialize)]
struct LinearProjectsResponse {
    projects: LinearProjectsConnection,
}

#[derive(Debug, Deserialize)]
struct LinearProjectsConnection {
    #[serde(default)]
    nodes: Vec<LinearProject>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearProject {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinearUsersConnection {
    #[serde(default)]
    nodes: Vec<LinearUser>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct LinearUser {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    active: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        optional_string_list, parse_max_results, parse_optional_priority, resolve_project_team_ids,
    };
    use crate::error::ToolError;

    #[test]
    fn parse_max_results_defaults_when_missing() {
        let args = serde_json::json!({});
        let value = parse_max_results(&args).expect("default max_results");
        assert_eq!(value, 20);
    }

    #[test]
    fn parse_max_results_rejects_out_of_range() {
        let args = serde_json::json!({"max_results": 1000});
        let err = parse_max_results(&args).expect_err("out of range");
        match err {
            ToolError::InvalidArguments(msg) => {
                assert!(msg.contains("max_results must be between"))
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_optional_priority_accepts_integer() {
        let args = serde_json::json!({"priority": 3});
        let value = parse_optional_priority(&args).expect("priority parse");
        assert_eq!(value, Some(3));
    }

    #[test]
    fn parse_optional_priority_rejects_non_integer() {
        let args = serde_json::json!({"priority": "high"});
        let err = parse_optional_priority(&args).expect_err("non integer priority");
        match err {
            ToolError::InvalidArguments(msg) => {
                assert!(msg.contains("priority must be an integer"))
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn optional_string_list_accepts_string_arrays() {
        let args = serde_json::json!({"team_ids": ["team_a", "team_b"]});
        let values = optional_string_list(&args, "team_ids").expect("parse team_ids");
        assert_eq!(
            values,
            Some(vec!["team_a".to_string(), "team_b".to_string()])
        );
    }

    #[test]
    fn optional_string_list_rejects_non_string_item() {
        let args = serde_json::json!({"team_ids": ["team_a", 123]});
        let err = optional_string_list(&args, "team_ids").expect_err("non string item");
        match err {
            ToolError::InvalidArguments(msg) => {
                assert!(msg.contains("array of strings"))
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn looks_like_uuid_accepts_valid_uuid_shape() {
        assert!(super::looks_like_uuid(
            "123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!super::looks_like_uuid("OPS-123"));
        assert!(!super::looks_like_uuid("not-a-uuid"));
    }

    #[test]
    fn resolve_project_team_ids_supports_team_id_alias() {
        let args = serde_json::json!({"team_id":"team_a"});
        let values = resolve_project_team_ids(&args).expect("parse team_id alias");
        assert_eq!(values, Some(vec!["team_a".to_string()]));
    }

    #[test]
    fn resolve_project_team_ids_merges_and_dedupes() {
        let args = serde_json::json!({"team_id":"team_a","team_ids":["team_b","team_a"]});
        let values = resolve_project_team_ids(&args).expect("merge team ids");
        assert_eq!(
            values,
            Some(vec!["team_a".to_string(), "team_b".to_string()])
        );
    }
}
