//! `linear-tui <command>`: a one-shot, scriptable CLI over the same
//! [`LinearClient`] the TUI and `serve` backend use. It's the surface an agent
//! (e.g. Claude Code) drives from any project to read and manage Linear.
//!
//! Every command prints human-readable text by default; pass `--json` for the
//! raw model objects (stable shapes, good for piping). Issues are addressed by
//! their human identifier ("ENG-123") everywhere — UUIDs are never needed.
//!
//! ```text
//!   linear-tui issues --mine
//!   linear-tui search "redis timeout"
//!   linear-tui view ENG-123
//!   linear-tui create --team ENG --title "Fix login" --assignee me --priority 2
//!   linear-tui comment ENG-123 "shipping today"
//!   linear-tui state ENG-123 "In Progress"
//!   linear-tui assign ENG-123 me
//! ```

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::json;

use crate::client::LinearClient;
use crate::config::Config;
use crate::models::{Team, User, View, WorkflowState};

/// Flags that take no value (everything else consumes the next token).
const BOOL_FLAGS: &[&str] = &["json", "mine"];

/// Entry point for any non-`serve` invocation that has at least one argument.
/// Loads config, builds the client, and runs the requested command to
/// completion on a single-threaded Tokio runtime.
pub fn run() -> Result<()> {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    // `main` only routes here when argv is non-empty.
    let cmd = argv.remove(0);
    if matches!(cmd.as_str(), "-h" | "--help" | "help") {
        print_help();
        return Ok(());
    }

    let args = ParsedArgs::parse(&argv);
    let json = args.bools.contains("json");

    let cfg = Config::load()?;
    let client = LinearClient::new(cfg.api_key.clone(), cfg.page_size)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(dispatch(&client, &cfg, &cmd, &args, json))
}

async fn dispatch(
    client: &LinearClient,
    cfg: &Config,
    cmd: &str,
    args: &ParsedArgs,
    json: bool,
) -> Result<()> {
    match cmd {
        "me" | "viewer" => {
            let u = client.viewer().await?;
            if json {
                print_json(&u)?;
            } else {
                println!("Signed in as {} ({})", u.label(), u.id);
            }
        }
        "teams" => {
            let teams = client.teams().await?;
            if json {
                print_json(&teams)?;
            } else if teams.is_empty() {
                println!("(no teams)");
            } else {
                for t in &teams {
                    println!("{:<8} {}", t.key, t.name);
                }
            }
        }
        "issues" => issues_cmd(client, cfg, args, json).await?,
        "search" => search_cmd(client, cfg, args, json).await?,
        "view" | "show" => view_cmd(client, args, json).await?,
        "create" | "new" => create_cmd(client, args, json).await?,
        "comment" => comment_cmd(client, args, json).await?,
        "state" => state_cmd(client, args, json).await?,
        "assign" => assign_cmd(client, args, json).await?,
        "states" => states_cmd(client, cfg, args, json).await?,
        "projects" => projects_cmd(client, cfg, args, json).await?,
        "members" => members_cmd(client, cfg, args, json).await?,
        other => {
            print_help();
            return Err(anyhow!("unknown command `{other}`"));
        }
    }
    Ok(())
}

// ----- Commands ----------------------------------------------------------

async fn issues_cmd(client: &LinearClient, cfg: &Config, args: &ParsedArgs, json: bool) -> Result<()> {
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
                .or_else(|| projects.iter().find(|p| p.name.to_lowercase().contains(&nl)))
                .ok_or_else(|| anyhow!("no project matching `{name}` in team {key}"))?;
            Some(proj.id.clone())
        }
        None => None,
    };

    let issues = client
        .list_issues(view, &viewer_id, team_key.as_deref(), project_id.as_deref(), limit)
        .await?;
    if json {
        print_json(&issues)?;
    } else {
        print_issue_list(&issues);
    }
    Ok(())
}

async fn search_cmd(client: &LinearClient, cfg: &Config, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn view_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn create_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn comment_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn state_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
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
        anyhow!("no state matching `{wanted}`. Available: {}", names.join(", "))
    })?;
    client.set_state(&uuid, &st.id).await?;
    if json {
        print_json(&json!({ "ok": true, "issue": id, "state": st.name }))?;
    } else {
        println!("{id} → {}", st.name);
    }
    Ok(())
}

