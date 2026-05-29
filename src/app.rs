//! UI-side application state and the logic that turns key presses into
//! [`Request`]s. Rendering lives in `ui.rs`; this module is render-agnostic.

use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedSender;

use crate::models::{Issue, IssueDetail, Project, Team, User, View, WorkflowState};
use crate::worker::{Request, Response};

/// Which of the navigable panes currently has focus. Tab cycles through them in
/// visual order (the left column top-to-bottom, then the issue list, then the
/// detail), mirroring slack-tui's Tab-to-move-focus behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Views,
    Teams,
    Projects,
    Issues,
    Detail,
}

impl Pane {
    fn next(self) -> Pane {
        match self {
            Pane::Views => Pane::Teams,
            Pane::Teams => Pane::Projects,
            Pane::Projects => Pane::Issues,
            Pane::Issues => Pane::Detail,
            Pane::Detail => Pane::Views,
        }
    }
    fn prev(self) -> Pane {
        match self {
            Pane::Views => Pane::Detail,
            Pane::Teams => Pane::Views,
            Pane::Projects => Pane::Teams,
            Pane::Issues => Pane::Projects,
            Pane::Detail => Pane::Issues,
        }
    }
}

/// A free-text prompt overlay (comment body / new-issue title).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Comment,
    CreateIssue,
}

/// A selectable-list overlay (state picker / assignee picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    State,
    Assignee,
}

/// The active modal overlay, if any.
pub enum Overlay {
    None,
    Input {
        kind: InputKind,
        buffer: String,
    },
    Picker {
        kind: PickerKind,
        /// (option id — empty string means "unassigned", label).
        items: Vec<(String, String)>,
        state: ListState,
    },
    Help,
}

pub struct App {
    pub tx: UnboundedSender<Request>,

    pub viewer: Option<User>,
    pub teams: Vec<Team>,
    pub teams_state: ListState,

    pub views_state: ListState,

    /// Projects for the selected team. Rendered with a leading "All projects"
    /// row, so `projects_state` index 0 means "no project filter".
    pub projects: Vec<Project>,
    pub projects_state: ListState,

    pub issues: Vec<Issue>,
    pub issues_state: ListState,
    pub current_view: View,
    /// Bumped on every issue (re)load so stale responses can be dropped.
    issues_epoch: u64,

    pub detail: Option<IssueDetail>,
    pub detail_scroll: u16,

    pub focus: Pane,
    pub overlay: Overlay,
    pub status: String,
    pub inflight: u32,
    pub should_quit: bool,
}

impl App {
    pub fn new(tx: UnboundedSender<Request>) -> Self {
        let mut teams_state = ListState::default();
        teams_state.select(Some(0));
        // View::ALL[0] is My Issues — the default selection.
        let mut views_state = ListState::default();
        views_state.select(Some(0));
        let mut projects_state = ListState::default();
        projects_state.select(Some(0)); // "All projects"
        App {
            tx,
            viewer: None,
            teams: Vec::new(),
            teams_state,
            views_state,
            projects: Vec::new(),
            projects_state,
            issues: Vec::new(),
            issues_state: ListState::default(),
            current_view: View::ALL[0],
            issues_epoch: 0,
            detail: None,
            detail_scroll: 0,
            focus: Pane::Views,
            overlay: Overlay::None,
            status: "Loading…".into(),
            inflight: 0,
            should_quit: false,
        }
    }

    // ----- request helpers ----------------------------------------------

    fn send(&mut self, req: Request) {
        self.inflight = self.inflight.saturating_add(1);
        if self.tx.send(req).is_err() {
            self.status = "worker stopped".into();
        }
    }

    pub fn bootstrap(&mut self) {
        self.send(Request::LoadViewer);
        self.send(Request::LoadTeams);
    }

    pub fn selected_team(&self) -> Option<&Team> {
        self.teams.get(self.teams_state.selected().unwrap_or(0))
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        self.issues.get(self.issues_state.selected()?)
    }

    /// The project id to filter by, or `None` when "All projects" (index 0) is
    /// selected.
    fn selected_project_id(&self) -> Option<String> {
        let idx = self.projects_state.selected()?;
        if idx == 0 {
            None
        } else {
            self.projects.get(idx - 1).map(|p| p.id.clone())
        }
    }

    fn reload_issues(&mut self) {
        if let Some(team) = self.selected_team().cloned() {
            let view = self.current_view;
            let project_id = self.selected_project_id();
            self.issues.clear();
            self.issues_state.select(None);
            self.issues_epoch += 1;
            self.status = format!("Loading {} · {}…", team.key, view.label());
            self.send(Request::LoadIssues {
                team_id: team.id,
                view,
                project_id,
                epoch: self.issues_epoch,
            });
        }
    }

