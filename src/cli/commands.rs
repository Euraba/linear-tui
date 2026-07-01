//! The individual CLI command handlers and the name-resolution helpers they
//! share. Each `*_cmd` maps one subcommand to [`LinearClient`] calls and prints
//! via the `super::output` formatters.

use anyhow::{anyhow, Result};
use serde_json::json;

use super::output::{print_issue_detail, print_issue_list, print_json};
use super::ParsedArgs;
use crate::config::Config;
use crate::domain::{Team, User, View, WorkflowState};
use crate::linear::LinearClient;

pub(super) async fn issues_cmd(
    client: &LinearClient,
    cfg: &Config,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let view = if args.bools.contains("mine") {
        View::MyIssues
    } else {
        parse_view(args.flag("view").unwrap_or("my"))?
    };
    let team_key = args.flag("team").map(str::to_uppercase);
    let limit = args
        .flag("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(cfg.page_size);

    // The "My Issues" filter needs the viewer id; nothing else does.
    let viewer_id = if view == View::MyIssues {
        client.viewer().await?.id
    } else {
        String::new()
    };

    // A `--project` name is resolved within its team (so `--team` is required).
    let project_id = match args.flag("project") {
        Some(name) => {
            let key = team_key
                .as_deref()
                .ok_or_else(|| anyhow!("--project requires --team <KEY>"))?;
            let team = resolve_team(client, key).await?;
            let projects = client.team_projects(&team.id).await?;
            let nl = name.to_lowercase();
            let proj = projects
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .or_else(|| {
                    projects
                        .iter()
                        .find(|p| p.name.to_lowercase().contains(&nl))
                })
                .ok_or_else(|| anyhow!("no project matching `{name}` in team {key}"))?;
            Some(proj.id.clone())
        }
        None => None,
    };

    let issues = client
        .list_issues(
            view,
            &viewer_id,
            team_key.as_deref(),
            project_id.as_deref(),
            limit,
        )
        .await?;
    if json {
        print_json(&issues)?;
    } else {
        print_issue_list(&issues);
    }
    Ok(())
}

pub(super) async fn search_cmd(
    client: &LinearClient,
    cfg: &Config,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let term = args.pos.join(" ");
    if term.trim().is_empty() {
        return Err(anyhow!("usage: linear-tui search <text...>"));
    }
    let limit = args
        .flag("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(cfg.page_size);
    let issues = client.search(&term, limit).await?;
    if json {
        print_json(&issues)?;
    } else {
        print_issue_list(&issues);
    }
    Ok(())
}

pub(super) async fn view_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
    let id = args
        .pos
        .first()
        .ok_or_else(|| anyhow!("usage: linear-tui view <ISSUE>"))?;
    let detail = client.issue_detail(id).await?;
    if json {
        print_json(&detail)?;
    } else {
        print_issue_detail(&detail);
    }
    Ok(())
}

pub(super) async fn create_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
    let key = args
        .flag("team")
        .map(str::to_uppercase)
        .ok_or_else(|| anyhow!("create requires --team <KEY>"))?;
    let title = args
        .flag("title")
        .ok_or_else(|| anyhow!("create requires --title <TITLE>"))?;
    let team = resolve_team(client, &key).await?;

    let priority = match args.flag("priority") {
        Some(s) => {
            let p: i64 = s.parse().map_err(|_| anyhow!("--priority must be 0–4"))?;
            if !(0..=4).contains(&p) {
                return Err(anyhow!(
                    "--priority must be 0–4 (0=none, 1=urgent, 2=high, 3=medium, 4=low)"
                ));
            }
            Some(p)
        }
        None => None,
    };
    let assignee_id = match args.flag("assignee") {
        Some("me") => Some(client.viewer().await?.id),
        Some(name) => Some(resolve_member(client, &team.id, name).await?.id),
        None => None,
    };
    // `--parent ENG-100` makes this a sub-issue; resolve the identifier to a UUID.
    let parent_id = match args.flag("parent") {
        Some(p) => Some(client.resolve_issue(p).await?.0),
        None => None,
    };

    let (ident, url) = client
        .create_issue_full(
            &team.id,
            title,
            args.flag("description"),
            priority,
            assignee_id.as_deref(),
            parent_id.as_deref(),
        )
        .await?;
    if json {
        print_json(&json!({ "identifier": ident, "url": url }))?;
    } else {
        println!("Created {ident}");
        if let Some(u) = url {
            println!("{u}");
        }
    }
    Ok(())
}

