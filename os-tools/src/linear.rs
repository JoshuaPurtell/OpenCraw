use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

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

const QUERY_ISSUE_PRIORITY_VALUES: &str = r#"
query IssuePriorityValues {
  issuePriorityValues {
    priority
    label
  }
}
"#;

const QUERY_PROJECT_STATUSES: &str = r#"
query ProjectStatuses($first: Int!) {
  projectStatuses(first: $first, orderBy: updatedAt) {
    nodes {
      id
      name
      type
    }
  }
}
"#;

const QUERY_ISSUE_BY_ID: &str = r#"
query IssueById($id: String!) {
  issue(id: $id) {
    id
    identifier
    team {
      id
      key
      name
    }
  }
}
"#;

const QUERY_ISSUES_BY_IDENTIFIER: &str = r#"
query IssuesByIdentifier($id: ID!, $first: Int!) {
  issues(filter: { id: { eq: $id } }, first: $first, orderBy: updatedAt) {
    nodes {
      id
      identifier
      team {
        id
        key
        name
      }
    }
  }
}
"#;

const QUERY_TEAM_STATES: &str = r#"
query TeamStates($id: String!, $first: Int!) {
  team(id: $id) {
    states(first: $first, orderBy: updatedAt) {
      nodes {
        id
        name
        type
      }
    }
  }
}
"#;

const QUERY_GET_PROJECT: &str = r#"
query ProjectById($id: String!) {
  project(id: $id) {
    id
    name
    url
  }
}
"#;

const QUERY_PROJECT_BY_ID_VERIFY_STATE_SCALAR: &str = r#"
query ProjectById($id: String!) {
  project(id: $id) {
    id
    name
    url
    priority
    state
  }
}
"#;

