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

mod commands;
mod output;

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};

use crate::config::Config;
use crate::linear::LinearClient;

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
        output::print_help();
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
    use commands::*;
    match cmd {
        "me" | "viewer" => {
            let u = client.viewer().await?;
            if json {
                output::print_json(&u)?;
            } else {
                println!("Signed in as {} ({})", u.label(), u.id);
            }
        }
        "teams" => {
            let teams = client.teams().await?;
            if json {
                output::print_json(&teams)?;
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
            output::print_help();
            return Err(anyhow!("unknown command `{other}`"));
        }
    }
    Ok(())
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
        let a = ParsedArgs::parse(&argv(&[
            "ENG-1", "hello", "world", "--team", "ENG", "--json",
        ]));
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
}