async fn assign_cmd(client: &LinearClient, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn states_cmd(client: &LinearClient, cfg: &Config, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn projects_cmd(client: &LinearClient, cfg: &Config, args: &ParsedArgs, json: bool) -> Result<()> {
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

async fn members_cmd(client: &LinearClient, cfg: &Config, args: &ParsedArgs, json: bool) -> Result<()> {
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
            members
                .iter()
                .find(|m| m.label().to_lowercase().contains(&nl) || m.name.to_lowercase().contains(&nl))
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
        "progress" | "in progress" | "in-progress" | "inprogress" | "started" | "doing" => "started",
        "backlog" => "backlog",
        "cancel" | "canceled" | "cancelled" => "canceled",
        "triage" => "triage",
        _ => return None,
    })
}

fn parse_view(s: &str) -> Result<View> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "my" | "mine" | "myissues" | "my-issues" => View::MyIssues,
        "active" => View::Active,
        "backlog" => View::Backlog,
        "all" => View::All,
        other => return Err(anyhow!("unknown view `{other}` (use: my|active|backlog|all)")),
    })
}

// ----- Output ------------------------------------------------------------

fn print_json<T: Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn print_issue_list(issues: &[crate::models::Issue]) {
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

fn print_issue_detail(d: &crate::models::IssueDetail) {
    println!("{}  {}", d.identifier, d.title);
    if let Some(s) = &d.state {
        println!("State:    {} ({})", s.name, s.kind);
    }
    println!(
        "Assignee: {}",
        d.assignee.as_ref().map(|a| a.label()).unwrap_or("—")
    );
    println!("Priority: {}", priority_word(d.priority));
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

fn priority_word(p: i64) -> &'static str {
    match p {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "None",
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

fn print_help() {
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

// ----- Argument parsing --------------------------------------------------

/// A minimal argv split into positionals, value flags (`--key value`) and
/// boolean flags ([`BOOL_FLAGS`]). Deliberately tiny — the command set is small
/// and fixed, so a full parser dependency isn't worth it.
struct ParsedArgs {
    pos: Vec<String>,
    flags: HashMap<String, String>,
    bools: HashSet<String>,
}

impl ParsedArgs {
    fn parse(args: &[String]) -> Self {
        let mut pos = Vec::new();
        let mut flags = HashMap::new();
        let mut bools = HashSet::new();
        let mut i = 0;
        while i < args.len() {
            if let Some(name) = args[i].strip_prefix("--") {
                if BOOL_FLAGS.contains(&name) {
                    bools.insert(name.to_string());
                } else if let Some(value) = args.get(i + 1) {
                    flags.insert(name.to_string(), value.clone());
                    i += 1;
                } else {
                    // Trailing `--key` with no value: record it empty.
                    flags.insert(name.to_string(), String::new());
                }
            } else {
                pos.push(args[i].clone());
            }
            i += 1;
        }
        Self { pos, flags, bools }
    }

    fn flag(&self, key: &str) -> Option<&str> {
        self.flags.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_positionals_flags_and_bools() {
        let a = ParsedArgs::parse(&argv(&["ENG-1", "hello", "world", "--team", "ENG", "--json"]));
        assert_eq!(a.pos, ["ENG-1", "hello", "world"]);
        assert_eq!(a.flag("team"), Some("ENG"));
        assert!(a.bools.contains("json"));
        assert!(!a.bools.contains("mine"));
    }

    #[test]
    fn trailing_value_flag_without_value_is_empty() {
        let a = ParsedArgs::parse(&argv(&["--title"]));
        assert_eq!(a.flag("title"), Some(""));
    }

    #[test]
    fn view_parsing_is_lenient() {
        assert_eq!(parse_view("my").unwrap(), View::MyIssues);
        assert_eq!(parse_view("Active").unwrap(), View::Active);
        assert!(parse_view("nope").is_err());
    }

    #[test]
    fn state_matches_by_name_then_type() {
        let states = vec![
            WorkflowState { id: "1".into(), name: "Todo".into(), kind: "unstarted".into(), color: None },
            WorkflowState { id: "2".into(), name: "In Progress".into(), kind: "started".into(), color: None },
            WorkflowState { id: "3".into(), name: "Done".into(), kind: "completed".into(), color: None },
        ];
        assert_eq!(match_state(&states, "in progress").unwrap().id, "2"); // exact (ci)
        assert_eq!(match_state(&states, "prog").unwrap().id, "2"); // substring
        assert_eq!(match_state(&states, "done").unwrap().id, "3"); // type keyword
        assert_eq!(match_state(&states, "todo").unwrap().id, "1");
        assert!(match_state(&states, "shipped").is_none());
    }

    #[test]
    fn truncate_counts_chars() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("café", 10), "café");
    }
}
