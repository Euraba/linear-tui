//! Reusable GraphQL fragments/strings for the Linear API.

/// The issue-summary field selection shared by every list-shaped query
/// ([`issues`](super::client::LinearClient::issues), `search`, `list_issues`)
/// and by the sub-issue nodes in `issue_detail`. Defined as a GraphQL fragment
/// so a query references it with `...IssueFields` and appends this text to the
/// document (see [`with_issue_fields`]) — keeping the field list in one place.
pub const ISSUE_FIELDS: &str = r#"
fragment IssueFields on Issue {
  id identifier title priority
  state { id name type color }
  assignee { id name displayName }
}"#;

/// Append the [`ISSUE_FIELDS`] fragment definition to a query that spreads
/// `...IssueFields`. Kept as plain concatenation (not `format!`) so the query
/// bodies stay readable without doubling every GraphQL brace.
pub fn with_issue_fields(query: &str) -> String {
    let mut q = String::with_capacity(query.len() + ISSUE_FIELDS.len());
    q.push_str(query);
    q.push_str(ISSUE_FIELDS);
    q
}
