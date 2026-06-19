//! A thin async wrapper over Linear's GraphQL API.
//!
//! Auth: a personal API key passed verbatim in the `Authorization` header
//! (Linear personal keys do NOT use a `Bearer ` prefix).

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::images;
use crate::models::{Issue, IssueDetail, Project, Team, User, View, WorkflowState};

const ENDPOINT: &str = "https://api.linear.app/graphql";

#[derive(Clone)]
pub struct LinearClient {
    http: reqwest::Client,
    api_key: String,
    page_size: u32,
}

impl LinearClient {
    pub fn new(api_key: String, page_size: u32) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("linear-tui/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            api_key,
            page_size,
        })
    }

    /// Execute a GraphQL operation and return the `data` object as raw JSON.
    async fn request(&self, query: &str, variables: Value) -> Result<Value> {
        let resp = self
            .http
            .post(ENDPOINT)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| {
            anyhow!("Linear returned a non-JSON response (HTTP {status}): {e}")
        })?;

        if let Some(errors) = body.get("errors") {
            if !errors.is_null() {
                let msg = errors
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown GraphQL error");
                return Err(anyhow!("Linear API error: {msg}"));
            }
        }

        body.get("data")
            .cloned()
            .ok_or_else(|| anyhow!("Linear response missing `data` (HTTP {status})"))
    }

    /// Run a query and deserialize the value at `data.<path>` into `T`.
    async fn query_at<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
        path: &[&str],
    ) -> Result<T> {
        let mut data = self.request(query, variables).await?;
        for key in path {
            data = data
                .get(*key)
                .cloned()
                .ok_or_else(|| anyhow!("missing field `{key}` in response"))?;
        }
        serde_json::from_value(data).map_err(|e| anyhow!("decode error: {e}"))
    }

    // ----- Queries -------------------------------------------------------

    /// The authenticated user.
    pub async fn viewer(&self) -> Result<User> {
        let q = r#"query { viewer { id name displayName } }"#;
        self.query_at(q, json!({}), &["viewer"]).await
    }

    /// All teams the key can see.
    pub async fn teams(&self) -> Result<Vec<Team>> {
        let q = r#"query { teams(first: 100) { nodes { id name key } } }"#;
        self.query_at(q, json!({}), &["teams", "nodes"]).await
    }

    /// Workflow states for a team (for the state-change picker).
    pub async fn team_states(&self, team_id: &str) -> Result<Vec<WorkflowState>> {
        let q = r#"
            query($id: String!) {
              team(id: $id) {
                states(first: 50) { nodes { id name type color } }
              }
            }"#;
        self.query_at(q, json!({ "id": team_id }), &["team", "states", "nodes"])
            .await
    }

    /// Projects belonging to a team, most-recently-updated first.
    pub async fn team_projects(&self, team_id: &str) -> Result<Vec<Project>> {
        let q = r#"
            query($id: String!) {
              team(id: $id) {
                projects(first: 100, orderBy: updatedAt) {
                  nodes { id name state }
                }
              }
            }"#;
        self.query_at(q, json!({ "id": team_id }), &["team", "projects", "nodes"])
            .await
    }

    /// Members of a team (for the assignee picker).
    pub async fn team_members(&self, team_id: &str) -> Result<Vec<User>> {
        let q = r#"
            query($id: String!) {
              team(id: $id) {
                members(first: 100) { nodes { id name displayName } }
              }
            }"#;
        self.query_at(q, json!({ "id": team_id }), &["team", "members", "nodes"])
            .await
    }

    /// Issues for a team filtered according to the chosen [`View`], optionally
    /// narrowed to a single project. View and project filters combine (AND).
    pub async fn issues(
        &self,
        team_id: &str,
        view: View,
        viewer_id: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let mut filter = match view {
            View::Active => json!({ "state": { "type": { "in": ["unstarted", "started"] } } }),
            View::Backlog => json!({ "state": { "type": { "eq": "backlog" } } }),
            View::MyIssues => json!({
                "assignee": { "id": { "eq": viewer_id } },
                "state": { "type": { "nin": ["completed", "canceled"] } }
            }),
            View::All => json!({}),
        };
        if let Some(pid) = project_id {
            filter["project"] = json!({ "id": { "eq": pid } });
        }

        let q = r#"
            query($id: String!, $filter: IssueFilter, $n: Int!) {
              team(id: $id) {
                issues(filter: $filter, first: $n, orderBy: updatedAt) {
                  nodes {
                    id identifier title priority
                    state { id name type color }
                    assignee { id name displayName }
                  }
                }
              }
            }"#;
        self.query_at(
            q,
            json!({ "id": team_id, "filter": filter, "n": self.page_size }),
            &["team", "issues", "nodes"],
        )
        .await
    }

    /// Full detail for a single issue, including comments.
    pub async fn issue_detail(&self, issue_id: &str) -> Result<IssueDetail> {
        let q = r#"
            query($id: String!) {
              issue(id: $id) {
                id identifier title description priority url
                state { id name type color }
                assignee { id name displayName }
                parent { id identifier title }
                children(first: 50) {
                  nodes {
                    id identifier title priority
                    state { id name type color }
                    assignee { id name displayName }
                  }
                }
                comments(first: 50) {
                  nodes { body createdAt user { id name displayName } }
                }
              }
            }"#;
        // `comments` and `children` arrive as `{ nodes: [...] }`; flatten the
        // connections to plain arrays before decoding.
        let mut data = self.request(q, json!({ "id": issue_id })).await?;
        let mut issue = data
            .get_mut("issue")
            .map(Value::take)
            .ok_or_else(|| anyhow!("issue not found"))?;
        for key in ["comments", "children"] {
            if let Some(nodes) = issue
                .get_mut(key)
                .and_then(|c| c.get_mut("nodes"))
                .map(Value::take)
            {
                issue[key] = nodes;
            }
        }
        serde_json::from_value(issue).map_err(|e| anyhow!("decode error: {e}"))
    }

    /// Download the raw bytes of an image URL. The personal API key is attached
    /// only for Linear-hosted URLs (where it's required), never for third-party
    /// hosts referenced in an issue — see [`images::is_linear_host`].
    pub async fn fetch_image(&self, url: &str) -> Result<Vec<u8>> {
        let mut req = self.http.get(url);
        if images::is_linear_host(url) {
            req = req.header("Authorization", &self.api_key);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("image fetch failed (HTTP {status})"));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    // ----- Mutations -----------------------------------------------------

    async fn expect_success(&self, query: &str, variables: Value, root: &str) -> Result<()> {
        let data = self.request(query, variables).await?;
        let ok = data
            .get(root)
            .and_then(|r| r.get("success"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(anyhow!("Linear reported the mutation did not succeed"))
        }
    }

    pub async fn set_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let q = r#"
            mutation($id: String!, $stateId: String!) {
              issueUpdate(id: $id, input: { stateId: $stateId }) { success }
            }"#;
        self.expect_success(q, json!({ "id": issue_id, "stateId": state_id }), "issueUpdate")
            .await
    }

    pub async fn set_assignee(&self, issue_id: &str, assignee_id: Option<&str>) -> Result<()> {
        let q = r#"
            mutation($id: String!, $assigneeId: String) {
              issueUpdate(id: $id, input: { assigneeId: $assigneeId }) { success }
            }"#;
        self.expect_success(
            q,
            json!({ "id": issue_id, "assigneeId": assignee_id }),
            "issueUpdate",
        )
        .await
    }

    pub async fn add_comment(&self, issue_id: &str, body: &str) -> Result<()> {
        let q = r#"
            mutation($id: String!, $body: String!) {
              commentCreate(input: { issueId: $id, body: $body }) { success }
            }"#;
        self.expect_success(q, json!({ "id": issue_id, "body": body }), "commentCreate")
            .await
    }

    /// Create an issue and return its human identifier (e.g. "ENG-123").
    pub async fn create_issue(&self, team_id: &str, title: &str) -> Result<String> {
        Ok(self
            .create_issue_full(team_id, title, None, None, None, None)
            .await?
            .0)
    }

    /// Create an issue with optional description, priority (0–4), assignee, and
    /// parent (passing a `parent_id` makes it a sub-issue). Returns the new
    /// issue's human identifier (e.g. "ENG-123") and web URL.
    pub async fn create_issue_full(
        &self,
        team_id: &str,
        title: &str,
        description: Option<&str>,
        priority: Option<i64>,
        assignee_id: Option<&str>,
        parent_id: Option<&str>,
    ) -> Result<(String, Option<String>)> {
        let mut input = json!({ "teamId": team_id, "title": title });
        if let Some(d) = description {
            input["description"] = json!(d);
        }
        if let Some(p) = priority {
            input["priority"] = json!(p);
        }
        if let Some(a) = assignee_id {
            input["assigneeId"] = json!(a);
        }
        if let Some(parent) = parent_id {
            input["parentId"] = json!(parent);
        }
        let q = r#"
            mutation($input: IssueCreateInput!) {
              issueCreate(input: $input) {
                success
                issue { identifier url }
              }
            }"#;
        let data = self.request(q, json!({ "input": input })).await?;
        let created = data.get("issueCreate");
        let ok = created
            .and_then(|c| c.get("success"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);
        if !ok {
            return Err(anyhow!("issue creation did not succeed"));
        }
        let issue = created.and_then(|c| c.get("issue"));
        let identifier = issue
            .and_then(|i| i.get("identifier"))
            .and_then(|i| i.as_str())
            .unwrap_or("(new issue)")
            .to_string();
        let url = issue
            .and_then(|i| i.get("url"))
            .and_then(|u| u.as_str())
            .map(str::to_string);
        Ok((identifier, url))
    }

    /// Full-text search across every issue the key can see (Linear's
    /// `searchIssues`). Returns the same summary rows as the issue list.
    pub async fn search(&self, term: &str, first: u32) -> Result<Vec<Issue>> {
        let q = r#"
            query($term: String!, $n: Int!) {
              searchIssues(term: $term, first: $n) {
                nodes {
                  id identifier title priority
                  state { id name type color }
                  assignee { id name displayName }
                }
              }
            }"#;
        self.query_at(q, json!({ "term": term, "n": first }), &["searchIssues", "nodes"])
            .await
    }

    /// List issues filtered by [`View`], optionally narrowed to a team (by key)
    /// and/or project. Unlike [`Self::issues`] this is *not* scoped to one team,
    /// so the CLI can answer "my issues" across the whole workspace.
    pub async fn list_issues(
        &self,
        view: View,
        viewer_id: &str,
        team_key: Option<&str>,
        project_id: Option<&str>,
        first: u32,
    ) -> Result<Vec<Issue>> {
        let mut filter = match view {
            View::Active => json!({ "state": { "type": { "in": ["unstarted", "started"] } } }),
            View::Backlog => json!({ "state": { "type": { "eq": "backlog" } } }),
            View::MyIssues => json!({
                "assignee": { "id": { "eq": viewer_id } },
                "state": { "type": { "nin": ["completed", "canceled"] } }
            }),
            View::All => json!({}),
        };
        if let Some(key) = team_key {
            filter["team"] = json!({ "key": { "eq": key } });
        }
        if let Some(pid) = project_id {
            filter["project"] = json!({ "id": { "eq": pid } });
        }
        let q = r#"
            query($filter: IssueFilter, $n: Int!) {
              issues(filter: $filter, first: $n, orderBy: updatedAt) {
                nodes {
                  id identifier title priority
                  state { id name type color }
                  assignee { id name displayName }
                }
              }
            }"#;
        self.query_at(q, json!({ "filter": filter, "n": first }), &["issues", "nodes"])
            .await
    }

    /// Resolve a human identifier ("ENG-123") or UUID to its canonical UUID and
    /// owning team UUID. The `issue(id:)` field accepts either form; mutations
    /// need the UUID, and state/assignee name lookups need the team.
    pub async fn resolve_issue(&self, id: &str) -> Result<(String, String)> {
        let q = r#"query($id: String!) { issue(id: $id) { id team { id } } }"#;
        let data = self.request(q, json!({ "id": id })).await?;
        let issue = data
            .get("issue")
            .filter(|v| !v.is_null())
            .ok_or_else(|| anyhow!("no issue `{id}`"))?;
        let uuid = issue
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("issue `{id}` missing id"))?
            .to_string();
        let team = issue
            .get("team")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("issue `{id}` missing team"))?
            .to_string();
        Ok((uuid, team))
    }
}
