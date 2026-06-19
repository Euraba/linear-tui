//! Plain data structures mirroring the slice of the Linear domain we care about.
//!
//! These intentionally map closely onto the slack-tui mental model:
//!   Slack Team    -> Linear `Team`
//!   Slack Channel -> a `View` (a saved filter over issues, e.g. "Active", "My Issues")
//!   Slack message -> a Linear `Issue` (and its `IssueDetail` when opened)

use serde::{Deserialize, Serialize};

/// A Linear team (the top-level grouping, like a Slack workspace/team).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub key: String,
}

/// A Linear project within a team. Selecting one narrows the issue list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub state: Option<String>,
}

/// A workflow state an issue can be in (Todo, In Progress, Done, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    /// One of: triage, backlog, unstarted, started, completed, canceled.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// A Linear user (used for assignees / members).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
}

impl User {
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
}

/// A summary row for an issue, as shown in the issue list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub state: Option<WorkflowState>,
    #[serde(default)]
    pub assignee: Option<User>,
}

impl Issue {
    pub fn priority_label(&self) -> &'static str {
        match self.priority {
            1 => "URG",
            2 => "HIGH",
            3 => "MED",
            4 => "LOW",
            _ => "—",
        }
    }
}

/// A single comment on an issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub body: String,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
}

/// The full detail of an issue, fetched when the user opens it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub state: Option<WorkflowState>,
    #[serde(default)]
    pub assignee: Option<User>,
    /// The parent issue, when this issue is a sub-issue.
    #[serde(default)]
    pub parent: Option<Issue>,
    /// Sub-issues of this issue (its `children` in Linear).
    #[serde(default)]
    pub children: Vec<Issue>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// A built-in "view" over a team's issues. Analogous to a Slack channel:
/// selecting one populates the issue list. We keep these client-side so the
/// tool works for any team without depending on the user's saved Linear views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Issues in `unstarted` or `started` states.
    Active,
    /// Issues in `backlog` states.
    Backlog,
    /// Issues assigned to the authenticated user (any active state).
    MyIssues,
    /// Everything, most-recently-updated first.
    All,
}

impl View {
    /// Order shown in the Views pane. My Issues first — it's the default.
    pub const ALL: [View; 4] = [View::MyIssues, View::Active, View::Backlog, View::All];

    pub fn label(&self) -> &'static str {
        match self {
            View::Active => "Active",
            View::Backlog => "Backlog",
            View::MyIssues => "My Issues",
            View::All => "All",
        }
    }
}
