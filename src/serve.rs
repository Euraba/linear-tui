//! `linear-tui serve`: a stdio JSON-RPC backend for the Neovim plugin.
//!
//! The plugin spawns this as a long-lived child process and talks to it over
//! pipes — one JSON object per line, in each direction:
//!
//! ```text
//!  -->  {"id":1,"method":"issues","params":{"team_id":"…","view":"MyIssues"}}
//!  <--  {"id":1,"ok":true,"result":[ … ]}
//!  <--  {"id":2,"ok":false,"error":"Linear API error: …"}
//! ```
//!
//! It reuses the exact same [`Config`], [`LinearClient`] and `models` as the TUI,
//! so the plugin is a thin frontend with zero API/GraphQL logic of its own — and
//! never touches the API key (this process loads it). Requests are handled
//! concurrently; every response is written through a single task so lines never
//! interleave.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::OnceCell;

use crate::client::LinearClient;
use crate::config::Config;
use crate::models::View;

/// Entry point for the `serve` subcommand. Loads config, builds the client, and
/// runs the stdio loop on a multi-thread Tokio runtime.
pub fn run() -> Result<()> {
    let cfg = Config::load()?;
    let client = LinearClient::new(cfg.api_key.clone(), cfg.page_size)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(client, cfg))
}

async fn serve(client: LinearClient, cfg: Config) -> Result<()> {
    let (tx_out, mut rx_out) = unbounded_channel::<String>();

    // One writer owns stdout: concurrently-produced responses are emitted as
    // whole lines, so they never interleave.
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(mut line) = rx_out.recv().await {
            line.push('\n');
            if stdout.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    // The viewer id (for the "My Issues" filter) is fetched at most once, lazily,
    // and shared across all concurrent requests.
    let viewer_id: Arc<OnceCell<String>> = Arc::new(OnceCell::new());
    let cfg = Arc::new(cfg);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let client = client.clone();
        let tx_out = tx_out.clone();
        let viewer_id = viewer_id.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let resp = process(&client, &viewer_id, &cfg, &line).await;
            let _ = tx_out.send(resp.to_string());
        });
    }

    // stdin closed (the editor exited): let the writer drain and finish.
    drop(tx_out);
    let _ = writer.await;
    Ok(())
}

/// Parse one request line and produce its response value. This never returns an
/// `Err` — failures become `{ok:false, error}` so the read loop keeps going.
async fn process(
    client: &LinearClient,
    viewer_id: &OnceCell<String>,
    cfg: &Config,
    line: &str,
) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return json!({ "id": Value::Null, "ok": false, "error": format!("invalid JSON request: {e}") })
        }
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    match dispatch(client, viewer_id, cfg, method, &params).await {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(e) => json!({ "id": id, "ok": false, "error": format!("{e:#}") }),
    }
}

async fn dispatch(
    client: &LinearClient,
    viewer_id: &OnceCell<String>,
    cfg: &Config,
    method: &str,
    params: &Value,
) -> Result<Value> {
    let result = match method {
        // Non-secret config the frontend needs to pick the initial team. The API
        // key is deliberately never exposed here.
        "config" => json!({
            "default_team": cfg.default_team,
            "page_size": cfg.page_size,
        }),
        "viewer" => serde_json::to_value(client.viewer().await?)?,
        "teams" => serde_json::to_value(client.teams().await?)?,
        "team_states" => serde_json::to_value(client.team_states(&req_str(params, "team_id")?).await?)?,
        "team_projects" => {
            serde_json::to_value(client.team_projects(&req_str(params, "team_id")?).await?)?
        }
        "team_members" => {
            serde_json::to_value(client.team_members(&req_str(params, "team_id")?).await?)?
        }
        "issues" => {
            let team_id = req_str(params, "team_id")?;
            let view = parse_view(opt_str(params, "view").as_deref().unwrap_or("MyIssues"))?;
            let project_id = opt_str(params, "project_id");
            let vid = ensure_viewer(client, viewer_id).await?;
            let issues = client
                .issues(&team_id, view, &vid, project_id.as_deref(), &crate::models::Filters::default())
                .await?;
            serde_json::to_value(issues)?
        }
        "issue_detail" => {
            serde_json::to_value(client.issue_detail(&req_str(params, "issue_id")?).await?)?
        }
        "set_state" => {
            client
                .set_state(&req_str(params, "issue_id")?, &req_str(params, "state_id")?)
                .await?;
            json!({ "success": true })
        }
        "set_assignee" => {
            client
                .set_assignee(&req_str(params, "issue_id")?, opt_str(params, "assignee_id").as_deref())
                .await?;
            json!({ "success": true })
        }
        "add_comment" => {
            client
                .add_comment(&req_str(params, "issue_id")?, &req_str(params, "body")?)
                .await?;
            json!({ "success": true })
        }
        "create_issue" => {
            let id = client
                .create_issue(&req_str(params, "team_id")?, &req_str(params, "title")?)
                .await?;
            json!({ "identifier": id })
        }
        other => return Err(anyhow!("unknown method `{other}`")),
    };
    Ok(result)
}

/// Fetch + cache the viewer id (needed for the "My Issues" filter).
async fn ensure_viewer(client: &LinearClient, viewer_id: &OnceCell<String>) -> Result<String> {
    let id = viewer_id
        .get_or_try_init(|| async { client.viewer().await.map(|u| u.id) })
        .await?;
    Ok(id.clone())
}

fn parse_view(s: &str) -> Result<View> {
    Ok(match s {
        "MyIssues" => View::MyIssues,
        "Active" => View::Active,
        "Backlog" => View::Backlog,
        "All" => View::All,
        other => return Err(anyhow!("unknown view `{other}`")),
    })
}

/// Required string parameter.
fn req_str(params: &Value, key: &str) -> Result<String> {
    opt_str(params, key).ok_or_else(|| anyhow!("missing string param `{key}`"))
}

/// Optional string parameter (absent or JSON null -> None).
fn opt_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}