pub(super) async fn comment_cmd(
    client: &LinearClient,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let id = args
        .pos
        .first()
        .ok_or_else(|| anyhow!("usage: linear-tui comment <ISSUE> <body...>"))?;
    let body = match args.pos.get(1..) {
        Some(rest) if !rest.is_empty() => rest.join(" "),
        _ => args.flag("body").unwrap_or_default().to_string(),
    };
    if body.trim().is_empty() {
        return Err(anyhow!("comment body is empty"));
    }
    let (uuid, _team) = client.resolve_issue(id).await?;
    client.add_comment(&uuid, &body).await?;
    if json {
        print_json(&json!({ "ok": true, "issue": id }))?;
    } else {
        println!("Commented on {id}");
    }
    Ok(())
}

pub(super) async fn state_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
    let id = args
        .pos
        .first()
        .ok_or_else(|| anyhow!("usage: linear-tui state <ISSUE> <state>"))?;
    let wanted = args.pos.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    if wanted.trim().is_empty() {
        return Err(anyhow!("usage: linear-tui state <ISSUE> <state>"));
    }
    let (uuid, team_id) = client.resolve_issue(id).await?;
    let states = client.team_states(&team_id).await?;
    let st = match_state(&states, &wanted).ok_or_else(|| {
        let names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
        anyhow!(
            "no state matching `{wanted}`. Available: {}",
            names.join(", ")
        )
    })?;
    client.set_state(&uuid, &st.id).await?;
    if json {
        print_json(&json!({ "ok": true, "issue": id, "state": st.name }))?;
    } else {
        println!("{id} → {}", st.name);
    }
    Ok(())
}

pub(super) async fn assign_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
    let id = args
        .pos
        .first()
        .ok_or_else(|| anyhow!("usage: linear-tui assign <ISSUE> <me|none|name>"))?;
    let target = args.pos.get(1).map(String::as_str).unwrap_or("me");
    let (uuid, team_id) = client.resolve_issue(id).await?;

    let (assignee_id, label) = match target.to_lowercase().as_str() {
        "none" | "unassign" | "unassigned" | "nobody" => (None, "unassigned".to_string()),
        "me" => {
            let u = client.viewer().await?;
            let label = format!("@{}", u.label());
            (Some(u.id), label)
        }
        _ => {
            let m = resolve_member(client, &team_id, target).await?;
            let label = format!("@{}", m.label());
            (Some(m.id), label)
        }
    };
    client.set_assignee(&uuid, assignee_id.as_deref()).await?;
    if json {
        print_json(&json!({ "ok": true, "issue": id, "assignee": label }))?;
    } else {
        println!("{id} → {label}");
    }
    Ok(())
}

pub(super) async fn states_cmd(
    client: &LinearClient,
    cfg: &Config,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let team = resolve_team(client, &require_team(cfg, args)?).await?;
    let states = client.team_states(&team.id).await?;
    if json {
        print_json(&states)?;
    } else {
        for s in &states {
            println!("{:<24} ({})", s.name, s.kind);
        }
    }
    Ok(())
}

pub(super) async fn projects_cmd(
    client: &LinearClient,
    cfg: &Config,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let team = resolve_team(client, &require_team(cfg, args)?).await?;
    let projects = client.team_projects(&team.id).await?;
    if json {
        print_json(&projects)?;
    } else if projects.is_empty() {
        println!("(no projects)");
    } else {
        for p in &projects {
            match &p.state {
                Some(state) => println!("{:<32} ({state})", p.name),
                None => println!("{}", p.name),
            }
        }
    }
    Ok(())
}

