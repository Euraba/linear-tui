//! Human-readable and JSON output formatting for the CLI.

use anyhow::Result;
use serde::Serialize;

use crate::domain::{priority_label, Issue, IssueDetail};

pub(super) fn print_json<T: Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

pub(super) fn print_issue_list(issues: &[Issue]) {
    if issues.is_empty() {
        println!("(no issues)");
        return;
    }
    for it in issues {
        let state = it.state.as_ref().map(|s| s.name.as_str()).unwrap_or("—");
        let who = it
            .assignee
            .as_ref()
            .map(|a| format!("@{}", a.label()))
            .unwrap_or_default();
        println!(
            "{:<10} {:<4} {:<16} {:<14} {}",
            it.identifier,
            it.priority_label(),
            truncate(state, 16),
            truncate(&who, 14),
            it.title,
        );
    }
}

pub(super) fn print_issue_detail(d: &IssueDetail) {
    println!("{}  {}", d.identifier, d.title);
    if let Some(s) = &d.state {
        println!("State:    {} ({})", s.name, s.kind);
    }
    println!(
        "Assignee: {}",
        d.assignee.as_ref().map(|a| a.label()).unwrap_or("—")
    );
    println!("Priority: {}", priority_label(d.priority));
    if let Some(p) = &d.parent {
        println!("Parent:   {} {}", p.identifier, p.title);
    }
    if let Some(url) = &d.url {
        println!("URL:      {url}");
    }
    if let Some(desc) = d.description.as_deref().filter(|s| !s.trim().is_empty()) {
        println!("\nDescription:\n{desc}");
    }
    if !d.children.is_empty() {
        println!("\nSub-issues ({}):", d.children.len());
        for c in &d.children {
            let state = c.state.as_ref().map(|s| s.name.as_str()).unwrap_or("—");
            println!("  {:<10} [{}] {}", c.identifier, state, c.title);
        }
    }
    if !d.comments.is_empty() {
        println!("\nComments ({}):", d.comments.len());
        for c in &d.comments {
            let who = c.user.as_ref().map(|u| u.label()).unwrap_or("?");
            let when = c.created_at.as_deref().unwrap_or("");
            println!("  {who}  {when}");
            for line in c.body.lines() {
                println!("    {line}");
            }
        }
    }
}

/// Truncate to `max` characters (not bytes), appending "…" when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub(super) fn print_help() {
    println!(
        "linear-tui — Linear from the terminal\n\
         \n\
         Usage:\n\
         \x20 linear-tui                      launch the interactive TUI\n\
         \x20 linear-tui <command> [args]     run a one-shot CLI command\n\
         \n\
         Read commands:\n\
         \x20 me                              show the authenticated user\n\
         \x20 teams                           list teams (KEY  name)\n\
         \x20 issues [--team K] [--view V]    list issues; V = my|active|backlog|all (default my)\n\
         \x20        [--project NAME] [--mine] [--limit N]\n\
         \x20 search <text...> [--limit N]    full-text search across all issues\n\
         \x20 view <ISSUE>                    full detail (description, sub-issues, comments)\n\
         \x20 states  --team K                list a team's workflow states\n\
         \x20 projects --team K               list a team's projects\n\
         \x20 members  --team K               list a team's members\n\
         \n\
         Write commands:\n\
         \x20 create --team K --title T [--description D] [--priority 0-4] [--assignee me|name] [--parent ISSUE]\n\
         \x20 comment <ISSUE> <body...>       add a comment\n\
         \x20 state   <ISSUE> <state>         change workflow state (name or done/todo/progress/…)\n\
         \x20 assign  <ISSUE> <me|none|name>  set or clear the assignee\n\
         \n\
         Issues are addressed by identifier (e.g. ENG-123). Add --json to any\n\
         command for machine-readable output. Config/API key: see `linear-tui` README.\n\
         \n\
         Other:\n\
         \x20 serve                           stdio JSON-RPC backend (Neovim plugin)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_counts_chars() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("café", 10), "café");
    }
}