const QUERY_PROJECT_BY_ID_VERIFY_STATUS_OBJECT: &str = r#"
query ProjectById($id: String!) {
  project(id: $id) {
    id
    name
    url
    priority
    status {
      id
      name
      type
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

const MUTATION_PROJECT_UPDATE: &str = r#"
mutation ProjectUpdate($id: String!, $input: ProjectUpdateInput!) {
  projectUpdate(id: $id, input: $input) {
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
pub struct LinearLimits {
    pub default_max_results: usize,
    pub max_results_cap: usize,
    pub reference_lookup_max_results: usize,
    pub graphql_max_query_chars: usize,
    pub graphql_max_variables_bytes: usize,
}

impl Default for LinearLimits {
    fn default() -> Self {
        Self {
            default_max_results: 20,
            max_results_cap: 100,
            reference_lookup_max_results: 100,
            graphql_max_query_chars: 64_000,
            graphql_max_variables_bytes: 128_000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LinearActionToggles {
    pub whoami: bool,
    pub list_assigned: bool,
    pub list_users: bool,
    pub list_teams: bool,
    pub list_projects: bool,
    pub get_project: bool,
    pub create_issue: bool,
    pub create_project: bool,
    pub update_project: bool,
    pub update_issue: bool,
    pub assign_issue: bool,
    pub comment_issue: bool,
    pub graphql_query: bool,
    pub graphql_mutation: bool,
}

impl LinearActionToggles {
    pub fn all_enabled() -> Self {
        Self {
            whoami: true,
            list_assigned: true,
            list_users: true,
            list_teams: true,
            list_projects: true,
            get_project: true,
            create_issue: true,
            create_project: true,
            update_project: true,
            update_issue: true,
            assign_issue: true,
            comment_issue: true,
            graphql_query: true,
            graphql_mutation: true,
        }
    }

    fn allows(self, action: &str) -> bool {
        match action {
            "whoami" => self.whoami,
            "list_assigned" => self.list_assigned,
            "list_users" => self.list_users,
            "list_teams" => self.list_teams,
            "list_projects" => self.list_projects,
            "get_project" => self.get_project,
            "create_issue" => self.create_issue,
            "create_project" => self.create_project,
            "update_project" => self.update_project,
            "update_issue" => self.update_issue,
            "assign_issue" => self.assign_issue,
            "comment_issue" => self.comment_issue,
            "graphql_query" => self.graphql_query,
            "graphql_mutation" => self.graphql_mutation,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LinearToolConfig {
    pub graphql_url: String,
    pub default_team_id: Option<String>,
    pub action_toggles: LinearActionToggles,
    pub limits: LinearLimits,
}

#[derive(Clone)]
pub struct LinearTool {
    http: reqwest::Client,
    graphql_url: Url,
    api_key: String,
    default_team_id: Option<String>,
    action_toggles: LinearActionToggles,
    limits: LinearLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LinearPriorityInput {
    Numeric(i64),
    Label(String),
}

#[derive(Clone, Debug)]
struct ResolvedProjectStatusReference {
    id: String,
    name: Option<String>,
}

struct UpdateIssueFields<'a> {
    title: Option<&'a str>,
    description: Option<&'a str>,
    assignee_id: Option<&'a str>,
    priority: Option<LinearPriorityInput>,
    state_ref: Option<&'a str>,
    project_ref: Option<&'a str>,
}

struct CreateIssueFields<'a> {
    title: &'a str,
    description: Option<&'a str>,
    team_ref: Option<&'a str>,
    assignee_id: Option<&'a str>,
    priority: Option<LinearPriorityInput>,
    project_ref: Option<&'a str>,
    state_ref: Option<&'a str>,
}

impl LinearTool {
    pub fn new(api_key: &str, config: LinearToolConfig) -> Result<Self> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ToolError::InvalidArguments(
                "linear api key is required".to_string(),
            ));
        }
        let graphql_url_raw = config.graphql_url.trim();
        if graphql_url_raw.is_empty() {
            return Err(ToolError::InvalidArguments(
                "linear graphql_url is required".to_string(),
            ));
        }
        let graphql_url = Url::parse(graphql_url_raw)
            .map_err(|e| ToolError::InvalidArguments(format!("invalid linear graphql_url: {e}")))?;
        let limits = config.limits;
        validate_linear_limits(limits)?;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let default_team_id = config
            .default_team_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Ok(Self {
            http,
            graphql_url,
            api_key: api_key.to_string(),
            default_team_id,
            action_toggles: config.action_toggles,
            limits,
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

    async fn get_project(&self, project_ref: &str) -> Result<serde_json::Value> {
        let project_id = self.resolve_project_reference(project_ref).await?;
        let data = self
            .graphql(
                QUERY_GET_PROJECT,
                json!({
                    "id": project_id,
                }),
            )
            .await?;
        let project = data.get("project").ok_or_else(|| {
            ToolError::ExecutionFailed("linear response missing project".to_string())
        })?;
        if project.is_null() {
            return Err(ToolError::ExecutionFailed(
                "linear project query returned null project".to_string(),
            ));
        }
        Ok(json!({
            "project": project
        }))
    }

    async fn create_issue(&self, fields: CreateIssueFields<'_>) -> Result<serde_json::Value> {
        let title = fields.title.trim();
        if title.is_empty() {
            return Err(ToolError::InvalidArguments(
                "title must not be empty".to_string(),
            ));
        }

        let team_ref = fields
            .team_ref
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.default_team_id.clone())
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "team_id is required (or set channels.linear.default_team_id)".to_string(),
                )
            })?;
        let team_id = self.resolve_team_reference(team_ref.as_str()).await?;

        let assignee_id = self.resolve_assignee_reference(fields.assignee_id).await?;
        let description = fields
            .description
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let project_id = match fields
            .project_ref
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(project_ref) => Some(self.resolve_project_reference(project_ref).await?),
            None => None,
        };
        let state_id = self
            .resolve_issue_state_for_team(&team_id, fields.state_ref)
            .await?;
        let priority = self.resolve_issue_priority_input(fields.priority).await?;
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
        if let Some(value) = project_id {
            input.insert("projectId".to_string(), json!(value));
        }
        if let Some(value) = state_id {
            input.insert("stateId".to_string(), json!(value));
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
        team_refs: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ToolError::InvalidArguments(
                "name must not be empty".to_string(),
            ));
        }
        let description = description.map(str::trim).filter(|value| !value.is_empty());

        let mut team_ids = self
            .resolve_team_references(team_refs.unwrap_or_default())
            .await?;
        if team_ids.is_empty() {
            if let Some(default_team_ref) = self
                .default_team_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                team_ids.push(self.resolve_team_reference(default_team_ref).await?);
            }
        }
        if team_ids.is_empty() {
            return Err(ToolError::InvalidArguments(
                "team_ids must include at least one team id (or set channels.linear.default_team_id)"
                    .to_string(),
            ));
        }
        team_ids.sort();
        team_ids.dedup();
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

    async fn update_project(
        &self,
        project_ref: &str,
        name: Option<&str>,
        description: Option<&str>,
        priority: Option<LinearPriorityInput>,
        status_ref: Option<&str>,
    ) -> Result<serde_json::Value> {
        let project_id = self.resolve_project_reference(project_ref).await?;
        let name = name.map(str::trim).filter(|value| !value.is_empty());
        let description = description.map(str::trim).filter(|value| !value.is_empty());
        let resolved_status = self.resolve_project_status_reference(status_ref).await?;
        let priority = self.resolve_project_priority_input(priority)?;

        let mut input = serde_json::Map::new();
        if let Some(value) = name {
            input.insert("name".to_string(), json!(value));
        }
        if let Some(value) = description {
            input.insert("description".to_string(), json!(value));
        }
        if let Some(value) = priority {
            input.insert("priority".to_string(), json!(value));
        }
        if let Some(status) = resolved_status.as_ref() {
            input.insert("statusId".to_string(), json!(status.id));
        }
        if input.is_empty() {
            return Err(ToolError::InvalidArguments(
                "update_project requires at least one field: name, description, priority, status/state".to_string(),
            ));
        }

        let data = self
            .graphql(
                MUTATION_PROJECT_UPDATE,
                json!({
                    "id": project_id,
                    "input": input,
                }),
            )
            .await?;
        let payload = data
            .get("projectUpdate")
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear response missing projectUpdate".to_string())
            })?
            .clone();
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("linear projectUpdate missing success".to_string())
            })?;
        if !success {
            return Err(ToolError::ExecutionFailed(
                "linear projectUpdate returned success=false".to_string(),
            ));
        }
        let project = payload.get("project").cloned().ok_or_else(|| {
            ToolError::ExecutionFailed("linear projectUpdate missing project".to_string())
        })?;

        let requested_fields = requested_project_update_fields(
            name,
            description,
            priority,
            resolved_status.as_ref().map(|status| status.id.as_str()),
        );
        let verification = if requested_fields.is_empty() {
            json!({
                "attempted": false,
                "verified": false,
                "requested_fields": []
            })
        } else {
            match self
                .fetch_project_snapshot_for_verification(&project_id)
                .await
            {
                Ok(snapshot) => {
                    let mut verified_fields = Vec::new();
                    let mut unverified_fields = Vec::new();
                    let mut mismatches = Vec::new();

                    if let Some(expected_name) = name {
                        match project_snapshot_name(&snapshot) {
                            Some(actual) if actual == expected_name => {
                                verified_fields.push("name".to_string())
                            }
                            Some(actual) => mismatches.push(format!(
                                "name expected {expected_name:?} but got {actual:?}"
                            )),
                            None => unverified_fields.push("name".to_string()),
                        }
                    }
                    if let Some(expected_priority) = priority {
                        match project_snapshot_priority(&snapshot) {
                            Some(actual) if actual == expected_priority => {
                                verified_fields.push("priority".to_string())
                            }
                            Some(actual) => mismatches.push(format!(
                                "priority expected {expected_priority} but got {actual}"
                            )),
                            None => unverified_fields.push("priority".to_string()),
                        }
                    }
                    if let Some(expected_status) = resolved_status.as_ref() {
                        match project_snapshot_status_id(&snapshot) {
                            Some(actual_id)
                                if normalize_for_compare(actual_id)
                                    == normalize_for_compare(expected_status.id.as_str()) =>
                            {
                                verified_fields.push("status".to_string());
                            }
                            Some(actual_id) => mismatches.push(format!(
                                "status.id expected {:?} but got {:?}",
                                expected_status.id, actual_id
                            )),
                            None => match (
                                expected_status.name.as_deref(),
                                project_snapshot_status_label(&snapshot),
                            ) {
                                (Some(expected_name), Some(actual_name))
                                    if normalize_for_compare(expected_name)
                                        == normalize_for_compare(actual_name.as_str()) =>
                                {
                                    verified_fields.push("status".to_string());
                                }
                                (Some(expected_name), Some(actual_name)) => {
                                    mismatches.push(format!(
                                        "status.name expected {:?} but got {:?}",
                                        expected_name, actual_name
                                    ))
                                }
                                _ => unverified_fields.push("status".to_string()),
                            },
                        }
                    }
                    if description.is_some() {
                        // The current verification snapshot intentionally avoids
                        // requesting project description fields to keep schema
                        // compatibility with narrower Linear API surfaces.
                        unverified_fields.push("description".to_string());
                    }

                    if !mismatches.is_empty() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "linear project update verification failed: {}",
                            mismatches.join("; ")
                        )));
                    }

                    json!({
                        "attempted": true,
                        "verified": unverified_fields.is_empty(),
                        "requested_fields": requested_fields,
                        "verified_fields": verified_fields,
                        "unverified_fields": unverified_fields,
                        "project": snapshot
                    })
                }
                Err(warning) => json!({
                    "attempted": true,
                    "verified": false,
                    "requested_fields": requested_fields,
                    "warning": warning
                }),
            }
        };

        Ok(json!({
            "status": "updated",
            "project": project,
            "verification": verification,
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
        fields: UpdateIssueFields<'_>,
    ) -> Result<serde_json::Value> {
        let issue_id = self.resolve_issue_reference(issue_id).await?;
        let assignee_id = self.resolve_assignee_reference(fields.assignee_id).await?;
        let title = fields
            .title
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let description = fields
            .description
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let state_id = self
            .resolve_issue_state_for_issue(&issue_id, fields.state_ref)
            .await?;
        let project_id = match fields
            .project_ref
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(project_ref) => Some(self.resolve_project_reference(project_ref).await?),
            None => None,
        };
        let priority = self.resolve_issue_priority_input(fields.priority).await?;

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
                "update_issue requires at least one field: title, description, assignee_id, priority, state/state_id, project_id/project_ref".to_string(),
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

    async fn fetch_issue_priority_values(&self) -> Result<Vec<LinearPriorityValue>> {
        let data = self.graphql(QUERY_ISSUE_PRIORITY_VALUES, json!({})).await?;
        let parsed: LinearIssuePriorityValuesResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if parsed.issue_priority_values.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "linear issuePriorityValues returned no values".to_string(),
            ));
        }
        Ok(parsed.issue_priority_values)
    }

    async fn resolve_issue_priority_input(
        &self,
        priority: Option<LinearPriorityInput>,
    ) -> Result<Option<i64>> {
        let Some(priority) = priority else {
            return Ok(None);
        };
        let values = self.fetch_issue_priority_values().await?;
        if values.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "linear issuePriorityValues returned no values".to_string(),
            ));
        }
        match priority {
            LinearPriorityInput::Numeric(value) => {
                if values.iter().any(|entry| entry.priority == value) {
                    return Ok(Some(value));
                }
                let available = format_priority_values(&values);
                Err(ToolError::InvalidArguments(format!(
                    "priority value {value} is not valid; available: {available}"
                )))
            }
            LinearPriorityInput::Label(label) => {
                let normalized_input = normalize_for_compare(label.as_str());
                let matches = values
                    .iter()
                    .filter(|entry| normalize_for_compare(entry.label.as_str()) == normalized_input)
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    return Ok(matches.first().map(|entry| entry.priority));
                }
                if matches.len() > 1 {
                    let labels = matches
                        .iter()
                        .map(|entry| entry.label.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(ToolError::InvalidArguments(format!(
                        "priority label {label:?} is ambiguous; matches: {labels}"
                    )));
                }
                let available = format_priority_values(&values);
                Err(ToolError::InvalidArguments(format!(
                    "unknown priority label {label:?}; available: {available}"
                )))
            }
        }
    }

    fn resolve_project_priority_input(
        &self,
        priority: Option<LinearPriorityInput>,
    ) -> Result<Option<i64>> {
        let Some(priority) = priority else {
            return Ok(None);
        };
        match priority {
            LinearPriorityInput::Numeric(value) => {
                if (0..=4).contains(&value) {
                    return Ok(Some(value));
                }
                Err(ToolError::InvalidArguments(format!(
                    "project priority value {value} is not valid; available: 0 (No priority), 1 (Urgent), 2 (High), 3 (Normal), 4 (Low)"
                )))
            }
            LinearPriorityInput::Label(label) => {
                let Some(value) = resolve_project_priority_label(label.as_str()) else {
                    return Err(ToolError::InvalidArguments(format!(
                        "unknown project priority label {label:?}; available: no priority, urgent, high, normal, low"
                    )));
                };
                Ok(Some(value))
            }
        }
    }

    async fn resolve_project_status_reference(
        &self,
        status_ref: Option<&str>,
    ) -> Result<Option<ResolvedProjectStatusReference>> {
        let Some(status_ref) = status_ref.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        if looks_like_uuid(status_ref) {
            return Ok(Some(ResolvedProjectStatusReference {
                id: status_ref.to_string(),
                name: None,
            }));
        }

        let data = self
            .graphql(
                QUERY_PROJECT_STATUSES,
                json!({
                    "first": self.limits.reference_lookup_max_results,
                }),
            )
            .await?;
        let parsed: LinearProjectStatusesResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let statuses = parsed.project_statuses.nodes;
        if statuses.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "linear projectStatuses returned no statuses".to_string(),
            ));
        }

        let normalized_ref = normalize_for_compare(status_ref);
        let exact_name = statuses
            .iter()
            .filter(|status| normalize_for_compare(status.name.as_str()) == normalized_ref)
            .collect::<Vec<_>>();
        if exact_name.len() == 1 {
            return Ok(exact_name
                .first()
                .map(|status| status.to_resolved_reference()));
        }
        if exact_name.len() > 1 {
            let names = exact_name
                .iter()
                .map(|status| format!("{} ({})", status.name, status.status_type))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "project status {status_ref:?} is ambiguous: {names}; pass UUID statusId"
            )));
        }

        let exact_type = statuses
            .iter()
            .filter(|status| normalize_for_compare(status.status_type.as_str()) == normalized_ref)
            .collect::<Vec<_>>();
        if exact_type.len() == 1 {
            return Ok(exact_type
                .first()
                .map(|status| status.to_resolved_reference()));
        }
        if exact_type.len() > 1 {
            let names = exact_type
                .iter()
                .map(|status| status.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "project status {status_ref:?} matched status type with multiple values: {names}; pass exact status name or UUID statusId"
            )));
        }

        let contains = statuses
            .iter()
            .filter(|status| {
                normalize_for_compare(status.name.as_str()).contains(normalized_ref.as_str())
            })
            .collect::<Vec<_>>();
        if contains.len() == 1 {
            return Ok(contains
                .first()
                .map(|status| status.to_resolved_reference()));
        }
        if contains.len() > 1 {
            let names = contains
                .iter()
                .map(|status| format!("{} ({})", status.name, status.status_type))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "project status {status_ref:?} matched multiple statuses: {names}; pass exact status name or UUID statusId"
            )));
        }
        let available = statuses
            .iter()
            .map(|status| format!("{} ({})", status.name, status.status_type))
            .collect::<Vec<_>>()
            .join(", ");
        Err(ToolError::InvalidArguments(format!(
            "project status {status_ref:?} was not found; available statuses: {available}"
        )))
    }

    async fn resolve_team_references(&self, refs: Vec<String>) -> Result<Vec<String>> {
        let mut team_ids = Vec::with_capacity(refs.len());
        for team_ref in refs {
            let team_ref = team_ref.trim();
            if team_ref.is_empty() {
                continue;
            }
            team_ids.push(self.resolve_team_reference(team_ref).await?);
        }
        team_ids.sort();
        team_ids.dedup();
        Ok(team_ids)
    }

    async fn resolve_team_reference(&self, team_ref: &str) -> Result<String> {
        let team_ref = team_ref.trim();
        if team_ref.is_empty() {
            return Err(ToolError::InvalidArguments(
                "team_id must not be empty".to_string(),
            ));
        }
        if looks_like_uuid(team_ref) {
            return Ok(team_ref.to_string());
        }

        let data = self
            .graphql(
                QUERY_LIST_TEAMS,
                json!({
                    "first": self.limits.reference_lookup_max_results,
                }),
            )
            .await?;
        let parsed: LinearTeamsResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let needle = team_ref.to_ascii_lowercase();
        let mut exact_id: Option<String> = None;
        let mut exact_key: Option<String> = None;
        let mut exact_name: Option<String> = None;
        let mut contains_matches: Vec<(String, String, String)> = Vec::new();
        for team in parsed.teams.nodes {
            if team.id.eq_ignore_ascii_case(team_ref) {
                exact_id = Some(team.id);
                break;
            }
            if team.key.eq_ignore_ascii_case(team_ref) {
                exact_key = Some(team.id.clone());
            }
            if team.name.eq_ignore_ascii_case(team_ref) {
                exact_name = Some(team.id.clone());
            }
            if team.key.to_ascii_lowercase().contains(&needle)
                || team.name.to_ascii_lowercase().contains(&needle)
            {
                contains_matches.push((team.id, team.key, team.name));
            }
        }
        if let Some(id) = exact_id.or(exact_key).or(exact_name) {
            return Ok(id);
        }
        if contains_matches.len() == 1 {
            return Ok(contains_matches
                .into_iter()
                .next()
                .map(|(id, _, _)| id)
                .unwrap_or_default());
        }
        if contains_matches.len() > 1 {
            let names = contains_matches
                .into_iter()
                .map(|(_, key, name)| format!("{name} ({key})"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "team reference {team_ref:?} matched multiple teams: {names}; pass a UUID team_id"
            )));
        }

        Err(ToolError::InvalidArguments(format!(
            "team reference {team_ref:?} was not found; pass a UUID team_id, exact team key, or exact team name"
        )))
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
                QUERY_ISSUES_BY_IDENTIFIER,
                json!({
                    "id": issue_ref,
                    "first": self.limits.reference_lookup_max_results,
                }),
            )
            .await?;
        let parsed: LinearIssueRefsResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mut exact_identifier_matches: Vec<LinearIssueRef> = parsed
            .issues
            .nodes
            .iter()
            .filter(|issue| issue.identifier.eq_ignore_ascii_case(issue_ref))
            .cloned()
            .collect();
        if exact_identifier_matches.len() == 1 {
            return Ok(exact_identifier_matches
                .pop()
                .map(|issue| issue.id)
                .unwrap_or_default());
        }
        if parsed.issues.nodes.len() == 1 {
            return Ok(parsed
                .issues
                .nodes
                .into_iter()
                .next()
                .map(|issue| issue.id)
                .unwrap_or_default());
        }
        if exact_identifier_matches.len() > 1 {
            let refs = exact_identifier_matches
                .into_iter()
                .map(|issue| issue.identifier)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "issue reference {issue_ref:?} matched multiple issues: {refs}; pass a UUID issue_id"
            )));
        }

        // Compatibility fallback: if a workspace has unusual identifier settings,
        // check assigned issues before surfacing a hard failure.
        let data = self
            .graphql(
                QUERY_LIST_ASSIGNED,
                json!({
                    "first": self.limits.reference_lookup_max_results,
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

    async fn fetch_issue_team_id(&self, issue_id: &str) -> Result<String> {
        let data = self
            .graphql(
                QUERY_ISSUE_BY_ID,
                json!({
                    "id": issue_id,
                }),
            )
            .await?;
        let parsed: LinearIssueByIdResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let team = parsed.issue.team.ok_or_else(|| {
            ToolError::ExecutionFailed("linear issue is missing team data".to_string())
        })?;
        Ok(team.id)
    }

    async fn resolve_issue_state_for_issue(
        &self,
        issue_id: &str,
        state_ref: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(state_ref) = state_ref.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        if looks_like_uuid(state_ref) {
            return Ok(Some(state_ref.to_string()));
        }
        let team_id = self.fetch_issue_team_id(issue_id).await?;
        self.resolve_issue_state_for_team(&team_id, Some(state_ref))
            .await
    }

    async fn resolve_issue_state_for_team(
        &self,
        team_id: &str,
        state_ref: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(state_ref) = state_ref.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        if looks_like_uuid(state_ref) {
            return Ok(Some(state_ref.to_string()));
        }
        let team_id = self.resolve_team_reference(team_id).await?;
        let data = self
            .graphql(
                QUERY_TEAM_STATES,
                json!({
                    "id": team_id,
                    "first": self.limits.reference_lookup_max_results,
                }),
            )
            .await?;
        let parsed: LinearTeamStatesResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let states = parsed.team.states.nodes;
        if states.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "linear team has no workflow states".to_string(),
            ));
        }

        let normalized = normalize_for_compare(state_ref);
        let exact_name_matches = states
            .iter()
            .filter(|state| normalize_for_compare(state.name.as_str()) == normalized)
            .collect::<Vec<_>>();
        if exact_name_matches.len() == 1 {
            return Ok(exact_name_matches.first().map(|state| state.id.clone()));
        }
        if exact_name_matches.len() > 1 {
            let names = exact_name_matches
                .iter()
                .map(|state| format!("{} ({})", state.name, state.state_type))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "issue state {state_ref:?} is ambiguous for team {team_id}: {names}; pass UUID state_id"
            )));
        }

        let normalized_type_ref = normalize_for_compare(state_ref);
        let type_matches = states
            .iter()
            .filter(|state| normalize_for_compare(state.state_type.as_str()) == normalized_type_ref)
            .collect::<Vec<_>>();
        if type_matches.len() == 1 {
            return Ok(type_matches.first().map(|state| state.id.clone()));
        }
        if type_matches.len() > 1 {
            let names = type_matches
                .iter()
                .map(|state| state.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "issue state {state_ref:?} matched workflow type {normalized_type_ref:?} with multiple states: {names}; pass exact state name or UUID state_id"
            )));
        }

        let available = states
            .iter()
            .map(|state| format!("{} ({})", state.name, state.state_type))
            .collect::<Vec<_>>()
            .join(", ");
        Err(ToolError::InvalidArguments(format!(
            "issue state {state_ref:?} was not found for team {team_id}; available states: {available}"
        )))
    }

    async fn resolve_project_reference(&self, project_ref: &str) -> Result<String> {
        let project_ref = project_ref.trim();
        if project_ref.is_empty() {
            return Err(ToolError::InvalidArguments(
                "project_id must not be empty".to_string(),
            ));
        }
        if looks_like_uuid(project_ref) {
            return Ok(project_ref.to_string());
        }

        let data = self
            .graphql(
                QUERY_LIST_PROJECTS,
                json!({
                    "first": self.limits.reference_lookup_max_results,
                }),
            )
            .await?;
        let parsed: LinearProjectsResponse =
            serde_json::from_value(data).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let needle = project_ref.to_ascii_lowercase();
        let mut exact_match: Option<String> = None;
        let mut contains_matches: Vec<(String, String)> = Vec::new();
        for project in parsed.projects.nodes {
            if project.id.eq_ignore_ascii_case(project_ref) {
                return Ok(project.id);
            }
            let project_name = project.name.trim();
            if project_name.is_empty() {
                continue;
            }
            if project_name.eq_ignore_ascii_case(project_ref) {
                exact_match = Some(project.id);
                break;
            }
            if project_name.to_ascii_lowercase().contains(&needle) {
                contains_matches.push((project.id, project_name.to_string()));
            }
        }
        if let Some(id) = exact_match {
            return Ok(id);
        }
        if contains_matches.len() == 1 {
            return Ok(contains_matches
                .into_iter()
                .next()
                .map(|(id, _)| id)
                .unwrap_or_default());
        }
        if contains_matches.len() > 1 {
            let names = contains_matches
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "project reference {project_ref:?} matched multiple projects: {names}; pass a UUID project_id"
            )));
        }
        Err(ToolError::InvalidArguments(format!(
            "project reference {project_ref:?} was not found; pass a UUID project_id or exact project name"
        )))
    }

    async fn fetch_project_snapshot_for_verification(
        &self,
        project_id: &str,
    ) -> std::result::Result<serde_json::Value, String> {
        let queries = [
            QUERY_PROJECT_BY_ID_VERIFY_STATUS_OBJECT,
            QUERY_PROJECT_BY_ID_VERIFY_STATE_SCALAR,
        ];
        let mut errors = Vec::new();
        for query in queries {
            match self.graphql(query, json!({ "id": project_id })).await {
                Ok(data) => {
                    if let Some(project) = data.get("project") {
                        if project.is_null() {
                            return Err(format!(
                                "verification query returned null project for id {project_id}"
                            ));
                        }
                        return Ok(project.clone());
                    }
                    errors.push("verification query missing project field".to_string());
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(format!(
            "unable to fetch project verification snapshot: {}",
            errors.join(" | ")
        ))
    }

    async fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value> {
        validate_graphql_request_payload(
            query,
            &variables,
            self.limits.graphql_max_query_chars,
            self.limits.graphql_max_variables_bytes,
        )?;
        let payload = json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .http
            .post(self.graphql_url.clone())
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
            let hint = linear_graphql_http_error_hint(&body)
                .map(|value| format!(" hint: {value}"))
                .unwrap_or_default();
            return Err(ToolError::ExecutionFailed(format!(
                "linear graphql failed: status={status} body={body}{hint}"
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

fn linear_graphql_http_error_hint(body: &str) -> Option<&'static str> {
    if body.contains("Cannot query field \"statuses\" on type \"Project\"") {
        return Some(
            "Linear Project has no `statuses` field. Use `status { id name type }` or `state`.",
        );
    }
    None
}

#[async_trait]
impl Tool for LinearTool {
    fn spec(&self) -> ToolSpec {
        let max_results_cap = self.limits.max_results_cap;
        ToolSpec {
            name: "linear".to_string(),
            description: "Manage Linear via API. Includes typed actions for common workflows and raw GraphQL query/mutation actions for full schema coverage."
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
                            "get_project",
                            "create_issue",
                            "create_project",
                            "update_project",
                            "update_issue",
                            "assign_issue",
                            "comment_issue",
                            "graphql_query",
                            "graphql_mutation"
                        ]
                    },
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": max_results_cap },
                    "document": { "type": "string", "description": "GraphQL document for graphql_query/graphql_mutation actions." },
                    "variables": {
                        "type": "object",
                        "additionalProperties": true,
                        "description": "GraphQL variables map for graphql_query/graphql_mutation actions."
                    },
                    "title": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "team_id": { "type": "string", "description": "Team reference. Accepts UUID, team key, or exact team name." },
                    "team_key": { "type": "string", "description": "Alias for team_id using team key." },
                    "team_name": { "type": "string", "description": "Alias for team_id using exact team name." },
                    "team_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Preferred for create_project; team_id is accepted as a single-id alias."
                    },
                    "assignee_id": { "type": "string", "description": "Linear user id or 'me'" },
                    "priority": {
                        "oneOf": [
                            { "type": "integer", "minimum": 0, "maximum": 4 },
                            { "type": "string" }
                        ],
                        "description": "Priority. Accepts numeric value (0..4) or label. For issue actions, labels are resolved from Linear issuePriorityValues. For project actions, labels map to the fixed contract: 0 none, 1 urgent, 2 high, 3 normal, 4 low."
                    },
                    "issue_id": { "type": "string" },
                    "project_id": { "type": "string" },
                    "project_ref": { "type": "string", "description": "Alias for project_id; accepts UUID or project name." },
                    "project_name": { "type": "string", "description": "Optional human-readable project name; also accepted as project reference for update_project." },
                    "projectid": { "type": "string", "description": "Legacy alias for project_id." },
                    "state_id": { "type": "string" },
                    "stateid": { "type": "string", "description": "Alias for state_id." },
                    "status_id": { "type": "string", "description": "Project status UUID for update_project." },
                    "statusid": { "type": "string", "description": "Alias for status_id." },
                    "state": { "type": "string", "description": "State/status reference. For create/update_issue, accepts state UUID or exact state name in the issue's team workflow. For update_project, accepts project status UUID, exact project status name, or project status type." },
                    "status": { "type": "string", "description": "Alias for state reference." },
                    "project_status": { "type": "string", "description": "Alias for project status reference on update_project." },
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
        let action = normalize_linear_action(action.as_str());
        if !is_supported_linear_action(action) {
            return Err(ToolError::InvalidArguments(format!(
                "unknown action: {action}"
            )));
        }
        self.ensure_action_enabled(action)?;
        match action {
            "whoami" => self.whoami().await,
            "list_assigned" => {
                let max_results = parse_max_results(&arguments, self.limits)?;
                self.list_assigned(max_results).await
            }
            "list_users" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments, self.limits)?;
                self.list_users(query.as_deref(), max_results).await
            }
            "list_teams" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments, self.limits)?;
                self.list_teams(query.as_deref(), max_results).await
            }
            "list_projects" => {
                let query = optional_string(&arguments, "query")?;
                let max_results = parse_max_results(&arguments, self.limits)?;
                self.list_projects(query.as_deref(), max_results).await
            }
            "get_project" => {
                let project_ref = require_project_reference(&arguments)?;
                self.get_project(&project_ref).await
            }
            "create_issue" => {
                let title = require_string(&arguments, "title")?;
                let description = optional_string(&arguments, "description")?;
                let team_ref =
                    optional_string_first(&arguments, &["team_id", "team_key", "team_name"])?;
                let assignee_id = optional_string(&arguments, "assignee_id")?;
                let priority = parse_optional_priority(&arguments)?;
                let project_ref = optional_project_reference(&arguments)?;
                let state_ref = parse_optional_issue_state_reference(&arguments)?;
                self.create_issue(CreateIssueFields {
                    title: &title,
                    description: description.as_deref(),
                    team_ref: team_ref.as_deref(),
                    assignee_id: assignee_id.as_deref(),
                    priority,
                    project_ref: project_ref.as_deref(),
                    state_ref: state_ref.as_deref(),
                })
                .await
            }
            "create_project" => {
                let name = require_string(&arguments, "name")?;
                let description = optional_string(&arguments, "description")?;
                let team_ids = resolve_project_team_ids(&arguments)?;
                self.create_project(&name, description.as_deref(), team_ids)
                    .await
            }
            "update_project" => {
                let project_id = require_project_reference(&arguments)?;
                let name = optional_string(&arguments, "name")?;
                let description = optional_string(&arguments, "description")?;
                let priority = parse_optional_priority(&arguments)?;
                let status_ref = parse_optional_project_status_reference(&arguments)?;
                self.update_project(
                    &project_id,
                    name.as_deref(),
                    description.as_deref(),
                    priority,
                    status_ref.as_deref(),
                )
                .await
            }
            "update_issue" => {
                let issue_id = require_string(&arguments, "issue_id")?;
                let title = optional_string(&arguments, "title")?;
                let description = optional_string(&arguments, "description")?;
                let assignee_id = optional_string(&arguments, "assignee_id")?;
                let priority = parse_optional_priority(&arguments)?;
                let state_ref = parse_optional_issue_state_reference(&arguments)?;
                let project_ref = optional_project_reference(&arguments)?;
                self.update_issue(
                    &issue_id,
                    UpdateIssueFields {
                        title: title.as_deref(),
                        description: description.as_deref(),
                        assignee_id: assignee_id.as_deref(),
                        priority,
                        state_ref: state_ref.as_deref(),
                        project_ref: project_ref.as_deref(),
                    },
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
            "graphql_query" => {
                let document = require_graphql_document(&arguments)?;
                let variables = optional_graphql_variables(&arguments)?;
                let data = self.graphql(&document, variables).await?;
                Ok(json!({ "data": data }))
            }
            "graphql_mutation" => {
                let document = require_graphql_document(&arguments)?;
                let variables = optional_graphql_variables(&arguments)?;
                let data = self.graphql(&document, variables).await?;
                Ok(json!({ "data": data }))
            }
            _ => Err(ToolError::InvalidArguments("unknown action".to_string())),
        }
    }
}

fn parse_max_results(arguments: &serde_json::Value, limits: LinearLimits) -> Result<usize> {
    match arguments.get("max_results") {
        None => Ok(limits.default_max_results),
        Some(value) => {
            let value = value.as_u64().ok_or_else(|| {
                ToolError::InvalidArguments("max_results must be an integer".to_string())
            })?;
            let value = usize::try_from(value).map_err(|_| {
                ToolError::InvalidArguments("max_results is out of range".to_string())
            })?;
            if !(1..=limits.max_results_cap).contains(&value) {
                return Err(ToolError::InvalidArguments(format!(
                    "max_results must be between 1 and {}",
                    limits.max_results_cap
                )));
            }
            Ok(value)
        }
    }
}

fn require_graphql_document(arguments: &serde_json::Value) -> Result<String> {
    let document = require_string_first(arguments, &["document", "graphql", "query"], "document")?;
    if document.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "document must not be empty".to_string(),
        ));
    }
    Ok(document)
}

