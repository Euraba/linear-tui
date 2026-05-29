//! The async bridge between the synchronous UI loop and the network.
//!
//! The UI thread never blocks on HTTP: it pushes a [`Request`] onto a channel
//! and later drains [`Response`]s. A single background task owns the
//! [`LinearClient`] and services requests one at a time.

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::client::LinearClient;
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
    LoadIssueDetail { issue_id: String },
    /// Workflow states for the state picker.
    LoadStates { team_id: String },
    /// Members for the assignee picker.
    LoadMembers { team_id: String },
    SetState { issue_id: String, state_id: String },
    SetAssignee { issue_id: String, assignee_id: Option<String> },
    AddComment { issue_id: String, body: String },
    CreateIssue { team_id: String, title: String },
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
    IssueDetail(Box<IssueDetail>),
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
        Request::LoadIssueDetail { issue_id } => {
            Response::IssueDetail(Box::new(client.issue_detail(&issue_id).await?))
        }
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
        Request::CreateIssue { team_id, title } => {
            let id = client.create_issue(&team_id, &title).await?;
            Response::ActionDone {
                message: format!("Created {id}"),
                refresh: true,
            }
        }
    })
}
