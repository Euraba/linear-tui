//! Issue-list filters layered on top of the selected view/project.

use super::format::priority_label;

/// Who an issue is assigned to, as a filter. Combines (AND) with the selected
/// [`View`](super::View), overriding the view's own assignee constraint when
/// not `Any`.
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
        let priority = self
            .priority
            .map(|p| p.to_string())
            .unwrap_or_else(|| "any".into());
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
        assert!(
            sig.contains("c=me") && sig.contains("s=started") && sig.contains("p=1"),
            "{sig}"
        );
        let sum = f.summary();
        assert!(
            sum.contains("@tanay") && sum.contains("by:me") && sum.contains("In Progress"),
            "{sum}"
        );
    }

    #[test]
    fn distinct_filters_get_distinct_signatures() {
        let a = Filters {
            priority: Some(1),
            ..Default::default()
        };
        let b = Filters {
            priority: Some(2),
            ..Default::default()
        };
        assert_ne!(a.signature(), b.signature());
        assert_ne!(a.signature(), Filters::default().signature());
    }
}