fn optional_graphql_variables(arguments: &serde_json::Value) -> Result<serde_json::Value> {
    let Some(value) = arguments.get("variables") else {
        return Ok(json!({}));
    };
    if !value.is_object() {
        return Err(ToolError::InvalidArguments(
            "variables must be an object".to_string(),
        ));
    }
    Ok(value.clone())
}

fn parse_optional_priority(arguments: &serde_json::Value) -> Result<Option<LinearPriorityInput>> {
    let Some(value) = arguments.get("priority") else {
        return Ok(None);
    };
    if let Some(raw) = value.as_i64() {
        return Ok(Some(LinearPriorityInput::Numeric(raw)));
    }
    if let Some(raw) = value.as_str() {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ToolError::InvalidArguments(
                "priority label must not be empty".to_string(),
            ));
        }
        return Ok(Some(LinearPriorityInput::Label(raw.to_string())));
    }
    Err(ToolError::InvalidArguments(
        "priority must be an integer or label string".to_string(),
    ))
}

fn parse_optional_project_status_reference(
    arguments: &serde_json::Value,
) -> Result<Option<String>> {
    optional_string_first(
        arguments,
        &["status_id", "statusid", "status", "project_status", "state"],
    )
}

fn parse_optional_issue_state_reference(arguments: &serde_json::Value) -> Result<Option<String>> {
    optional_string_first(arguments, &["state_id", "stateid", "state", "status"])
}