    /// Reload the project list for the selected team, resetting the project
    /// filter back to "All projects".
    fn reload_projects(&mut self) {
        self.projects.clear();
        self.projects_state.select(Some(0));
        if let Some(team) = self.selected_team().cloned() {
            self.send(Request::LoadProjects { team_id: team.id });
        }
    }

    fn open_selected_issue(&mut self) {
        if let Some(issue) = self.selected_issue().cloned() {
            self.detail = None;
            self.detail_scroll = 0;
            self.focus = Pane::Detail;
            self.send(Request::LoadIssueDetail {
                issue_id: issue.id,
            });
        }
    }

    // ----- response handling --------------------------------------------

    pub fn on_response(&mut self, resp: Response) {
        self.inflight = self.inflight.saturating_sub(1);
        match resp {
            Response::Viewer(u) => {
                self.status = format!("Signed in as {}", u.label());
                self.viewer = Some(u);
            }
            Response::Teams(teams) => {
                self.teams = teams;
                if self.teams.is_empty() {
                    self.status = "No teams visible to this API key".into();
                } else {
                    self.teams_state.select(Some(0));
                    self.status = format!("{} team(s)", self.teams.len());
                    self.reload_projects();
                    self.reload_issues();
                }
            }
            Response::Projects(projects) => {
                self.projects = projects;
            }
            Response::Issues {
                view,
                epoch,
                issues,
            } => {
                // Drop results from a superseded request (team/view switched
                // since, or scrolled past during fast navigation).
                if epoch == self.issues_epoch {
                    self.issues = issues;
                    self.issues_state
                        .select((!self.issues.is_empty()).then_some(0));
                    self.status = format!("{} issue(s) · {}", self.issues.len(), view.label());
                }
            }
            Response::IssueDetail(d) => {
                self.status = format!("{} {}", d.identifier, d.title);
                self.detail = Some(*d);
            }
            Response::States(states) => self.open_state_picker(states),
            Response::Members(members) => self.open_assignee_picker(members),
            Response::ActionDone { message, refresh } => {
                self.status = message;
                if refresh {
                    // Refresh both the list and the open detail.
                    self.reload_issues();
                    if let Some(d) = &self.detail {
                        let id = d.id.clone();
                        self.send(Request::LoadIssueDetail { issue_id: id });
                    }
                }
            }
            Response::Error(e) => {
                self.status = format!("⚠ {e}");
            }
        }
    }

    // ----- pickers / overlays -------------------------------------------

    fn open_state_picker(&mut self, states: Vec<WorkflowState>) {
        let items: Vec<(String, String)> = states
            .into_iter()
            .filter(|s| s.kind != "triage")
            .map(|s| (s.id, format!("{} ({})", s.name, s.kind)))
            .collect();
        let mut state = ListState::default();
        state.select((!items.is_empty()).then_some(0));
        self.overlay = Overlay::Picker {
            kind: PickerKind::State,
            items,
            state,
        };
    }

    fn open_assignee_picker(&mut self, members: Vec<User>) {
        let mut items = vec![(String::new(), "— Unassigned —".to_string())];
        items.extend(members.into_iter().map(|m| {
            let label = m.label().to_string();
            (m.id, label)
        }));
        let mut state = ListState::default();
        state.select(Some(0));
        self.overlay = Overlay::Picker {
            kind: PickerKind::Assignee,
            items,
            state,
        };
    }

    fn confirm_picker(&mut self) {
        // Take ownership of the overlay so we can drop it before sending.
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::Picker { kind, items, state } = overlay else {
            self.overlay = overlay;
            return;
        };
        let Some(idx) = state.selected() else { return };
        let Some((id, _label)) = items.get(idx).cloned() else {
            return;
        };
        let Some(issue) = self.selected_issue().cloned() else {
            return;
        };
        match kind {
            PickerKind::State => self.send(Request::SetState {
                issue_id: issue.id,
                state_id: id,
            }),
            PickerKind::Assignee => self.send(Request::SetAssignee {
                issue_id: issue.id,
                assignee_id: (!id.is_empty()).then_some(id),
            }),
        }
    }

    fn confirm_input(&mut self) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::Input { kind, buffer } = overlay else {
            self.overlay = overlay;
            return;
        };
        let text = buffer.trim().to_string();
        if text.is_empty() {
            self.status = "Cancelled (empty input)".into();
            return;
        }
        match kind {
            InputKind::Comment => {
                if let Some(issue) = self.selected_issue().cloned() {
                    self.send(Request::AddComment {
                        issue_id: issue.id,
                        body: text,
                    });
                }
            }
            InputKind::CreateIssue => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::CreateIssue {
                        team_id: team.id,
                        title: text,
                    });
                }
            }
        }
    }
}

