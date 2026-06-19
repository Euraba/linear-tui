//! The async bridge between the synchronous UI loop and the network.
//!
//! The UI thread never blocks on HTTP: it pushes a [`Request`] onto a channel
//! and later drains [`Response`]s. A single background task owns the
//! [`LinearClient`] and services requests one at a time.

use image::DynamicImage;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::client::LinearClient;
use crate::images;
use crate::models::{Issue, IssueDetail, Project, Team, User, View, WorkflowState};

/// Work the UI asks the background task to perform.
#[derive(Debug)]
pub enum Request {
    LoadViewer,
    LoadTeams,
    /// Fetch the issue list for a team + view, optionally narrowed to a
    /// project. `epoch` is echoed back so the UI can discard results from a
    /// superseded request (fast scrolling).
    LoadIssues {
        team_id: String,
        view: View,
        project_id: Option<String>,
        epoch: u64,
    },
    LoadProjects { team_id: String },
    /// Fetch full detail for an issue. `epoch` is echoed back so the UI can
    /// drop results for an issue it's no longer hovering.
    LoadIssueDetail { issue_id: String, epoch: u64 },
    /// Download + decode an image embedded in an issue. Runs as a detached task
    /// (see [`spawn`]) so a slow image fetch never delays issue navigation.
    LoadImage { url: String },
    /// Workflow states for the state picker.
    LoadStates { team_id: String },
    /// Members for the assignee picker.
    LoadMembers { team_id: String },
    SetState { issue_id: String, state_id: String },
    SetAssignee { issue_id: String, assignee_id: Option<String> },
    AddComment { issue_id: String, body: String },
    /// Create an issue. A `parent_id` makes it a sub-issue of that issue.
    CreateIssue {
        team_id: String,
        title: String,
        parent_id: Option<String>,
    },
}

/// Results / events flowing back to the UI.
#[derive(Debug)]
pub enum Response {
    Viewer(User),
    Teams(Vec<Team>),
    Projects(Vec<Project>),
    Issues {
        view: View,
        epoch: u64,
        issues: Vec<Issue>,
    },
    IssueDetail { epoch: u64, detail: Box<IssueDetail> },
    /// A decoded image, keyed by the URL the UI requested.
    Image { url: String, image: Box<DynamicImage> },
    /// An image download/decode failed; `error` is shown in the viewer.
    ImageFailed { url: String, error: String },
    States(Vec<WorkflowState>),
    Members(Vec<User>),
    /// A mutation finished; the string is a human-readable status line.
    /// `refresh` asks the UI to reload the current issue/list.
    ActionDone { message: String, refresh: bool },
    Error(String),
}

/// Spawn the worker. Returns the sender for [`Request`]s; responses arrive on
/// `tx_resp`. The viewer id is shared so "My Issues" filtering works without a
/// round-trip on every request.
pub fn spawn(
    client: LinearClient,
    mut rx: UnboundedReceiver<Request>,
    tx: UnboundedSender<Response>,
) {
    tokio::spawn(async move {
        // Cache the viewer id once we learn it (needed for the MyIssues filter).
        let mut viewer_id = String::new();

        while let Some(req) = rx.recv().await {
            // Image fetches run concurrently as detached tasks: they can be slow
            // (network + decode) and must not block issue/detail navigation,
            // which flows through the sequential `handle` path below.
            if let Request::LoadImage { url } = req {
                let client = client.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let resp = match load_image(&client, &url).await {
                        Ok(image) => Response::Image {
                            url,
                            image: Box::new(image),
                        },
                        Err(e) => Response::ImageFailed {
                            url,
                            error: format!("{e:#}"),
                        },
                    };
                    let _ = tx.send(resp);
                });
                continue;
            }

            let result = handle(&client, &mut viewer_id, req).await;
            // If the UI is gone, stop.
            let send = match result {
                Ok(resp) => tx.send(resp),
                Err(e) => tx.send(Response::Error(format!("{e:#}"))),
            };
            if send.is_err() {
                break;
            }
        }
    });
}

async fn handle(
    client: &LinearClient,
    viewer_id: &mut String,
    req: Request,
) -> anyhow::Result<Response> {
    Ok(match req {
        Request::LoadViewer => {
            let user = client.viewer().await?;
            *viewer_id = user.id.clone();
            Response::Viewer(user)
        }
        Request::LoadTeams => Response::Teams(client.teams().await?),
        Request::LoadIssues {
            team_id,
            view,
            project_id,
            epoch,
        } => {
            // Ensure we know who we are before a MyIssues fetch.
            if viewer_id.is_empty() {
                *viewer_id = client.viewer().await?.id;
            }
            let issues = client
                .issues(&team_id, view, viewer_id, project_id.as_deref())
                .await?;
            Response::Issues {
                view,
                epoch,
                issues,
            }
        }
        Request::LoadProjects { team_id } => {
            Response::Projects(client.team_projects(&team_id).await?)
        }
        Request::LoadIssueDetail { issue_id, epoch } => Response::IssueDetail {
            epoch,
            detail: Box::new(client.issue_detail(&issue_id).await?),
        },
        // Intercepted in `spawn` before reaching here.
        Request::LoadImage { .. } => unreachable!("LoadImage runs as a detached task"),
        Request::LoadStates { team_id } => Response::States(client.team_states(&team_id).await?),
        Request::LoadMembers { team_id } => {
            Response::Members(client.team_members(&team_id).await?)
        }
        Request::SetState { issue_id, state_id } => {
            client.set_state(&issue_id, &state_id).await?;
            Response::ActionDone {
                message: "State updated".into(),
                refresh: true,
            }
        }
        Request::SetAssignee {
            issue_id,
            assignee_id,
        } => {
            client.set_assignee(&issue_id, assignee_id.as_deref()).await?;
            Response::ActionDone {
                message: "Assignee updated".into(),
                refresh: true,
            }
        }
        Request::AddComment { issue_id, body } => {
            client.add_comment(&issue_id, &body).await?;
            Response::ActionDone {
                message: "Comment added".into(),
                refresh: true,
            }
        }
        Request::CreateIssue { team_id, title, parent_id } => {
            let (id, _url) = client
                .create_issue_full(&team_id, &title, None, None, None, parent_id.as_deref())
                .await?;
            let message = match &parent_id {
                Some(_) => format!("Created sub-issue {id}"),
                None => format!("Created {id}"),
            };
            Response::ActionDone { message, refresh: true }
        }
    })
}

/// Fetch an image's bytes (disk cache first, then network) and decode them.
async fn load_image(client: &LinearClient, url: &str) -> anyhow::Result<DynamicImage> {
    let cache = images::cache_path(url);

    // Serve from the on-disk cache when we have valid bytes there.
    if let Some(path) = &cache {
        if let Ok(bytes) = tokio::fs::read(path).await {
            if let Ok(image) = decode(bytes).await {
                return Ok(image);
            }
            // Cached bytes were unreadable/corrupt — fall through and re-fetch.
        }
    }

    let bytes = client.fetch_image(url).await?;
    if let Some(path) = &cache {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        // Best-effort: a cache write failure shouldn't fail the load.
        let _ = tokio::fs::write(path, &bytes).await;
    }
    decode(bytes).await
}

/// Decode image bytes off the async runtime (decoding is CPU-bound/blocking).
async fn decode(bytes: Vec<u8>) -> anyhow::Result<DynamicImage> {
    tokio::task::spawn_blocking(move || image::load_from_memory(&bytes))
        .await?
        .map_err(|e| anyhow::anyhow!("could not decode image: {e}"))
}