fn require_project_reference(arguments: &serde_json::Value) -> Result<String> {
    require_string_first(
        arguments,
        &[
            "project_id",
            "projectid",
            "project_ref",
            "project",
            "project_name",
        ],
        "project_id",
    )
}

fn optional_project_reference(arguments: &serde_json::Value) -> Result<Option<String>> {
    optional_string_first(
        arguments,
        &[
            "project_id",
            "projectid",
            "project_ref",
            "project",
            "project_name",
        ],
    )
}

fn require_string_first(
    arguments: &serde_json::Value,
    keys: &[&str],
    label: &str,
) -> Result<String> {
    optional_string_first(arguments, keys)?.ok_or_else(|| {
        ToolError::InvalidArguments(format!(
            "missing key: {label} (aliases: {})",
            keys.join(", ")
        ))
    })
}

fn optional_string_first(arguments: &serde_json::Value, keys: &[&str]) -> Result<Option<String>> {
    for key in keys {
        match arguments.get(*key) {
            None => continue,
            Some(serde_json::Value::String(value)) => return Ok(Some(value.clone())),
            Some(other) => {
                return Err(ToolError::InvalidArguments(format!(
                    "key {key} must be string, got {other:?}"
                )));
            }
        }
    }
    Ok(None)
}

fn normalize_linear_action(action: &str) -> &str {
    let trimmed = action.trim();
    if trimmed.eq_ignore_ascii_case("issuecreate") {
        return "create_issue";
    }
    if trimmed.eq_ignore_ascii_case("projectcreate") {
        return "create_project";
    }
    if trimmed.eq_ignore_ascii_case("projectupdate") {
        return "update_project";
    }
    if trimmed.eq_ignore_ascii_case("issueupdate") {
        return "update_issue";
    }
    if trimmed.eq_ignore_ascii_case("commentcreate") {
        return "comment_issue";
    }
    if trimmed.eq_ignore_ascii_case("viewer") {
        return "whoami";
    }
    if trimmed.eq_ignore_ascii_case("projects") {
        return "list_projects";
    }
    if trimmed.eq_ignore_ascii_case("users") {
        return "list_users";
    }
    if trimmed.eq_ignore_ascii_case("teams") {
        return "list_teams";
    }
    if trimmed.eq_ignore_ascii_case("assignedissues") {
        return "list_assigned";
    }
    if trimmed.eq_ignore_ascii_case("updateproject") {
        return "update_project";
    }
    if trimmed.eq_ignore_ascii_case("createproject") {
        return "create_project";
    }
    if trimmed.eq_ignore_ascii_case("createissue") {
        return "create_issue";
    }
    if trimmed.eq_ignore_ascii_case("updateissue") {
        return "update_issue";
    }
    if trimmed.eq_ignore_ascii_case("assignissue") {
        return "assign_issue";
    }
    if trimmed.eq_ignore_ascii_case("commentissue") {
        return "comment_issue";
    }
    if trimmed.eq_ignore_ascii_case("listprojects") {
        return "list_projects";
    }
    if trimmed.eq_ignore_ascii_case("getproject") {
        return "get_project";
    }
    if trimmed.eq_ignore_ascii_case("listusers") {
        return "list_users";
    }
    if trimmed.eq_ignore_ascii_case("listassigned") {
        return "list_assigned";
    }
    if trimmed.eq_ignore_ascii_case("listteams") {
        return "list_teams";
    }
    if trimmed.eq_ignore_ascii_case("graphqlquery")
        || trimmed.eq_ignore_ascii_case("query")
        || trimmed.eq_ignore_ascii_case("graphql_query")
    {
        return "graphql_query";
    }
    if trimmed.eq_ignore_ascii_case("graphqlmutation")
        || trimmed.eq_ignore_ascii_case("mutation")
        || trimmed.eq_ignore_ascii_case("graphql_mutation")
    {
        return "graphql_mutation";
    }
    trimmed
}

