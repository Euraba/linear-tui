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

/// Who an issue is assigned to, as a filter. Combines (AND) with the selected
/// [`View`], overriding the view's own assignee constraint when not `Any`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AssigneeFilter {
    #[default]
    Any,
    /// The authenticated user (resolved to their id by the client).
    Me,
    Unassigned,
    Person {
        id: String,
        label: String,
    },
}

/// Who created an issue, as a filter. Every issue has a creator, so there's no
/// "unassigned" variant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CreatorFilter {
    #[default]
    Any,
    Me,
    Person {
        id: String,
        label: String,
    },
}

/// A Linear workflow-state *type* — team-agnostic, so it can be filtered on
/// without first loading a specific team's states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateType {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
}

impl StateType {
    /// Cycle order in the filter overlay.
    pub const ALL: [StateType; 6] = [
        StateType::Triage,
        StateType::Backlog,
        StateType::Unstarted,
        StateType::Started,
        StateType::Completed,
        StateType::Canceled,
    ];

    /// The value Linear's API expects for `state.type`.
    pub fn api(self) -> &'static str {
        match self {
            StateType::Triage => "triage",
            StateType::Backlog => "backlog",
            StateType::Unstarted => "unstarted",
            StateType::Started => "started",
            StateType::Completed => "completed",
            StateType::Canceled => "canceled",
        }
    }

    /// Human label (mirrors Linear's default state names).
    pub fn label(self) -> &'static str {
        match self {
            StateType::Triage => "Triage",
            StateType::Backlog => "Backlog",
            StateType::Unstarted => "Todo",
            StateType::Started => "In Progress",
            StateType::Completed => "Done",
            StateType::Canceled => "Canceled",
        }
    }
}

/// The set of issue-list filters layered on top of the selected view/project.
/// All fields default to "no constraint"; [`Filters::is_active`] is false then.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    pub assignee: AssigneeFilter,
    pub creator: CreatorFilter,
    pub state: Option<StateType>,
    /// Linear priority value: 0=none, 1=urgent, 2=high, 3=medium, 4=low.
    pub priority: Option<i64>,
}

impl Filters {
    pub fn is_active(&self) -> bool {
        self.assignee != AssigneeFilter::Any
            || self.creator != CreatorFilter::Any
            || self.state.is_some()
            || self.priority.is_some()
    }

    /// Stable, compact signature used to namespace cached issue lists so a
    /// filtered list never collides with the unfiltered one.
    pub fn signature(&self) -> String {
        if !self.is_active() {
            return "-".into();
        }
        let assignee = match &self.assignee {
            AssigneeFilter::Any => "any".to_string(),
            AssigneeFilter::Me => "me".to_string(),
            AssigneeFilter::Unassigned => "none".to_string(),
            AssigneeFilter::Person { id, .. } => format!("u:{id}"),
        };
        let creator = match &self.creator {
            CreatorFilter::Any => "any".to_string(),
            CreatorFilter::Me => "me".to_string(),
            CreatorFilter::Person { id, .. } => format!("u:{id}"),
        };
        let state = self.state.map(|s| s.api()).unwrap_or("any");
        let priority = self.priority.map(|p| p.to_string()).unwrap_or_else(|| "any".into());
        format!("a={assignee},c={creator},s={state},p={priority}")
    }

    /// One-line human summary for the issues-pane title (e.g. "@tanay · Urgent").
    /// Empty when no filter is active.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        match &self.assignee {
            AssigneeFilter::Any => {}
            AssigneeFilter::Me => parts.push("@me".into()),
            AssigneeFilter::Unassigned => parts.push("unassigned".into()),
            AssigneeFilter::Person { label, .. } => parts.push(format!("@{label}")),
        }
        match &self.creator {
            CreatorFilter::Any => {}
            CreatorFilter::Me => parts.push("by:me".into()),
            CreatorFilter::Person { label, .. } => parts.push(format!("by:{label}")),
        }
        if let Some(s) = self.state {
            parts.push(s.label().to_string());
        }
        if let Some(p) = self.priority {
            parts.push(priority_label(p).to_string());
        }
        parts.join(" · ")
    }
}

/// Human label for a Linear priority value (0–4).
pub fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "No priority",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filters_are_inactive() {
        let f = Filters::default();
        assert!(!f.is_active());
        assert_eq!(f.signature(), "-");
        assert_eq!(f.summary(), "");
    }

    #[test]
    fn active_filters_have_signature_and_summary() {
        let f = Filters {
            assignee: AssigneeFilter::Person {
                id: "u1".into(),
                label: "tanay".into(),
            },
            creator: CreatorFilter::Me,
            state: Some(StateType::Started),
            priority: Some(1),
        };
        assert!(f.is_active());
        let sig = f.signature();
        assert!(sig.contains("a=u:u1"), "{sig}");
        assert!(sig.contains("c=me") && sig.contains("s=started") && sig.contains("p=1"), "{sig}");
        let sum = f.summary();
        assert!(sum.contains("@tanay") && sum.contains("by:me") && sum.contains("In Progress"), "{sum}");
    }

    #[test]
    fn distinct_filters_get_distinct_signatures() {
        let a = Filters { priority: Some(1), ..Default::default() };
        let b = Filters { priority: Some(2), ..Default::default() };
        assert_ne!(a.signature(), b.signature());
        assert_ne!(a.signature(), Filters::default().signature());
    }
}
