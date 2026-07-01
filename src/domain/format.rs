//! Provider-agnostic display formatting for Linear domain values.
//!
//! These produce plain strings/chars with no dependency on the frontend
//! (ratatui, CLI, or the JSON-RPC `serve` mode), so every surface renders
//! priorities, state glyphs, and timestamps identically. Frontend-specific
//! styling (colors, ratatui `Style`) lives in `ui::style`, not here.

/// Full human label for a Linear priority value (0=none, 1=urgent … 4=low).
pub fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "No priority",
    }
}

/// Compact priority label for dense list columns (e.g. the issue list).
pub fn priority_abbrev(p: i64) -> &'static str {
    match p {
        1 => "URG",
        2 => "HIGH",
        3 => "MED",
        4 => "LOW",
        _ => "—",
    }
}

/// A single-character glyph for a workflow-state `type` (see
/// [`WorkflowState::kind`](crate::domain::WorkflowState)).
pub fn state_glyph(kind: &str) -> char {
    match kind {
        "completed" => '✓',
        "canceled" => '✗',
        "started" => '◐',
        "unstarted" => '○',
        "backlog" => '·',
        "triage" => '△',
        _ => '•',
    }
}

/// Trim an ISO-8601 timestamp down to `YYYY-MM-DD HH:MM`.
pub fn short_date(s: &str) -> String {
    s.replace('T', " ").chars().take(16).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_labels_cover_all_values() {
        assert_eq!(priority_label(1), "Urgent");
        assert_eq!(priority_label(0), "No priority");
        assert_eq!(priority_label(9), "No priority");
        assert_eq!(priority_abbrev(2), "HIGH");
        assert_eq!(priority_abbrev(0), "—");
    }

    #[test]
    fn short_date_trims_to_minutes() {
        assert_eq!(short_date("2026-07-01T12:34:56.789Z"), "2026-07-01 12:34");
    }
}