fn is_supported_linear_action(action: &str) -> bool {
    matches!(
        action,
        "whoami"
            | "list_assigned"
            | "list_users"
            | "list_teams"
            | "list_projects"
            | "get_project"
            | "create_issue"
            | "create_project"
            | "update_project"
            | "update_issue"
            | "assign_issue"
            | "comment_issue"
            | "graphql_query"
            | "graphql_mutation"
    )
}

fn validate_linear_limits(limits: LinearLimits) -> Result<()> {
    if limits.default_max_results == 0 {
        return Err(ToolError::InvalidArguments(
            "linear limits.default_max_results must be > 0".to_string(),
        ));
    }
    if limits.max_results_cap == 0 {
        return Err(ToolError::InvalidArguments(
            "linear limits.max_results_cap must be > 0".to_string(),
        ));
    }
    if limits.default_max_results > limits.max_results_cap {
        return Err(ToolError::InvalidArguments(
            "linear limits.default_max_results must be <= limits.max_results_cap".to_string(),
        ));
    }
    if limits.reference_lookup_max_results == 0 {
        return Err(ToolError::InvalidArguments(
            "linear limits.reference_lookup_max_results must be > 0".to_string(),
        ));
    }
    if limits.graphql_max_query_chars == 0 {
        return Err(ToolError::InvalidArguments(
            "linear limits.graphql_max_query_chars must be > 0".to_string(),
        ));
    }
    if limits.graphql_max_variables_bytes == 0 {
        return Err(ToolError::InvalidArguments(
            "linear limits.graphql_max_variables_bytes must be > 0".to_string(),
        ));
    }
    Ok(())
}

