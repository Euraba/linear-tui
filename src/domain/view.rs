//! The built-in "views" over a team's issues.

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

    /// Parse a view name leniently (case-insensitive, with aliases). Shared by
    /// the CLI (`my`/`active`/…) and the `serve` JSON-RPC mode (which sends the
    /// PascalCase variant names) so both accept the same spellings. Returns
    /// `None` for an unknown name; callers supply their own error message.
    pub fn parse(s: &str) -> Option<View> {
        Some(match s.to_ascii_lowercase().as_str() {
            "my" | "mine" | "myissues" | "my-issues" => View::MyIssues,
            "active" => View::Active,
            "backlog" => View::Backlog,
            "all" => View::All,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_lenient_and_covers_serve_names() {
        assert_eq!(View::parse("my"), Some(View::MyIssues));
        assert_eq!(View::parse("MyIssues"), Some(View::MyIssues)); // serve sends this
        assert_eq!(View::parse("ACTIVE"), Some(View::Active));
        assert_eq!(View::parse("nope"), None);
    }
}