// ----- key handling -----------------------------------------------------

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    /// Returns nothing; mutates state. Quit is signalled via `should_quit`.
    pub fn on_key(&mut self, key: KeyEvent) {
        // Overlays capture all input while open.
        match &mut self.overlay {
            Overlay::Help => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Input { buffer, .. } => {
                match key.code {
                    KeyCode::Esc => self.overlay = Overlay::None,
                    KeyCode::Enter => self.confirm_input(),
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Char(c) => buffer.push(c),
                    _ => {}
                }
                return;
            }
            Overlay::Picker { state, items, .. } => {
                let len = items.len();
                match key.code {
                    KeyCode::Esc => self.overlay = Overlay::None,
                    KeyCode::Enter => self.confirm_picker(),
                    KeyCode::Down | KeyCode::Char('j') => move_sel(state, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => move_sel(state, len, -1),
                    _ => {}
                }
                return;
            }
            Overlay::None => {}
        }

        // Global keys.
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.overlay = Overlay::Help;
                return;
            }
            KeyCode::Tab => {
                self.focus = self.focus.next();
                return;
            }
            KeyCode::BackTab => {
                self.focus = self.focus.prev();
                return;
            }
            KeyCode::Char('r') => {
                self.reload_issues();
                return;
            }
            // Create issue works from anywhere as long as a team is selected.
            KeyCode::Char('n') => {
                if self.selected_team().is_some() {
                    self.overlay = Overlay::Input {
                        kind: InputKind::CreateIssue,
                        buffer: String::new(),
                    };
                }
                return;
            }
            _ => {}
        }

        match self.focus {
            Pane::Views => self.keys_views(key),
            Pane::Teams => self.keys_teams(key),
            Pane::Projects => self.keys_projects(key),
            Pane::Issues => self.keys_issues(key),
            Pane::Detail => self.keys_detail(key),
        }
    }

    fn keys_teams(&mut self, key: KeyEvent) {
        let len = self.teams.len();
        match key.code {
            // Switching team reloads its projects and issues live — no Enter/r.
            KeyCode::Down | KeyCode::Char('j') => {
                move_sel(&mut self.teams_state, len, 1);
                self.reload_projects();
                self.reload_issues();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_sel(&mut self.teams_state, len, -1);
                self.reload_projects();
                self.reload_issues();
            }
            KeyCode::Enter => self.focus = Pane::Issues,
            _ => {}
        }
    }

    fn keys_projects(&mut self, key: KeyEvent) {
        // +1 for the leading "All projects" row.
        let len = self.projects.len() + 1;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                move_sel(&mut self.projects_state, len, 1);
                self.reload_issues();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_sel(&mut self.projects_state, len, -1);
                self.reload_issues();
            }
            KeyCode::Enter => self.focus = Pane::Issues,
            _ => {}
        }
    }

    fn keys_views(&mut self, key: KeyEvent) {
        let len = View::ALL.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                move_sel(&mut self.views_state, len, 1);
                self.apply_selected_view();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_sel(&mut self.views_state, len, -1);
                self.apply_selected_view();
            }
            KeyCode::Enter => self.focus = Pane::Issues,
            _ => {}
        }
    }

    /// Set `current_view` from the Views-pane selection and reload.
    fn apply_selected_view(&mut self) {
        let idx = self.views_state.selected().unwrap_or(0);
        self.current_view = View::ALL[idx];
        self.reload_issues();
    }

    fn keys_issues(&mut self, key: KeyEvent) {
        let len = self.issues.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => move_sel(&mut self.issues_state, len, 1),
            KeyCode::Up | KeyCode::Char('k') => move_sel(&mut self.issues_state, len, -1),
            KeyCode::Enter => self.open_selected_issue(),
            KeyCode::Char('s') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadStates { team_id: team.id });
                }
            }
            KeyCode::Char('a') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadMembers { team_id: team.id });
                }
            }
            KeyCode::Char('m')
                if self.selected_issue().is_some() => {
                    self.overlay = Overlay::Input {
                        kind: InputKind::Comment,
                        buffer: String::new(),
                    };
                }
            _ => {}
        }
    }

    fn keys_detail(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = self.detail_scroll.saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1)
            }
            KeyCode::Char('s') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadStates { team_id: team.id });
                }
            }
            KeyCode::Char('a') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadMembers { team_id: team.id });
                }
            }
            KeyCode::Char('m')
                if self.selected_issue().is_some() => {
                    self.overlay = Overlay::Input {
                        kind: InputKind::Comment,
                        buffer: String::new(),
                    };
                }
            _ => {}
        }
    }
}

/// Move a list selection by `delta`, clamping to `[0, len)`.
fn move_sel(state: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        state.select(None);
        return;
    }
    let cur = state.selected().unwrap_or(0) as i32;
    let next = (cur + delta).clamp(0, len as i32 - 1);
    state.select(Some(next as usize));
}