fn validate_graphql_request_payload(
    query: &str,
    variables: &serde_json::Value,
    max_query_chars: usize,
    max_variables_bytes: usize,
) -> Result<()> {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        return Err(ToolError::InvalidArguments(
            "graphql document must not be empty".to_string(),
        ));
    }
    if query_trimmed.chars().count() > max_query_chars {
        return Err(ToolError::InvalidArguments(format!(
            "graphql document exceeds max length ({max_query_chars} chars)"
        )));
    }
    if !variables.is_object() {
        return Err(ToolError::InvalidArguments(
            "graphql variables must be an object".to_string(),
        ));
    }
    let encoded = serde_json::to_vec(variables)
        .map_err(|e| ToolError::InvalidArguments(format!("invalid graphql variables JSON: {e}")))?;
    if encoded.len() > max_variables_bytes {
        return Err(ToolError::InvalidArguments(format!(
            "graphql variables exceed max size ({max_variables_bytes} bytes)"
        )));
    }
    Ok(())
}

fn requested_project_update_fields(
    name: Option<&str>,
    description: Option<&str>,
    priority: Option<i64>,
    state: Option<&str>,
) -> Vec<String> {
    let mut fields = Vec::new();
    if name.is_some() {
        fields.push("name".to_string());
    }
    if description.is_some() {
        fields.push("description".to_string());
    }
    if priority.is_some() {
        fields.push("priority".to_string());
    }
    if state.is_some() {
        fields.push("state".to_string());
    }
    fields
}