pub(super) async fn members_cmd(
    client: &LinearClient,
    cfg: &Config,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    let team = resolve_team(client, &require_team(cfg, args)?).await?;
    let members = client.team_members(&team.id).await?;
    if json {
        print_json(&members)?;
    } else {
        for m in &members {
            println!("{}", m.label());
        }
    }
    Ok(())
}

// ----- Resolution helpers ------------------------------------------------

/// Find a team by key (case-insensitive). Keys are Linear's short prefixes
/// ("ENG", "BES"), the same value shown in issue identifiers.
async fn resolve_team(client: &LinearClient, key: &str) -> Result<Team> {
    let teams = client.teams().await?;
    teams
        .into_iter()
        .find(|t| t.key.eq_ignore_ascii_case(key))
        .ok_or_else(|| anyhow!("no team with key `{key}` (run `linear-tui teams`)"))
}

/// Find a team member by display name / name (case-insensitive, exact then
/// substring).
async fn resolve_member(client: &LinearClient, team_id: &str, name: &str) -> Result<User> {
    let members = client.team_members(team_id).await?;
    let nl = name.to_lowercase();
    members
        .iter()
        .find(|m| m.label().eq_ignore_ascii_case(name) || m.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            members.iter().find(|m| {
                m.label().to_lowercase().contains(&nl) || m.name.to_lowercase().contains(&nl)
            })
        })
        .cloned()
        .ok_or_else(|| anyhow!("no team member matching `{name}`"))
}

/// The team key for commands that need one: `--team` wins, else the configured
/// `default_team`, else an error.
fn require_team(cfg: &Config, args: &ParsedArgs) -> Result<String> {
    args.flag("team")
        .map(str::to_uppercase)
        .or_else(|| cfg.default_team.as_deref().map(str::to_uppercase))
        .ok_or_else(|| anyhow!("this command needs --team <KEY> (or a default_team in config)"))
}

/// Match a requested state to a team's workflow states: exact name, then name
/// substring, then a `type` keyword ("done", "todo", "in progress", …).
fn match_state<'a>(states: &'a [WorkflowState], wanted: &str) -> Option<&'a WorkflowState> {
    let w = wanted.to_lowercase();
    states
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(wanted))
        .or_else(|| states.iter().find(|s| s.name.to_lowercase().contains(&w)))
        .or_else(|| type_keyword(&w).and_then(|t| states.iter().find(|s| s.kind == t)))
}

/// Map a loose keyword to a Linear workflow-state `type`.
fn type_keyword(w: &str) -> Option<&'static str> {
    Some(match w {
        "done" | "complete" | "completed" | "closed" => "completed",
        "todo" | "unstarted" | "open" => "unstarted",
        "progress" | "in progress" | "in-progress" | "inprogress" | "started" | "doing" => {
            "started"
        }
        "backlog" => "backlog",
        "cancel" | "canceled" | "cancelled" => "canceled",
        "triage" => "triage",
        _ => return None,
    })
}

fn parse_view(s: &str) -> Result<View> {
    View::parse(s).ok_or_else(|| anyhow!("unknown view `{s}` (use: my|active|backlog|all)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_parsing_is_lenient() {
        assert_eq!(parse_view("my").unwrap(), View::MyIssues);
        assert_eq!(parse_view("Active").unwrap(), View::Active);
        assert!(parse_view("nope").is_err());
    }

    #[test]
    fn state_matches_by_name_then_type() {
        let states = vec![
            WorkflowState {
                id: "1".into(),
                name: "Todo".into(),
                kind: "unstarted".into(),
                color: None,
            },
            WorkflowState {
                id: "2".into(),
                name: "In Progress".into(),
                kind: "started".into(),
                color: None,
            },
            WorkflowState {
                id: "3".into(),
                name: "Done".into(),
                kind: "completed".into(),
                color: None,
            },
        ];
        assert_eq!(match_state(&states, "in progress").unwrap().id, "2"); // exact (ci)
        assert_eq!(match_state(&states, "prog").unwrap().id, "2"); // substring
        assert_eq!(match_state(&states, "done").unwrap().id, "3"); // type keyword
        assert_eq!(match_state(&states, "todo").unwrap().id, "1");
        assert!(match_state(&states, "shipped").is_none());
    }
}