fn project_snapshot_name(snapshot: &serde_json::Value) -> Option<&str> {
    snapshot
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn project_snapshot_priority(snapshot: &serde_json::Value) -> Option<i64> {
    snapshot.get("priority").and_then(|value| value.as_i64())
}

fn project_snapshot_status_id(snapshot: &serde_json::Value) -> Option<&str> {
    snapshot
        .get("status")
        .and_then(|value| value.get("id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn project_snapshot_status_label(snapshot: &serde_json::Value) -> Option<String> {
    if let Some(value) = snapshot
        .get("status")
        .and_then(|status| status.get("name"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }

    let state = snapshot.get("state")?;
    if let Some(value) = state.as_str() {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    state
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_for_compare(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn resolve_project_priority_label(value: &str) -> Option<i64> {
    match normalize_for_compare(value).as_str() {
        "0" | "none" | "no priority" | "no-priority" | "unset" => Some(0),
        "1" | "urgent" | "p1" => Some(1),
        "2" | "high" | "p2" => Some(2),
        "3" | "normal" | "medium" | "med" | "p3" => Some(3),
        "4" | "low" | "p4" => Some(4),
        _ => None,
    }
}

fn format_priority_values(values: &[LinearPriorityValue]) -> String {
    values
        .iter()
        .map(|entry| format!("{} ({})", entry.label, entry.priority))
        .collect::<Vec<_>>()
        .join(", ")
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

#[derive(Debug, Deserialize)]
struct LinearIssueRefConnection {
    #[serde(default)]
    nodes: Vec<LinearIssueRef>,
}

#[derive(Debug, Deserialize, Clone)]
struct LinearIssueRef {
    id: String,
    identifier: String,
    #[serde(default)]
    team: Option<LinearTeam>,
}

#[derive(Debug, Deserialize)]
struct LinearIssueRefsResponse {
    issues: LinearIssueRefConnection,
}

#[derive(Debug, Deserialize)]
struct LinearIssueByIdResponse {
    issue: LinearIssueRef,
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

#[derive(Debug, Deserialize, serde::Serialize, Clone)]
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

#[derive(Debug, Deserialize)]
struct LinearProjectStatusConnection {
    #[serde(default)]
    nodes: Vec<LinearProjectStatus>,
}

#[derive(Debug, Deserialize)]
struct LinearProjectStatusesResponse {
    #[serde(rename = "projectStatuses")]
    project_statuses: LinearProjectStatusConnection,
}

#[derive(Debug, Deserialize, Clone)]
struct LinearProjectStatus {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    status_type: String,
}

impl LinearProjectStatus {
    fn to_resolved_reference(&self) -> ResolvedProjectStatusReference {
        let name = self.name.trim();
        ResolvedProjectStatusReference {
            id: self.id.clone(),
            name: (!name.is_empty()).then(|| self.name.clone()),
        }
    }
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

#[derive(Debug, Deserialize, Clone)]
struct LinearPriorityValue {
    priority: i64,
    label: String,
}

#[derive(Debug, Deserialize)]
struct LinearIssuePriorityValuesResponse {
    #[serde(rename = "issuePriorityValues")]
    issue_priority_values: Vec<LinearPriorityValue>,
}

#[derive(Debug, Deserialize)]
struct LinearWorkflowStateConnection {
    #[serde(default)]
    nodes: Vec<LinearWorkflowState>,
}

#[derive(Debug, Deserialize)]
struct LinearTeamStates {
    states: LinearWorkflowStateConnection,
}

#[derive(Debug, Deserialize)]
struct LinearTeamStatesResponse {
    team: LinearTeamStates,
}

#[derive(Debug, Deserialize, Clone)]
struct LinearWorkflowState {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    state_type: String,
}

#[cfg(test)]
mod tests {
    use super::{
        LinearLimits, LinearPriorityInput, normalize_linear_action, optional_string_list,
        parse_max_results, parse_optional_issue_state_reference, parse_optional_priority,
        parse_optional_project_status_reference, project_snapshot_status_id,
        project_snapshot_status_label, require_project_reference, resolve_project_priority_label,
        resolve_project_team_ids,
    };
    use crate::error::ToolError;

    fn test_limits() -> LinearLimits {
        LinearLimits {
            default_max_results: 20,
            max_results_cap: 100,
            reference_lookup_max_results: 100,
            graphql_max_query_chars: 64_000,
            graphql_max_variables_bytes: 128_000,
        }
    }

    #[test]
    fn parse_max_results_defaults_when_missing() {
        let args = serde_json::json!({});
        let value = parse_max_results(&args, test_limits()).expect("default max_results");
        assert_eq!(value, 20);
    }

    #[test]
    fn parse_max_results_rejects_out_of_range() {
        let args = serde_json::json!({"max_results": 1000});
        let err = parse_max_results(&args, test_limits()).expect_err("out of range");
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
        assert_eq!(value, Some(LinearPriorityInput::Numeric(3)));
    }

    #[test]
    fn parse_optional_priority_accepts_label() {
        let args = serde_json::json!({"priority": "high"});
        let value = parse_optional_priority(&args).expect("label parse");
        assert_eq!(value, Some(LinearPriorityInput::Label("high".to_string())));
    }

    #[test]
    fn parse_optional_priority_rejects_non_integer_or_string() {
        let args = serde_json::json!({"priority": true});
        let err = parse_optional_priority(&args).expect_err("invalid priority type");
        match err {
            ToolError::InvalidArguments(msg) => {
                assert!(msg.contains("priority must be an integer or label string"))
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

    #[test]
    fn parse_optional_project_status_reference_prefers_status_id_then_status() {
        let status_id = "123e4567-e89b-12d3-a456-426614174000";
        let status_args =
            serde_json::json!({"status_id":status_id,"status":"In Progress","state":"Planned"});
        assert_eq!(
            parse_optional_project_status_reference(&status_args).expect("parse status_id"),
            Some(status_id.to_string())
        );
    }

    #[test]
    fn parse_optional_project_status_reference_accepts_aliases() {
        let status_args = serde_json::json!({"status":"started"});
        assert_eq!(
            parse_optional_project_status_reference(&status_args).expect("parse status"),
            Some("started".to_string())
        );
        let project_status_args = serde_json::json!({"project_status":"started"});
        assert_eq!(
            parse_optional_project_status_reference(&project_status_args)
                .expect("parse project_status"),
            Some("started".to_string())
        );
    }

    #[test]
    fn parse_optional_issue_state_reference_prefers_state_id() {
        let state_id = "123e4567-e89b-12d3-a456-426614174000";
        let args = serde_json::json!({"state_id":state_id, "state":"In Progress"});
        assert_eq!(
            parse_optional_issue_state_reference(&args).expect("state_id alias"),
            Some(state_id.to_string())
        );
    }

    #[test]
    fn parse_optional_issue_state_reference_accepts_state_name() {
        let args = serde_json::json!({"state":"In Progress"});
        assert_eq!(
            parse_optional_issue_state_reference(&args).expect("state name accepted"),
            Some("In Progress".to_string())
        );
    }

    #[test]
    fn require_project_reference_accepts_aliases() {
        let args = serde_json::json!({"projectid":"proj-123"});
        assert_eq!(
            require_project_reference(&args).expect("projectid alias"),
            "proj-123".to_string()
        );
    }

    #[test]
    fn normalize_linear_action_accepts_compact_aliases() {
        assert_eq!(normalize_linear_action("updateproject"), "update_project");
        assert_eq!(normalize_linear_action("createissue"), "create_issue");
        assert_eq!(normalize_linear_action("getproject"), "get_project");
        assert_eq!(normalize_linear_action("projectUpdate"), "update_project");
        assert_eq!(normalize_linear_action("graphqlquery"), "graphql_query");
        assert_eq!(normalize_linear_action("mutation"), "graphql_mutation");
    }

    #[test]
    fn project_snapshot_status_helpers_prefer_status_object() {
        let snapshot = serde_json::json!({
            "status": {
                "id": "28d6dc49-a353-464c-bb30-085775df1491",
                "name": "In Progress"
            },
            "state": "started"
        });
        assert_eq!(
            project_snapshot_status_id(&snapshot),
            Some("28d6dc49-a353-464c-bb30-085775df1491")
        );
        assert_eq!(
            project_snapshot_status_label(&snapshot),
            Some("In Progress".to_string())
        );
    }

    #[test]
    fn project_snapshot_status_helpers_fallback_to_state_scalar() {
        let snapshot = serde_json::json!({
            "state": "started"
        });
        assert_eq!(project_snapshot_status_id(&snapshot), None);
        assert_eq!(
            project_snapshot_status_label(&snapshot),
            Some("started".to_string())
        );
    }

    #[test]
    fn resolve_project_priority_label_maps_expected_values() {
        assert_eq!(resolve_project_priority_label("urgent"), Some(1));
        assert_eq!(resolve_project_priority_label("high"), Some(2));
        assert_eq!(resolve_project_priority_label("normal"), Some(3));
        assert_eq!(resolve_project_priority_label("low"), Some(4));
        assert_eq!(resolve_project_priority_label("no priority"), Some(0));
        assert_eq!(resolve_project_priority_label("p3"), Some(3));
    }

    #[test]
    fn resolve_project_priority_label_rejects_unknown_value() {
        assert_eq!(resolve_project_priority_label("critical"), None);
    }
}
